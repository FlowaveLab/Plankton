use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
};

use chrono::Utc;
use plankton_protocol::password_changes::{
    PasswordCatalogMetadata, PasswordChangeDiff, PasswordChangeDiffEntry, PasswordChangeImpact,
    PasswordChangeOperation, PasswordFieldSummary, PasswordItemDiff, PasswordItemSummary,
};
use sha2::{Digest, Sha256};

use crate::exposure::{
    exposure_policy_from_metadata, inherits_exposure_policy, item_exposure_policy_from_metadata,
    store_exposure_policy, store_item_exposure_policy, EXPOSURE_INHERITANCE_METADATA_KEY,
    EXPOSURE_POLICY_METADATA_KEY, ITEM_EXPOSURE_POLICY_METADATA_KEY,
};
use crate::value_resolver::{
    load_secret_catalog_file_optional, local_secret_catalog_path,
    resolve_imported_reference_with_default_programs, sanitize_metadata, sanitize_optional_text,
    sanitize_tags, save_secret_catalog_file, ImportedSecretReference,
    LocalSecretLiteralMetadataRecord, SecretCatalogFile, SecretImportError, SecretSourceLocator,
};

pub fn password_catalog_metadata() -> Result<PasswordCatalogMetadata, SecretImportError> {
    password_catalog_metadata_at(local_secret_catalog_path().as_path())
}

pub fn preview_password_changes(
    operations: &[PasswordChangeOperation],
) -> Result<(String, PasswordChangeDiff), SecretImportError> {
    preview_password_changes_at(local_secret_catalog_path().as_path(), operations)
}

pub fn apply_password_changes(
    operations: &[PasswordChangeOperation],
    expected_revision: &str,
) -> Result<PasswordChangeDiff, SecretImportError> {
    apply_password_changes_at(
        local_secret_catalog_path().as_path(),
        operations,
        expected_revision,
    )
}

pub fn password_catalog_metadata_at(
    path: &Path,
) -> Result<PasswordCatalogMetadata, SecretImportError> {
    let catalog = load_secret_catalog_file_optional(path)?;
    Ok(project_password_catalog(&catalog))
}

pub fn preview_password_changes_at(
    path: &Path,
    operations: &[PasswordChangeOperation],
) -> Result<(String, PasswordChangeDiff), SecretImportError> {
    let catalog = load_secret_catalog_file_optional(path)?;
    let base_revision = catalog_revision(&catalog);
    let before = project_password_catalog(&catalog);
    let mut proposed = catalog;
    apply_operations(&mut proposed, operations, false)?;
    let after = project_password_catalog(&proposed);
    Ok((base_revision, catalog_diff(&before, &after, operations)))
}

pub fn apply_password_changes_at(
    path: &Path,
    operations: &[PasswordChangeOperation],
    expected_revision: &str,
) -> Result<PasswordChangeDiff, SecretImportError> {
    let mut catalog = load_secret_catalog_file_optional(path)?;
    let before = project_password_catalog(&catalog);
    let actual_revision = catalog_revision(&catalog);
    if actual_revision != expected_revision {
        return Err(SecretImportError::CatalogConflict {
            expected: expected_revision.to_string(),
            actual: actual_revision,
        });
    }
    apply_operations(&mut catalog, operations, true)?;
    let after = project_password_catalog(&catalog);
    let diff = catalog_diff(&before, &after, operations);
    save_secret_catalog_file(path, &catalog)?;
    Ok(diff)
}

fn project_password_catalog(catalog: &SecretCatalogFile) -> PasswordCatalogMetadata {
    let record_ids_by_item = explicit_record_ids_by_item(catalog);
    let literal_metadata = catalog
        .literal_entries
        .iter()
        .map(|entry| (entry.resource.as_str(), entry))
        .collect::<BTreeMap<_, _>>();
    let mut items = BTreeMap::<String, PasswordItemSummary>::new();

    for (resource, value) in catalog.secrets.iter().chain(&catalog.values) {
        let entry = literal_metadata.get(resource.as_str()).copied();
        let metadata = entry
            .map(|entry| entry.metadata.clone())
            .unwrap_or_default();
        let item_id = metadata
            .get("item_id")
            .cloned()
            .unwrap_or_else(|| resource.clone());
        let record_id = projected_record_id(
            &metadata,
            &format!("literal:{resource}"),
            &record_ids_by_item,
        );
        let title = metadata
            .get("item_title")
            .cloned()
            .or_else(|| entry.and_then(|entry| entry.display_name.clone()))
            .unwrap_or_else(|| item_id.clone());
        let label = metadata
            .get("field_label")
            .cloned()
            .unwrap_or_else(|| resource.rsplit('/').next().unwrap_or(resource).to_string());
        let item = items
            .entry(record_id.clone())
            .or_insert_with(|| PasswordItemSummary {
                record_id,
                item_id,
                title,
                description: entry.and_then(|entry| entry.description.clone()),
                tags: entry.map(|entry| entry.tags.clone()).unwrap_or_default(),
                metadata: public_metadata(&metadata),
                default_exposure_policy: item_exposure_policy_from_metadata(&metadata),
                fields: Vec::new(),
            });
        item.fields.push(PasswordFieldSummary {
            resource_id: resource.clone(),
            label,
            provider_kind: "local".to_string(),
            vault: metadata
                .get("vault")
                .cloned()
                .or_else(|| Some("Plankton".to_string())),
            has_value: !value.is_empty(),
            exposure_policy: exposure_policy_from_metadata(&metadata),
            inherits_exposure_policy: inherits_exposure_policy(&metadata),
        });
    }

    for reference in &catalog.imports {
        let identity = ImportedIdentity::from_reference(reference);
        let item_id = reference
            .metadata
            .get("item_id")
            .cloned()
            .unwrap_or(identity.item_id);
        let record_id =
            projected_record_id(&reference.metadata, &identity.seed, &record_ids_by_item);
        let title = reference
            .metadata
            .get("item_title")
            .cloned()
            .unwrap_or(identity.title);
        let label = reference
            .metadata
            .get("field_label")
            .cloned()
            .unwrap_or_else(|| reference.source_locator.field_selector().to_string());
        let item = items
            .entry(record_id.clone())
            .or_insert_with(|| PasswordItemSummary {
                record_id,
                item_id,
                title,
                description: reference.description.clone(),
                tags: reference.tags.clone(),
                metadata: public_metadata(&reference.metadata),
                default_exposure_policy: item_exposure_policy_from_metadata(&reference.metadata),
                fields: Vec::new(),
            });
        item.fields.push(PasswordFieldSummary {
            resource_id: reference.resource.clone(),
            label,
            provider_kind: reference.provider_kind().to_string(),
            vault: reference.metadata.get("vault").cloned().or(identity.vault),
            has_value: reference
                .value
                .as_deref()
                .is_some_and(|value| !value.is_empty()),
            exposure_policy: exposure_policy_from_metadata(&reference.metadata),
            inherits_exposure_policy: inherits_exposure_policy(&reference.metadata),
        });
    }

    let mut items = items.into_values().collect::<Vec<_>>();
    for item in &mut items {
        item.tags = sanitize_tags(std::mem::take(&mut item.tags));
        item.fields
            .sort_by(|left, right| left.resource_id.cmp(&right.resource_id));
    }
    items.sort_by(|left, right| left.item_id.cmp(&right.item_id));
    PasswordCatalogMetadata {
        revision: metadata_revision(&items),
        items,
    }
}

struct ImportedIdentity {
    seed: String,
    item_id: String,
    title: String,
    vault: Option<String>,
}

