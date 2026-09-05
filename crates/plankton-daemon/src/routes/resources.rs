use std::{collections::BTreeMap, io, path::Path, time::UNIX_EPOCH};

use axum::{extract::State, Json};
use chrono::Utc;
use plankton_core::{
    default_value_resolver, is_direct_access, list_local_secret_catalog, load_settings,
    local_secret_catalog_path,
    resources::{
        search::{ResourceSearchEngine, SearchQuery},
        BackendKind, ResourceDocument,
    },
    AccessRequest, ApprovalStatus, AuditAction, ImportedSecretReference, LocalSecretCatalog,
    LocalSecretLiteralEntry, PolicyMode, RequestContext, ValueResolver,
};
use plankton_protocol::{
    daemon::{RequestEnvelope, ResponseEnvelope},
    error::{ErrorCode, ErrorSeverity, ErrorSource, PlanktonError},
    resources::{
        ResourceAccessCallChainNode, ResourceAccessRequest, ResourceAccessResponse,
        ResourceAccessState, ResourceAccessStatusRequest, ResourceSearchRequest,
        ResourceSearchResponse,
    },
    PROTOCOL_VERSION,
};
use plankton_store::{
    BackendBindingRecord, RequestQueryResult, SqliteStore, StoreError, VaultManifestRecord,
};

use crate::server::ServerState;

pub async fn search_resources(
    Json(request): Json<RequestEnvelope<ResourceSearchRequest>>,
) -> Json<ResponseEnvelope<ResourceSearchResponse>> {
    if let Err(error) = request.validate_version() {
        return Json(failure(
            request.correlation_id,
            ErrorCode::ProtocolMismatch,
            error.to_string(),
            false,
        ));
    }
    if let Err(error) = request.payload.validate() {
        return Json(failure(
            request.correlation_id,
            ErrorCode::InvalidRequest,
            error.to_string(),
            false,
        ));
    }
    let correlation_id = request.correlation_id;
    let catalog = match tokio::task::spawn_blocking(list_local_secret_catalog).await {
        Ok(Ok(catalog)) => catalog,
        Ok(Err(error)) => {
            return Json(failure(
                correlation_id,
                ErrorCode::StorageFailed,
                error.to_string(),
                false,
            ));
        }
        Err(error) => {
            return Json(failure(
                correlation_id,
                ErrorCode::Internal,
                format!("resource index task failed: {error}"),
                true,
            ));
        }
    };
    let (_, store) = match open_store().await {
        Ok(pair) => pair,
        Err(error) => {
            return Json(failure(
                correlation_id,
                ErrorCode::StorageFailed,
                error.to_string(),
                true,
            ));
        }
    };
    let enabled_backends = match enabled_external_backends(&store).await {
        Ok(backends) => backends,
        Err(error) => {
            return Json(failure(
                correlation_id,
                ErrorCode::StorageFailed,
                error.to_string(),
                true,
            ));
        }
    };
    let generation = match catalog_generation() {
        Ok(generation) => generation,
        Err(error) => {
            return Json(failure(
                correlation_id,
                ErrorCode::StorageFailed,
                format!("failed to read resource catalog generation: {error}"),
                true,
            ));
        }
    };
    let documents = catalog_documents(catalog, &enabled_backends);
    let engine = match synchronized_resource_search_engine(&store, generation, &documents).await {
        Ok(engine) => engine,
        Err(error) => {
            return Json(failure(
                correlation_id,
                ErrorCode::StorageFailed,
                format!("failed to refresh resource search index: {error}"),
                true,
            ));
        }
    };
    let query = SearchQuery {
        text: request.payload.query,
        tag_all: request.payload.tag_all,
        tag_any: request.payload.tag_any,
        field_key: request.payload.field_key,
        notes: request.payload.notes,
        limit: request.payload.limit,
        cursor: request.payload.cursor,
    };
    match engine.search(&query) {
        Ok(data) => Json(success(correlation_id, data)),
        Err(error) => Json(failure(
            correlation_id,
            ErrorCode::InvalidRequest,
            error.to_string(),
            false,
        )),
    }
}

