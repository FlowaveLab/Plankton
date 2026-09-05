use std::path::PathBuf;
use std::str::FromStr;
use std::time::Duration;

use chrono::{DateTime, Utc};
use plankton_core::{
    automatic_decision_from_batch, build_prompt_context, build_provider_input_snapshot,
    context_matches_resource_selector, evaluate_automatic_disposition, render_request_template,
    sanitize_request_context_for_storage, semantic_call_chain_sha256,
    shared_resource_metadata_sha256, AccessRequest, ApprovalStatus, AuditRecord,
    AutomaticDecisionSource, AutomaticDecisionTrace, BatchResourceDecision, DashboardData,
    Decision, DomainError, EvaluationState, LlmSuggestion, PlanktonSettings, PolicyMode,
    ProviderError, ProviderInputSnapshot, RequestContext, TemplateError,
    APPROVAL_BATCH_TICKET_TTL_SECONDS, DEFAULT_USER_PROVIDER_KIND, LLM_ADVICE_TEMPLATE_VERSION,
    PROMPT_CONTRACT_VERSION,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::{
    sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions},
    Row, SqlitePool,
};
use tracing::instrument;

#[derive(Debug, Clone)]
pub struct SqliteStore {
    pub(crate) pool: SqlitePool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RequestQueryResult {
    pub request: AccessRequest,
    pub audit_records: Vec<AuditRecord>,
}

#[derive(Debug)]
struct BatchTicketMatch {
    shared_suggestion: Option<LlmSuggestion>,
    source_request_id: String,
    decision: BatchResourceDecision,
    provider_kind: Option<String>,
    provider_model: Option<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("database error: {0}")]
    Database(#[from] sqlx::Error),
    #[error("migration error: {0}")]
    Migration(#[from] sqlx::migrate::MigrateError),
    #[error("template error: {0}")]
    Template(#[from] TemplateError),
    #[error("domain error: {0}")]
    Domain(#[from] DomainError),
    #[error("serialization error: {0}")]
    Serialization(#[from] serde_json::Error),
    #[error("provider error: {0}")]
    Provider(#[from] ProviderError),
    #[error("invalid datetime in storage: {0}")]
    InvalidDateTime(String),
    #[error("request {0} was not found")]
    NotFound(String),
    #[error("invalid stored value for {field}: {value}")]
    InvalidStoredValue { field: &'static str, value: String },
    #[error("resource search index contains multiple generations")]
    InconsistentSearchGeneration,
    #[error("limit must be between 1 and {maximum}, received {received}")]
    InvalidLimit { maximum: u16, received: u16 },
}

async fn find_matching_batch_ticket(
    pool: &SqlitePool,
    context: &RequestContext,
) -> Result<Option<BatchTicketMatch>, StoreError> {
    let Some(metadata_sha256) = shared_resource_metadata_sha256(&context.resource_metadata) else {
        return Ok(None);
    };
    let call_chain_sha256 = semantic_call_chain_sha256(&context.call_chain);
    let rows = sqlx::query(
        r#"
        SELECT ticket.source_request_id, ticket.resource_selector, ticket.suggested_decision,
               ticket.rationale_summary, ticket.risk_score, ticket.provider_kind, ticket.provider_model,
               source.llm_suggestion_json
        FROM approval_batch_tickets AS ticket
        JOIN access_requests AS source ON source.id = ticket.source_request_id
        WHERE ticket.semantic_call_chain_sha256 = ?
          AND ticket.requested_by = ?
          AND ticket.reason = ?
          AND ticket.shared_resource_metadata_sha256 = ?
          AND ticket.template_version = ?
          AND ticket.prompt_contract_version = ?
          AND ticket.expires_at > ?
        ORDER BY ticket.created_at DESC
        "#,
    )
    .bind(call_chain_sha256)
    .bind(&context.requested_by)
    .bind(&context.reason)
    .bind(metadata_sha256)
    .bind(LLM_ADVICE_TEMPLATE_VERSION)
    .bind(PROMPT_CONTRACT_VERSION)
    .bind(Utc::now().to_rfc3339())
    .fetch_all(pool)
    .await?;

    for row in rows {
        let resource_selector: String = row.try_get("resource_selector")?;
        if !context_matches_resource_selector(context, &resource_selector) {
            continue;
        }
        let risk_score = u8::try_from(row.try_get::<i64, _>("risk_score")?).map_err(|_| {
            StoreError::InvalidStoredValue {
                field: "approval_batch_tickets.risk_score",
                value: "outside u8 range".to_string(),
            }
        })?;
        return Ok(Some(BatchTicketMatch {
            shared_suggestion: parse_optional_json::<LlmSuggestion>(
                row.try_get("llm_suggestion_json")?,
            )?,
            source_request_id: row.try_get("source_request_id")?,
            decision: BatchResourceDecision {
                resource_selector,
                suggested_decision: parse_enum(
                    row.try_get::<String, _>("suggested_decision")?.as_str(),
                )?,
                rationale_summary: row.try_get("rationale_summary")?,
                risk_score,
            },
            provider_kind: row.try_get("provider_kind")?,
            provider_model: row.try_get("provider_model")?,
        }));
    }
    Ok(None)
}

pub(crate) async fn refresh_shared_review(
    pool: &SqlitePool,
    request: &mut AccessRequest,
) -> Result<(), StoreError> {
    let Some(source_id) = request
        .automatic_decision
        .as_ref()
        .and_then(|trace| trace.batch_source_request_id.as_deref())
    else {
        return Ok(());
    };
    let encoded = sqlx::query_scalar::<_, Option<String>>(
        "SELECT llm_suggestion_json FROM access_requests WHERE id = ?",
    )
    .bind(source_id)
    .fetch_optional(pool)
    .await?
    .flatten();
    let shared = parse_optional_json::<LlmSuggestion>(encoded)?;
    if let (Some(shared), Some(current)) = (shared, request.llm_suggestion.as_mut()) {
        current.exposure_report = shared.exposure_report;
        current.provider_trace = shared.provider_trace;
        current.usage = shared.usage;
    }
    Ok(())
}

async fn insert_batch_tickets(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    request: &AccessRequest,
) -> Result<(), StoreError> {
    let Some(suggestion) = request.llm_suggestion.as_ref() else {
        return Ok(());
    };
    if request.policy_mode != PolicyMode::LlmAutomatic
        || suggestion.error.is_some()
        || suggestion.batch_decisions.is_empty()
    {
        return Ok(());
    }
    let Some(automatic_decision) = request.automatic_decision.as_ref() else {
        return Ok(());
    };
    if automatic_decision.decision_source != AutomaticDecisionSource::LlmSuggestion
        || automatic_decision.fail_closed
        || !automatic_decision.provider_called
    {
        return Ok(());
    }
    let Some(metadata_sha256) = shared_resource_metadata_sha256(&request.context.resource_metadata)
    else {
        return Ok(());
    };
    let call_chain_sha256 = semantic_call_chain_sha256(&request.context.call_chain);
    let created_at = suggestion.generated_at;
    let expires_at = created_at + chrono::Duration::seconds(APPROVAL_BATCH_TICKET_TTL_SECONDS);

    for decision in &suggestion.batch_decisions {
        sqlx::query(
            r#"
            INSERT INTO approval_batch_tickets (
                id, source_request_id, semantic_call_chain_sha256, requested_by, reason,
                shared_resource_metadata_sha256, resource_selector, suggested_decision,
                rationale_summary, risk_score, provider_kind, provider_model, template_version,
                prompt_contract_version, created_at, expires_at
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind(uuid::Uuid::new_v4().to_string())
        .bind(&request.id)
        .bind(&call_chain_sha256)
        .bind(&request.context.requested_by)
        .bind(&request.context.reason)
        .bind(&metadata_sha256)
        .bind(&decision.resource_selector)
        .bind(enum_to_string(&decision.suggested_decision)?)
        .bind(&decision.rationale_summary)
        .bind(i64::from(decision.risk_score))
        .bind(&suggestion.provider_kind)
        .bind(&suggestion.provider_model)
        .bind(&suggestion.template_version)
        .bind(&suggestion.prompt_contract_version)
        .bind(created_at.to_rfc3339())
        .bind(expires_at.to_rfc3339())
        .execute(&mut **tx)
        .await?;
    }
    Ok(())
}

impl SqliteStore {
    #[instrument(skip(settings))]
    pub async fn new(settings: &PlanktonSettings) -> Result<Self, StoreError> {
        ensure_sqlite_parent_dir(&settings.database_url)?;
        let options = SqliteConnectOptions::from_str(&settings.database_url)?
            .create_if_missing(true)
            .journal_mode(SqliteJournalMode::Wal)
            .busy_timeout(Duration::from_secs(5));
        let pool = SqlitePoolOptions::new()
            .max_connections(5)
            .connect_with(options)
            .await?;

        // Migrations are embedded so packaged clients never depend on loose SQL files.
        sqlx::migrate!("./migrations").run(&pool).await?;
        Ok(Self { pool })
    }

    #[instrument(skip(self, settings, context))]
    pub async fn submit_request(
        &self,
        settings: &PlanktonSettings,
        context: RequestContext,
        policy_mode: PolicyMode,
    ) -> Result<AccessRequest, StoreError> {
        let prompt_context = build_prompt_context(&context);
        let stored_context = sanitize_request_context_for_storage(&context);
        let rendered_prompt = render_request_template(
            &settings.request_template,
            &prompt_context,
            policy_mode,
            &settings.locale,
        )?;
        let (provider_kind, provider_input) = match policy_mode {
            PolicyMode::ManualOnly => (None, None),
            PolicyMode::Assisted | PolicyMode::LlmAutomatic => (
                Some(normalized_provider_kind(settings)),
                Some(build_provider_input_snapshot(
                    settings,
                    policy_mode,
                    &context,
                    &prompt_context,
                )?),
            ),
        };
        let mut request = AccessRequest::new_pending(
            stored_context,
            policy_mode,
            provider_kind,
            rendered_prompt,
            provider_input,
            None,
        );
        let mut audits = vec![request.record_submission_audit()];
        if policy_mode == PolicyMode::LlmAutomatic {
            if let Some(ticket) = find_matching_batch_ticket(&self.pool, &request.context).await? {
                request.evaluation_state = EvaluationState::Completed;
                let sanitized_context = request
                    .provider_input
                    .as_ref()
                    .map(|input| input.sanitized_context.clone())
                    .unwrap_or_else(|| build_prompt_context(&request.context));
                let automatic_decision = automatic_decision_from_batch(
                    ticket.source_request_id,
                    &ticket.decision,
                    ticket.provider_kind,
                    ticket.provider_model,
                    request
                        .provider_input
                        .as_ref()
                        .map(|input| input.decision_policy)
                        .unwrap_or_default(),
                    &sanitized_context,
                    ticket
                        .shared_suggestion
                        .as_ref()
                        .and_then(|suggestion| suggestion.exposure_report.as_ref()),
                );
                // Keep the source session and shared report available to the audit UI.
                request.llm_suggestion = ticket.shared_suggestion.map(|mut suggestion| {
                    suggestion.suggested_decision = ticket.decision.suggested_decision;
                    suggestion.rationale_summary = ticket.decision.rationale_summary.clone();
                    suggestion.risk_score = ticket.decision.risk_score;
                    suggestion
                });
                audits.extend(request.apply_automatic_decision(automatic_decision)?);
            }
        }

        // Keep the batch-ticket lookup outside the transaction so concurrent submissions do not
        // both acquire read snapshots and then contend while upgrading them to SQLite writers.
        // Ticket reuse is only an optimization; if a ticket is created between the lookup and the
        // insert, this request can safely proceed through a fresh evaluation.
        let mut tx = self.pool.begin().await?;
        sqlx::query(
            r#"
            INSERT INTO access_requests (
                id, resource, requested_by, reason, policy_mode, approval_status, evaluation_state,
                final_decision, provider_kind, rendered_prompt, provider_input_json,
                llm_suggestion_json, automatic_decision_json, context_json, created_at, updated_at,
                resolved_at
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind(&request.id)
        .bind(&request.context.resource)
        .bind(&request.context.requested_by)
        .bind(&request.context.reason)
        .bind(enum_to_string(&request.policy_mode)?)
        .bind(enum_to_string(&request.approval_status)?)
        .bind(enum_to_string(&request.evaluation_state)?)
        .bind(option_enum_to_string(&request.final_decision)?)
        .bind(&request.provider_kind)
        .bind(&request.rendered_prompt)
        .bind(option_json_string(&request.provider_input)?)
        .bind(option_json_string(&request.llm_suggestion)?)
        .bind(option_json_string(&request.automatic_decision)?)
        .bind(serde_json::to_string(&request.context)?)
        .bind(request.created_at.to_rfc3339())
        .bind(request.updated_at.to_rfc3339())
        .bind(option_datetime(&request.resolved_at))
        .execute(&mut *tx)
        .await?;

        insert_audits(&mut tx, &audits).await?;
        if request.evaluation_state == EvaluationState::Queued {
            let operation_id = evaluation_operation_id(&request.id);
            sqlx::query(
                r#"
                INSERT INTO interrupted_operations (
                    id, operation_kind, operation_key, state_json, status, started_at,
                    heartbeat_at, finished_at
                ) VALUES (?, 'llm_evaluation', ?, ?, 'queued', ?, ?, NULL)
                "#,
            )
            .bind(operation_id)
            .bind(&request.id)
            .bind(serde_json::json!({"phase": "queued"}).to_string())
            .bind(request.created_at.to_rfc3339())
            .bind(request.created_at.to_rfc3339())
            .execute(&mut *tx)
            .await?;
        }
        tx.commit().await?;

        Ok(request)
    }

    #[instrument(skip(self))]
    pub async fn get_request(&self, request_id: &str) -> Result<RequestQueryResult, StoreError> {
        let request_row = sqlx::query(
            r#"
            SELECT
                id, resource, policy_mode, approval_status, evaluation_state, final_decision,
                provider_kind, rendered_prompt, provider_input_json, llm_suggestion_json,
                automatic_decision_json, context_json, created_at, updated_at, resolved_at
            FROM access_requests
            WHERE id = ?
            "#,
        )
        .bind(request_id)
        .fetch_optional(&self.pool)
        .await?;

        let request_row =
            request_row.ok_or_else(|| StoreError::NotFound(request_id.to_string()))?;
        let mut request = decode_request(&request_row)?;
        refresh_shared_review(&self.pool, &mut request).await?;

        let audit_rows = sqlx::query(
            r#"
            SELECT id, request_id, action, actor, note, payload_json, created_at
            FROM audit_records
            WHERE request_id = ?
            ORDER BY created_at ASC
            "#,
        )
        .bind(request_id)
        .fetch_all(&self.pool)
        .await?;

        let audit_records = audit_rows
            .iter()
            .map(decode_audit)
            .collect::<Result<Vec<_>, _>>()?;

        Ok(RequestQueryResult {
            request,
            audit_records,
        })
    }

    #[instrument(skip(self))]
    pub async fn list_human_review_request_ids(&self) -> Result<Vec<String>, StoreError> {
        // Match AccessRequest::human_review_required without decoding prompts,
        // call chains, provider traces, or historical audit payloads.
        Ok(sqlx::query_scalar(
            r#"
                SELECT id FROM access_requests
                WHERE approval_status = 'pending'
                  AND (policy_mode = 'manual_only'
                       OR (policy_mode IN ('assisted', 'llm_automatic')
                           AND evaluation_state NOT IN ('queued', 'running')))
                ORDER BY created_at DESC
                "#,
        )
        .fetch_all(&self.pool)
        .await?)
    }

    #[instrument(skip(self))]
    pub async fn list_pending_requests(&self) -> Result<Vec<AccessRequest>, StoreError> {
        let rows = sqlx::query(
            r#"
            SELECT
                id, resource, policy_mode, approval_status, evaluation_state, final_decision,
                provider_kind, rendered_prompt, provider_input_json, llm_suggestion_json,
                automatic_decision_json, context_json, created_at, updated_at, resolved_at
            FROM access_requests
            WHERE approval_status = ?
            ORDER BY created_at DESC
            "#,
        )
        .bind("pending")
        .fetch_all(&self.pool)
        .await?;

        rows.iter().map(decode_request).collect()
    }

    #[instrument(skip(self))]
    pub async fn list_related_requests(
        &self,
        request_id: &str,
    ) -> Result<Vec<AccessRequest>, StoreError> {
        let selected = self.get_request(request_id).await?.request;
        let source_id = selected
            .automatic_decision
            .as_ref()
            .and_then(|trace| trace.batch_source_request_id.as_deref())
            .unwrap_or(&selected.id);
        let start =
            selected.created_at - chrono::Duration::seconds(APPROVAL_BATCH_TICKET_TTL_SECONDS);
        let end =
            selected.created_at + chrono::Duration::seconds(APPROVAL_BATCH_TICKET_TTL_SECONDS);
        let rows = sqlx::query(
            r#"SELECT * FROM access_requests
               WHERE id = ? OR id = ?
                  OR json_extract(automatic_decision_json, '$.batch_source_request_id') = ?
                  OR (julianday(created_at) BETWEEN julianday(?) AND julianday(?)
                      AND json_extract(context_json, '$.requested_by') = ?
                      AND json_extract(context_json, '$.reason') = ?)
               ORDER BY created_at ASC, id ASC"#,
        )
        .bind(&selected.id)
        .bind(source_id)
        .bind(source_id)
        .bind(start.to_rfc3339())
        .bind(end.to_rfc3339())
        .bind(&selected.context.requested_by)
        .bind(&selected.context.reason)
        .fetch_all(&self.pool)
        .await?;
        let shared_metadata = shared_resource_metadata_sha256(&selected.context.resource_metadata);
        let chain = semantic_call_chain_sha256(&selected.context.call_chain);
        let mut related = Vec::new();
        for row in rows {
            let mut candidate = decode_request(&row)?;
            let explicit = candidate.id == selected.id
                || candidate.id == source_id
                || candidate
                    .automatic_decision
                    .as_ref()
                    .and_then(|trace| trace.batch_source_request_id.as_deref())
                    == Some(source_id);
            let semantic = shared_metadata.is_some()
                && !selected.context.call_chain.is_empty()
                && shared_resource_metadata_sha256(&candidate.context.resource_metadata)
                    == shared_metadata
                && semantic_call_chain_sha256(&candidate.context.call_chain) == chain
                && candidate.context.requested_by == selected.context.requested_by
                && candidate.context.reason == selected.context.reason
                && (candidate.created_at - selected.created_at)
                    .num_seconds()
                    .abs()
                    <= APPROVAL_BATCH_TICKET_TTL_SECONDS;
            if explicit || semantic {
                refresh_shared_review(&self.pool, &mut candidate).await?;
                related.push(candidate);
            }
        }
        Ok(related)
    }

    #[instrument(skip(self))]
    pub async fn list_resolved_requests(
        &self,
        query: &str,
        limit: u16,
        offset: u32,
    ) -> Result<Vec<AccessRequest>, StoreError> {
        if limit == 0 || limit > 100 {
            return Err(StoreError::InvalidLimit {
                maximum: 100,
                received: limit,
            });
        }
        let pattern = format!("%{}%", query.trim().to_ascii_lowercase());
        let rows = sqlx::query(
            r#"
            SELECT
                id, resource, policy_mode, approval_status, evaluation_state, final_decision,
                provider_kind, rendered_prompt, provider_input_json, llm_suggestion_json,
                automatic_decision_json, context_json, created_at, updated_at, resolved_at
            FROM access_requests
            WHERE resolved_at IS NOT NULL
              AND (? = '%%' OR LOWER(resource) LIKE ? OR LOWER(context_json) LIKE ?)
            ORDER BY resolved_at DESC, id DESC
            LIMIT ? OFFSET ?
            "#,
        )
        .bind(&pattern)
        .bind(&pattern)
        .bind(&pattern)
        .bind(i64::from(limit))
        .bind(i64::from(offset))
        .fetch_all(&self.pool)
        .await?;

        rows.iter().map(decode_request).collect()
    }

    #[instrument(skip(self))]
    pub async fn count_resolved_requests(&self, query: &str) -> Result<u64, StoreError> {
        let pattern = format!("%{}%", query.trim().to_ascii_lowercase());
        let count: i64 = sqlx::query_scalar(
            r#"
            SELECT COUNT(*)
            FROM access_requests
            WHERE resolved_at IS NOT NULL
              AND (? = '%%' OR LOWER(resource) LIKE ? OR LOWER(context_json) LIKE ?)
            "#,
        )
        .bind(&pattern)
        .bind(&pattern)
        .bind(&pattern)
        .fetch_one(&self.pool)
        .await?;
        u64::try_from(count).map_err(|_| StoreError::InvalidStoredValue {
            field: "resolved_request_count",
            value: count.to_string(),
        })
    }

    #[instrument(skip(self))]
    pub async fn list_audit_records(&self, limit: u32) -> Result<Vec<AuditRecord>, StoreError> {
        let rows = sqlx::query(
            r#"
            SELECT id, request_id, action, actor, note, payload_json, created_at
            FROM audit_records
            ORDER BY created_at DESC
            LIMIT ?
            "#,
        )
        .bind(limit as i64)
        .fetch_all(&self.pool)
        .await?;

        rows.iter().map(decode_audit).collect()
    }

    #[instrument(skip(self))]
    pub async fn list_audit_records_for_recent_requests(
        &self,
        request_limit: u32,
    ) -> Result<Vec<AuditRecord>, StoreError> {
        let rows = sqlx::query(
            r#"
            WITH recent_requests AS (
                SELECT request_id, MAX(created_at) AS latest_at
                FROM audit_records
                GROUP BY request_id
                ORDER BY latest_at DESC
                LIMIT ?
            )
            SELECT
                audit_records.id,
                audit_records.request_id,
                audit_records.action,
                audit_records.actor,
                audit_records.note,
                audit_records.payload_json,
                audit_records.created_at
            FROM audit_records
            INNER JOIN recent_requests
                ON recent_requests.request_id = audit_records.request_id
            ORDER BY
                recent_requests.latest_at DESC,
                audit_records.created_at ASC,
                audit_records.id ASC
            "#,
        )
        .bind(request_limit as i64)
        .fetch_all(&self.pool)
        .await?;

        rows.iter().map(decode_audit).collect()
    }

    #[instrument(skip(self))]
    pub async fn dashboard(&self, limit: u32) -> Result<DashboardData, StoreError> {
        Ok(DashboardData {
            pending_requests: self.list_pending_requests().await?,
            recent_audit_records: self.list_audit_records_for_recent_requests(limit).await?,
        })
    }

    #[instrument(skip(self, actor, note))]
    pub async fn record_decision(
        &self,
        _settings: &PlanktonSettings,
        request_id: &str,
        decision: Decision,
        actor: &str,
        note: Option<String>,
    ) -> Result<AccessRequest, StoreError> {
        self.record_decision_internal(request_id, decision, actor, note)
            .await
    }

    /// Records a direct-field policy decision.
    #[instrument(skip(self, note))]
    pub async fn record_direct_policy_decision(
        &self,
        request_id: &str,
        actor: &str,
        note: Option<String>,
    ) -> Result<AccessRequest, StoreError> {
        self.record_decision_internal(request_id, Decision::Allow, actor, note)
            .await
    }

    async fn record_decision_internal(
        &self,
        request_id: &str,
        decision: Decision,
        actor: &str,
        note: Option<String>,
    ) -> Result<AccessRequest, StoreError> {
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;

        let row = sqlx::query(
            r#"
            SELECT
                id, resource, policy_mode, approval_status, evaluation_state, final_decision,
                provider_kind, rendered_prompt, provider_input_json, llm_suggestion_json,
                automatic_decision_json, context_json, created_at, updated_at, resolved_at
            FROM access_requests
            WHERE id = ?
            "#,
        )
        .bind(request_id)
        .fetch_optional(&mut *tx)
        .await?;

        let row = row.ok_or_else(|| StoreError::NotFound(request_id.to_string()))?;
        let mut request = decode_request(&row)?;
        let audits = request.apply_manual_decision(decision, actor.to_string(), note)?;
        sqlx::query(
            r#"
            UPDATE access_requests
            SET approval_status = ?, evaluation_state = ?, final_decision = ?, updated_at = ?,
                resolved_at = ?
            WHERE id = ?
            "#,
        )
        .bind(enum_to_string(&request.approval_status)?)
        .bind(enum_to_string(&request.evaluation_state)?)
        .bind(option_enum_to_string(&request.final_decision)?)
        .bind(request.updated_at.to_rfc3339())
        .bind(option_datetime(&request.resolved_at))
        .bind(&request.id)
        .execute(&mut *tx)
        .await?;

        if request.evaluation_state == EvaluationState::Superseded {
            finish_evaluation_operation_in_tx(
                &mut tx,
                request_id,
                "superseded",
                request.updated_at,
            )
            .await?;
        }
        insert_audits(&mut tx, &audits).await?;
        tx.commit().await?;

        Ok(request)
    }

    #[instrument(skip(self))]
    pub async fn list_queued_evaluation_request_ids(&self) -> Result<Vec<String>, StoreError> {
        Ok(sqlx::query_scalar(
            r#"
            SELECT id
            FROM access_requests
            WHERE evaluation_state = 'queued' AND approval_status = 'pending'
            ORDER BY created_at, id
            "#,
        )
        .fetch_all(&self.pool)
        .await?)
    }

    #[instrument(skip(self))]
    pub async fn claim_evaluation(
        &self,
        request_id: &str,
        started_at: DateTime<Utc>,
    ) -> Result<Option<AccessRequest>, StoreError> {
        let mut tx = self.pool.begin().await?;
        // Make the first statement a write. A read followed by a write lets two deferred SQLite
        // transactions acquire shared snapshots and then fail immediately while upgrading, even
        // when busy_timeout is configured. The atomic transition serializes only this short claim.
        let claimed_row = sqlx::query(
            r#"
            UPDATE access_requests
            SET evaluation_state = 'running', updated_at = ?
            WHERE id = ? AND evaluation_state = 'queued' AND approval_status = 'pending'
            RETURNING
                id, resource, policy_mode, approval_status, evaluation_state, final_decision,
                provider_kind, rendered_prompt, provider_input_json, llm_suggestion_json,
                automatic_decision_json, context_json, created_at, updated_at, resolved_at
            "#,
        )
        .bind(started_at.to_rfc3339())
        .bind(request_id)
        .fetch_optional(&mut *tx)
        .await?;

        let Some(claimed_row) = claimed_row else {
            let row = sqlx::query(
                r#"
                SELECT
                    id, resource, policy_mode, approval_status, evaluation_state, final_decision,
                    provider_kind, rendered_prompt, provider_input_json, llm_suggestion_json,
                    automatic_decision_json, context_json, created_at, updated_at, resolved_at
                FROM access_requests
                WHERE id = ?
                "#,
            )
            .bind(request_id)
            .fetch_optional(&mut *tx)
            .await?
            .ok_or_else(|| StoreError::NotFound(request_id.to_string()))?;
            let mut request = decode_request(&row)?;
            if request.evaluation_state != EvaluationState::Queued {
                return Ok(None);
            }
            request.evaluation_state = EvaluationState::Superseded;
            sqlx::query(
                "UPDATE access_requests SET evaluation_state = ?, updated_at = ? WHERE id = ? AND evaluation_state = 'queued'",
            )
            .bind(enum_to_string(&request.evaluation_state)?)
            .bind(started_at.to_rfc3339())
            .bind(request_id)
            .execute(&mut *tx)
            .await?;
            finish_evaluation_operation_in_tx(&mut tx, request_id, "superseded", started_at)
                .await?;
            tx.commit().await?;
            return Ok(None);
        };
        let mut request = decode_request(&claimed_row)?;

        let operation_result = sqlx::query(
            r#"
            UPDATE interrupted_operations
            SET status = 'running', state_json = ?, heartbeat_at = ?, finished_at = NULL
            WHERE operation_kind = 'llm_evaluation' AND operation_key = ? AND status = 'queued'
            "#,
        )
        .bind(serde_json::json!({"phase": "provider_evaluation"}).to_string())
        .bind(started_at.to_rfc3339())
        .bind(request_id)
        .execute(&mut *tx)
        .await?;
        if operation_result.rows_affected() != 1 {
            return Err(StoreError::NotFound(evaluation_operation_id(request_id)));
        }

        request.evaluation_state = EvaluationState::Running;
        request.updated_at = started_at;
        tx.commit().await?;
        Ok(Some(request))
    }

    #[instrument(skip(self))]
    pub async fn heartbeat_evaluation(
        &self,
        request_id: &str,
        heartbeat_at: DateTime<Utc>,
    ) -> Result<(), StoreError> {
        match self
            .heartbeat_operation(
                &evaluation_operation_id(request_id),
                &serde_json::json!({"phase": "provider_evaluation"}),
                heartbeat_at,
            )
            .await
        {
            Err(StoreError::NotFound(id)) => {
                let status: Option<String> =
                    sqlx::query_scalar("SELECT status FROM interrupted_operations WHERE id = ?")
                        .bind(&id)
                        .fetch_optional(&self.pool)
                        .await?;
                if status.as_deref() == Some("superseded") {
                    Ok(())
                } else {
                    Err(StoreError::NotFound(id))
                }
            }
            result => result,
        }
    }

    #[instrument(skip(self))]
    pub async fn interrupt_evaluation(
        &self,
        request_id: &str,
        interrupted_at: DateTime<Utc>,
    ) -> Result<bool, StoreError> {
        let mut tx = self.pool.begin().await?;
        let result = sqlx::query(
            r#"
            UPDATE access_requests
            SET evaluation_state = 'interrupted', updated_at = ?
            WHERE id = ?
              AND evaluation_state IN ('queued', 'running')
              AND approval_status = 'pending'
            "#,
        )
        .bind(interrupted_at.to_rfc3339())
        .bind(request_id)
        .execute(&mut *tx)
        .await?;

        if result.rows_affected() == 1 {
            finish_evaluation_operation_in_tx(&mut tx, request_id, "interrupted", interrupted_at)
                .await?;
            tx.commit().await?;
            return Ok(true);
        }

        let existing = sqlx::query_scalar::<_, String>(
            "SELECT evaluation_state FROM access_requests WHERE id = ?",
        )
        .bind(request_id)
        .fetch_optional(&mut *tx)
        .await?;
        match existing.as_deref() {
            None => Err(StoreError::NotFound(request_id.to_string())),
            Some("interrupted") => {
                finish_evaluation_operation_in_tx(
                    &mut tx,
                    request_id,
                    "interrupted",
                    interrupted_at,
                )
                .await?;
                tx.commit().await?;
                Ok(false)
            }
            Some(_) => {
                tx.commit().await?;
                Ok(false)
            }
        }
    }

    #[instrument(skip(self, suggestion))]
    pub async fn finalize_evaluation(
        &self,
        request_id: &str,
        suggestion: LlmSuggestion,
    ) -> Result<AccessRequest, StoreError> {
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        let row = sqlx::query(
            r#"
            SELECT
                id, resource, policy_mode, approval_status, evaluation_state, final_decision,
                provider_kind, rendered_prompt, provider_input_json, llm_suggestion_json,
                automatic_decision_json, context_json, created_at, updated_at, resolved_at
            FROM access_requests
            WHERE id = ?
            "#,
        )
        .bind(request_id)
        .fetch_optional(&mut *tx)
        .await?
        .ok_or_else(|| StoreError::NotFound(request_id.to_string()))?;
        let mut request = decode_request(&row)?;

        // Human approval ends automatic authority, but retains the provider's
        // decision and subsequent same-session evidence for human auditing.
        if request.evaluation_state == EvaluationState::Superseded
            && request.llm_suggestion.is_none()
        {
            request.llm_suggestion = Some(suggestion);
            request.updated_at = Utc::now();
            sqlx::query(
                "UPDATE access_requests SET llm_suggestion_json = ?, updated_at = ? WHERE id = ?",
            )
            .bind(option_json_string(&request.llm_suggestion)?)
            .bind(request.updated_at.to_rfc3339())
            .bind(request_id)
            .execute(&mut *tx)
            .await?;
            if let Some(audit) = request.record_llm_suggestion_audit() {
                insert_audits(&mut tx, &[audit]).await?;
            }
            tx.commit().await?;
            return Ok(request);
        }

        if matches!(
            request.evaluation_state,
            EvaluationState::NotRequired
                | EvaluationState::Completed
                | EvaluationState::Failed
                | EvaluationState::Interrupted
                | EvaluationState::Superseded
        ) {
            return Ok(request);
        }
        if request.evaluation_state != EvaluationState::Running {
            return Err(StoreError::InvalidStoredValue {
                field: "evaluation_state",
                value: format!(
                    "request {request_id} cannot finalize from {:?}",
                    request.evaluation_state
                ),
            });
        }
        if request.approval_status != ApprovalStatus::Pending {
            request.evaluation_state = EvaluationState::Superseded;
            request.updated_at = Utc::now();
            sqlx::query(
                "UPDATE access_requests SET evaluation_state = 'superseded', updated_at = ? WHERE id = ? AND evaluation_state = 'running'",
            )
            .bind(request.updated_at.to_rfc3339())
            .bind(request_id)
            .execute(&mut *tx)
            .await?;
            finish_evaluation_operation_in_tx(
                &mut tx,
                request_id,
                "superseded",
                request.updated_at,
            )
            .await?;
            tx.commit().await?;
            return Ok(request);
        }

        let evaluation_failed = suggestion.error.is_some();
        request.llm_suggestion = Some(suggestion);
        let mut audits = request
            .record_llm_suggestion_audit()
            .into_iter()
            .collect::<Vec<_>>();
        if request.policy_mode == PolicyMode::LlmAutomatic {
            let sanitized_context = request
                .provider_input
                .as_ref()
                .map(|input| input.sanitized_context.clone())
                .unwrap_or_else(|| build_prompt_context(&request.context));
            let automatic_decision = evaluate_automatic_disposition(
                request.provider_kind.as_deref(),
                request.provider_input.as_ref(),
                request.llm_suggestion.as_ref(),
                &sanitized_context,
            );
            audits.extend(request.apply_automatic_decision(automatic_decision)?);
        }
        request.evaluation_state = if evaluation_failed {
            EvaluationState::Failed
        } else {
            EvaluationState::Completed
        };
        request.updated_at = request
            .llm_suggestion
            .as_ref()
            .map(|value| value.generated_at)
            .unwrap_or_else(Utc::now);
        let terminal_status = if evaluation_failed {
            "failed"
        } else {
            "completed"
        };

        let result = sqlx::query(
            r#"
            UPDATE access_requests
            SET approval_status = ?, evaluation_state = ?, final_decision = ?,
                llm_suggestion_json = ?, automatic_decision_json = ?, updated_at = ?,
                resolved_at = ?
            WHERE id = ? AND evaluation_state = 'running' AND approval_status = 'pending'
            "#,
        )
        .bind(enum_to_string(&request.approval_status)?)
        .bind(enum_to_string(&request.evaluation_state)?)
        .bind(option_enum_to_string(&request.final_decision)?)
        .bind(option_json_string(&request.llm_suggestion)?)
        .bind(option_json_string(&request.automatic_decision)?)
        .bind(request.updated_at.to_rfc3339())
        .bind(option_datetime(&request.resolved_at))
        .bind(request_id)
        .execute(&mut *tx)
        .await?;
        if result.rows_affected() != 1 {
            return Err(StoreError::InvalidStoredValue {
                field: "evaluation_state",
                value: format!("request {request_id} lost evaluation finalization race"),
            });
        }

        insert_batch_tickets(&mut tx, &request).await?;
        insert_audits(&mut tx, &audits).await?;
        finish_evaluation_operation_in_tx(&mut tx, request_id, terminal_status, request.updated_at)
            .await?;
        tx.commit().await?;
        Ok(request)
    }

    pub async fn update_evaluation_details(
        &self,
        request_id: &str,
        suggestion: LlmSuggestion,
    ) -> Result<AccessRequest, StoreError> {
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        let row = sqlx::query(
            r#"
            SELECT
                id, resource, policy_mode, approval_status, evaluation_state, final_decision,
                provider_kind, rendered_prompt, provider_input_json, llm_suggestion_json,
                automatic_decision_json, context_json, created_at, updated_at, resolved_at
            FROM access_requests
            WHERE id = ?
            "#,
        )
        .bind(request_id)
        .fetch_optional(&mut *tx)
        .await?
        .ok_or_else(|| StoreError::NotFound(request_id.to_string()))?;
        let mut request = decode_request(&row)?;
        let Some(existing) = request.llm_suggestion.as_ref() else {
            return Err(StoreError::InvalidStoredValue {
                field: "llm_suggestion_json",
                value: format!("request {request_id} has no finalized AI decision"),
            });
        };
        if existing.prompt_sha256 != suggestion.prompt_sha256
            || existing.suggested_decision != suggestion.suggested_decision
            || existing.rationale_summary != suggestion.rationale_summary
            || existing.risk_score != suggestion.risk_score
            || existing.batch_decisions != suggestion.batch_decisions
        {
            return Err(StoreError::InvalidStoredValue {
                field: "llm_suggestion_json",
                value: format!("request {request_id} enrichment attempted to change its decision"),
            });
        }
        request.llm_suggestion = Some(suggestion);
        request.updated_at = Utc::now();
        sqlx::query(
            "UPDATE access_requests SET llm_suggestion_json = ?, updated_at = ? WHERE id = ?",
        )
        .bind(option_json_string(&request.llm_suggestion)?)
        .bind(request.updated_at.to_rfc3339())
        .bind(request_id)
        .execute(&mut *tx)
        .await?;
        if let Some(audit) = request.record_llm_review_details_audit() {
            insert_audits(&mut tx, &[audit]).await?;
        }
        tx.commit().await?;
        Ok(request)
    }
}

fn ensure_sqlite_parent_dir(database_url: &str) -> Result<(), StoreError> {
    if let Some(path) = database_url.strip_prefix("sqlite://") {
        let path = PathBuf::from(path);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(sqlx::Error::Io)?;
        }
    }

    Ok(())
}

fn option_datetime(value: &Option<DateTime<Utc>>) -> Option<String> {
    value.as_ref().map(DateTime::<Utc>::to_rfc3339)
}

fn enum_to_string<T: serde::Serialize>(value: &T) -> Result<String, StoreError> {
    let value = serde_json::to_value(value)?;
    Ok(value
        .as_str()
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| value.to_string()))
}

fn option_enum_to_string<T: serde::Serialize>(
    value: &Option<T>,
) -> Result<Option<String>, StoreError> {
    match value {
        Some(value) => Ok(Some(enum_to_string(value)?)),
        None => Ok(None),
    }
}

fn option_json_string<T: serde::Serialize>(
    value: &Option<T>,
) -> Result<Option<String>, StoreError> {
    match value {
        Some(value) => Ok(Some(serde_json::to_string(value)?)),
        None => Ok(None),
    }
}

fn normalized_provider_kind(settings: &PlanktonSettings) -> String {
    let provider_kind = settings.provider_kind.trim().to_ascii_lowercase();
    if provider_kind.is_empty() {
        DEFAULT_USER_PROVIDER_KIND.to_string()
    } else {
        provider_kind
    }
}

fn evaluation_operation_id(request_id: &str) -> String {
    format!("llm-{request_id}")
}

async fn finish_evaluation_operation_in_tx(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    request_id: &str,
    terminal_status: &str,
    finished_at: DateTime<Utc>,
) -> Result<(), StoreError> {
    let result = sqlx::query(
        r#"
        UPDATE interrupted_operations
        SET status = ?, state_json = ?, heartbeat_at = ?, finished_at = ?
        WHERE operation_kind = 'llm_evaluation'
          AND operation_key = ?
          AND status IN ('queued', 'running')
        "#,
    )
    .bind(terminal_status)
    .bind(serde_json::json!({"phase": terminal_status}).to_string())
    .bind(finished_at.to_rfc3339())
    .bind(finished_at.to_rfc3339())
    .bind(request_id)
    .execute(&mut **tx)
    .await?;
    if result.rows_affected() == 1 {
        return Ok(());
    }

    let existing = sqlx::query_scalar::<_, String>(
        r#"
        SELECT status
        FROM interrupted_operations
        WHERE operation_kind = 'llm_evaluation' AND operation_key = ?
        "#,
    )
    .bind(request_id)
    .fetch_optional(&mut **tx)
    .await?;
    match existing {
        Some(status) if status == terminal_status => Ok(()),
        Some(status) => Err(StoreError::InvalidStoredValue {
            field: "operation_status",
            value: format!(
                "evaluation request {request_id} is {status}, cannot finish as {terminal_status}"
            ),
        }),
        None => Err(StoreError::NotFound(evaluation_operation_id(request_id))),
    }
}

async fn insert_audits<'a>(
    tx: &mut sqlx::Transaction<'a, sqlx::Sqlite>,
    audits: &[AuditRecord],
) -> Result<(), StoreError> {
    for audit in audits {
        sqlx::query(
            r#"
            INSERT INTO audit_records (id, request_id, action, actor, note, payload_json, created_at)
            VALUES (?, ?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind(&audit.id)
        .bind(&audit.request_id)
        .bind(enum_to_string(&audit.action)?)
        .bind(&audit.actor)
        .bind(&audit.note)
        .bind(audit.payload.to_string())
        .bind(audit.created_at.to_rfc3339())
        .execute(&mut **tx)
        .await?;
    }

    Ok(())
}

fn decode_request(row: &sqlx::sqlite::SqliteRow) -> Result<AccessRequest, StoreError> {
    let mut context: RequestContext =
        serde_json::from_str(row.try_get::<String, _>("context_json")?.as_str())?;
    context.resource = row.try_get("resource")?;
    let policy_mode = parse_enum(row.try_get::<String, _>("policy_mode")?.as_str())?;
    let approval_status = parse_enum(row.try_get::<String, _>("approval_status")?.as_str())?;
    let evaluation_state = parse_enum(row.try_get::<String, _>("evaluation_state")?.as_str())?;
    let final_decision = match row.try_get::<Option<String>, _>("final_decision")? {
        Some(value) => Some(parse_enum(value.as_str())?),
        None => None,
    };
    let provider_input = parse_optional_json::<ProviderInputSnapshot>(
        row.try_get::<Option<String>, _>("provider_input_json")?,
    )?;
    let llm_suggestion = parse_optional_json::<LlmSuggestion>(
        row.try_get::<Option<String>, _>("llm_suggestion_json")?,
    )?;
    let automatic_decision = parse_optional_json::<AutomaticDecisionTrace>(
        row.try_get::<Option<String>, _>("automatic_decision_json")?,
    )?;

    Ok(AccessRequest {
        id: row.try_get("id")?,
        context,
        policy_mode,
        approval_status,
        evaluation_state,
        final_decision,
        provider_kind: row.try_get("provider_kind")?,
        rendered_prompt: row.try_get("rendered_prompt")?,
        provider_input,
        llm_suggestion,
        automatic_decision,
        created_at: parse_datetime(row.try_get::<String, _>("created_at")?.as_str())?,
        updated_at: parse_datetime(row.try_get::<String, _>("updated_at")?.as_str())?,
        resolved_at: row
            .try_get::<Option<String>, _>("resolved_at")?
            .map(|value| parse_datetime(value.as_str()))
            .transpose()?,
    })
}

fn decode_audit(row: &sqlx::sqlite::SqliteRow) -> Result<AuditRecord, StoreError> {
    let payload: Value = serde_json::from_str(row.try_get::<String, _>("payload_json")?.as_str())?;
    let action = parse_enum(row.try_get::<String, _>("action")?.as_str())?;

    Ok(AuditRecord {
        id: row.try_get("id")?,
        request_id: row.try_get("request_id")?,
        action,
        actor: row.try_get("actor")?,
        note: row.try_get("note")?,
        payload,
        created_at: parse_datetime(row.try_get::<String, _>("created_at")?.as_str())?,
    })
}

fn parse_datetime(value: &str) -> Result<DateTime<Utc>, StoreError> {
    DateTime::parse_from_rfc3339(value)
        .map(|timestamp| timestamp.with_timezone(&Utc))
        .map_err(|_| StoreError::InvalidDateTime(value.to_string()))
}

fn parse_enum<T>(value: &str) -> Result<T, StoreError>
where
    T: for<'de> serde::Deserialize<'de>,
{
    let quoted = format!("\"{value}\"");
    Ok(serde_json::from_str(&quoted)?)
}

fn parse_optional_json<T>(value: Option<String>) -> Result<Option<T>, StoreError>
where
    T: for<'de> serde::Deserialize<'de>,
{
    match value {
        Some(value) => Ok(Some(serde_json::from_str(value.as_str())?)),
        None => Ok(None),
    }
}

#[cfg(test)]
mod tests {
    use tempfile::tempdir;

    use plankton_core::{
        load_settings, request_llm_suggestion, ApprovalStatus, AuditAction, AutomaticDisposition,
        BatchResourceDecision, CallChainNode, CallChainNodeSource, CallChainPreviewStatus,
        Decision, EvaluationState, LlmReviewDetailState, LlmReviewProgress, LlmSuggestion,
        PolicyMode, ProviderTrace, RequestContext, SuggestedDecision,
        APPROVAL_BATCH_TICKET_TTL_SECONDS,
    };

    use super::SqliteStore;

    fn test_settings() -> plankton_core::PlanktonSettings {
        let temp = tempdir().expect("temp directory should be created");
        let mut settings = load_settings().expect("default settings should load");
        settings.database_url = format!("sqlite://{}", temp.path().join("plankton.db").display());
        settings.provider_kind = "mock".to_string();
        settings.llm_approval_allow_enabled = true;
        settings.llm_approval_deny_enabled = true;
        settings.llm_approval_escalate_enabled = true;
        settings
    }

    async fn evaluate_queued_request(
        store: &SqliteStore,
        settings: &plankton_core::PlanktonSettings,
        request: plankton_core::AccessRequest,
    ) -> plankton_core::AccessRequest {
        let running = store
            .claim_evaluation(&request.id, chrono::Utc::now())
            .await
            .expect("claim queued evaluation")
            .expect("request should be queued");
        let suggestion = request_llm_suggestion(
            settings,
            running.policy_mode,
            running
                .provider_input
                .as_ref()
                .expect("queued request should persist provider input"),
        )
        .await;
        store
            .finalize_evaluation(&running.id, suggestion)
            .await
            .expect("finalize evaluation")
    }

    #[tokio::test]
    async fn human_review_ids_match_domain_rules_without_loading_evidence() {
        let settings = test_settings();
        let store = SqliteStore::new(&settings).await.unwrap();
        let mut request = store
            .submit_request(
                &settings,
                RequestContext::new(
                    "secret/review-ids".into(),
                    "Review query".into(),
                    "test".into(),
                ),
                PolicyMode::ManualOnly,
            )
            .await
            .unwrap();
        // These fields must not be read by a status-only monitor, even if an
        // unrelated evidence payload is malformed or expensive to decode.
        sqlx::query("UPDATE access_requests SET context_json = 'invalid', llm_suggestion_json = 'invalid' WHERE id = ?")
                .bind(&request.id).execute(&store.pool).await.unwrap();
        for policy in [
            PolicyMode::ManualOnly,
            PolicyMode::Assisted,
            PolicyMode::LlmAutomatic,
        ] {
            for approval in [
                ApprovalStatus::Pending,
                ApprovalStatus::Approved,
                ApprovalStatus::Rejected,
            ] {
                for evaluation in [
                    EvaluationState::NotRequired,
                    EvaluationState::Queued,
                    EvaluationState::Running,
                    EvaluationState::Completed,
                    EvaluationState::Failed,
                    EvaluationState::Interrupted,
                    EvaluationState::Superseded,
                ] {
                    request.policy_mode = policy;
                    request.approval_status = approval;
                    request.evaluation_state = evaluation;
                    sqlx::query("UPDATE access_requests SET policy_mode = ?, approval_status = ?, evaluation_state = ? WHERE id = ?")
                            .bind(serde_json::to_value(policy).unwrap().as_str().unwrap())
                            .bind(serde_json::to_value(approval).unwrap().as_str().unwrap())
                            .bind(serde_json::to_value(evaluation).unwrap().as_str().unwrap())
                            .bind(&request.id).execute(&store.pool).await.unwrap();
                    let ids = store.list_human_review_request_ids().await.unwrap();
                    assert_eq!(
                        ids.contains(&request.id),
                        request.human_review_required(),
                        "{policy:?}/{approval:?}/{evaluation:?}"
                    );
                }
            }
        }
    }

    #[tokio::test]
    async fn approval_history_reset_keeps_password_change_data() {
        let settings = test_settings();
        let store = SqliteStore::new(&settings)
            .await
            .expect("store should initialize");
        let request = store
            .submit_request(
                &settings,
                RequestContext::new(
                    "secret/history-reset".into(),
                    "Verify incompatible history reset".into(),
                    "alice".into(),
                ),
                PolicyMode::ManualOnly,
            )
            .await
            .expect("approval request should be stored");
        sqlx::query(
            r#"
            INSERT INTO approval_batch_tickets (
                id, source_request_id, semantic_call_chain_sha256, requested_by, reason,
                shared_resource_metadata_sha256, resource_selector, suggested_decision,
                rationale_summary, risk_score, provider_kind, provider_model, template_version,
                prompt_contract_version, created_at, expires_at
            ) VALUES (?, ?, 'chain', 'alice', 'reason', 'metadata', 'secret/history-reset',
                      'allow', 'bounded', 1, 'mock', NULL, '11', 'prompt_context.v11', ?, ?)
            "#,
        )
        .bind("ticket-reset")
        .bind(&request.id)
        .bind("2026-08-16T00:00:00Z")
        .bind("2026-08-16T01:00:00Z")
        .execute(&store.pool)
        .await
        .expect("batch ticket should be stored");
        sqlx::query(
            r#"
            INSERT INTO password_changes (
                id, batch_id, reason, requested_by, state, version, confirmed_version,
                base_revision, operations_json, operation_ids_json, diff_json,
                successor_change_id, created_at, updated_at, confirmed_at, committed_at,
                error_message
            ) VALUES ('password-reset-sentinel', 'batch', 'unrelated', 'alice', 'draft', 1,
                      NULL, 'revision', '[]', '[]', '{}', NULL, ?, ?, NULL, NULL, NULL)
            "#,
        )
        .bind("2026-08-16T00:00:00Z")
        .bind("2026-08-16T00:00:00Z")
        .execute(&store.pool)
        .await
        .expect("password change sentinel should be stored");

        sqlx::raw_sql(include_str!(
            "../migrations/0011_reset_approval_history.sql"
        ))
        .execute(&store.pool)
        .await
        .expect("history reset should execute");

        for table in ["access_requests", "audit_records", "approval_batch_tickets"] {
            let count = sqlx::query_scalar::<_, i64>(&format!("SELECT COUNT(*) FROM {table}"))
                .fetch_one(&store.pool)
                .await
                .expect("history count should be queryable");
            assert_eq!(count, 0, "history table {table} was not reset");
        }
        let password_changes =
            sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM password_changes")
                .fetch_one(&store.pool)
                .await
                .expect("password change count should be queryable");
        assert_eq!(password_changes, 1);
    }

    #[tokio::test]
    async fn concurrent_evaluation_claims_wait_for_the_sqlite_writer() {
        let settings = test_settings();
        let store = SqliteStore::new(&settings)
            .await
            .expect("store should initialize");
        let first = store
            .submit_request(
                &settings,
                RequestContext::new(
                    "secret/concurrent-a".into(),
                    "Concurrent evaluation A".into(),
                    "alice".into(),
                ),
                PolicyMode::Assisted,
            )
            .await
            .expect("first queued request");
        let second = store
            .submit_request(
                &settings,
                RequestContext::new(
                    "secret/concurrent-b".into(),
                    "Concurrent evaluation B".into(),
                    "alice".into(),
                ),
                PolicyMode::Assisted,
            )
            .await
            .expect("second queued request");

        let first_store = store.clone();
        let second_store = store.clone();
        let now = chrono::Utc::now();
        let (first_claim, second_claim) = tokio::join!(
            first_store.claim_evaluation(&first.id, now),
            second_store.claim_evaluation(&second.id, now),
        );

        assert!(first_claim.expect("first concurrent claim").is_some());
        assert!(second_claim.expect("second concurrent claim").is_some());
    }

    #[tokio::test]
    async fn concurrent_automatic_submissions_wait_for_the_sqlite_writer() {
        let settings = test_settings();
        let store = SqliteStore::new(&settings)
            .await
            .expect("store should initialize");

        let mut submissions = tokio::task::JoinSet::new();
        for index in 0..24 {
            let store = store.clone();
            let settings = settings.clone();
            submissions.spawn(async move {
                store
                    .submit_request(
                        &settings,
                        RequestContext::new(
                            format!("secret/concurrent-submit-{index}"),
                            format!("Concurrent automatic submission {index}"),
                            "alice".into(),
                        ),
                        PolicyMode::LlmAutomatic,
                    )
                    .await
            });
        }

        let mut request_ids = Vec::new();
        while let Some(result) = submissions.join_next().await {
            let request = result
                .expect("submission task should complete")
                .expect("concurrent submission should wait for the SQLite writer");
            request_ids.push(request.id);
        }
        request_ids.sort();
        request_ids.dedup();
        assert_eq!(request_ids.len(), 24);
    }

    #[tokio::test]
    async fn persists_request_and_decision_round_trip() {
        let temp = tempdir().expect("temp directory should be created");
        let mut settings = load_settings().expect("default settings should load");
        settings.database_url = format!("sqlite://{}", temp.path().join("plankton.db").display());
        settings.provider_kind = "mock".to_string();

        let store = SqliteStore::new(&settings)
            .await
            .expect("store should initialize");

        let request = store
            .submit_request(
                &settings,
                RequestContext::new(
                    "secret/api-token".to_string(),
                    "Need smoke test access".to_string(),
                    "alice".to_string(),
                ),
                PolicyMode::ManualOnly,
            )
            .await
            .expect("request should be inserted");

        let updated = store
            .record_decision(
                &settings,
                &request.id,
                Decision::Allow,
                "reviewer",
                Some("approved".to_string()),
            )
            .await
            .expect("decision should be persisted");

        assert_eq!(
            updated.approval_status,
            plankton_core::ApprovalStatus::Approved
        );

        let fetched = store
            .get_request(&request.id)
            .await
            .expect("request should load");
        assert_eq!(fetched.audit_records.len(), 2);
    }

    #[tokio::test]
    async fn persists_assisted_request_with_llm_suggestion() {
        let temp = tempdir().expect("temp directory should be created");
        let mut settings = load_settings().expect("default settings should load");
        settings.database_url = format!("sqlite://{}", temp.path().join("plankton.db").display());
        settings.provider_kind = "mock".to_string();

        let store = SqliteStore::new(&settings)
            .await
            .expect("store should initialize");

        let mut context = RequestContext::new(
            "secret/dev-token".to_string(),
            "Need smoke test access".to_string(),
            "alice".to_string(),
        );
        context
            .metadata
            .insert("environment".to_string(), "dev".to_string());

        let request = store
            .submit_request(&settings, context, PolicyMode::Assisted)
            .await
            .expect("request should be inserted");
        let request = evaluate_queued_request(&store, &settings, request).await;

        assert_eq!(request.policy_mode, PolicyMode::Assisted);
        assert_eq!(request.provider_kind.as_deref(), Some("mock"));
        assert!(request.provider_input.is_some());
        assert!(request.llm_suggestion.is_some());

        let fetched = store
            .get_request(&request.id)
            .await
            .expect("request should load");
        assert_eq!(fetched.audit_records.len(), 2);
        assert!(fetched
            .audit_records
            .iter()
            .any(|record| record.action == plankton_core::AuditAction::LlmSuggestionGenerated));
    }

    #[tokio::test]
    async fn persists_human_override_audit_for_assisted_requests() {
        let temp = tempdir().expect("temp directory should be created");
        let mut settings = load_settings().expect("default settings should load");
        settings.database_url = format!("sqlite://{}", temp.path().join("plankton.db").display());
        settings.provider_kind = "mock".to_string();

        let store = SqliteStore::new(&settings)
            .await
            .expect("store should initialize");

        let request = store
            .submit_request(
                &settings,
                RequestContext::new(
                    "secret/dev-token".to_string(),
                    "Need smoke test access".to_string(),
                    "alice".to_string(),
                ),
                PolicyMode::Assisted,
            )
            .await
            .expect("request should be inserted");
        let request = evaluate_queued_request(&store, &settings, request).await;

        let updated = store
            .record_decision(
                &settings,
                &request.id,
                Decision::Deny,
                "reviewer",
                Some("override mock allow".to_string()),
            )
            .await
            .expect("decision should be persisted");

        assert_eq!(
            updated.approval_status,
            plankton_core::ApprovalStatus::Rejected
        );

        let fetched = store
            .get_request(&request.id)
            .await
            .expect("request should load");
        assert!(fetched
            .audit_records
            .iter()
            .any(|record| record.action == plankton_core::AuditAction::HumanDecisionOverrodeLlm));
    }

    #[tokio::test]
    async fn persists_llm_automatic_allow_with_system_auto_audits() {
        let settings = test_settings();

        let store = SqliteStore::new(&settings)
            .await
            .expect("store should initialize");

        let mut context = RequestContext::new(
            "secret/dev-token".to_string(),
            "Need smoke test access".to_string(),
            "alice".to_string(),
        );
        context
            .metadata
            .insert("environment".to_string(), "dev".to_string());

        let request = store
            .submit_request(&settings, context, PolicyMode::LlmAutomatic)
            .await
            .expect("automatic request should be inserted");
        let request = evaluate_queued_request(&store, &settings, request).await;

        assert_eq!(request.approval_status, ApprovalStatus::Approved);
        assert_eq!(request.final_decision, Some(Decision::Allow));
        assert_eq!(
            request
                .automatic_decision
                .as_ref()
                .map(|decision| decision.auto_disposition),
            Some(AutomaticDisposition::Allow)
        );
        assert!(request.provider_input.is_some());
        assert!(request.llm_suggestion.is_some());

        let fetched = store
            .get_request(&request.id)
            .await
            .expect("request should load");
        assert_eq!(fetched.audit_records.len(), 4);
        assert_eq!(
            fetched
                .audit_records
                .iter()
                .filter(|record| record.action == AuditAction::ApprovalRecorded)
                .count(),
            1
        );
        assert!(fetched
            .audit_records
            .iter()
            .any(|record| record.action == AuditAction::AutomaticDecisionRecorded));
        assert_eq!(
            sqlx::query_scalar::<_, String>(
                "SELECT actor_type FROM audit_records WHERE request_id = ? AND action = 'approval_recorded'"
            )
            .bind(&request.id)
            .fetch_one(&store.pool)
            .await
            .expect("approval actor_type should be queryable"),
            "system_auto"
        );
    }

    #[tokio::test]
    async fn retains_environment_evidence_and_allows_low_risk_request() {
        let settings = test_settings();

        let store = SqliteStore::new(&settings)
            .await
            .expect("store should initialize");

        let mut context = RequestContext::new(
            "secret/dev-token".to_string(),
            "Need smoke test access".to_string(),
            "alice".to_string(),
        );
        context.env_vars.insert(
            "OPENAI_API_KEY".to_string(),
            "sk-test-super-secret-value".to_string(),
        );

        let request = store
            .submit_request(&settings, context, PolicyMode::LlmAutomatic)
            .await
            .expect("automatic request should be inserted");
        let request = evaluate_queued_request(&store, &settings, request).await;

        assert_eq!(request.approval_status, ApprovalStatus::Approved);
        assert_eq!(request.final_decision, Some(Decision::Allow));
        assert!(request.provider_input.is_some());
        assert!(request.llm_suggestion.is_some());
        assert_eq!(
            request
                .automatic_decision
                .as_ref()
                .map(|decision| decision.auto_disposition),
            Some(AutomaticDisposition::Allow)
        );
        assert_eq!(
            request
                .automatic_decision
                .as_ref()
                .map(|decision| decision.provider_called),
            Some(true)
        );

        let fetched = store
            .get_request(&request.id)
            .await
            .expect("request should load");
        assert_eq!(fetched.audit_records.len(), 4);
        assert!(fetched
            .audit_records
            .iter()
            .any(|record| record.action == AuditAction::AutomaticDecisionRecorded));
        assert_eq!(
            sqlx::query_scalar::<_, String>(
                "SELECT actor_type FROM audit_records WHERE request_id = ? AND action = 'approval_recorded'"
            )
            .bind(&request.id)
            .fetch_one(&store.pool)
            .await
            .expect("automatic approval actor_type should be queryable"),
            "system_auto"
        );
    }

    #[tokio::test]
    async fn preserves_evidence_in_storage_and_provider_contexts() {
        let temp = tempdir().expect("temp directory should be created");
        let mut settings = load_settings().expect("default settings should load");
        settings.database_url = format!("sqlite://{}", temp.path().join("plankton.db").display());
        settings.provider_kind = "mock".to_string();

        let store = SqliteStore::new(&settings)
            .await
            .expect("store should initialize");

        let mut context = RequestContext::new(
            "secret/demo".to_string(),
            "Need smoke test access".to_string(),
            "alice".to_string(),
        );
        context.script_path = Some("/Users/zqqqqz2000/private/run-secret.sh".to_string());
        let mut shell = CallChainNode::best_effort_path("/Users/zqqqqz2000/private/run-secret.sh");
        shell.process_name = Some("bash".to_string());
        shell.executable_path = Some("/bin/bash".to_string());
        shell.argv = vec![
            "/bin/bash".to_string(),
            "/Users/zqqqqz2000/private/run-secret.sh".to_string(),
            "--token=aaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".to_string(),
        ];
        shell.preview_text = Some("echo secret".into());
        context.call_chain = vec![shell];
        context.env_vars.insert(
            "OPENAI_API_KEY".to_string(),
            "sk-test-super-secret-value".to_string(),
        );
        context.env_vars.insert(
            "SESSION_TOKEN".to_string(),
            "super-secret-session-token".to_string(),
        );
        context.metadata.insert(
            "api_token".to_string(),
            "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".to_string(),
        );

        let request = store
            .submit_request(&settings, context, PolicyMode::Assisted)
            .await
            .expect("request should be inserted");

        let (resource, context_json, provider_input_json): (String, String, String) =
            sqlx::query_as(
                r#"
            SELECT resource, context_json, provider_input_json
            FROM access_requests
            WHERE id = ?
            "#,
            )
            .bind(&request.id)
            .fetch_one(&store.pool)
            .await
            .expect("request payloads should be queryable");

        assert_eq!(resource, "secret/demo");
        assert!(context_json.contains("/Users/zqqqqz2000/private/run-secret.sh"));
        assert!(provider_input_json.contains("/Users/zqqqqz2000/private/run-secret.sh"));
        assert!(provider_input_json.contains("\"resource\":\"secret/demo\""));
        assert!(provider_input_json.contains("\"resource_tags\":"));
        assert!(provider_input_json.contains("\"metadata\":"));
        assert!(provider_input_json.contains("\"reason\":\"Need smoke test access\""));
        assert!(provider_input_json.contains("\"requested_by\":\"alice\""));
        assert!(provider_input_json
            .contains("\"script_path\":\"/Users/zqqqqz2000/private/run-secret.sh\""));
        assert!(provider_input_json.contains("\"call_chain\":"));
        assert!(provider_input_json.contains("\"call_chain_details\":"));
        assert!(provider_input_json
            .contains("\"env_var_names\":[\"OPENAI_API_KEY\",\"SESSION_TOKEN\"]"));
        assert!(provider_input_json.contains("\"allowed_read_files\":"));
        assert!(provider_input_json.contains("/Users/zqqqqz2000/private/run-secret.sh"));
        assert!(provider_input_json.contains("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"));
        assert!(provider_input_json.contains("--token="));
        assert!(context_json.contains("sk-test-super-secret-value"));
        assert!(context_json.contains("super-secret-session-token"));
        assert!(context_json.contains("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"));
        assert!(!context_json.contains("\"env_vars\":{}"));
        assert!(context_json.contains("--token="));
        assert!(context_json.contains("\"preview_text\":\"echo secret\""));
    }

    #[tokio::test]
    async fn records_repeated_allow_deny_and_pending_requests_without_state_leaks() {
        let temp = tempdir().expect("temp directory should be created");
        let mut settings = load_settings().expect("default settings should load");
        settings.database_url = format!("sqlite://{}", temp.path().join("plankton.db").display());
        settings.provider_kind = "mock".to_string();

        let store = SqliteStore::new(&settings)
            .await
            .expect("store should initialize");

        let allow_request = store
            .submit_request(
                &settings,
                RequestContext::new(
                    "secret/shared-token".to_string(),
                    "Need shared access".to_string(),
                    "alice".to_string(),
                ),
                PolicyMode::ManualOnly,
            )
            .await
            .expect("allow request should be inserted");
        store
            .record_decision(
                &settings,
                &allow_request.id,
                Decision::Allow,
                "reviewer",
                Some("approved".to_string()),
            )
            .await
            .expect("allow decision should persist");

        tokio::time::sleep(std::time::Duration::from_millis(5)).await;

        let superseding_allow = store
            .submit_request(
                &settings,
                RequestContext::new(
                    "secret/shared-token".to_string(),
                    "Need newer shared access".to_string(),
                    "alice".to_string(),
                ),
                PolicyMode::Assisted,
            )
            .await
            .expect("superseding allow request should be inserted");
        store
            .interrupt_evaluation(&superseding_allow.id, chrono::Utc::now())
            .await
            .expect("AI evaluation should become unavailable before human review");
        store
            .record_decision(
                &settings,
                &superseding_allow.id,
                Decision::Allow,
                "reviewer",
                Some("approved again".to_string()),
            )
            .await
            .expect("superseding allow decision should persist");

        let denied_request = store
            .submit_request(
                &settings,
                RequestContext::new(
                    "secret/denied-token".to_string(),
                    "Need denied access".to_string(),
                    "alice".to_string(),
                ),
                PolicyMode::ManualOnly,
            )
            .await
            .expect("denied request should be inserted");
        store
            .record_decision(
                &settings,
                &denied_request.id,
                Decision::Deny,
                "reviewer",
                Some("denied".to_string()),
            )
            .await
            .expect("deny decision should persist");

        let pending_request = store
            .submit_request(
                &settings,
                RequestContext::new(
                    "secret/pending-token".to_string(),
                    "Need pending access".to_string(),
                    "alice".to_string(),
                ),
                PolicyMode::ManualOnly,
            )
            .await
            .expect("pending request should be inserted");
        assert_eq!(pending_request.approval_status, ApprovalStatus::Pending);

        let dashboard = store
            .dashboard(20)
            .await
            .expect("dashboard should load all request states");
        assert_eq!(dashboard.pending_requests, vec![pending_request]);
        assert!(dashboard
            .recent_audit_records
            .iter()
            .any(|record| record.request_id == superseding_allow.id));
        assert!(dashboard
            .recent_audit_records
            .iter()
            .any(|record| record.request_id == denied_request.id));
    }

    #[tokio::test]
    async fn dashboard_limits_audit_history_by_complete_request_groups() {
        let settings = test_settings();
        let store = SqliteStore::new(&settings)
            .await
            .expect("store should initialize");
        let mut request_ids = Vec::new();

        for index in 0..3 {
            let request = store
                .submit_request(
                    &settings,
                    RequestContext::new(
                        format!("secret/grouped/{index}"),
                        format!("Grouped audit request {index}"),
                        "alice".to_string(),
                    ),
                    PolicyMode::ManualOnly,
                )
                .await
                .expect("request should be inserted");
            store
                .record_decision(
                    &settings,
                    &request.id,
                    Decision::Allow,
                    "reviewer",
                    Some(format!("approved {index}")),
                )
                .await
                .expect("decision should persist");
            request_ids.push(request.id);
            tokio::time::sleep(std::time::Duration::from_millis(2)).await;
        }

        let dashboard = store
            .dashboard(2)
            .await
            .expect("dashboard should load grouped audit history");
        let grouped_ids = dashboard
            .recent_audit_records
            .iter()
            .map(|record| record.request_id.as_str())
            .collect::<std::collections::BTreeSet<_>>();

        assert_eq!(grouped_ids.len(), 2);
        assert!(!grouped_ids.contains(request_ids[0].as_str()));
        assert!(grouped_ids.contains(request_ids[1].as_str()));
        assert!(grouped_ids.contains(request_ids[2].as_str()));
        assert_eq!(dashboard.recent_audit_records.len(), 4);
        for request_id in grouped_ids {
            assert_eq!(
                dashboard
                    .recent_audit_records
                    .iter()
                    .filter(|record| record.request_id == request_id)
                    .count(),
                2
            );
        }
    }

    #[tokio::test]
    async fn related_requests_cross_status_and_history_pages_without_mixing_commands() {
        let settings = test_settings();
        let store = SqliteStore::new(&settings).await.expect("store");
        let first = store
            .submit_request(
                &settings,
                batch_context("A", "same intent", 1, "command"),
                PolicyMode::ManualOnly,
            )
            .await
            .expect("first");
        let second = store
            .submit_request(
                &settings,
                batch_context("B", "same intent", 2, "command"),
                PolicyMode::ManualOnly,
            )
            .await
            .expect("second");
        store
            .record_decision(&settings, &second.id, Decision::Allow, "human", None)
            .await
            .expect("resolve second");
        for index in 0..9 {
            let unrelated = store
                .submit_request(
                    &settings,
                    batch_context("C", "same intent", index + 3, "different command"),
                    PolicyMode::ManualOnly,
                )
                .await
                .expect("unrelated");
            store
                .record_decision(&settings, &unrelated.id, Decision::Allow, "human", None)
                .await
                .expect("resolve unrelated");
        }
        let mut other_item = batch_context("D", "same intent", 30, "command");
        other_item
            .resource_metadata
            .insert("item_id".into(), "another item".into());
        store
            .submit_request(&settings, other_item, PolicyMode::ManualOnly)
            .await
            .expect("other item");
        let expired = store
            .submit_request(
                &settings,
                batch_context("E", "same intent", 31, "command"),
                PolicyMode::ManualOnly,
            )
            .await
            .expect("expired");
        sqlx::query("UPDATE access_requests SET created_at = ? WHERE id = ?")
            .bind((first.created_at - chrono::Duration::minutes(6)).to_rfc3339())
            .bind(&expired.id)
            .execute(&store.pool)
            .await
            .expect("age request");
        assert!(!store
            .list_resolved_requests("", 8, 0)
            .await
            .expect("history page")
            .iter()
            .any(|request| request.id == second.id));
        for selected in [&first.id, &second.id] {
            let related = store
                .list_related_requests(selected)
                .await
                .expect("related");
            assert_eq!(related.len(), 2);
            assert!(related.iter().any(|request| request.id == first.id
                && request.approval_status == ApprovalStatus::Pending));
            assert!(related.iter().any(|request| request.id == second.id
                && request.approval_status == ApprovalStatus::Approved));
        }
    }

    #[tokio::test]
    async fn resolved_request_history_has_independent_count_search_and_pages() {
        let settings = test_settings();
        let store = SqliteStore::new(&settings)
            .await
            .expect("store should initialize");
        for index in 0..17 {
            let request = store
                .submit_request(
                    &settings,
                    RequestContext::new(
                        format!("secret/resolved/{index}"),
                        format!("Resolved reason {index}"),
                        if index == 16 {
                            "special-actor"
                        } else {
                            "alice"
                        }
                        .to_string(),
                    ),
                    PolicyMode::ManualOnly,
                )
                .await
                .expect("resolved request should be inserted");
            store
                .record_decision(&settings, &request.id, Decision::Allow, "reviewer", None)
                .await
                .expect("resolved decision should persist");
        }

        assert_eq!(
            store
                .count_resolved_requests("")
                .await
                .expect("count resolved requests"),
            17
        );
        assert_eq!(
            store
                .list_resolved_requests("", 8, 8)
                .await
                .expect("second resolved page")
                .len(),
            8
        );
        let searched = store
            .list_resolved_requests("special-actor", 8, 0)
            .await
            .expect("search resolved requests");
        assert_eq!(searched.len(), 1);
        assert_eq!(searched[0].context.requested_by, "special-actor");
    }

    #[tokio::test]
    async fn installs_daemon_resource_and_diagnostics_schema() {
        let settings = test_settings();
        let store = SqliteStore::new(&settings)
            .await
            .expect("store should initialize");

        let required_tables = [
            "backend_bindings",
            "vault_manifests",
            "resource_items",
            "resource_sections",
            "resource_fields",
            "resource_tags",
            "resource_item_tags",
            "resource_aliases",
            "resource_search_documents",
            "sync_states",
            "interrupted_operations",
            "diagnostic_errors",
        ];

        for table in required_tables {
            let exists = sqlx::query_scalar::<_, i64>(
                "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = ?",
            )
            .bind(table)
            .fetch_one(&store.pool)
            .await
            .expect("schema query should succeed");
            assert_eq!(exists, 1, "missing table {table}");
        }
    }

    #[tokio::test]
    async fn async_evaluation_migration_backfills_legacy_rows_without_re_evaluation() {
        let temp = tempdir().expect("temp directory");
        let database_url = format!("sqlite://{}", temp.path().join("legacy.db").display());
        let options = database_url
            .parse::<sqlx::sqlite::SqliteConnectOptions>()
            .expect("sqlite options")
            .create_if_missing(true);
        let pool = sqlx::SqlitePool::connect_with(options)
            .await
            .expect("legacy database");
        sqlx::raw_sql(
            r#"
            CREATE TABLE access_requests (
                id TEXT PRIMARY KEY NOT NULL,
                resource TEXT NOT NULL,
                requested_by TEXT NOT NULL,
                reason TEXT NOT NULL,
                policy_mode TEXT NOT NULL,
                approval_status TEXT NOT NULL,
                final_decision TEXT,
                provider_kind TEXT,
                rendered_prompt TEXT NOT NULL,
                provider_input_json TEXT,
                llm_suggestion_json TEXT,
                automatic_decision_json TEXT,
                context_json TEXT NOT NULL,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL,
                resolved_at TEXT
            );
            CREATE TABLE interrupted_operations (
                id TEXT PRIMARY KEY NOT NULL,
                operation_kind TEXT NOT NULL,
                operation_key TEXT NOT NULL,
                state_json TEXT NOT NULL,
                status TEXT NOT NULL,
                started_at TEXT NOT NULL,
                heartbeat_at TEXT NOT NULL,
                finished_at TEXT
            );
            CREATE VIEW approval_requests AS
            SELECT id, resource FROM access_requests;
            INSERT INTO access_requests (
                id, resource, requested_by, reason, policy_mode, approval_status, final_decision,
                provider_kind, rendered_prompt, provider_input_json, llm_suggestion_json,
                automatic_decision_json, context_json, created_at, updated_at, resolved_at
            ) VALUES
                ('legacy-manual', 'secret/manual', 'alice', 'manual', 'manual_only', 'pending',
                 NULL, NULL, '', NULL, NULL, NULL, '{}', '2026-01-01T00:00:00Z',
                 '2026-01-01T00:00:00Z', NULL),
                ('legacy-auto', 'secret/auto', 'alice', 'auto', 'llm_automatic', 'approved',
                 'allow', 'mock', '', '{}', '{}', '{}', '{}', '2026-01-01T00:00:00Z',
                 '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z');
            "#,
        )
        .execute(&pool)
        .await
        .expect("legacy schema");

        sqlx::raw_sql(include_str!("../migrations/0008_async_evaluation.sql"))
            .execute(&pool)
            .await
            .expect("async evaluation migration");

        let states = sqlx::query_as::<_, (String, String)>(
            "SELECT id, evaluation_state FROM access_requests ORDER BY id",
        )
        .fetch_all(&pool)
        .await
        .expect("backfilled states");
        assert_eq!(
            states,
            vec![
                ("legacy-auto".into(), "completed".into()),
                ("legacy-manual".into(), "not_required".into()),
            ]
        );
    }

    #[tokio::test]
    async fn persists_queued_running_and_completed_evaluation_states() {
        let settings = test_settings();
        let store = SqliteStore::new(&settings)
            .await
            .expect("store should initialize");
        let request = store
            .submit_request(
                &settings,
                RequestContext::new(
                    "secret/dev-token".into(),
                    "Need asynchronous evaluation".into(),
                    "alice".into(),
                ),
                PolicyMode::LlmAutomatic,
            )
            .await
            .expect("request should be durably queued");

        assert_eq!(request.approval_status, ApprovalStatus::Pending);
        assert_eq!(request.evaluation_state, EvaluationState::Queued);
        assert!(request.llm_suggestion.is_none());
        assert_eq!(
            evaluation_operation_status(&store, &request.id).await,
            "queued"
        );

        let running = store
            .claim_evaluation(&request.id, chrono::Utc::now())
            .await
            .expect("claim should persist")
            .expect("queued request should be claimable");
        assert_eq!(running.evaluation_state, EvaluationState::Running);
        assert_eq!(
            evaluation_operation_status(&store, &request.id).await,
            "running"
        );

        let completed = store
            .finalize_evaluation(&request.id, suggestion_for(&running, None))
            .await
            .expect("evaluation should complete");
        assert_eq!(completed.evaluation_state, EvaluationState::Completed);
        assert_eq!(completed.approval_status, ApprovalStatus::Approved);
        assert_eq!(
            evaluation_operation_status(&store, &request.id).await,
            "completed"
        );

        let repeated = store
            .finalize_evaluation(&request.id, suggestion_for(&running, None))
            .await
            .expect("terminal finalization should be idempotent");
        assert_eq!(repeated, completed);
    }

    #[tokio::test]
    async fn progressive_review_details_update_after_the_decision_without_changing_it() {
        let settings = test_settings();
        let store = SqliteStore::new(&settings)
            .await
            .expect("store should initialize");
        let request = store
            .submit_request(
                &settings,
                RequestContext::new(
                    "secret/dev-token".into(),
                    "Need progressive review details".into(),
                    "alice".into(),
                ),
                PolicyMode::LlmAutomatic,
            )
            .await
            .expect("request should be queued");
        let running = store
            .claim_evaluation(&request.id, chrono::Utc::now())
            .await
            .expect("claim")
            .expect("queued request");
        let mut decision = suggestion_for(&running, None);
        decision.provider_trace = Some(review_trace(LlmReviewDetailState::Running, 0, 6));
        let decided = store
            .finalize_evaluation(&request.id, decision.clone())
            .await
            .expect("decision should finalize");
        assert_eq!(decided.approval_status, ApprovalStatus::Approved);

        let mut enriched = decision;
        enriched.provider_trace = Some(review_trace(LlmReviewDetailState::Complete, 6, 6));
        let updated = store
            .update_evaluation_details(&request.id, enriched)
            .await
            .expect("details should update after resolution");
        assert_eq!(updated.final_decision, Some(Decision::Allow));
        assert_eq!(updated.approval_status, ApprovalStatus::Approved);
        assert_eq!(
            updated
                .llm_suggestion
                .as_ref()
                .and_then(|suggestion| suggestion.provider_trace.as_ref())
                .and_then(|trace| trace.review_progress.as_ref())
                .map(|progress| progress.state),
            Some(LlmReviewDetailState::Complete)
        );
        let audits = store
            .get_request(&request.id)
            .await
            .expect("request")
            .audit_records;
        assert!(audits
            .iter()
            .any(|audit| audit.action == AuditAction::LlmReviewDetailsUpdated));
    }

    #[tokio::test]
    async fn provider_failure_retains_human_review_and_records_failed_state() {
        let settings = test_settings();
        let store = SqliteStore::new(&settings)
            .await
            .expect("store should initialize");
        let request = store
            .submit_request(
                &settings,
                RequestContext::new(
                    "secret/dev-token".into(),
                    "Need assisted evaluation".into(),
                    "alice".into(),
                ),
                PolicyMode::Assisted,
            )
            .await
            .expect("request should be queued");
        let running = store
            .claim_evaluation(&request.id, chrono::Utc::now())
            .await
            .expect("claim")
            .expect("queued request");

        let failed = store
            .finalize_evaluation(
                &request.id,
                suggestion_for(&running, Some("provider unavailable".into())),
            )
            .await
            .expect("failure should be durably finalized");

        assert_eq!(failed.evaluation_state, EvaluationState::Failed);
        assert_eq!(failed.approval_status, ApprovalStatus::Pending);
        assert_eq!(
            evaluation_operation_status(&store, &request.id).await,
            "failed"
        );
    }

    #[tokio::test]
    async fn stale_running_evaluation_is_interrupted_without_resolving_approval() {
        let settings = test_settings();
        let store = SqliteStore::new(&settings)
            .await
            .expect("store should initialize");
        let request = store
            .submit_request(
                &settings,
                RequestContext::new(
                    "secret/dev-token".into(),
                    "Need recoverable evaluation".into(),
                    "alice".into(),
                ),
                PolicyMode::Assisted,
            )
            .await
            .expect("request should be queued");
        let now = chrono::Utc::now();
        store
            .claim_evaluation(&request.id, now - chrono::Duration::minutes(5))
            .await
            .expect("claim")
            .expect("queued request");

        store
            .recover_stale_operations(now, chrono::Duration::minutes(1))
            .await
            .expect("recovery should succeed");
        let recovered = store
            .get_request(&request.id)
            .await
            .expect("request should remain visible")
            .request;

        assert_eq!(recovered.evaluation_state, EvaluationState::Interrupted);
        assert_eq!(recovered.approval_status, ApprovalStatus::Pending);
        assert_eq!(
            evaluation_operation_status(&store, &request.id).await,
            "interrupted"
        );
    }

    #[tokio::test]
    async fn human_decision_preempts_ai_without_losing_late_evidence() {
        let settings = test_settings();
        let store = SqliteStore::new(&settings).await.expect("store");
        for mode in [PolicyMode::Assisted, PolicyMode::LlmAutomatic] {
            for decision in [Decision::Allow, Decision::Deny] {
                let request = store
                    .submit_request(
                        &settings,
                        RequestContext::new(
                            "secret/prod-token".into(),
                            "human precedence".into(),
                            "alice".into(),
                        ),
                        mode,
                    )
                    .await
                    .expect("request");
                let running = store
                    .claim_evaluation(&request.id, chrono::Utc::now())
                    .await
                    .expect("claim")
                    .expect("queued request");
                let human = store
                    .record_decision(
                        &settings,
                        &request.id,
                        decision,
                        "reviewer",
                        Some("human wins".into()),
                    )
                    .await
                    .expect("human decision must not wait");
                assert_eq!(human.final_decision, Some(decision));
                assert_eq!(human.evaluation_state, EvaluationState::Superseded);
                assert_eq!(
                    evaluation_operation_status(&store, &request.id).await,
                    "superseded"
                );
                store
                    .heartbeat_evaluation(&request.id, chrono::Utc::now())
                    .await
                    .expect("late heartbeat is harmless");

                let mut suggestion = suggestion_for(&running, None);
                suggestion.suggested_decision = match decision {
                    Decision::Allow => SuggestedDecision::Deny,
                    Decision::Deny => SuggestedDecision::Allow,
                };
                suggestion.batch_decisions.push(BatchResourceDecision {
                    resource_selector: "secret/other".into(),
                    suggested_decision: SuggestedDecision::Allow,
                    rationale_summary: "late batch grant must not be issued".into(),
                    risk_score: 0,
                });
                let late = store
                    .finalize_evaluation(&request.id, suggestion.clone())
                    .await
                    .expect("late decision retained for audit");
                assert_eq!(late.final_decision, human.final_decision);
                assert_eq!(late.approval_status, human.approval_status);
                assert_eq!(late.resolved_at, human.resolved_at);
                assert_eq!(late.evaluation_state, EvaluationState::Superseded);
                assert!(late.automatic_decision.is_none());
                assert_eq!(late.llm_suggestion, Some(suggestion.clone()));

                suggestion.provider_trace =
                    Some(review_trace(LlmReviewDetailState::Complete, 2, 2));
                store
                    .update_evaluation_details(&request.id, suggestion.clone())
                    .await
                    .expect("late same-session audit");
                let view = store.get_request(&request.id).await.expect("read back");
                assert_eq!(view.request.final_decision, Some(decision));
                assert_eq!(view.request.llm_suggestion, Some(suggestion));
                assert!(view
                    .audit_records
                    .iter()
                    .any(|audit| audit.action == AuditAction::HumanDecisionOverrodeLlm));
                assert!(view
                    .audit_records
                    .iter()
                    .any(|audit| audit.action == AuditAction::LlmReviewDetailsUpdated));
                assert!(!view
                    .audit_records
                    .iter()
                    .any(|audit| audit.action == AuditAction::AutomaticDecisionRecorded));
                let tickets: i64 = sqlx::query_scalar(
                    "SELECT COUNT(*) FROM approval_batch_tickets WHERE source_request_id = ?",
                )
                .bind(&request.id)
                .fetch_one(&store.pool)
                .await
                .expect("tickets");
                assert_eq!(tickets, 0);
            }
        }
    }

    #[tokio::test]
    async fn human_decision_prevents_queued_evaluation_from_starting() {
        let settings = test_settings();
        let store = SqliteStore::new(&settings).await.expect("store");
        for decision in [Decision::Allow, Decision::Deny] {
            let request = store
                .submit_request(
                    &settings,
                    RequestContext::new("secret/dev-token".into(), "queued".into(), "alice".into()),
                    PolicyMode::LlmAutomatic,
                )
                .await
                .expect("request");
            store
                .record_decision(&settings, &request.id, decision, "reviewer", None)
                .await
                .expect("human");
            assert!(store
                .claim_evaluation(&request.id, chrono::Utc::now())
                .await
                .expect("claim")
                .is_none());
            assert!(!store
                .list_queued_evaluation_request_ids()
                .await
                .expect("queue")
                .contains(&request.id));
            assert_eq!(
                evaluation_operation_status(&store, &request.id).await,
                "superseded"
            );
        }
    }

    #[tokio::test]
    async fn human_preemption_rolls_back_if_operation_cannot_be_updated() {
        let settings = test_settings();
        let store = SqliteStore::new(&settings).await.expect("store");
        let request = store
            .submit_request(
                &settings,
                RequestContext::new(
                    "secret/dev-token".into(),
                    "atomic human decision".into(),
                    "alice".into(),
                ),
                PolicyMode::LlmAutomatic,
            )
            .await
            .expect("request");
        sqlx::query("DELETE FROM interrupted_operations WHERE operation_key = ?")
            .bind(&request.id)
            .execute(&store.pool)
            .await
            .expect("remove operation");
        assert!(store
            .record_decision(&settings, &request.id, Decision::Allow, "reviewer", None)
            .await
            .is_err());
        let view = store.get_request(&request.id).await.expect("read back");
        assert_eq!(view.request.approval_status, ApprovalStatus::Pending);
        assert_eq!(view.request.evaluation_state, EvaluationState::Queued);
        assert_eq!(view.request.final_decision, None);
        assert!(!view
            .audit_records
            .iter()
            .any(|audit| audit.action == AuditAction::ApprovalRecorded));
    }

    #[tokio::test]
    async fn missing_operation_prevents_partial_claim_transition() {
        let settings = test_settings();
        let store = SqliteStore::new(&settings)
            .await
            .expect("store should initialize");
        let request = store
            .submit_request(
                &settings,
                RequestContext::new(
                    "secret/dev-token".into(),
                    "Need atomic claim".into(),
                    "alice".into(),
                ),
                PolicyMode::Assisted,
            )
            .await
            .expect("request should be queued");
        sqlx::query(
            "DELETE FROM interrupted_operations WHERE operation_kind = 'llm_evaluation' AND operation_key = ?",
        )
        .bind(&request.id)
        .execute(&store.pool)
        .await
        .expect("remove operation fixture");

        store
            .claim_evaluation(&request.id, chrono::Utc::now())
            .await
            .expect_err("claim must fail when durable operation state is missing");
        let unchanged = store
            .get_request(&request.id)
            .await
            .expect("request")
            .request;
        assert_eq!(unchanged.evaluation_state, EvaluationState::Queued);
        assert_eq!(unchanged.approval_status, ApprovalStatus::Pending);
    }

    #[tokio::test]
    async fn interrupting_one_evaluation_does_not_touch_other_active_requests() {
        let settings = test_settings();
        let store = SqliteStore::new(&settings)
            .await
            .expect("store should initialize");
        let first = store
            .submit_request(
                &settings,
                RequestContext::new(
                    "secret/first".into(),
                    "interrupt only this request".into(),
                    "alice".into(),
                ),
                PolicyMode::Assisted,
            )
            .await
            .expect("first request");
        let second = store
            .submit_request(
                &settings,
                RequestContext::new(
                    "secret/second".into(),
                    "keep this request active".into(),
                    "alice".into(),
                ),
                PolicyMode::Assisted,
            )
            .await
            .expect("second request");
        store
            .claim_evaluation(&first.id, chrono::Utc::now())
            .await
            .expect("claim first")
            .expect("first queued");
        store
            .claim_evaluation(&second.id, chrono::Utc::now())
            .await
            .expect("claim second")
            .expect("second queued");

        assert!(store
            .interrupt_evaluation(&first.id, chrono::Utc::now())
            .await
            .expect("interrupt first"));
        assert!(!store
            .interrupt_evaluation(&first.id, chrono::Utc::now())
            .await
            .expect("repeated interrupt is idempotent"));

        let interrupted = store
            .get_request(&first.id)
            .await
            .expect("first request")
            .request;
        let untouched = store
            .get_request(&second.id)
            .await
            .expect("second request")
            .request;
        assert_eq!(interrupted.evaluation_state, EvaluationState::Interrupted);
        assert_eq!(interrupted.approval_status, ApprovalStatus::Pending);
        assert_eq!(untouched.evaluation_state, EvaluationState::Running);
        assert_eq!(untouched.approval_status, ApprovalStatus::Pending);
        assert_eq!(
            evaluation_operation_status(&store, &first.id).await,
            "interrupted"
        );
        assert_eq!(
            evaluation_operation_status(&store, &second.id).await,
            "running"
        );
    }

    #[tokio::test]
    async fn batch_requests_share_the_source_session_and_latest_audit() {
        let settings = test_settings();
        let store = SqliteStore::new(&settings).await.unwrap();
        let source = store
            .submit_request(
                &settings,
                batch_context("A", "shared", 1, "same"),
                PolicyMode::LlmAutomatic,
            )
            .await
            .unwrap();
        let running = store
            .claim_evaluation(&source.id, chrono::Utc::now())
            .await
            .unwrap()
            .unwrap();
        let mut suggestion = suggestion_for(&running, None);
        let mut trace = review_trace(LlmReviewDetailState::Running, 0, 6);
        trace.session_id = Some("one-shared-session".into());
        suggestion.provider_trace = Some(trace);
        suggestion.batch_decisions = vec![BatchResourceDecision {
            resource_selector: "B".into(),
            suggested_decision: SuggestedDecision::Allow,
            rationale_summary: "shared decision".into(),
            risk_score: 0,
        }];
        store
            .finalize_evaluation(&source.id, suggestion.clone())
            .await
            .unwrap();
        let reused = store
            .submit_request(
                &settings,
                batch_context("B", "shared", 2, "same"),
                PolicyMode::LlmAutomatic,
            )
            .await
            .unwrap();
        assert_eq!(reused.final_decision, Some(Decision::Allow));
        assert_eq!(
            reused
                .llm_suggestion
                .as_ref()
                .unwrap()
                .provider_trace
                .as_ref()
                .unwrap()
                .session_id
                .as_deref(),
            Some("one-shared-session")
        );
        suggestion
            .provider_trace
            .as_mut()
            .unwrap()
            .review_progress
            .as_mut()
            .unwrap()
            .state = LlmReviewDetailState::Complete;
        store
            .update_evaluation_details(&source.id, suggestion)
            .await
            .unwrap();
        let latest = store.get_request(&reused.id).await.unwrap().request;
        assert_eq!(
            latest
                .llm_suggestion
                .unwrap()
                .provider_trace
                .unwrap()
                .review_progress
                .unwrap()
                .state,
            LlmReviewDetailState::Complete
        );
        let read = crate::SqliteReadStore::new(&settings)
            .await
            .unwrap()
            .get_status(&reused.id)
            .await
            .unwrap()
            .request;
        assert_eq!(
            read.llm_suggestion
                .unwrap()
                .provider_trace
                .unwrap()
                .review_progress
                .unwrap()
                .state,
            LlmReviewDetailState::Complete
        );
        assert_eq!(read.final_decision, Some(Decision::Allow));
        let related = store.list_related_requests(&reused.id).await.unwrap();
        assert_eq!(related.len(), 2);
        for request in related {
            let trace = request.llm_suggestion.unwrap().provider_trace.unwrap();
            assert_eq!(trace.session_id.as_deref(), Some("one-shared-session"));
            assert_eq!(
                trace.review_progress.unwrap().state,
                LlmReviewDetailState::Complete
            );
        }
    }

    #[tokio::test]
    async fn semantic_batch_ticket_reuses_per_key_decision_only_for_matching_context() {
        let settings = test_settings();
        let store = SqliteStore::new(&settings)
            .await
            .expect("store should initialize");
        let source = store
            .submit_request(
                &settings,
                batch_context("SECRET_ID", "shared note", 10, "command-a"),
                PolicyMode::LlmAutomatic,
            )
            .await
            .expect("source request");
        let running = store
            .claim_evaluation(&source.id, chrono::Utc::now())
            .await
            .expect("claim source")
            .expect("source queued");
        let mut suggestion = suggestion_for(&running, None);
        suggestion.batch_decisions = vec![
            BatchResourceDecision {
                resource_selector: "SECRET_KEY".into(),
                suggested_decision: SuggestedDecision::Allow,
                rationale_summary: "same command and credential item".into(),
                risk_score: 18,
            },
            BatchResourceDecision {
                resource_selector: "DENIED_KEY".into(),
                suggested_decision: SuggestedDecision::Deny,
                rationale_summary: "the command must not read this field".into(),
                risk_score: 91,
            },
            BatchResourceDecision {
                resource_selector: "REVIEW_KEY".into(),
                suggested_decision: SuggestedDecision::Escalate,
                rationale_summary: "this field still needs a person".into(),
                risk_score: 67,
            },
        ];
        store
            .finalize_evaluation(&source.id, suggestion)
            .await
            .expect("finalize source");

        let reused = store
            .submit_request(
                &settings,
                batch_context("SECRET_KEY", "shared note", 99, "command-a"),
                PolicyMode::LlmAutomatic,
            )
            .await
            .expect("matching request");
        assert_eq!(reused.evaluation_state, EvaluationState::Completed);
        assert_eq!(reused.approval_status, ApprovalStatus::Approved);
        assert_eq!(reused.final_decision, Some(Decision::Allow));
        let trace = reused.automatic_decision.expect("batch trace");
        assert!(!trace.provider_called);
        assert_eq!(
            trace.batch_source_request_id.as_deref(),
            Some(source.id.as_str())
        );

        let denied = store
            .submit_request(
                &settings,
                batch_context("DENIED_KEY", "shared note", 100, "command-a"),
                PolicyMode::LlmAutomatic,
            )
            .await
            .expect("denied batch request");
        assert_eq!(denied.evaluation_state, EvaluationState::Completed);
        assert_eq!(denied.approval_status, ApprovalStatus::Rejected);
        assert_eq!(denied.final_decision, Some(Decision::Deny));

        let escalated = store
            .submit_request(
                &settings,
                batch_context("REVIEW_KEY", "shared note", 101, "command-a"),
                PolicyMode::LlmAutomatic,
            )
            .await
            .expect("escalated batch request");
        assert_eq!(escalated.evaluation_state, EvaluationState::Completed);
        assert_eq!(escalated.approval_status, ApprovalStatus::Pending);
        assert_eq!(escalated.final_decision, None);

        let changed_command = store
            .submit_request(
                &settings,
                batch_context("SECRET_KEY", "shared note", 102, "command-b"),
                PolicyMode::LlmAutomatic,
            )
            .await
            .expect("changed command request");
        assert_eq!(changed_command.evaluation_state, EvaluationState::Queued);

        let changed_note = store
            .submit_request(
                &settings,
                batch_context("SECRET_KEY", "different note", 103, "command-a"),
                PolicyMode::LlmAutomatic,
            )
            .await
            .expect("changed metadata request");
        assert_eq!(changed_note.evaluation_state, EvaluationState::Queued);

        let untrusted_source = store
            .submit_request(
                &settings,
                batch_context("SECRET_ID", "untrusted note", 104, "command-c"),
                PolicyMode::LlmAutomatic,
            )
            .await
            .expect("untrusted source request");
        let untrusted_running = store
            .claim_evaluation(&untrusted_source.id, chrono::Utc::now())
            .await
            .expect("claim untrusted source")
            .expect("untrusted source queued");
        let mut untrusted_suggestion = suggestion_for(&untrusted_running, None);
        untrusted_suggestion.template_version = "untrusted-template".into();
        untrusted_suggestion.batch_decisions = vec![BatchResourceDecision {
            resource_selector: "SECRET_KEY".into(),
            suggested_decision: SuggestedDecision::Allow,
            rationale_summary: "must not bypass the template guardrail".into(),
            risk_score: 10,
        }];
        let untrusted_result = store
            .finalize_evaluation(&untrusted_source.id, untrusted_suggestion)
            .await
            .expect("finalize untrusted source");
        assert_eq!(untrusted_result.approval_status, ApprovalStatus::Pending);
        let after_untrusted = store
            .submit_request(
                &settings,
                batch_context("SECRET_KEY", "untrusted note", 105, "command-c"),
                PolicyMode::LlmAutomatic,
            )
            .await
            .expect("request after untrusted source");
        assert_eq!(after_untrusted.evaluation_state, EvaluationState::Queued);

        let expired_source = store
            .submit_request(
                &settings,
                batch_context("SECRET_ID", "expired note", 106, "command-d"),
                PolicyMode::LlmAutomatic,
            )
            .await
            .expect("expired source request");
        let expired_running = store
            .claim_evaluation(&expired_source.id, chrono::Utc::now())
            .await
            .expect("claim expired source")
            .expect("expired source queued");
        let mut expired_suggestion = suggestion_for(&expired_running, None);
        expired_suggestion.generated_at =
            chrono::Utc::now() - chrono::Duration::seconds(APPROVAL_BATCH_TICKET_TTL_SECONDS + 1);
        expired_suggestion.batch_decisions = vec![BatchResourceDecision {
            resource_selector: "SECRET_KEY".into(),
            suggested_decision: SuggestedDecision::Allow,
            rationale_summary: "expired decision".into(),
            risk_score: 10,
        }];
        store
            .finalize_evaluation(&expired_source.id, expired_suggestion)
            .await
            .expect("finalize expired source");
        let after_expiry = store
            .submit_request(
                &settings,
                batch_context("SECRET_KEY", "expired note", 107, "command-d"),
                PolicyMode::LlmAutomatic,
            )
            .await
            .expect("request after ticket expiry");
        assert_eq!(after_expiry.evaluation_state, EvaluationState::Queued);
    }

    fn batch_context(resource: &str, note: &str, pid: u32, command: &str) -> RequestContext {
        let mut context = RequestContext::new(
            resource.to_string(),
            "same read-only operation".into(),
            "alice".into(),
        );
        context.resource_metadata.extend([
            ("item_id".into(), "credential-item".into()),
            ("item_title".into(), "Credential".into()),
            ("field_key".into(), resource.into()),
            ("field_label".into(), resource.into()),
            ("record_id".into(), format!("record-{resource}")),
            ("resource_note".into(), note.into()),
            ("source_kind".into(), "plankton".into()),
        ]);
        context.call_chain = vec![CallChainNode {
            pid: Some(pid),
            ppid: Some(pid.saturating_sub(1)),
            process_name: Some("bash".into()),
            executable_path: Some("/bin/bash".into()),
            argv: vec!["bash".into(), "-lc".into(), command.into()],
            resolved_file_path: None,
            source: CallChainNodeSource::BestEffort,
            previewable: false,
            preview_status: CallChainPreviewStatus::NotPreviewable,
            preview_text: None,
            preview_error: None,
        }];
        context
    }

    async fn evaluation_operation_status(store: &SqliteStore, request_id: &str) -> String {
        sqlx::query_scalar(
            "SELECT status FROM interrupted_operations WHERE operation_kind = 'llm_evaluation' AND operation_key = ?",
        )
        .bind(request_id)
        .fetch_one(&store.pool)
        .await
        .expect("evaluation operation status")
    }

    fn suggestion_for(
        request: &plankton_core::AccessRequest,
        error: Option<String>,
    ) -> LlmSuggestion {
        let provider_input = request
            .provider_input
            .as_ref()
            .expect("queued evaluation should persist provider input");
        LlmSuggestion {
            template_id: provider_input.template_id.clone(),
            template_version: provider_input.template_version.clone(),
            prompt_contract_version: provider_input.prompt_contract_version.clone(),
            prompt_sha256: provider_input.prompt_sha256.clone(),
            suggested_decision: if request.context.resource.contains("prod") {
                SuggestedDecision::Deny
            } else {
                SuggestedDecision::Allow
            },
            rationale_summary: "store state transition fixture".into(),
            risk_score: 20,
            batch_decisions: Vec::new(),
            exposure_report: None,
            json_repair_strategy: None,
            provider_kind: "mock".into(),
            provider_model: Some("mock-suggestion-v1".into()),
            provider_response_id: None,
            x_request_id: None,
            provider_trace: None,
            usage: None,
            error,
            generated_at: chrono::Utc::now(),
        }
    }

    fn review_trace(
        state: LlmReviewDetailState,
        completed_units: u16,
        total_units: u16,
    ) -> ProviderTrace {
        ProviderTrace {
            decision_attempts: Vec::new(),
            audit_events: Vec::new(),
            session_configuration: None,
            rendered_prompt: None,
            transport: Some("stdio".into()),
            protocol: Some("acp".into()),
            api_version: None,
            output_format: Some("ndjson".into()),
            stop_reason: None,
            package_name: None,
            package_version: None,
            session_id: Some("session-test".into()),
            client_request_id: None,
            agent_name: Some("test-agent".into()),
            agent_version: None,
            beta_headers: Vec::new(),
            review_progress: Some(LlmReviewProgress {
                state,
                completed_units,
                total_units,
                error: None,
                updated_at: chrono::Utc::now(),
            }),
        }
    }
}