impl ImportedIdentity {
    fn from_reference(reference: &ImportedSecretReference) -> Self {
        match &reference.source_locator {
            SecretSourceLocator::KeepassxcCli {
                database, entry, ..
            } => Self {
                seed: format!("keepass:{}:{entry}", database.display()),
                item_id: entry.clone(),
                title: entry.clone(),
                vault: database
                    .file_stem()
                    .and_then(|value| value.to_str())
                    .map(ToOwned::to_owned)
                    .or_else(|| Some("Plankton".to_string())),
            },
            SecretSourceLocator::OnePasswordCli {
                account,
                vault,
                item,
                vault_id,
                item_id,
                ..
            } => Self {
                seed: format!(
                    "1password:{account}:{}:{}",
                    vault_id.as_deref().unwrap_or(vault),
                    item_id.as_deref().unwrap_or(item)
                ),
                item_id: item.clone(),
                title: item.clone(),
                vault: Some(vault.clone()),
            },
            SecretSourceLocator::BitwardenCli {
                account,
                organization,
                collection,
                folder,
                item,
                item_id,
                ..
            } => {
                let container = collection
                    .as_deref()
                    .or(folder.as_deref())
                    .or(organization.as_deref())
                    .unwrap_or(account);
                Self {
                    seed: format!(
                        "bitwarden:{account}:{container}:{}",
                        item_id.as_deref().unwrap_or(item)
                    ),
                    item_id: item.clone(),
                    title: item.clone(),
                    vault: Some(container.to_string()),
                }
            }
            SecretSourceLocator::DotenvFile {
                file_path,
                namespace,
                prefix,
                ..
            } => {
                let fallback = namespace
                    .as_deref()
                    .or(prefix.as_deref())
                    .map(ToOwned::to_owned)
                    .or_else(|| {
                        let file_name = file_path.file_name()?.to_str()?;
                        if file_name == ".env" {
                            file_path
                                .parent()?
                                .file_name()?
                                .to_str()
                                .map(|parent| format!("{parent} environment"))
                        } else {
                            Some(file_name.to_string())
                        }
                    })
                    .unwrap_or_else(|| "environment".to_string());
                Self {
                    seed: format!(
                        "dotenv:{}:{}:{}",
                        file_path.display(),
                        namespace.as_deref().unwrap_or_default(),
                        prefix.as_deref().unwrap_or_default()
                    ),
                    item_id: fallback.clone(),
                    title: fallback,
                    vault: Some(".env".to_string()),
                }
            }
        }
    }
}

fn stable_record_id(seed: &str) -> String {
    let digest = Sha256::digest(seed.as_bytes());
    format!("item-{}", hex_bytes(&digest[..12]))
}

fn explicit_record_ids_by_item(catalog: &SecretCatalogFile) -> BTreeMap<String, BTreeSet<String>> {
    catalog
        .literal_entries
        .iter()
        .map(|entry| &entry.metadata)
        .chain(catalog.imports.iter().map(|reference| &reference.metadata))
        .filter_map(|metadata| {
            Some((
                metadata.get("item_id")?.clone(),
                metadata.get("record_id")?.clone(),
            ))
        })
        .fold(BTreeMap::new(), |mut record_ids, (item_id, record_id)| {
            record_ids
                .entry(item_id)
                .or_insert_with(BTreeSet::new)
                .insert(record_id);
            record_ids
        })
}

fn projected_record_id(
    metadata: &BTreeMap<String, String>,
    fallback_seed: &str,
    record_ids_by_item: &BTreeMap<String, BTreeSet<String>>,
) -> String {
    if let Some(item_id) = metadata.get("item_id") {
        match record_ids_by_item.get(item_id) {
            Some(record_ids) if record_ids.len() == 1 => {
                return record_ids
                    .first()
                    .expect("a single record id exists")
                    .clone();
            }
            None => return stable_record_id(&format!("logical-item:{item_id}")),
            Some(_) => return stable_record_id(&format!("logical-item:{item_id}")),
        }
    }
    metadata
        .get("record_id")
        .cloned()
        .unwrap_or_else(|| stable_record_id(fallback_seed))
}

fn metadata_revision(items: &[PasswordItemSummary]) -> String {
    let encoded = serde_json::to_vec(items).expect("password metadata projection serializes");
    hex_bytes(&Sha256::digest(encoded))
}

fn catalog_revision(catalog: &SecretCatalogFile) -> String {
    let encoded = serde_json::to_vec(catalog).expect("password catalog serializes for revision");
    hex_bytes(&Sha256::digest(encoded))
}

fn hex_bytes(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn public_metadata(metadata: &BTreeMap<String, String>) -> BTreeMap<String, String> {
    metadata
        .iter()
        .filter(|(key, _)| {
            !matches!(key.as_str(), "record_id" | "item_id" | "item_title")
                && ![
                    EXPOSURE_POLICY_METADATA_KEY,
                    ITEM_EXPOSURE_POLICY_METADATA_KEY,
                    EXPOSURE_INHERITANCE_METADATA_KEY,
                ]
                .contains(&key.as_str())
        })
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect()
}

fn matching_item<'a>(
    metadata: &'a PasswordCatalogMetadata,
    selector: &str,
) -> Result<&'a PasswordItemSummary, SecretImportError> {
    let matches = metadata
        .items
        .iter()
        .filter(|item| item.item_id == selector || item.record_id == selector)
        .collect::<Vec<_>>();
    match matches.as_slice() {
        [] => Err(SecretImportError::ItemNotFound {
            item_id: selector.to_string(),
        }),
        [item] => Ok(item),
        _ => Err(SecretImportError::AmbiguousItem {
            item_id: selector.to_string(),
        }),
    }
}

fn apply_operations(
    catalog: &mut SecretCatalogFile,
    operations: &[PasswordChangeOperation],
    resolve_refreshes: bool,
) -> Result<(), SecretImportError> {
    for operation in operations {
        match operation {
            PasswordChangeOperation::UpdateItem {
                item_id,
                next_item_id,
                title,
                description,
                clear_description,
                tags,
                metadata,
            } => {
                let projection = project_password_catalog(catalog);
                let item = matching_item(&projection, item_id)?.clone();
                update_item(
                    catalog,
                    &item,
                    ItemUpdate {
                        next_item_id: next_item_id.as_deref(),
                        title: title.as_deref(),
                        description: description.as_deref(),
                        clear_description: *clear_description,
                        tags: tags.as_ref(),
                        metadata: metadata.as_ref(),
                    },
                );
            }
            PasswordChangeOperation::RenameResource {
                resource_id,
                next_resource_id,
            } => rename_resource(catalog, resource_id, next_resource_id)?,
            PasswordChangeOperation::RenameFieldLabel { resource_id, label } => {
                rename_field_label(catalog, resource_id, label)?
            }
            PasswordChangeOperation::SetItemExposurePolicy { item_id, policy } => {
                let projection = project_password_catalog(catalog);
                let item = matching_item(&projection, item_id)?.clone();
                for field in &item.fields {
                    let metadata = field_metadata_mut(catalog, &field.resource_id)?;
                    store_item_exposure_policy(
                        metadata,
                        policy,
                        (!field.inherits_exposure_policy).then_some(&field.exposure_policy),
                    )
                    .map_err(invalid_exposure_policy)?;
                }
            }
            PasswordChangeOperation::InheritFieldExposurePolicy { resource_id } => {
                let projection = project_password_catalog(catalog);
                let (item, _) = field_owner(&projection, resource_id)?;
                let policy = item.default_exposure_policy.clone();
                store_item_exposure_policy(
                    field_metadata_mut(catalog, resource_id)?,
                    &policy,
                    None,
                )
                .map_err(invalid_exposure_policy)?;
            }
            PasswordChangeOperation::SetFieldExposurePolicy {
                resource_id,
                policy,
            } => set_field_exposure_policy(catalog, resource_id, policy)?,
            PasswordChangeOperation::UpdateField {
                resource_id,
                label,
                exposure_policy,
            } => {
                if label.is_none() && exposure_policy.is_none() {
                    return Err(SecretImportError::InvalidExposurePolicy {
                        message: "field update must include a label or exposure policy".into(),
                    });
                }
                if let Some(label) = label {
                    rename_field_label(catalog, resource_id, label)?;
                }
                if let Some(policy) = exposure_policy {
                    set_field_exposure_policy(catalog, resource_id, policy)?;
                }
            }
            PasswordChangeOperation::MoveField {
                resource_id,
                target_item_id,
                target_title,
            } => move_field(
                catalog,
                resource_id,
                target_item_id,
                target_title.as_deref(),
            )?,
            PasswordChangeOperation::MergeItems {
                source_item_id,
                target_item_id,
            } => merge_items(catalog, source_item_id, target_item_id)?,
            PasswordChangeOperation::DeleteField { resource_id } => {
                delete_field(catalog, resource_id)?
            }
            PasswordChangeOperation::DeleteDuplicateField {
                resource_id,
                canonical_resource_id,
            } => delete_duplicate_field(catalog, resource_id, canonical_resource_id)?,
            PasswordChangeOperation::RefreshItem { item_id } => {
                let projection = project_password_catalog(catalog);
                let item = matching_item(&projection, item_id)?.clone();
                if resolve_refreshes {
                    refresh_item(catalog, &item)?;
                }
            }
            PasswordChangeOperation::DeleteItem { item_id } => {
                let projection = project_password_catalog(catalog);
                let item = matching_item(&projection, item_id)?.clone();
                delete_item(catalog, &item);
            }
        }
    }
    Ok(())
}

struct ItemUpdate<'a> {
    next_item_id: Option<&'a str>,
    title: Option<&'a str>,
    description: Option<&'a str>,
    clear_description: bool,
    tags: Option<&'a Vec<String>>,
    metadata: Option<&'a BTreeMap<String, String>>,
}