pub async fn access_resource(
    State(state): State<ServerState>,
    Json(request): Json<RequestEnvelope<ResourceAccessRequest>>,
) -> Json<ResponseEnvelope<ResourceAccessResponse>> {
    if let Err(error) = request.validate_version() {
        return Json(failure(
            request.correlation_id,
            ErrorCode::ProtocolMismatch,
            error.to_string(),
            false,
        ));
    }
    let correlation_id = request.correlation_id;
    if request.payload.resource_id.trim().is_empty()
        || request.payload.reason.trim().is_empty()
        || request.payload.requested_by.trim().is_empty()
    {
        return Json(failure(
            correlation_id,
            ErrorCode::InvalidRequest,
            "resource_id, reason, and requested_by are required".into(),
            false,
        ));
    }
    match resource_backend_enabled(&state.store, &request.payload.resource_id).await {
        Ok(true) => {}
        Ok(false) => {
            return Json(failure(
                correlation_id,
                ErrorCode::BackendUnavailable,
                "the resource is not available from an enabled credential backend".into(),
                false,
            ));
        }
        Err(error) => {
            return Json(failure(
                correlation_id,
                ErrorCode::StorageFailed,
                error.to_string(),
                true,
            ));
        }
    }
    let annotations = tokio::task::spawn_blocking({
        let resource_id = request.payload.resource_id.clone();
        move || resource_annotations(&resource_id)
    })
    .await;
    let (tags, resource_metadata) = match annotations {
        Ok(Ok(annotations)) => annotations,
        Ok(Err(error)) => {
            return Json(failure(
                correlation_id,
                ErrorCode::StorageFailed,
                error.to_string(),
                false,
            ));
        }
        Err(error) => {
            return Json(failure(
                correlation_id,
                ErrorCode::Internal,
                format!("resource annotation task failed: {error}"),
                true,
            ));
        }
    };
    let context = resource_access_context(request.payload, tags, resource_metadata);
    let settings = match state.settings.current() {
        Ok(settings) => settings,
        Err(error) => {
            return Json(failure(
                correlation_id,
                ErrorCode::Internal,
                format!("failed to reload daemon settings: {error}"),
                true,
            ));
        }
    };
    if is_direct_access(&context.resource_metadata) {
        let submitted = match state
            .store
            .submit_request(&settings, context, PolicyMode::ManualOnly)
            .await
        {
            Ok(request) => request,
            Err(error) => {
                return Json(failure(
                    correlation_id,
                    ErrorCode::StorageFailed,
                    error.to_string(),
                    true,
                ));
            }
        };
        let approved = match state
            .store
            .record_direct_policy_decision(
                &submitted.id,
                "field_exposure_policy",
                Some("Direct field: human and LLM access does not require approval.".into()),
            )
            .await
        {
            Ok(request) => request,
            Err(error) => {
                return Json(failure(
                    correlation_id,
                    ErrorCode::StorageFailed,
                    error.to_string(),
                    true,
                ));
            }
        };
        let resource_id = approved.context.resource.clone();
        let resolved = tokio::task::spawn_blocking(move || {
            let resolver = default_value_resolver()?;
            resolver.resolve(&resource_id)
        })
        .await;
        return match resolved {
            Ok(Ok(value)) => {
                let mut response = response_from_request(
                    &approved,
                    Some("Direct field: approval bypassed by its exposure policy.".into()),
                );
                response.value = Some(value);
                Json(success(correlation_id, response))
            }
            Ok(Err(error)) => Json(failure(
                correlation_id,
                ErrorCode::BackendFailed,
                error.to_string(),
                true,
            )),
            Err(error) => Json(failure(
                correlation_id,
                ErrorCode::Internal,
                format!("resource resolver task failed: {error}"),
                true,
            )),
        };
    }
    let submitted = state
        .store
        .submit_request(&settings, context, settings.default_policy_mode)
        .await;
    match submitted {
        Ok(created) => {
            if created.evaluation_state == plankton_core::EvaluationState::Queued
                && matches!(
                    created.policy_mode,
                    PolicyMode::Assisted | PolicyMode::LlmAutomatic
                )
            {
                state.evaluations.spawn(created.id.clone());
            }
            Json(success(
                correlation_id,
                submission_response_from_request(&created),
            ))
        }
        Err(error) => Json(failure(
            correlation_id,
            ErrorCode::StorageFailed,
            error.to_string(),
            true,
        )),
    }
}

