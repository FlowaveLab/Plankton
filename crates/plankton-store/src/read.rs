use std::path::PathBuf;
use std::str::FromStr;
use std::time::Duration;

use chrono::{DateTime, Utc};
use plankton_core::{
    AccessRequest, ApprovalStatus, AuditAction, AutomaticDecisionTrace, Decision, EvaluationState,
    LlmSuggestion, PlanktonSettings, PolicyMode, ProviderInputSnapshot, RequestContext,
    SuggestedDecision,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::{
    sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions, SqliteRow},
    Row, SqlitePool,
};
use tracing::instrument;

use crate::StoreError;

#[derive(Debug, Clone)]
pub struct SqliteReadStore {
    pool: SqlitePool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct QueueRequestRecord {
    pub id: String,
    pub resource: String,
    pub requested_by: String,
    pub reason: String,
    pub policy_mode: PolicyMode,
    pub approval_status: ApprovalStatus,
    pub evaluation_state: EvaluationState,
    pub final_decision: Option<Decision>,
    pub provider_kind: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub resolved_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AuditFeedRecord {
    pub id: String,
    pub request_id: String,
    pub action: AuditAction,
    pub decision: Option<SuggestedDecision>,
    pub actor_type: String,
    pub actor_id: String,
    pub note: Option<String>,
    pub resource: String,
    pub policy_mode: Option<PolicyMode>,
    pub approval_status: Option<ApprovalStatus>,
    pub request_reason: Option<String>,
    pub payload: Value,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RequestAuditView {
    pub request: AccessRequest,
    pub audit_records: Vec<AuditFeedRecord>,
}

impl SqliteReadStore {
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

        sqlx::migrate!("./migrations").run(&pool).await?;
        Ok(Self { pool })
    }

    #[instrument(skip(self))]
    pub async fn list_queue(&self, limit: u32) -> Result<Vec<QueueRequestRecord>, StoreError> {
        let rows = sqlx::query(
            r#"
            SELECT
                id, resource, requested_by, reason, policy_mode, approval_status,
                evaluation_state, decision AS final_decision, provider_kind, created_at,
                updated_at, resolved_at
            FROM approval_requests
            WHERE approval_status = ?
            ORDER BY created_at DESC
            LIMIT ?
            "#,
        )
        .bind("pending")
        .bind(limit as i64)
        .fetch_all(&self.pool)
        .await?;

        rows.iter().map(decode_queue_request).collect()
    }

    #[instrument(skip(self))]
    pub async fn get_status(&self, request_id: &str) -> Result<RequestAuditView, StoreError> {
        let request_row = sqlx::query(
            r#"
            SELECT
                id, resource, policy_mode, approval_status, evaluation_state,
                decision AS final_decision, provider_kind, rendered_prompt, provider_input_json,
                llm_suggestion_json,
                automatic_decision_json, context_json, created_at, updated_at, resolved_at
            FROM approval_requests
            WHERE id = ?
            "#,
        )
        .bind(request_id)
        .fetch_optional(&self.pool)
        .await?;

        let request_row =
            request_row.ok_or_else(|| StoreError::NotFound(request_id.to_string()))?;
        let mut request = decode_access_request(&request_row)?;
        crate::sqlite::refresh_shared_review(&self.pool, &mut request).await?;

        let audit_rows = sqlx::query(
            r#"
            SELECT
                id,
                request_id,
                action,
                decision,
                actor_type,
                actor_id,
                COALESCE(approval_note, note) AS note,
                resource,
                policy_mode,
                approval_status,
                request_reason,
                payload_json,
                created_at
            FROM audit_records
            WHERE request_id = ?
            ORDER BY created_at DESC
            "#,
        )
        .bind(request_id)
        .fetch_all(&self.pool)
        .await?;

        let audit_records = audit_rows
            .iter()
            .map(decode_audit_feed)
            .collect::<Result<Vec<_>, _>>()?;

        Ok(RequestAuditView {
            request,
            audit_records,
        })
    }

    #[instrument(skip(self))]
    pub async fn list_audit(&self, limit: u32) -> Result<Vec<AuditFeedRecord>, StoreError> {
        let rows = sqlx::query(
            r#"
            SELECT
                id,
                request_id,
                action,
                decision,
                actor_type,
                actor_id,
                COALESCE(approval_note, note) AS note,
                resource,
                policy_mode,
                approval_status,
                request_reason,
                payload_json,
                created_at
            FROM audit_records
            ORDER BY created_at DESC
            LIMIT ?
            "#,
        )
        .bind(limit as i64)
        .fetch_all(&self.pool)
        .await?;

        rows.iter().map(decode_audit_feed).collect()
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

fn decode_queue_request(row: &SqliteRow) -> Result<QueueRequestRecord, StoreError> {
    Ok(QueueRequestRecord {
        id: row.try_get("id")?,
        resource: row.try_get("resource")?,
        requested_by: row.try_get("requested_by")?,
        reason: row.try_get("reason")?,
        policy_mode: parse_enum(row.try_get::<String, _>("policy_mode")?.as_str())?,
        approval_status: parse_enum(row.try_get::<String, _>("approval_status")?.as_str())?,
        evaluation_state: parse_enum(row.try_get::<String, _>("evaluation_state")?.as_str())?,
        final_decision: row
            .try_get::<Option<String>, _>("final_decision")?
            .map(|value| parse_enum(value.as_str()))
            .transpose()?,
        provider_kind: row.try_get("provider_kind")?,
        created_at: parse_datetime(row.try_get::<String, _>("created_at")?.as_str())?,
        updated_at: parse_datetime(row.try_get::<String, _>("updated_at")?.as_str())?,
        resolved_at: row
            .try_get::<Option<String>, _>("resolved_at")?
            .map(|value| parse_datetime(value.as_str()))
            .transpose()?,
    })
}

fn decode_access_request(row: &SqliteRow) -> Result<AccessRequest, StoreError> {
    let mut context: RequestContext =
        serde_json::from_str(row.try_get::<String, _>("context_json")?.as_str())?;
    context.resource = row.try_get("resource")?;
    let provider_input = parse_optional_json::<ProviderInputSnapshot>(
        row.try_get::<Option<String>, _>("provider_input_json")?,
    )?;
    Ok(AccessRequest {
        id: row.try_get("id")?,
        context,
        policy_mode: parse_enum(row.try_get::<String, _>("policy_mode")?.as_str())?,
        approval_status: parse_enum(row.try_get::<String, _>("approval_status")?.as_str())?,
        evaluation_state: parse_enum(row.try_get::<String, _>("evaluation_state")?.as_str())?,
        final_decision: row
            .try_get::<Option<String>, _>("final_decision")?
            .map(|value| parse_enum(value.as_str()))
            .transpose()?,
        provider_kind: row.try_get("provider_kind")?,
        rendered_prompt: row.try_get("rendered_prompt")?,
        provider_input,
        llm_suggestion: parse_optional_json::<LlmSuggestion>(
            row.try_get::<Option<String>, _>("llm_suggestion_json")?,
        )?,
        automatic_decision: parse_optional_json::<AutomaticDecisionTrace>(
            row.try_get::<Option<String>, _>("automatic_decision_json")?,
        )?,
        created_at: parse_datetime(row.try_get::<String, _>("created_at")?.as_str())?,
        updated_at: parse_datetime(row.try_get::<String, _>("updated_at")?.as_str())?,
        resolved_at: row
            .try_get::<Option<String>, _>("resolved_at")?
            .map(|value| parse_datetime(value.as_str()))
            .transpose()?,
    })
}

fn decode_audit_feed(row: &SqliteRow) -> Result<AuditFeedRecord, StoreError> {
    let payload: Value = serde_json::from_str(row.try_get::<String, _>("payload_json")?.as_str())?;
    let action: AuditAction = parse_enum(row.try_get::<String, _>("action")?.as_str())?;
    let actor_id = row
        .try_get::<Option<String>, _>("actor_id")?
        .unwrap_or_default();
    let actor_type = row
        .try_get::<Option<String>, _>("actor_type")?
        .unwrap_or_else(|| default_actor_type(action, actor_id.as_str()).to_string());
    let resource = row
        .try_get::<Option<String>, _>("resource")?
        .unwrap_or_default();

    Ok(AuditFeedRecord {
        id: row.try_get("id")?,
        request_id: row.try_get("request_id")?,
        action,
        decision: row
            .try_get::<Option<String>, _>("decision")?
            .map(|value| parse_enum(value.as_str()))
            .transpose()?,
        actor_type,
        actor_id,
        note: row.try_get("note")?,
        resource,
        policy_mode: row
            .try_get::<Option<String>, _>("policy_mode")?
            .map(|value| parse_enum(value.as_str()))
            .transpose()?,
        approval_status: row
            .try_get::<Option<String>, _>("approval_status")?
            .map(|value| parse_enum(value.as_str()))
            .transpose()?,
        request_reason: row.try_get("request_reason")?,
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

fn default_actor_type(action: AuditAction, actor_id: &str) -> &'static str {
    if actor_id == "system_auto" {
        return "system_auto";
    }

    match action {
        AuditAction::RequestSubmitted => "requester",
        AuditAction::LlmSuggestionGenerated => "llm",
        AuditAction::LlmSuggestionFailed => "llm",
        AuditAction::LlmReviewDetailsUpdated => "llm",
        AuditAction::ApprovalRecorded => "human",
        AuditAction::AutomaticDecisionRecorded => "system_auto",
        AuditAction::AutomaticEscalatedToHuman => "system_auto",
        AuditAction::HumanDecisionOverrodeLlm => "human",
        AuditAction::LegacyNoteEvaluated => "llm",
        AuditAction::StatusViewed => "viewer",
    }
}

#[cfg(test)]
mod tests {
    use plankton_core::{
        load_settings, request_llm_suggestion, AuditAction, Decision, PolicyMode, RequestContext,
        SuggestedDecision,
    };
    use tempfile::tempdir;

    use crate::SqliteStore;

    use super::SqliteReadStore;

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
    async fn exposes_queue_projection_and_checker_view() {
        let temp = tempdir().expect("temp directory should be created");
        let mut settings = load_settings().expect("default settings should load");
        settings.database_url = format!("sqlite://{}", temp.path().join("plankton.db").display());

        let store = SqliteStore::new(&settings)
            .await
            .expect("store should initialize");
        let read_store = SqliteReadStore::new(&settings)
            .await
            .expect("read store should initialize");

        let request = store
            .submit_request(
                &settings,
                RequestContext::new(
                    "secret/demo".to_string(),
                    "Need manual review".to_string(),
                    "alice".to_string(),
                ),
                PolicyMode::ManualOnly,
            )
            .await
            .expect("request should be inserted");

        let queue = read_store
            .list_queue(10)
            .await
            .expect("queue projection should load");
        assert_eq!(queue.len(), 1);
        assert_eq!(queue[0].id, request.id);
        assert_eq!(queue[0].resource, "secret/demo");
        assert_eq!(
            sqlx::query_scalar::<_, String>("SELECT resource FROM approval_requests WHERE id = ?")
                .bind(&request.id)
                .fetch_one(&read_store.pool)
                .await
                .expect("approval_requests view should be queryable"),
            "secret/demo"
        );

        let status = read_store
            .get_status(&request.id)
            .await
            .expect("status projection should load");
        assert_eq!(status.request.context.reason, "Need manual review");
        assert_eq!(status.audit_records.len(), 1);
        assert_eq!(status.audit_records[0].actor_type, "requester");
        assert_eq!(status.audit_records[0].resource, "secret/demo");
    }

    #[tokio::test]
    async fn backfills_audit_read_fields_for_manual_decisions() {
        let temp = tempdir().expect("temp directory should be created");
        let mut settings = load_settings().expect("default settings should load");
        settings.database_url = format!("sqlite://{}", temp.path().join("plankton.db").display());

        let store = SqliteStore::new(&settings)
            .await
            .expect("store should initialize");
        let read_store = SqliteReadStore::new(&settings)
            .await
            .expect("read store should initialize");

        let request = store
            .submit_request(
                &settings,
                RequestContext::new(
                    "secret/prod".to_string(),
                    "Need deploy access".to_string(),
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
                Decision::Deny,
                "reviewer",
                Some("missing ticket".to_string()),
            )
            .await
            .expect("decision should be persisted");

        let audit = read_store
            .list_audit(10)
            .await
            .expect("audit projection should load");
        let decision_record = audit
            .iter()
            .find(|record| record.request_id == request.id && record.actor_id == "reviewer")
            .expect("approval audit record should be projected");

        assert_eq!(decision_record.actor_type, "human");
        assert_eq!(decision_record.decision, Some(SuggestedDecision::Deny));
        assert_eq!(
            decision_record.approval_status,
            Some(plankton_core::ApprovalStatus::Rejected)
        );
        assert_eq!(decision_record.note.as_deref(), Some("missing ticket"));
        assert_eq!(
            sqlx::query_scalar::<_, String>(
                "SELECT actor_type FROM audit_records WHERE request_id = ? AND actor_id = ?"
            )
            .bind(&request.id)
            .bind("reviewer")
            .fetch_one(&read_store.pool)
            .await
            .expect("audit_records read fields should be persisted"),
            "human"
        );
    }

    #[tokio::test]
    async fn projects_llm_suggestion_fields_for_assisted_requests() {
        let temp = tempdir().expect("temp directory should be created");
        let mut settings = load_settings().expect("default settings should load");
        settings.database_url = format!("sqlite://{}", temp.path().join("plankton.db").display());
        settings.provider_kind = "mock".to_string();

        let store = SqliteStore::new(&settings)
            .await
            .expect("store should initialize");
        let read_store = SqliteReadStore::new(&settings)
            .await
            .expect("read store should initialize");

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

        let status = read_store
            .get_status(&request.id)
            .await
            .expect("status projection should load");

        assert!(status.request.provider_input.is_some());
        assert!(status.request.llm_suggestion.is_some());
        assert!(status
            .audit_records
            .iter()
            .any(|record| record.action == AuditAction::LlmSuggestionGenerated));
    }

    #[tokio::test]
    async fn projects_automatic_decision_fields_for_auto_requests() {
        let temp = tempdir().expect("temp directory should be created");
        let mut settings = load_settings().expect("default settings should load");
        settings.database_url = format!("sqlite://{}", temp.path().join("plankton.db").display());
        settings.provider_kind = "mock".to_string();

        let store = SqliteStore::new(&settings)
            .await
            .expect("store should initialize");
        let read_store = SqliteReadStore::new(&settings)
            .await
            .expect("read store should initialize");

        let request = store
            .submit_request(
                &settings,
                RequestContext::new(
                    "secret/dev-token".to_string(),
                    "Need smoke test access".to_string(),
                    "alice".to_string(),
                ),
                PolicyMode::LlmAutomatic,
            )
            .await
            .expect("request should be inserted");
        let request = evaluate_queued_request(&store, &settings, request).await;

        let status = read_store
            .get_status(&request.id)
            .await
            .expect("status projection should load");

        assert!(status.request.automatic_decision.is_some());
        let auto_audit = status
            .audit_records
            .iter()
            .find(|record| record.action == AuditAction::AutomaticDecisionRecorded)
            .expect("automatic decision audit should be projected");
        assert_eq!(auto_audit.actor_type, "system_auto");
        assert_eq!(auto_audit.decision, Some(SuggestedDecision::Allow));
    }
}