fn update_item(
    catalog: &mut SecretCatalogFile,
    item: &PasswordItemSummary,
    update: ItemUpdate<'_>,
) {
    let resources = resource_set(item);
    for reference in &mut catalog.imports {
        if !resources.contains(reference.resource.as_str()) {
            continue;
        }
        apply_metadata_update(
            &mut reference.metadata,
            item,
            update.next_item_id,
            update.title,
            update.metadata,
        );
        apply_common_update(
            &mut reference.description,
            &mut reference.tags,
            update.description,
            update.clear_description,
            update.tags,
        );
    }
    for resource in &resources {
        if !catalog.secrets.contains_key(*resource) && !catalog.values.contains_key(*resource) {
            continue;
        }
        let entry = literal_metadata_mut(&mut catalog.literal_entries, resource);
        apply_metadata_update(
            &mut entry.metadata,
            item,
            update.next_item_id,
            update.title,
            update.metadata,
        );
        if let Some(title) = update
            .title
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            entry.display_name = Some(title.to_string());
        }
        apply_common_update(
            &mut entry.description,
            &mut entry.tags,
            update.description,
            update.clear_description,
            update.tags,
        );
    }
}

fn apply_metadata_update(
    target: &mut BTreeMap<String, String>,
    item: &PasswordItemSummary,
    next_item_id: Option<&str>,
    title: Option<&str>,
    replacement: Option<&BTreeMap<String, String>>,
) {
    if let Some(replacement) = replacement {
        let policy_metadata = target
            .iter()
            .filter(|(key, _)| {
                [
                    EXPOSURE_POLICY_METADATA_KEY,
                    ITEM_EXPOSURE_POLICY_METADATA_KEY,
                    EXPOSURE_INHERITANCE_METADATA_KEY,
                ]
                .contains(&key.as_str())
            })
            .map(|(key, value)| (key.clone(), value.clone()))
            .collect::<BTreeMap<_, _>>();
        *target = sanitize_metadata(replacement.clone());
        for key in [
            EXPOSURE_POLICY_METADATA_KEY,
            ITEM_EXPOSURE_POLICY_METADATA_KEY,
            EXPOSURE_INHERITANCE_METADATA_KEY,
        ] {
            target.remove(key);
        }
        target.extend(policy_metadata);
    }
    target.insert("record_id".to_string(), item.record_id.clone());
    if let Some(value) = next_item_id
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        target.insert("item_id".to_string(), value.to_string());
    }
    if let Some(value) = title.map(str::trim).filter(|value| !value.is_empty()) {
        target.insert("item_title".to_string(), value.to_string());
    }
}

fn apply_common_update(
    description_target: &mut Option<String>,
    tags_target: &mut Vec<String>,
    description: Option<&str>,
    clear_description: bool,
    tags: Option<&Vec<String>>,
) {
    if clear_description {
        *description_target = None;
    } else if let Some(description) = description {
        *description_target = sanitize_optional_text(Some(description.to_string()));
    }
    if let Some(tags) = tags {
        *tags_target = sanitize_tags(tags.clone());
    }
}

fn literal_metadata_mut<'a>(
    entries: &'a mut Vec<LocalSecretLiteralMetadataRecord>,
    resource: &str,
) -> &'a mut LocalSecretLiteralMetadataRecord {
    if let Some(index) = entries.iter().position(|entry| entry.resource == resource) {
        return &mut entries[index];
    }
    entries.push(LocalSecretLiteralMetadataRecord {
        resource: resource.to_string(),
        display_name: None,
        description: None,
        tags: Vec::new(),
        metadata: BTreeMap::new(),
    });
    entries.last_mut().expect("literal metadata was appended")
}

#[derive(Debug, Clone)]
struct FieldTarget {
    record_id: String,
    item_id: String,
    title: String,
    field_labels: BTreeSet<String>,
    default_exposure_policy: Option<plankton_protocol::exposure::CredentialExposurePolicy>,
}

fn field_target(
    projection: &PasswordCatalogMetadata,
    selector: &str,
    title_for_new_item: Option<&str>,
) -> Result<FieldTarget, SecretImportError> {
    let selector = selector.trim();
    if selector.is_empty() {
        return Err(SecretImportError::ItemNotFound {
            item_id: selector.to_string(),
        });
    }
    let matches = projection
        .items
        .iter()
        .filter(|item| item.item_id == selector || item.record_id == selector)
        .collect::<Vec<_>>();
    match matches.as_slice() {
        [item] => Ok(FieldTarget {
            record_id: item.record_id.clone(),
            item_id: item.item_id.clone(),
            title: item.title.clone(),
            default_exposure_policy: Some(item.default_exposure_policy.clone()),
            field_labels: item
                .fields
                .iter()
                .map(|field| field.label.clone())
                .collect(),
        }),
        [] => {
            let title = title_for_new_item
                .map(str::trim)
                .filter(|title| !title.is_empty())
                .ok_or_else(|| SecretImportError::MissingTargetItemTitle {
                    item_id: selector.to_string(),
                })?;
            Ok(FieldTarget {
                record_id: stable_record_id(&format!("logical-item:{selector}")),
                item_id: selector.to_string(),
                title: title.to_string(),
                field_labels: BTreeSet::new(),
                default_exposure_policy: None,
            })
        }
        _ => Err(SecretImportError::AmbiguousItem {
            item_id: selector.to_string(),
        }),
    }
}

fn field_owner<'a>(
    projection: &'a PasswordCatalogMetadata,
    resource: &str,
) -> Result<(&'a PasswordItemSummary, &'a PasswordFieldSummary), SecretImportError> {
    let mut matches = projection.items.iter().flat_map(|item| {
        item.fields
            .iter()
            .filter(move |field| field.resource_id == resource)
            .map(move |field| (item, field))
    });
    let Some(owner) = matches.next() else {
        return Err(SecretImportError::ResourceNotFound {
            resource: resource.to_string(),
        });
    };
    if matches.next().is_some() {
        return Err(SecretImportError::DuplicateResource {
            resource: resource.to_string(),
        });
    }
    Ok(owner)
}

fn assign_field_to_target(
    catalog: &mut SecretCatalogFile,
    resource: &str,
    target: &FieldTarget,
) -> Result<(), SecretImportError> {
    let metadata = field_metadata_mut(catalog, resource)?;
    let effective = exposure_policy_from_metadata(metadata);
    let default_policy = target
        .default_exposure_policy
        .clone()
        .unwrap_or_else(|| item_exposure_policy_from_metadata(metadata));
    let custom = !inherits_exposure_policy(metadata) || effective != default_policy;
    store_item_exposure_policy(metadata, &default_policy, custom.then_some(&effective))
        .map_err(invalid_exposure_policy)?;
    if let Some(reference) = catalog
        .imports
        .iter_mut()
        .find(|reference| reference.resource == resource)
    {
        reference
            .metadata
            .insert("record_id".to_string(), target.record_id.clone());
        reference
            .metadata
            .insert("item_id".to_string(), target.item_id.clone());
        reference
            .metadata
            .insert("item_title".to_string(), target.title.clone());
        return Ok(());
    }
    if catalog.secrets.contains_key(resource) || catalog.values.contains_key(resource) {
        let entry = literal_metadata_mut(&mut catalog.literal_entries, resource);
        entry
            .metadata
            .insert("record_id".to_string(), target.record_id.clone());
        entry
            .metadata
            .insert("item_id".to_string(), target.item_id.clone());
        entry
            .metadata
            .insert("item_title".to_string(), target.title.clone());
        return Ok(());
    }
    Err(SecretImportError::ResourceNotFound {
        resource: resource.to_string(),
    })
}

fn move_field(
    catalog: &mut SecretCatalogFile,
    resource: &str,
    target_item_id: &str,
    target_title: Option<&str>,
) -> Result<(), SecretImportError> {
    let projection = project_password_catalog(catalog);
    let (owner, field) = field_owner(&projection, resource)?;
    let target = field_target(&projection, target_item_id, target_title)?;
    if owner.record_id == target.record_id {
        return Ok(());
    }
    if target.field_labels.contains(&field.label) {
        return Err(SecretImportError::DuplicateFieldLabel {
            item_id: target.item_id,
            label: field.label.clone(),
        });
    }
    assign_field_to_target(catalog, resource, &target)
}