fn resource_access_context(
    request: ResourceAccessRequest,
    resource_tags: Vec<String>,
    resource_metadata: BTreeMap<String, String>,
) -> RequestContext {
    let ResourceAccessRequest {
        resource_id,
        reason,
        requested_by,
        script_path,
        call_chain_details,
        call_chain,
        mut metadata,
    } = request;
    let script_path = script_path.or_else(|| metadata.remove("script_path"));
    let structured_call_chain = if call_chain_details.is_empty() {
        call_chain
            .iter()
            .cloned()
            .map(plankton_core::CallChainNode::best_effort_path)
            .collect::<Vec<_>>()
    } else {
        call_chain_details
            .into_iter()
            .map(core_call_chain_node)
            .collect::<Vec<_>>()
    };
    let summary = structured_call_chain
        .iter()
        .filter_map(|node| node.prompt_display_path())
        .collect::<Vec<_>>()
        .join(" -> ");
    if !summary.is_empty() {
        metadata.insert("call_chain_summary".into(), summary);
    }

    let mut context = RequestContext::new(resource_id, reason, requested_by);
    context.resource_tags = resource_tags;
    context.resource_metadata = resource_metadata;
    context.script_path = script_path;
    context.call_chain = structured_call_chain;
    context.metadata = metadata;
    context
}

fn core_call_chain_node(node: ResourceAccessCallChainNode) -> plankton_core::CallChainNode {
    let previewable = node
        .resolved_file_path
        .as_deref()
        .is_some_and(|path| !path.trim().is_empty());
    plankton_core::CallChainNode {
        pid: node.pid,
        ppid: node.ppid,
        process_name: node.process_name,
        executable_path: node.executable_path,
        argv: node.argv,
        resolved_file_path: node.resolved_file_path,
        source: plankton_core::CallChainNodeSource::BestEffort,
        previewable,
        preview_status: if previewable {
            plankton_core::CallChainPreviewStatus::PathOnly
        } else {
            plankton_core::CallChainPreviewStatus::NotPreviewable
        },
        preview_text: None,
        preview_error: None,
    }
}

pub async fn access_status(
    State(state): State<ServerState>,
    Json(request): Json<RequestEnvelope<ResourceAccessStatusRequest>>,
) -> Json<ResponseEnvelope<ResourceAccessResponse>> {
    if let Err(error) = request.validate_version() {
        return Json(failure(
            request.correlation_id,
            ErrorCode::ProtocolMismatch,
            error.to_string(),
            false,
        ));
    }
    let correlation_id = request.correlation_id;
    let result = match state.store.get_request(&request.payload.request_id).await {
        Ok(result) => result,
        Err(StoreError::NotFound(_)) => {
            return Json(failure(
                correlation_id,
                ErrorCode::NotFound,
                "access request was not found".into(),
                false,
            ));
        }
        Err(error) => {
            return Json(failure(
                correlation_id,
                ErrorCode::StorageFailed,
                error.to_string(),
                true,
            ));
        }
    };
    let note = decision_note(&result);
    if result.request.approval_status != ApprovalStatus::Approved {
        return Json(success(
            correlation_id,
            response_from_request(&result.request, note),
        ));
    }
    match resource_backend_enabled(&state.store, &result.request.context.resource).await {
        Ok(true) => {}
        Ok(false) => {
            return Json(failure(
                correlation_id,
                ErrorCode::BackendUnavailable,
                "the resource is not available from an enabled credential backend".into(),
                false,
            ));
        }
        Err(error) => {
            return Json(failure(
                correlation_id,
                ErrorCode::StorageFailed,
                error.to_string(),
                true,
            ));
        }
    }

    let resource_id = result.request.context.resource.clone();
    let resolved = tokio::task::spawn_blocking(move || {
        let resolver = default_value_resolver()?;
        resolver.resolve(&resource_id)
    })
    .await;
    match resolved {
        Ok(Ok(value)) => {
            let mut response = response_from_request(&result.request, note);
            response.value = Some(value);
            Json(success(correlation_id, response))
        }
        Ok(Err(error)) => {
            let diagnostic = PlanktonError {
                code: ErrorCode::BackendFailed,
                user_message: "credential provider could not resolve the approved resource"
                    .to_string(),
                internal_message: Some(error.to_string()),
                public_context: BTreeMap::new(),
                internal_context: BTreeMap::from([(
                    "resource_id".to_string(),
                    result.request.context.resource.clone(),
                )]),
                severity: ErrorSeverity::Error,
                retryable: true,
                timestamp: Utc::now(),
                correlation_id,
                source: ErrorSource::Backend {
                    backend_id: "credential_backend".to_string(),
                },
            };
            if let Err(store_error) = state.store.record_diagnostic_error(&diagnostic).await {
                return Json(failure(
                    correlation_id,
                    ErrorCode::StorageFailed,
                    format!("failed to persist provider diagnostic: {store_error}"),
                    true,
                ));
            }
            Json(ResponseEnvelope::Failure {
                protocol_version: PROTOCOL_VERSION,
                correlation_id,
                error: diagnostic.ai_safe(),
            })
        }
        Err(error) => Json(failure(
            correlation_id,
            ErrorCode::Internal,
            format!("resource resolver task failed: {error}"),
            true,
        )),
    }
}

async fn open_store() -> anyhow::Result<(plankton_core::PlanktonSettings, SqliteStore)> {
    let settings = load_settings()?;
    let store = SqliteStore::new(&settings).await?;
    Ok((settings, store))
}

async fn enabled_external_backends(store: &SqliteStore) -> anyhow::Result<Vec<BackendKind>> {
    Ok(store
        .list_backend_bindings(true)
        .await?
        .into_iter()
        .map(|binding| binding.backend_kind)
        .collect())
}

async fn synchronized_resource_search_engine(
    store: &SqliteStore,
    generation: u64,
    documents: &[ResourceDocument],
) -> Result<ResourceSearchEngine, StoreError> {
    let (stored_generation, stored_documents) = store.load_resource_search_index().await?;
    if stored_generation == generation && same_resource_documents(&stored_documents, documents) {
        return Ok(ResourceSearchEngine::new(stored_documents, generation));
    }

    ensure_resource_index_hierarchy(store, documents).await?;
    store
        .replace_resource_search_index(generation, documents)
        .await?;
    let (stored_generation, stored_documents) = store.load_resource_search_index().await?;
    Ok(ResourceSearchEngine::new(
        stored_documents,
        stored_generation,
    ))
}

fn same_resource_documents(left: &[ResourceDocument], right: &[ResourceDocument]) -> bool {
    left.len() == right.len()
        && left.iter().all(|document| {
            right.iter().any(|candidate| {
                candidate.resource_id == document.resource_id && candidate == document
            })
        })
}

async fn ensure_resource_index_hierarchy(
    store: &SqliteStore,
    documents: &[ResourceDocument],
) -> Result<(), StoreError> {
    let existing = store.list_backend_bindings(false).await?;
    let now = Utc::now();
    for (id, kind, name, enabled, capabilities) in [
        (
            "plankton",
            BackendKind::Local,
            "Plankton",
            true,
            vec!["search", "read", "create", "sync"],
        ),
        (
            "onepassword",
            BackendKind::OnePassword,
            "1Password",
            false,
            vec!["search", "read", "create"],
        ),
        (
            "bitwarden",
            BackendKind::Bitwarden,
            "Bitwarden",
            false,
            vec!["search", "read", "create"],
        ),
    ] {
        if existing.iter().any(|binding| binding.id == id) {
            continue;
        }
        store
            .upsert_backend_binding(&BackendBindingRecord {
                id: id.to_string(),
                backend_kind: kind,
                display_name: name.to_string(),
                enabled,
                config: serde_json::json!({}),
                capabilities: capabilities.into_iter().map(str::to_string).collect(),
                created_at: now,
                updated_at: now,
            })
            .await?;
    }

    let mut vaults = BTreeMap::<String, String>::new();
    for document in documents {
        vaults
            .entry(document.backend_vault_id.clone())
            .or_insert_with(|| document.backend_binding_id.clone());
    }
    for (vault_id, backend_binding_id) in vaults {
        store
            .upsert_vault_manifest(&VaultManifestRecord {
                id: vault_id.clone(),
                backend_binding_id,
                display_name: vault_id,
                format_version: 4,
                local_path: None,
                revision: 0,
                archived: false,
                created_at: now,
                updated_at: now,
            })
            .await?;
    }
    Ok(())
}

async fn resource_backend_enabled(store: &SqliteStore, resource_id: &str) -> anyhow::Result<bool> {
    let resource_id = resource_id.to_string();
    let backend = tokio::task::spawn_blocking(move || {
        let catalog = list_local_secret_catalog()?;
        Ok::<_, plankton_core::SecretImportError>(
            catalog
                .imports
                .into_iter()
                .find(|entry| entry.resource == resource_id)
                .map(|entry| backend_kind_for_reference(&entry)),
        )
    })
    .await??;
    let Some(backend) = backend else {
        return Ok(true);
    };
    if backend == BackendKind::Local {
        return Ok(true);
    }
    Ok(store
        .list_backend_bindings(true)
        .await?
        .into_iter()
        .any(|binding| binding.backend_kind == backend))
}

fn catalog_documents(
    catalog: LocalSecretCatalog,
    enabled_backends: &[BackendKind],
) -> Vec<ResourceDocument> {
    let literal_documents = catalog.literals.into_iter().map(literal_document);
    let imported_documents = catalog
        .imports
        .into_iter()
        .filter(|entry| {
            let kind = backend_kind_for_reference(entry);
            kind == BackendKind::Local || enabled_backends.contains(&kind)
        })
        .map(imported_document);
    literal_documents.chain(imported_documents).collect()
}