fn merge_items(
    catalog: &mut SecretCatalogFile,
    source_item_id: &str,
    target_item_id: &str,
) -> Result<(), SecretImportError> {
    let projection = project_password_catalog(catalog);
    let source = matching_item(&projection, source_item_id)?.clone();
    let target = field_target(&projection, target_item_id, None)?;
    if source.record_id == target.record_id {
        return Ok(());
    }
    for field in &source.fields {
        if target.field_labels.contains(&field.label) {
            return Err(SecretImportError::DuplicateFieldLabel {
                item_id: target.item_id.clone(),
                label: field.label.clone(),
            });
        }
    }
    for field in source.fields {
        assign_field_to_target(catalog, &field.resource_id, &target)?;
    }
    Ok(())
}

fn comparable_value(
    catalog: &SecretCatalogFile,
    resource: &str,
) -> Result<String, SecretImportError> {
    if let Some(value) = catalog
        .secrets
        .get(resource)
        .or_else(|| catalog.values.get(resource))
        .filter(|value| !value.is_empty())
    {
        return Ok(value.clone());
    }
    if let Some(reference) = catalog
        .imports
        .iter()
        .find(|reference| reference.resource == resource)
    {
        if let Some(value) = reference.value.as_deref().filter(|value| !value.is_empty()) {
            return Ok(value.to_string());
        }
        return resolve_imported_reference_with_default_programs(reference, resource).map_err(
            |source| SecretImportError::VerifySource {
                resource: resource.to_string(),
                source,
            },
        );
    }
    Err(SecretImportError::ResourceNotFound {
        resource: resource.to_string(),
    })
}

fn delete_duplicate_field(
    catalog: &mut SecretCatalogFile,
    resource: &str,
    canonical_resource: &str,
) -> Result<(), SecretImportError> {
    if resource == canonical_resource {
        return Err(SecretImportError::DuplicateFieldMismatch {
            resource: resource.to_string(),
            canonical_resource: canonical_resource.to_string(),
        });
    }
    let duplicate_value = comparable_value(catalog, resource)?;
    let canonical_value = comparable_value(catalog, canonical_resource)?;
    let duplicate_digest = Sha256::digest(duplicate_value.as_bytes());
    let canonical_digest = Sha256::digest(canonical_value.as_bytes());
    if duplicate_digest != canonical_digest {
        return Err(SecretImportError::DuplicateFieldMismatch {
            resource: resource.to_string(),
            canonical_resource: canonical_resource.to_string(),
        });
    }
    delete_field_record(catalog, resource);
    Ok(())
}

fn delete_field_record(catalog: &mut SecretCatalogFile, resource: &str) {
    catalog.secrets.remove(resource);
    catalog.values.remove(resource);
    catalog
        .imports
        .retain(|reference| reference.resource != resource);
    catalog
        .literal_entries
        .retain(|entry| entry.resource != resource);
}

fn rename_resource(
    catalog: &mut SecretCatalogFile,
    resource: &str,
    next_resource: &str,
) -> Result<(), SecretImportError> {
    let resource = resource.trim();
    let next_resource = next_resource.trim();
    if resource.is_empty() || next_resource.is_empty() {
        return Err(SecretImportError::MissingResource);
    }
    if resource == next_resource {
        return Ok(());
    }
    if resource_exists(catalog, next_resource) {
        return Err(SecretImportError::DuplicateResource {
            resource: next_resource.to_string(),
        });
    }
    if !resource_exists(catalog, resource) {
        return Err(SecretImportError::ResourceNotFound {
            resource: resource.to_string(),
        });
    }
    if let Some(value) = catalog.secrets.remove(resource) {
        catalog.secrets.insert(next_resource.to_string(), value);
    }
    if let Some(value) = catalog.values.remove(resource) {
        catalog.values.insert(next_resource.to_string(), value);
    }
    for reference in &mut catalog.imports {
        if reference.resource == resource {
            reference.resource = next_resource.to_string();
        }
    }
    for entry in &mut catalog.literal_entries {
        if entry.resource == resource {
            entry.resource = next_resource.to_string();
        }
    }
    Ok(())
}

fn rename_field_label(
    catalog: &mut SecretCatalogFile,
    resource: &str,
    label: &str,
) -> Result<(), SecretImportError> {
    let resource = resource.trim();
    let label = label.trim();
    if resource.is_empty() || label.is_empty() {
        return Err(SecretImportError::MissingResource);
    }
    let projection = project_password_catalog(catalog);
    let (owner, field) = field_owner(&projection, resource)?;
    if field.label == label {
        return Ok(());
    }
    if owner
        .fields
        .iter()
        .any(|candidate| candidate.resource_id != resource && candidate.label == label)
    {
        return Err(SecretImportError::DuplicateFieldLabel {
            item_id: owner.item_id.clone(),
            label: label.to_string(),
        });
    }
    if let Some(reference) = catalog
        .imports
        .iter_mut()
        .find(|reference| reference.resource == resource)
    {
        reference
            .metadata
            .insert("field_label".to_string(), label.to_string());
        reference.display_name = format!("{}:{label}", owner.title);
        return Ok(());
    }
    if catalog.secrets.contains_key(resource) || catalog.values.contains_key(resource) {
        let entry = literal_metadata_mut(&mut catalog.literal_entries, resource);
        entry
            .metadata
            .insert("field_label".to_string(), label.to_string());
        return Ok(());
    }
    Err(SecretImportError::ResourceNotFound {
        resource: resource.to_string(),
    })
}

fn invalid_exposure_policy(
    error: plankton_protocol::exposure::ExposurePolicyError,
) -> SecretImportError {
    SecretImportError::InvalidExposurePolicy {
        message: error.to_string(),
    }
}

fn field_metadata_mut<'a>(
    catalog: &'a mut SecretCatalogFile,
    resource: &str,
) -> Result<&'a mut BTreeMap<String, String>, SecretImportError> {
    if resource.trim().is_empty() {
        return Err(SecretImportError::MissingResource);
    }
    if let Some(reference) = catalog
        .imports
        .iter_mut()
        .find(|reference| reference.resource == resource)
    {
        return Ok(&mut reference.metadata);
    }
    if catalog.secrets.contains_key(resource) || catalog.values.contains_key(resource) {
        return Ok(&mut literal_metadata_mut(&mut catalog.literal_entries, resource).metadata);
    }
    Err(SecretImportError::ResourceNotFound {
        resource: resource.into(),
    })
}

fn set_field_exposure_policy(
    catalog: &mut SecretCatalogFile,
    resource: &str,
    policy: &plankton_protocol::exposure::CredentialExposurePolicy,
) -> Result<(), SecretImportError> {
    let metadata = field_metadata_mut(catalog, resource.trim())?;
    store_exposure_policy(metadata, policy).map_err(invalid_exposure_policy)?;
    metadata.insert(EXPOSURE_INHERITANCE_METADATA_KEY.into(), "custom".into());
    Ok(())
}

fn resource_exists(catalog: &SecretCatalogFile, resource: &str) -> bool {
    catalog.secrets.contains_key(resource)
        || catalog.values.contains_key(resource)
        || catalog
            .imports
            .iter()
            .any(|reference| reference.resource == resource)
}

fn refresh_item(
    catalog: &mut SecretCatalogFile,
    item: &PasswordItemSummary,
) -> Result<(), SecretImportError> {
    let resources = resource_set(item);
    for reference in &mut catalog.imports {
        if !resources.contains(reference.resource.as_str()) {
            continue;
        }
        let value =
            resolve_imported_reference_with_default_programs(reference, &reference.resource)
                .map_err(|source| SecretImportError::VerifySource {
                    resource: reference.resource.clone(),
                    source,
                })?;
        reference.value = Some(value);
        reference.last_verified_at = Some(Utc::now());
    }
    Ok(())
}

fn delete_item(catalog: &mut SecretCatalogFile, item: &PasswordItemSummary) {
    for resource in resource_set(item) {
        delete_field_record(catalog, resource);
    }
}

fn delete_field(catalog: &mut SecretCatalogFile, resource: &str) -> Result<(), SecretImportError> {
    let resource = resource.trim();
    if resource.is_empty() {
        return Err(SecretImportError::MissingResource);
    }
    let projection = project_password_catalog(catalog);
    field_owner(&projection, resource)?;
    delete_field_record(catalog, resource);
    Ok(())
}

fn resource_set(item: &PasswordItemSummary) -> BTreeSet<&str> {
    item.fields
        .iter()
        .map(|field| field.resource_id.as_str())
        .collect()
}