fn literal_document(entry: LocalSecretLiteralEntry) -> ResourceDocument {
    let field_key = provider_neutral_field_key(&entry.resource, &entry.metadata);
    let field_label = entry
        .metadata
        .get("field_label")
        .filter(|value| !value.trim().is_empty())
        .cloned()
        .unwrap_or_else(|| field_key.clone());
    let section = entry
        .metadata
        .get("section")
        .filter(|value| !value.trim().is_empty())
        .cloned()
        .unwrap_or_else(|| "Credentials".into());
    let display_name = entry
        .metadata
        .get("item_title")
        .filter(|value| !value.trim().is_empty())
        .cloned()
        .or(entry.display_name.clone())
        .unwrap_or_else(|| entry.resource.clone());
    ResourceDocument {
        backend_kind: BackendKind::Local,
        backend_binding_id: "plankton".into(),
        backend_vault_id: "plankton-default".into(),
        resource_id: entry.resource.clone(),
        display_name,
        aliases: Vec::new(),
        description: entry.description.clone(),
        notes: entry.description.unwrap_or_default(),
        tags: entry.tags,
        field_key,
        field_label,
        section,
        metadata: entry.metadata,
    }
}

fn imported_document(entry: ImportedSecretReference) -> ResourceDocument {
    let backend_kind = backend_kind_for_reference(&entry);
    let backend_binding_id = match backend_kind {
        BackendKind::OnePassword => "onepassword",
        BackendKind::Bitwarden => "bitwarden",
        BackendKind::Local | BackendKind::Custom => "plankton",
    };
    let backend_vault_id = imported_vault_id(&entry, backend_binding_id);
    let field_key = provider_neutral_field_key(&entry.resource, &entry.metadata);
    let field_label = entry
        .metadata
        .get("field_label")
        .filter(|value| !value.trim().is_empty())
        .cloned()
        .unwrap_or_else(|| field_key.clone());
    let section = entry
        .metadata
        .get("section")
        .filter(|value| !value.trim().is_empty())
        .cloned()
        .unwrap_or_else(|| "Credentials".into());
    let display_name = entry
        .metadata
        .get("item_title")
        .filter(|value| !value.trim().is_empty())
        .cloned()
        .unwrap_or_else(|| entry.display_name.clone());
    ResourceDocument {
        backend_kind,
        backend_binding_id: backend_binding_id.into(),
        backend_vault_id,
        resource_id: entry.resource,
        display_name,
        aliases: Vec::new(),
        description: entry.description.clone(),
        notes: entry.description.unwrap_or_default(),
        tags: entry.tags,
        field_key,
        field_label,
        section,
        metadata: entry.metadata,
    }
}

fn provider_neutral_field_key(resource_id: &str, metadata: &BTreeMap<String, String>) -> String {
    metadata
        .get("field_key")
        .filter(|value| !value.trim().is_empty())
        .cloned()
        .unwrap_or_else(|| {
            resource_id
                .rsplit('/')
                .next()
                .unwrap_or(resource_id)
                .to_string()
        })
}

fn imported_vault_id(entry: &ImportedSecretReference, backend_binding_id: &str) -> String {
    use std::hash::{Hash, Hasher};

    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    entry.provider_kind().hash(&mut hasher);
    entry
        .container_label()
        .unwrap_or("default")
        .hash(&mut hasher);
    format!("{backend_binding_id}-{:016x}", hasher.finish())
}

fn backend_kind_for_reference(entry: &ImportedSecretReference) -> BackendKind {
    match entry.provider_kind() {
        "1password_cli" => BackendKind::OnePassword,
        "bitwarden_cli" => BackendKind::Bitwarden,
        _ => BackendKind::Local,
    }
}

fn catalog_generation() -> io::Result<u64> {
    catalog_generation_for_path(&local_secret_catalog_path())
}

fn catalog_generation_for_path(path: &Path) -> io::Result<u64> {
    let modified = match std::fs::metadata(path) {
        Ok(metadata) => metadata.modified()?,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(0),
        Err(error) => return Err(error),
    };
    let duration = modified.duration_since(UNIX_EPOCH).map_err(|error| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("catalog modification time predates the Unix epoch: {error}"),
        )
    })?;
    Ok(duration.as_secs() ^ u64::from(duration.subsec_nanos()))
}