fn catalog_diff(
    before: &PasswordCatalogMetadata,
    after: &PasswordCatalogMetadata,
    operations: &[PasswordChangeOperation],
) -> PasswordChangeDiff {
    let before_by_record = item_map(before);
    let after_by_record = item_map(after);
    let before_resources = catalog_resource_ids(before);
    let after_resources = catalog_resource_ids(after);
    let record_ids = before_by_record
        .keys()
        .chain(after_by_record.keys())
        .copied()
        .collect::<BTreeSet<_>>();
    let mut items = Vec::new();
    for record_id in record_ids {
        let before_item = before_by_record.get(record_id).copied();
        let after_item = after_by_record.get(record_id).copied();
        let mut entries =
            item_diff_entries(before_item, after_item, &before_resources, &after_resources);
        let item = after_item
            .or(before_item)
            .expect("record id came from an item");
        if operations.iter().any(|operation| {
            matches!(operation, PasswordChangeOperation::RefreshItem { item_id } if item_id == &item.item_id || item_id == &item.record_id)
        }) {
            entries.push(PasswordChangeDiffEntry {
                path: "/snapshot".to_string(),
                label: "Refresh local snapshot".to_string(),
                before: None,
                after: Some("Refresh from upstream after confirmation".to_string()),
                impact: PasswordChangeImpact::Refresh,
            });
        }
        if !entries.is_empty() {
            items.push(PasswordItemDiff {
                record_id: item.record_id.clone(),
                item_id: item.item_id.clone(),
                title: item.title.clone(),
                vaults: before_item
                    .into_iter()
                    .chain(after_item)
                    .flat_map(|item| item.fields.iter())
                    .filter_map(|field| field.vault.clone())
                    .collect::<BTreeSet<_>>()
                    .into_iter()
                    .collect(),
                entries,
            });
        }
    }
    items.sort_by(|left, right| left.item_id.cmp(&right.item_id));
    let changed_fields = items
        .iter()
        .flat_map(|item| &item.entries)
        .filter(|entry| entry.path.starts_with("/fields/"))
        .count() as u32;
    let breaking_changes = items
        .iter()
        .flat_map(|item| &item.entries)
        .filter(|entry| {
            matches!(
                entry.impact,
                PasswordChangeImpact::References
                    | PasswordChangeImpact::Locator
                    | PasswordChangeImpact::Delete
            )
        })
        .count() as u32;
    PasswordChangeDiff {
        changed_items: items.len() as u32,
        changed_fields,
        breaking_changes,
        items,
    }
}

fn item_map(metadata: &PasswordCatalogMetadata) -> BTreeMap<&str, &PasswordItemSummary> {
    metadata
        .items
        .iter()
        .map(|item| (item.record_id.as_str(), item))
        .collect()
}

fn catalog_resource_ids(metadata: &PasswordCatalogMetadata) -> BTreeSet<&str> {
    metadata
        .items
        .iter()
        .flat_map(|item| item.fields.iter())
        .map(|field| field.resource_id.as_str())
        .collect()
}

fn item_diff_entries(
    before: Option<&PasswordItemSummary>,
    after: Option<&PasswordItemSummary>,
    before_resources: &BTreeSet<&str>,
    after_resources: &BTreeSet<&str>,
) -> Vec<PasswordChangeDiffEntry> {
    let mut entries = Vec::new();
    match (before, after) {
        (Some(before), Some(after)) => {
            replacement(
                &mut entries,
                "/item_id",
                "Item ID",
                &before.item_id,
                &after.item_id,
                PasswordChangeImpact::References,
            );
            replacement(
                &mut entries,
                "/title",
                "Title",
                &before.title,
                &after.title,
                PasswordChangeImpact::Metadata,
            );
            optional_replacement(
                &mut entries,
                "/description",
                "Description",
                before.description.as_deref(),
                after.description.as_deref(),
                PasswordChangeImpact::Metadata,
            );
            replacement(
                &mut entries,
                "/tags",
                "Tags",
                &before.tags.join(", "),
                &after.tags.join(", "),
                PasswordChangeImpact::Metadata,
            );
            metadata_diffs(&mut entries, &before.metadata, &after.metadata);
            field_diffs(
                &mut entries,
                before,
                after,
                before_resources,
                after_resources,
            );
        }
        (Some(before), None) => {
            let fields_were_moved = before
                .fields
                .iter()
                .all(|field| after_resources.contains(field.resource_id.as_str()));
            entries.push(PasswordChangeDiffEntry {
                path: "/item".to_string(),
                label: if fields_were_moved {
                    "Remove empty item after merge".to_string()
                } else {
                    "Delete item".to_string()
                },
                before: Some(before.title.clone()),
                after: None,
                impact: if fields_were_moved {
                    PasswordChangeImpact::Metadata
                } else {
                    PasswordChangeImpact::Delete
                },
            });
        }
        (None, Some(after)) => {
            entries.push(PasswordChangeDiffEntry {
                path: "/item".to_string(),
                label: "Create item".to_string(),
                before: None,
                after: Some(after.title.clone()),
                impact: PasswordChangeImpact::Metadata,
            });
            for field in &after.fields {
                entries.push(PasswordChangeDiffEntry {
                    path: format!("/fields/{}/resource_id", field.label),
                    label: format!("{} resource key", field.label),
                    before: None,
                    after: Some(field.resource_id.clone()),
                    impact: PasswordChangeImpact::Metadata,
                });
            }
        }
        (None, None) => {}
    }
    entries
}

fn metadata_diffs(
    entries: &mut Vec<PasswordChangeDiffEntry>,
    before: &BTreeMap<String, String>,
    after: &BTreeMap<String, String>,
) {
    for key in before
        .keys()
        .chain(after.keys())
        .map(String::as_str)
        .collect::<BTreeSet<_>>()
    {
        optional_replacement(
            entries,
            &format!("/metadata/{key}"),
            &format!("Metadata: {key}"),
            before.get(key).map(String::as_str),
            after.get(key).map(String::as_str),
            PasswordChangeImpact::Metadata,
        );
    }
}

fn field_diffs(
    entries: &mut Vec<PasswordChangeDiffEntry>,
    before: &PasswordItemSummary,
    after: &PasswordItemSummary,
    before_resources: &BTreeSet<&str>,
    after_resources: &BTreeSet<&str>,
) {
    optional_replacement(
        entries,
        "/default_exposure_policy",
        "Collection default exposure profile",
        Some(&serde_json::to_string(&before.default_exposure_policy).expect("policy serializes")),
        Some(&serde_json::to_string(&after.default_exposure_policy).expect("policy serializes")),
        PasswordChangeImpact::ExposurePolicy,
    );
    let before_fields = before
        .fields
        .iter()
        .map(|field| (field.label.as_str(), field.resource_id.as_str()))
        .collect::<BTreeMap<_, _>>();
    let after_fields = after
        .fields
        .iter()
        .map(|field| (field.label.as_str(), field.resource_id.as_str()))
        .collect::<BTreeMap<_, _>>();
    for label in before_fields
        .keys()
        .chain(after_fields.keys())
        .copied()
        .collect::<BTreeSet<_>>()
    {
        let before_resource = before_fields.get(label).copied();
        let after_resource = after_fields.get(label).copied();
        let impact = match (before_resource, after_resource) {
            (Some(resource), None) if after_resources.contains(resource) => {
                PasswordChangeImpact::Metadata
            }
            (Some(_), None) => PasswordChangeImpact::Delete,
            (None, Some(resource)) if before_resources.contains(resource) => {
                PasswordChangeImpact::Metadata
            }
            (None, Some(_)) => PasswordChangeImpact::Metadata,
            (Some(_), Some(_)) => PasswordChangeImpact::References,
            (None, None) => continue,
        };
        optional_replacement(
            entries,
            &format!("/fields/{label}/resource_id"),
            &format!("{label} resource key"),
            before_resource,
            after_resource,
            impact,
        );
    }

    let before_policies = before
        .fields
        .iter()
        .map(|field| (field.resource_id.as_str(), field))
        .collect::<BTreeMap<_, _>>();
    let after_policies = after
        .fields
        .iter()
        .map(|field| (field.resource_id.as_str(), field))
        .collect::<BTreeMap<_, _>>();
    for resource in before_policies
        .keys()
        .filter(|resource| after_policies.contains_key(**resource))
    {
        let before_field = before_policies[resource];
        let after_field = after_policies[resource];
        replacement(
            entries,
            &format!("/fields/{resource}/exposure_source"),
            &format!("{} exposure source", after_field.label),
            if before_field.inherits_exposure_policy {
                "Inherit collection default"
            } else {
                "Custom"
            },
            if after_field.inherits_exposure_policy {
                "Inherit collection default"
            } else {
                "Custom"
            },
            PasswordChangeImpact::Metadata,
        );
        if before_field.exposure_policy == after_field.exposure_policy {
            continue;
        }
        entries.push(PasswordChangeDiffEntry {
            path: format!("/fields/{resource}/exposure_policy"),
            label: format!("{} exposure policy", after_field.label),
            before: Some(
                serde_json::to_string(&before_field.exposure_policy)
                    .expect("exposure policy serializes"),
            ),
            after: Some(
                serde_json::to_string(&after_field.exposure_policy)
                    .expect("exposure policy serializes"),
            ),
            impact: PasswordChangeImpact::ExposurePolicy,
        });
    }
}