fn resource_annotations(
    resource_id: &str,
) -> Result<(Vec<String>, BTreeMap<String, String>), plankton_core::SecretImportError> {
    let catalog = list_local_secret_catalog()?;
    if let Some(entry) = catalog
        .literals
        .into_iter()
        .find(|entry| entry.resource == resource_id)
    {
        let mut metadata = entry.metadata;
        if let Some(description) = entry.description {
            metadata.insert("resource_note".into(), description);
        }
        return Ok((entry.tags, metadata));
    }
    if let Some(entry) = catalog
        .imports
        .into_iter()
        .find(|entry| entry.resource == resource_id)
    {
        let mut metadata = entry.metadata;
        if let Some(description) = entry.description {
            metadata.insert("resource_note".into(), description);
        }
        return Ok((entry.tags, metadata));
    }
    Ok((Vec::new(), BTreeMap::new()))
}

fn response_from_request(
    request: &AccessRequest,
    decision_note: Option<String>,
) -> ResourceAccessResponse {
    ResourceAccessResponse {
        request_id: request.id.clone(),
        resource_id: request.context.resource.clone(),
        state: match request.approval_status {
            ApprovalStatus::Pending => ResourceAccessState::Pending,
            ApprovalStatus::Approved => ResourceAccessState::Approved,
            ApprovalStatus::Rejected => ResourceAccessState::Denied,
        },
        human_review_required: request.human_review_required(),
        value: None,
        decision_note,
    }
}

fn submission_response_from_request(request: &AccessRequest) -> ResourceAccessResponse {
    let mut response = response_from_request(request, None);
    if response.state == ResourceAccessState::Approved {
        response.state = ResourceAccessState::Pending;
    }
    response
}

fn decision_note(result: &RequestQueryResult) -> Option<String> {
    result
        .audit_records
        .iter()
        .rev()
        .find(|record| record.action == AuditAction::ApprovalRecorded)
        .and_then(|record| record.note.clone())
}

fn success<T>(correlation_id: uuid::Uuid, data: T) -> ResponseEnvelope<T> {
    ResponseEnvelope::Success {
        protocol_version: PROTOCOL_VERSION,
        correlation_id,
        data,
    }
}

fn failure<T>(
    correlation_id: uuid::Uuid,
    code: ErrorCode,
    message: String,
    retryable: bool,
) -> ResponseEnvelope<T> {
    ResponseEnvelope::Failure {
        protocol_version: PROTOCOL_VERSION,
        correlation_id,
        error: PlanktonError {
            code,
            user_message: message,
            internal_message: None,
            public_context: BTreeMap::new(),
            internal_context: BTreeMap::new(),
            severity: ErrorSeverity::Error,
            retryable,
            timestamp: Utc::now(),
            correlation_id,
            source: ErrorSource::Daemon,
        }
        .ai_safe(),
    }
}

#[cfg(test)]
mod tests {
    use std::{collections::BTreeMap, path::PathBuf};

    use chrono::Utc;
    use plankton_core::{
        load_settings, AccessRequest, ApprovalStatus, ImportedSecretReference, LocalSecretCatalog,
        PolicyMode, RequestContext, SecretSourceLocator,
    };
    use plankton_protocol::resources::{
        ResourceAccessCallChainNode, ResourceAccessRequest, ResourceAccessState,
    };
    use plankton_store::SqliteStore;

    use super::{
        catalog_documents, catalog_generation_for_path, ensure_resource_index_hierarchy,
        imported_document, provider_neutral_field_key, resource_access_context,
        same_resource_documents, submission_response_from_request, BackendKind,
    };

    #[test]
    fn approved_submission_stays_pending_until_value_resolution() {
        let context = RequestContext::new(
            "plankton://field/test/value".into(),
            "read a test value".into(),
            "integration-test".into(),
        );
        let mut request = AccessRequest::new_pending(
            context,
            PolicyMode::LlmAutomatic,
            None,
            String::new(),
            None,
            None,
        );

        assert_eq!(
            submission_response_from_request(&request).state,
            ResourceAccessState::Pending
        );

        request.approval_status = ApprovalStatus::Approved;
        assert_eq!(
            submission_response_from_request(&request).state,
            ResourceAccessState::Pending
        );

        request.approval_status = ApprovalStatus::Rejected;
        assert_eq!(
            submission_response_from_request(&request).state,
            ResourceAccessState::Denied
        );
    }

    #[test]
    fn structured_resource_access_context_reaches_the_provider_shape() {
        let request = ResourceAccessRequest {
            resource_id: "plankton://field/example".into(),
            reason: "read one trace".into(),
            requested_by: "integration-test".into(),
            script_path: Some("/workspace/analyze.sh".into()),
            call_chain_details: vec![ResourceAccessCallChainNode {
                pid: Some(42),
                ppid: Some(1),
                process_name: Some("zsh".into()),
                executable_path: Some("/bin/zsh".into()),
                argv: vec!["/workspace/analyze.sh".into(), "--trace-id".into()],
                resolved_file_path: Some("/workspace/analyze.sh".into()),
            }],
            call_chain: vec!["legacy path must not override details".into()],
            metadata: BTreeMap::from([("operation".into(), "trace.read".into())]),
        };

        let context = resource_access_context(
            request,
            vec!["observability".into()],
            BTreeMap::from([("scope".into(), "read-only".into())]),
        );

        assert_eq!(
            context.script_path.as_deref(),
            Some("/workspace/analyze.sh")
        );
        assert_eq!(context.call_chain.len(), 1);
        assert_eq!(context.call_chain[0].pid, Some(42));
        assert_eq!(context.call_chain[0].argv[1], "--trace-id");
        assert_eq!(
            context
                .metadata
                .get("call_chain_summary")
                .map(String::as_str),
            Some("/workspace/analyze.sh")
        );
        assert_eq!(
            context.metadata.get("operation").map(String::as_str),
            Some("trace.read")
        );

        let provider_context = plankton_core::build_prompt_context(&context);
        assert_eq!(
            provider_context.script_path.as_deref(),
            Some("/workspace/analyze.sh")
        );
        assert_eq!(provider_context.call_chain_details.len(), 1);
        assert_eq!(
            provider_context.call_chain_details[0]
                .arguments
                .iter()
                .map(|argument| argument.text.as_str())
                .collect::<Vec<_>>(),
            ["/workspace/analyze.sh", "--trace-id"]
        );
    }

    #[test]
    fn legacy_resource_access_context_promotes_script_and_paths() {
        let request = ResourceAccessRequest {
            resource_id: "plankton://field/example".into(),
            reason: "legacy read".into(),
            requested_by: "integration-test".into(),
            script_path: None,
            call_chain_details: Vec::new(),
            call_chain: vec!["/bin/zsh".into(), "/workspace/legacy.sh".into()],
            metadata: BTreeMap::from([("script_path".into(), "/workspace/legacy.sh".into())]),
        };

        let context = resource_access_context(request, Vec::new(), BTreeMap::new());

        assert_eq!(context.script_path.as_deref(), Some("/workspace/legacy.sh"));
        assert_eq!(context.call_chain.len(), 2);
        assert_eq!(
            context.call_chain[1].resolved_file_path.as_deref(),
            Some("/workspace/legacy.sh")
        );
        assert!(!context.metadata.contains_key("script_path"));
        assert_eq!(
            context
                .metadata
                .get("call_chain_summary")
                .map(String::as_str),
            Some("/bin/zsh -> /workspace/legacy.sh")
        );
    }

    fn reference(resource: &str, source_locator: SecretSourceLocator) -> ImportedSecretReference {
        ImportedSecretReference {
            resource: resource.to_string(),
            display_name: resource.to_string(),
            description: Some("searchable note".into()),
            tags: vec!["tag".into()],
            metadata: BTreeMap::new(),
            value: None,
            source_locator,
            imported_at: Utc::now(),
            last_verified_at: None,
        }
    }

    #[test]
    fn excludes_optional_password_backends_until_enabled() {
        let onepassword = reference(
            "plankton://field/op/password",
            SecretSourceLocator::OnePasswordCli {
                account: "account".into(),
                account_id: None,
                vault: "vault".into(),
                item: "item".into(),
                field: "password".into(),
                vault_id: None,
                item_id: None,
                field_id: None,
            },
        );
        let local = reference(
            "plankton://field/local/password",
            SecretSourceLocator::KeepassxcCli {
                database: PathBuf::from("local.kdbx"),
                entry: "item".into(),
                field: "password".into(),
                unlock_secret_file: PathBuf::from(".unlock"),
                executable: PathBuf::from("keepassxc-cli"),
                executable_sha256: "digest".into(),
            },
        );
        let catalog = LocalSecretCatalog {
            catalog_path: PathBuf::from("catalog.toml"),
            literals: Vec::new(),
            imports: vec![onepassword.clone(), local.clone()],
        };

        let disabled = catalog_documents(catalog.clone(), &[]);
        assert_eq!(disabled.len(), 1);
        assert_eq!(disabled[0].resource_id, local.resource);

        let enabled = catalog_documents(catalog, &[BackendKind::OnePassword]);
        assert_eq!(enabled.len(), 2);
        assert!(enabled
            .iter()
            .any(|document| document.resource_id == onepassword.resource));
    }