fn replacement(
    entries: &mut Vec<PasswordChangeDiffEntry>,
    path: &str,
    label: &str,
    before: &str,
    after: &str,
    impact: PasswordChangeImpact,
) {
    optional_replacement(
        entries,
        path,
        label,
        (!before.is_empty()).then_some(before),
        (!after.is_empty()).then_some(after),
        impact,
    );
}

fn optional_replacement(
    entries: &mut Vec<PasswordChangeDiffEntry>,
    path: &str,
    label: &str,
    before: Option<&str>,
    after: Option<&str>,
    impact: PasswordChangeImpact,
) {
    if before != after {
        entries.push(PasswordChangeDiffEntry {
            path: path.to_string(),
            label: label.to_string(),
            before: before.map(ToOwned::to_owned),
            after: after.map(ToOwned::to_owned),
            impact,
        });
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::{
        apply_password_changes_at, password_catalog_metadata_at, preview_password_changes_at,
    };
    use crate::value_resolver::{
        import_secret_references_at, SecretImportBatchSpec, SecretImportSpec, SecretSourceLocator,
    };
    use plankton_protocol::password_changes::PasswordChangeOperation;
    use tempfile::tempdir;

    fn write_organization_catalog(path: &std::path::Path, duplicate_value: &str) {
        std::fs::write(
            path,
            format!(
                r#"
[secrets]
"secret/source/alpha" = "same-secret"
"secret/source/beta" = "beta-secret"
"secret/target/gamma" = "gamma-secret"
"secret/duplicate/alpha" = "{duplicate_value}"

[[literal_entries]]
resource = "secret/source/alpha"
display_name = "Source"
[literal_entries.metadata]
record_id = "record-source"
item_id = "source"
item_title = "Source"
field_label = "Alpha"

[[literal_entries]]
resource = "secret/source/beta"
display_name = "Source"
[literal_entries.metadata]
record_id = "record-source"
item_id = "source"
item_title = "Source"
field_label = "Beta"

[[literal_entries]]
resource = "secret/target/gamma"
display_name = "Target"
[literal_entries.metadata]
record_id = "record-target"
item_id = "target"
item_title = "Target"
field_label = "Gamma"

[[literal_entries]]
resource = "secret/duplicate/alpha"
display_name = "Duplicate"
[literal_entries.metadata]
record_id = "record-duplicate"
item_id = "duplicate"
item_title = "Duplicate"
field_label = "Alpha copy"
"#,
            ),
        )
        .expect("organization catalog fixture");
    }

    #[test]
    fn metadata_projection_and_diff_never_include_secret_values() {
        let directory = tempdir().expect("tempdir");
        let path = directory.path().join("catalog.toml");
        std::fs::write(
            directory.path().join("service.env"),
            "API_TOKEN=super-secret-value\n",
        )
        .expect("dotenv fixture");
        import_secret_references_at(
            &path,
            SecretImportBatchSpec {
                resource_template: None,
                imports: vec![SecretImportSpec {
                    resource: "secret/service/token".to_string(),
                    display_name: Some("Service token".to_string()),
                    description: None,
                    tags: vec!["legacy".to_string()],
                    metadata: BTreeMap::new(),
                    source_locator: SecretSourceLocator::DotenvFile {
                        file_path: directory.path().join("service.env"),
                        namespace: Some("service".to_string()),
                        prefix: None,
                        key: "API_TOKEN".to_string(),
                    },
                }],
            },
        )
        .expect("import");

        let metadata = password_catalog_metadata_at(&path).expect("metadata");
        let encoded = serde_json::to_string(&metadata).expect("serialize");
        assert!(!encoded.contains("super-secret-value"));

        let (_, diff) = preview_password_changes_at(
            &path,
            &[PasswordChangeOperation::UpdateItem {
                item_id: "service".to_string(),
                next_item_id: Some("production-service".to_string()),
                title: Some("Production service".to_string()),
                description: None,
                clear_description: false,
                tags: Some(vec!["production".to_string()]),
                metadata: None,
            }],
        )
        .expect("preview");
        let encoded = serde_json::to_string(&diff).expect("serialize");
        assert!(!encoded.contains("super-secret-value"));
        assert!(encoded.contains("production-service"));
        assert!(encoded.contains("Production service"));
    }

    #[test]
    fn cumulative_diff_collapses_intermediate_changes_against_the_baseline() {
        let directory = tempdir().expect("tempdir");
        let path = directory.path().join("catalog.toml");
        std::fs::write(
            directory.path().join("service.env"),
            "API_TOKEN=test-only-secret\n",
        )
        .expect("dotenv fixture");
        import_secret_references_at(
            &path,
            SecretImportBatchSpec {
                resource_template: None,
                imports: vec![SecretImportSpec {
                    resource: "secret/service/token".to_string(),
                    display_name: Some("Service token".to_string()),
                    description: None,
                    tags: Vec::new(),
                    metadata: BTreeMap::new(),
                    source_locator: SecretSourceLocator::DotenvFile {
                        file_path: directory.path().join("service.env"),
                        namespace: Some("service".to_string()),
                        prefix: None,
                        key: "API_TOKEN".to_string(),
                    },
                }],
            },
        )
        .expect("import");

        let operations = [
            PasswordChangeOperation::UpdateItem {
                item_id: "service".to_string(),
                next_item_id: None,
                title: Some("Intermediate title".to_string()),
                description: None,
                clear_description: false,
                tags: None,
                metadata: None,
            },
            PasswordChangeOperation::UpdateItem {
                item_id: "service".to_string(),
                next_item_id: None,
                title: Some("service".to_string()),
                description: None,
                clear_description: false,
                tags: None,
                metadata: None,
            },
        ];
        let (_, diff) = preview_password_changes_at(&path, &operations).expect("preview");
        assert_eq!(diff.changed_items, 0);
        assert!(diff.items.is_empty());
        assert!(!serde_json::to_string(&diff)
            .expect("serialize")
            .contains("test-only-secret"));
    }

    #[test]
    fn field_label_changes_are_metadata_only_and_keep_the_resource_key() {
        let directory = tempdir().expect("tempdir");
        let path = directory.path().join("catalog.toml");
        write_organization_catalog(&path, "same-secret");
        let operation = PasswordChangeOperation::RenameFieldLabel {
            resource_id: "secret/source/alpha".to_string(),
            label: "Access key ID".to_string(),
        };
        let (revision, diff) = preview_password_changes_at(&path, std::slice::from_ref(&operation))
            .expect("preview label change");
        let encoded = serde_json::to_string(&diff).expect("serialize diff");
        assert!(encoded.contains("Access key ID"));
        assert!(!encoded.contains("same-secret"));

        apply_password_changes_at(&path, &[operation], &revision).expect("apply label change");
        let metadata = password_catalog_metadata_at(&path).expect("metadata");
        let field = metadata
            .items
            .iter()
            .flat_map(|item| &item.fields)
            .find(|field| field.resource_id == "secret/source/alpha")
            .expect("renamed field");
        assert_eq!(field.label, "Access key ID");
        assert_eq!(field.resource_id, "secret/source/alpha");
    }

    #[test]
    fn collection_profiles_persist_inheritance_and_keep_custom_copies_independent() {
        use crate::exposure::{exposure_policy_from_metadata, inherits_exposure_policy};
        use plankton_protocol::exposure::CredentialExposurePolicy;
        let directory = tempdir().unwrap();
        let path = directory.path().join("catalog.toml");
        write_organization_catalog(&path, "same-secret");
        // Exercise both local literals and imported (including KDBX-backed) fields.
        let mut catalog = super::load_secret_catalog_file_optional(&path).unwrap();
        let beta = catalog
            .literal_entries
            .iter()
            .find(|entry| entry.resource == "secret/source/beta")
            .unwrap()
            .clone();
        catalog
            .literal_entries
            .retain(|entry| entry.resource != beta.resource);
        let value = catalog.secrets.remove(&beta.resource);
        catalog
            .imports
            .push(crate::value_resolver::ImportedSecretReference {
                resource: beta.resource,
                display_name: "Source:Beta".into(),
                description: None,
                tags: Vec::new(),
                metadata: beta.metadata,
                value,
                source_locator: SecretSourceLocator::DotenvFile {
                    file_path: directory.path().join("unused.env"),
                    namespace: None,
                    prefix: None,
                    key: "Beta".into(),
                },
                imported_at: chrono::Utc::now(),
                last_verified_at: None,
            });
        super::save_secret_catalog_file(&path, &catalog).unwrap();
        let apply = |operations: Vec<PasswordChangeOperation>| {
            let (revision, diff) = preview_password_changes_at(&path, &operations).unwrap();
            assert!(!serde_json::to_string(&diff)
                .unwrap()
                .contains("same-secret"));
            apply_password_changes_at(&path, &operations, &revision).unwrap()
        };
        let direct = CredentialExposurePolicy::direct();
        apply(vec![PasswordChangeOperation::SetItemExposurePolicy {
            item_id: "source".into(),
            policy: direct.clone(),
        }]);
        let source = || {
            password_catalog_metadata_at(&path)
                .unwrap()
                .items
                .into_iter()
                .find(|item| item.item_id == "source")
                .unwrap()
        };
        assert!(source()
            .fields
            .iter()
            .all(|field| field.inherits_exposure_policy && field.exposure_policy == direct));
        // Copying identical permissions is still a meaningful, reviewable source change.
        let diff = apply(vec![PasswordChangeOperation::SetFieldExposurePolicy {
            resource_id: "secret/source/beta".into(),
            policy: direct.clone(),
        }]);
        assert!(diff
            .items
            .iter()
            .flat_map(|item| &item.entries)
            .any(|entry| entry.path.ends_with("/exposure_source")));
        let protected = CredentialExposurePolicy::default();
        apply(vec![PasswordChangeOperation::SetItemExposurePolicy {
            item_id: "source".into(),
            policy: protected.clone(),
        }]);
        let item = source();
        assert_eq!(item.default_exposure_policy, protected);
        let alpha = item
            .fields
            .iter()
            .find(|field| field.label == "Alpha")
            .unwrap();
        let beta = item
            .fields
            .iter()
            .find(|field| field.label == "Beta")
            .unwrap();
        assert!(alpha.inherits_exposure_policy);
        assert_eq!(alpha.exposure_policy, protected);
        assert!(!beta.inherits_exposure_policy);
        assert_eq!(beta.exposure_policy, direct);
        apply(vec![PasswordChangeOperation::InheritFieldExposurePolicy {
            resource_id: "secret/source/beta".into(),
        }]);
        assert!(source()
            .fields
            .iter()
            .all(|field| field.inherits_exposure_policy && field.exposure_policy == protected));
        apply(vec![PasswordChangeOperation::SetItemExposurePolicy {
            item_id: "source".into(),
            policy: direct.clone(),
        }]);
        // Existing runtime resolvers read the updated effective policy from persisted metadata.
        let catalog = super::load_secret_catalog_file_optional(&path).unwrap();
        for entry in catalog
            .literal_entries
            .iter()
            .filter(|entry| entry.resource.starts_with("secret/source/"))
        {
            assert_eq!(exposure_policy_from_metadata(&entry.metadata), direct);
            assert!(inherits_exposure_policy(&entry.metadata));
        }
        // Invalid batches cannot partially change a collection.
        let before = std::fs::read(&path).unwrap();
        let revision = super::catalog_revision(&catalog);
        assert!(apply_password_changes_at(
            &path,
            &[
                PasswordChangeOperation::SetItemExposurePolicy {
                    item_id: "source".into(),
                    policy: protected
                },
                PasswordChangeOperation::InheritFieldExposurePolicy {
                    resource_id: "missing".into()
                },
            ],
            &revision
        )
        .is_err());
        assert_eq!(std::fs::read(&path).unwrap(), before);
    }

    #[test]
    fn moving_an_inherited_field_preserves_permissions_and_uses_the_destination_default() {
        use plankton_protocol::exposure::CredentialExposurePolicy;
        let directory = tempdir().unwrap();
        let path = directory.path().join("catalog.toml");
        write_organization_catalog(&path, "same-secret");
        let operations = [
            PasswordChangeOperation::SetItemExposurePolicy {
                item_id: "source".into(),
                policy: CredentialExposurePolicy::direct(),
            },
            PasswordChangeOperation::MoveField {
                resource_id: "secret/source/alpha".into(),
                target_item_id: "target".into(),
                target_title: None,
            },
        ];
        let (revision, _) = preview_password_changes_at(&path, &operations).unwrap();
        apply_password_changes_at(&path, &operations, &revision).unwrap();
        let item = password_catalog_metadata_at(&path)
            .unwrap()
            .items
            .into_iter()
            .find(|item| item.item_id == "target")
            .unwrap();
        assert_eq!(
            item.default_exposure_policy,
            CredentialExposurePolicy::default()
        );
        let moved = item
            .fields
            .iter()
            .find(|field| field.label == "Alpha")
            .unwrap();
        assert!(!moved.inherits_exposure_policy);
        assert_eq!(moved.exposure_policy, CredentialExposurePolicy::direct());
    }

    #[test]
    fn exposure_policy_change_is_graphical_metadata_and_never_contains_the_value() {
        let directory = tempdir().expect("tempdir");
        let path = directory.path().join("catalog.toml");
        write_organization_catalog(&path, "same-secret");
        let policy = plankton_protocol::exposure::CredentialExposurePolicy::direct();
        let operation = PasswordChangeOperation::SetFieldExposurePolicy {
            resource_id: "secret/source/alpha".to_string(),
            policy: policy.clone(),
        };

        let (revision, diff) = preview_password_changes_at(&path, std::slice::from_ref(&operation))
            .expect("preview exposure policy");
        let entry = diff
            .items
            .iter()
            .flat_map(|item| &item.entries)
            .find(|entry| {
                entry.impact
                    == plankton_protocol::password_changes::PasswordChangeImpact::ExposurePolicy
            })
            .expect("exposure policy diff");
        assert!(entry.path.ends_with("/exposure_policy"));
        assert!(!entry
            .before
            .as_deref()
            .unwrap_or_default()
            .contains("same-secret"));
        assert!(!entry
            .after
            .as_deref()
            .unwrap_or_default()
            .contains("same-secret"));

        apply_password_changes_at(&path, &[operation], &revision).expect("apply policy");
        let metadata = password_catalog_metadata_at(&path).expect("metadata");
        let field = metadata
            .items
            .iter()
            .flat_map(|item| &item.fields)
            .find(|field| field.resource_id == "secret/source/alpha")
            .expect("field");
        assert_eq!(field.exposure_policy, policy);
    }

    #[test]
    fn explicit_item_id_groups_legacy_fields_with_conflicting_record_ids() {
        let directory = tempdir().expect("tempdir");
        let path = directory.path().join("catalog.toml");
        std::fs::write(
            &path,
            r#"
[[imports]]
resource = "plankton://field/logical-env/ALPHA"
display_name = ".env"
provider_kind = "keepassxc_cli"
database = "/tmp/test.kdbx"
entry = "env/ALPHA"
field = "password"
unlock_secret_file = "/tmp/unlock"
executable = "/tmp/keepassxc-cli"
executable_sha256 = "test-sha"
imported_at = "2026-08-06T00:00:00Z"

[imports.metadata]
item_id = "logical-env"
item_title = ".env"
field_label = "ALPHA"
record_id = "legacy-alpha"

[[imports]]
resource = "plankton://field/logical-env/BETA"
display_name = ".env"
provider_kind = "keepassxc_cli"
database = "/tmp/test.kdbx"
entry = "env/BETA"
field = "password"
unlock_secret_file = "/tmp/unlock"
executable = "/tmp/keepassxc-cli"
executable_sha256 = "test-sha"
imported_at = "2026-08-06T00:00:00Z"

[imports.metadata]
item_id = "logical-env"
item_title = ".env"
field_label = "BETA"
record_id = "legacy-beta"
"#,
        )
        .expect("catalog fixture");

        let metadata = password_catalog_metadata_at(&path).expect("metadata");
        assert_eq!(metadata.items.len(), 1);
        assert_eq!(metadata.items[0].item_id, "logical-env");
        assert_eq!(metadata.items[0].fields.len(), 2);

        let (_, diff) = preview_password_changes_at(
            &path,
            &[PasswordChangeOperation::UpdateItem {
                item_id: "logical-env".to_string(),
                next_item_id: None,
                title: Some("Environment".to_string()),
                description: None,
                clear_description: false,
                tags: None,
                metadata: None,
            }],
        )
        .expect("unambiguous preview");
        assert_eq!(diff.changed_items, 1);
        assert_eq!(diff.items[0].entries.len(), 1);
    }

    #[test]
    fn move_field_splits_one_logical_item_without_changing_resource_keys() {
        let directory = tempdir().expect("tempdir");
        let path = directory.path().join("catalog.toml");
        write_organization_catalog(&path, "same-secret");

        let (revision, diff) = preview_password_changes_at(
            &path,
            &[PasswordChangeOperation::MoveField {
                resource_id: "secret/source/alpha".to_string(),
                target_item_id: "split-alpha".to_string(),
                target_title: Some("Split Alpha".to_string()),
            }],
        )
        .expect("split preview");

        assert_eq!(diff.changed_items, 2);
        assert_eq!(diff.changed_fields, 2);
        assert_eq!(diff.breaking_changes, 0);
        let encoded = serde_json::to_string(&diff).expect("serialize diff");
        assert!(!encoded.contains("same-secret"));

        apply_password_changes_at(
            &path,
            &[PasswordChangeOperation::MoveField {
                resource_id: "secret/source/alpha".to_string(),
                target_item_id: "split-alpha".to_string(),
                target_title: Some("Split Alpha".to_string()),
            }],
            &revision,
        )
        .expect("apply split");
        let metadata = password_catalog_metadata_at(&path).expect("metadata");
        let split = metadata
            .items
            .iter()
            .find(|item| item.item_id == "split-alpha")
            .expect("split item");
        assert_eq!(split.title, "Split Alpha");
        assert_eq!(split.fields[0].resource_id, "secret/source/alpha");
        assert_eq!(
            metadata
                .items
                .iter()
                .find(|item| item.item_id == "source")
                .expect("source item")
                .fields
                .len(),
            1
        );
    }

    #[test]
    fn merge_items_moves_every_field() {
        let directory = tempdir().expect("tempdir");
        let path = directory.path().join("catalog.toml");
        write_organization_catalog(&path, "same-secret");

        let (revision, diff) = preview_password_changes_at(
            &path,
            &[PasswordChangeOperation::MergeItems {
                source_item_id: "source".to_string(),
                target_item_id: "target".to_string(),
            }],
        )
        .expect("merge preview");
        assert_eq!(diff.breaking_changes, 0);
        assert!(diff.items.iter().any(|item| {
            item.entries
                .iter()
                .any(|entry| entry.label == "Remove empty item after merge")
        }));

        apply_password_changes_at(
            &path,
            &[PasswordChangeOperation::MergeItems {
                source_item_id: "source".to_string(),
                target_item_id: "target".to_string(),
            }],
            &revision,
        )
        .expect("apply merge");
        let metadata = password_catalog_metadata_at(&path).expect("metadata");
        assert!(metadata.items.iter().all(|item| item.item_id != "source"));
        assert_eq!(
            metadata
                .items
                .iter()
                .find(|item| item.item_id == "target")
                .expect("target item")
                .fields
                .len(),
            3
        );
    }

    #[test]
    fn delete_item_removes_all_of_its_fields_and_keeps_other_items() {
        let directory = tempdir().expect("tempdir");
        let path = directory.path().join("catalog.toml");
        write_organization_catalog(&path, "same-secret");
        let operation = PasswordChangeOperation::DeleteItem {
            item_id: "source".to_string(),
        };

        let (revision, diff) = preview_password_changes_at(&path, std::slice::from_ref(&operation))
            .expect("preview item deletion");
        assert_eq!(diff.changed_items, 1);
        assert_eq!(diff.changed_fields, 0);
        assert_eq!(diff.breaking_changes, 1);

        apply_password_changes_at(&path, &[operation], &revision).expect("apply item deletion");
        let metadata = password_catalog_metadata_at(&path).expect("metadata");
        assert!(metadata.items.iter().all(|item| item.item_id != "source"));
        assert!(metadata.items.iter().any(|item| item.item_id == "target"));

        let persisted = std::fs::read_to_string(path).expect("read catalog");
        assert!(!persisted.contains("secret/source/alpha"));
        assert!(!persisted.contains("secret/source/beta"));
        assert!(persisted.contains("secret/target/gamma"));
    }

    #[test]
    fn delete_field_removes_only_the_selected_field_and_its_value() {
        let directory = tempdir().expect("tempdir");
        let path = directory.path().join("catalog.toml");
        write_organization_catalog(&path, "same-secret");
        let operation = PasswordChangeOperation::DeleteField {
            resource_id: "secret/source/alpha".to_string(),
        };

        let (revision, diff) = preview_password_changes_at(&path, std::slice::from_ref(&operation))
            .expect("preview field deletion");
        assert_eq!(diff.changed_items, 1);
        assert_eq!(diff.changed_fields, 1);
        assert_eq!(diff.breaking_changes, 1);
        assert!(diff
            .items
            .iter()
            .flat_map(|item| &item.entries)
            .any(|entry| {
                entry.label == "Alpha resource key"
                    && entry.before.as_deref() == Some("secret/source/alpha")
                    && entry.after.is_none()
            }));

        apply_password_changes_at(&path, &[operation], &revision).expect("apply field deletion");
        let metadata = password_catalog_metadata_at(&path).expect("metadata");
        let source = metadata
            .items
            .iter()
            .find(|item| item.item_id == "source")
            .expect("source item remains");
        assert_eq!(source.fields.len(), 1);
        assert_eq!(source.fields[0].resource_id, "secret/source/beta");

        let persisted = std::fs::read_to_string(path).expect("read catalog");
        assert!(!persisted.contains("secret/source/alpha"));
        assert!(persisted.contains("secret/source/beta"));
        assert!(persisted.contains("secret/target/gamma"));
    }

    #[test]
    fn merge_items_rejects_field_label_collisions() {
        let directory = tempdir().expect("tempdir");
        let path = directory.path().join("catalog.toml");
        write_organization_catalog(&path, "same-secret");
        let catalog = std::fs::read_to_string(&path).expect("read fixture");
        std::fs::write(
            &path,
            catalog.replace("field_label = \"Gamma\"", "field_label = \"Alpha\""),
        )
        .expect("write colliding fixture");

        let error = preview_password_changes_at(
            &path,
            &[PasswordChangeOperation::MergeItems {
                source_item_id: "source".to_string(),
                target_item_id: "target".to_string(),
            }],
        )
        .expect_err("duplicate labels must not merge");
        assert!(matches!(
            error,
            crate::value_resolver::SecretImportError::DuplicateFieldLabel { .. }
        ));
    }

    #[test]
    fn dedupe_field_deletes_only_a_matching_stored_value() {
        let directory = tempdir().expect("tempdir");
        let matching_path = directory.path().join("matching.toml");
        write_organization_catalog(&matching_path, "same-secret");
        let (_, diff) = preview_password_changes_at(
            &matching_path,
            &[PasswordChangeOperation::DeleteDuplicateField {
                resource_id: "secret/duplicate/alpha".to_string(),
                canonical_resource_id: "secret/source/alpha".to_string(),
            }],
        )
        .expect("matching duplicate preview");
        assert_eq!(diff.breaking_changes, 1);
        assert!(!serde_json::to_string(&diff)
            .expect("serialize diff")
            .contains("same-secret"));

        let different_path = directory.path().join("different.toml");
        write_organization_catalog(&different_path, "different-secret");
        let error = preview_password_changes_at(
            &different_path,
            &[PasswordChangeOperation::DeleteDuplicateField {
                resource_id: "secret/duplicate/alpha".to_string(),
                canonical_resource_id: "secret/source/alpha".to_string(),
            }],
        )
        .expect_err("different values must not dedupe");
        assert!(matches!(
            error,
            crate::value_resolver::SecretImportError::DuplicateFieldMismatch { .. }
        ));
    }

    #[test]
    fn dedupe_field_resolves_an_external_reference_without_caching_its_value() {
        let directory = tempdir().expect("tempdir");
        let dotenv_path = directory.path().join("canonical.env");
        std::fs::write(&dotenv_path, "TOKEN=same-secret\n").expect("dotenv fixture");
        let catalog_path = directory.path().join("catalog.toml");
        std::fs::write(
            &catalog_path,
            format!(
                r#"
[secrets]
"secret/duplicate" = "same-secret"

[[imports]]
resource = "secret/canonical"
display_name = "Canonical"
provider_kind = "dotenv_file"
file_path = "{}"
key = "TOKEN"
imported_at = "2026-08-06T00:00:00Z"
"#,
                dotenv_path.display()
            ),
        )
        .expect("catalog fixture");

        let (_, diff) = preview_password_changes_at(
            &catalog_path,
            &[PasswordChangeOperation::DeleteDuplicateField {
                resource_id: "secret/duplicate".to_string(),
                canonical_resource_id: "secret/canonical".to_string(),
            }],
        )
        .expect("external reference comparison");

        assert_eq!(diff.breaking_changes, 1);
        let persisted = std::fs::read_to_string(&catalog_path).expect("read catalog");
        assert!(!persisted.contains("value = \"same-secret\""));
    }
}