    #[test]
    fn imported_search_fields_use_provider_neutral_resource_keys() {
        let public_key = reference(
            "plankton://field/langfuse/LANGFUSE_PUBLIC_KEY",
            SecretSourceLocator::KeepassxcCli {
                database: PathBuf::from("local.kdbx"),
                entry: "Langfuse".into(),
                field: "password".into(),
                unlock_secret_file: PathBuf::from(".unlock"),
                executable: PathBuf::from("keepassxc-cli"),
                executable_sha256: "digest".into(),
            },
        );
        let secret_key = reference(
            "plankton://field/langfuse/LANGFUSE_SECRET_KEY",
            SecretSourceLocator::KeepassxcCli {
                database: PathBuf::from("local.kdbx"),
                entry: "Langfuse".into(),
                field: "password".into(),
                unlock_secret_file: PathBuf::from(".unlock"),
                executable: PathBuf::from("keepassxc-cli"),
                executable_sha256: "digest".into(),
            },
        );

        assert_eq!(
            imported_document(public_key).field_key,
            "LANGFUSE_PUBLIC_KEY"
        );
        assert_eq!(
            imported_document(secret_key).field_key,
            "LANGFUSE_SECRET_KEY"
        );
    }

    #[tokio::test]
    async fn imported_password_fields_with_shared_upstream_names_build_the_search_index() {
        let public_key = reference(
            "plankton://field/langfuse/LANGFUSE_PUBLIC_KEY",
            SecretSourceLocator::KeepassxcCli {
                database: PathBuf::from("local.kdbx"),
                entry: "Langfuse public key".into(),
                field: "password".into(),
                unlock_secret_file: PathBuf::from(".unlock"),
                executable: PathBuf::from("keepassxc-cli"),
                executable_sha256: "digest".into(),
            },
        );
        let secret_key = reference(
            "plankton://field/langfuse/LANGFUSE_SECRET_KEY",
            SecretSourceLocator::KeepassxcCli {
                database: PathBuf::from("local.kdbx"),
                entry: "Langfuse secret key".into(),
                field: "password".into(),
                unlock_secret_file: PathBuf::from(".unlock"),
                executable: PathBuf::from("keepassxc-cli"),
                executable_sha256: "digest".into(),
            },
        );
        let documents = catalog_documents(
            LocalSecretCatalog {
                catalog_path: PathBuf::from("catalog.toml"),
                literals: Vec::new(),
                imports: vec![public_key, secret_key],
            },
            &[],
        );
        let directory = tempfile::tempdir().expect("temporary directory");
        let mut settings = load_settings().expect("settings");
        settings.database_url = format!("sqlite://{}", directory.path().join("store.db").display());
        let store = SqliteStore::new(&settings).await.expect("store");

        ensure_resource_index_hierarchy(&store, &documents)
            .await
            .expect("resource hierarchy");
        store
            .replace_resource_search_index(1, &documents)
            .await
            .expect("provider-neutral field keys must remain unique within the item");
    }

    #[test]
    fn explicit_search_field_key_overrides_the_resource_key() {
        assert_eq!(
            provider_neutral_field_key(
                "plankton://field/item/resource-key",
                &BTreeMap::from([("field_key".into(), "explicit-key".into())]),
            ),
            "explicit-key"
        );
    }

    #[test]
    fn missing_catalog_has_a_stable_zero_generation() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let generation = catalog_generation_for_path(&directory.path().join("missing.toml"))
            .expect("a missing catalog is the valid empty-catalog state");

        assert_eq!(generation, 0);
    }

    #[test]
    fn search_index_documents_keep_provider_vaults_separate() {
        let first = imported_document(reference(
            "plankton://field/first/password",
            SecretSourceLocator::OnePasswordCli {
                account: "account".into(),
                account_id: None,
                vault: "Private".into(),
                item: "first".into(),
                field: "password".into(),
                vault_id: None,
                item_id: None,
                field_id: None,
            },
        ));
        let second = imported_document(reference(
            "plankton://field/second/password",
            SecretSourceLocator::OnePasswordCli {
                account: "account".into(),
                account_id: None,
                vault: "Shared".into(),
                item: "second".into(),
                field: "password".into(),
                vault_id: None,
                item_id: None,
                field_id: None,
            },
        ));

        assert_eq!(first.backend_binding_id, "onepassword");
        assert_eq!(second.backend_binding_id, "onepassword");
        assert_ne!(first.backend_vault_id, second.backend_vault_id);
        assert!(same_resource_documents(
            &[first.clone(), second.clone()],
            &[second, first],
        ));
    }
}
