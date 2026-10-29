use chrono::{DateTime, Utc};
use plankton_core::{ApprovalStatus, EvaluationState, PolicyMode};
use sqlx::Row;

use crate::{SqliteStore, StoreError};

/// A small projection for polling. No prompt, context, model output, or audit
/// payload is fetched or decoded until a new notification needs its details.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApprovalNotificationStatus {
    pub id: String,
    pub resource: String,
    pub requested_by: String,
    pub reason: String,
    pub policy_mode: PolicyMode,
    pub approval_status: ApprovalStatus,
    pub evaluation_state: EvaluationState,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApprovalNotificationDetails {
    /// Read atomically with the details, since evaluation may finish between
    /// the lightweight poll and this on-demand lookup.
    pub status: ApprovalNotificationStatus,
    pub resource_label: String,
    pub decision_reason: String,
}

impl SqliteStore {
    /// `None` returns a complete lightweight snapshot for startup and polling.
    /// The optional time filter is inclusive but is not a commit-order cursor:
    /// requests can acquire their timestamp before waiting for a write lock.
    pub async fn list_approval_notification_statuses(
        &self,
        updated_since: Option<DateTime<Utc>>,
    ) -> Result<Vec<ApprovalNotificationStatus>, StoreError> {
        const COLUMNS: &str = "SELECT id, resource, requested_by, reason, policy_mode, approval_status, evaluation_state, updated_at FROM access_requests";
        let rows = match updated_since {
            Some(since) => {
                sqlx::query(&format!(
                    "{COLUMNS} WHERE updated_at >= ? ORDER BY updated_at, id"
                ))
                .bind(since.to_rfc3339())
                .fetch_all(&self.pool)
                .await?
            }
            None => {
                sqlx::query(&format!("{COLUMNS} ORDER BY updated_at, id"))
                    .fetch_all(&self.pool)
                    .await?
            }
        };
        rows.iter().map(decode_notification_status).collect()
    }

    /// Only called for a changed visible stage. SQLite returns a few scalar
    /// fields; the desktop never loads the potentially large evidence payloads.
    pub async fn approval_notification_details(
        &self,
        request_id: &str,
    ) -> Result<ApprovalNotificationDetails, StoreError> {
        let row = sqlx::query(
            r#"
            WITH request AS (
                SELECT id, resource, requested_by, reason, policy_mode, approval_status, evaluation_state, updated_at,
                    CASE WHEN json_valid(context_json) THEN context_json ELSE '{}' END AS context,
                    CASE WHEN json_valid(llm_suggestion_json) THEN llm_suggestion_json ELSE '{}' END AS suggestion,
                    CASE WHEN json_valid(automatic_decision_json) THEN automatic_decision_json ELSE '{}' END AS automatic
                FROM access_requests WHERE id = ?
            ), latest_approval AS (
                SELECT actor, note FROM audit_records
                WHERE request_id = ? AND action = 'approval_recorded'
                ORDER BY created_at DESC, rowid DESC LIMIT 1
            )
            SELECT id, resource, requested_by, reason, policy_mode, approval_status, evaluation_state, updated_at,
                json_extract(context, '$.resource_metadata.item_title') AS item_title,
                json_extract(context, '$.resource_metadata.display_name') AS display_name,
                json_extract(context, '$.resource_metadata.field_label') AS field_label,
                json_extract(context, '$.resource_metadata.field_key') AS field_key,
                (SELECT actor FROM latest_approval) AS decision_actor,
                (SELECT note FROM latest_approval) AS decision_note,
                json_extract(automatic, '$.auto_rationale_summary') AS automatic_reason,
                json_extract(suggestion, '$.rationale_summary') AS suggestion_reason,
                json_extract(suggestion, '$.error') AS suggestion_error
            FROM request
            "#,
        )
        .bind(request_id)
        .bind(request_id)
        .fetch_optional(&self.pool)
        .await?
        .ok_or_else(|| StoreError::NotFound(request_id.to_string()))?;
        let resource: String = row.try_get("resource")?;
        let title = nonempty(row.try_get("item_title")?).or(nonempty(row.try_get("display_name")?));
        let field = nonempty(row.try_get("field_label")?).or(nonempty(row.try_get("field_key")?));
        let resource_label = match (title, field) {
            (Some(title), Some(field)) if title != field => format!("{title} · {field}"),
            (Some(label), _) | (_, Some(label)) => label,
            _ => resource,
        };
        let status: ApprovalStatus = stored_enum(row.try_get("approval_status")?)?;
        let evaluation: EvaluationState = stored_enum(row.try_get("evaluation_state")?)?;
        let note = nonempty(row.try_get("decision_note")?);
        let actor = nonempty(row.try_get("decision_actor")?);
        let automatic = nonempty(row.try_get("automatic_reason")?);
        let suggestion = nonempty(row.try_get("suggestion_reason")?);
        let error = nonempty(row.try_get("suggestion_error")?);
        let decision_reason = match status {
            ApprovalStatus::Approved | ApprovalStatus::Rejected => {
                let fallback = if status == ApprovalStatus::Approved {
                    "Access approved; no reason was recorded."
                } else {
                    "Access rejected; no reason was recorded."
                };
                // A human override must never display the model's opposite
                // decision rationale when the human left no note.
                note.or_else(|| {
                    if actor.as_deref().is_some_and(|actor| actor != "system_auto") {
                        None
                    } else {
                        automatic.or(suggestion)
                    }
                })
                .unwrap_or_else(|| fallback.into())
            }
            ApprovalStatus::Pending => match evaluation {
                EvaluationState::Queued => "Waiting for automatic review.".into(),
                EvaluationState::Running => "Checking the requested credential access.".into(),
                EvaluationState::Failed | EvaluationState::Interrupted => {
                    error.unwrap_or_else(|| {
                        if evaluation == EvaluationState::Interrupted {
                            "Automatic review was interrupted; manual review is required.".into()
                        } else {
                            "Automatic review failed; manual review is required.".into()
                        }
                    })
                }
                _ => "Manual review is required.".into(),
            },
        };
        Ok(ApprovalNotificationDetails {
            status: decode_notification_status(&row)?,
            resource_label,
            decision_reason,
        })
    }
}

fn decode_notification_status(
    row: &sqlx::sqlite::SqliteRow,
) -> Result<ApprovalNotificationStatus, StoreError> {
    let timestamp: String = row.try_get("updated_at")?;
    Ok(ApprovalNotificationStatus {
        id: row.try_get("id")?,
        resource: row.try_get("resource")?,
        requested_by: row.try_get("requested_by")?,
        reason: row.try_get("reason")?,
        policy_mode: stored_enum(row.try_get("policy_mode")?)?,
        approval_status: stored_enum(row.try_get("approval_status")?)?,
        evaluation_state: stored_enum(row.try_get("evaluation_state")?)?,
        updated_at: DateTime::parse_from_rfc3339(&timestamp)
            .map(|value| value.with_timezone(&Utc))
            .map_err(|_| StoreError::InvalidDateTime(timestamp))?,
    })
}

fn stored_enum<T: serde::de::DeserializeOwned>(value: String) -> Result<T, StoreError> {
    Ok(serde_json::from_value(serde_json::Value::String(value))?)
}

fn nonempty(value: Option<String>) -> Option<String> {
    value
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;
    use plankton_core::{load_settings, Decision, RequestContext};

    async fn store() -> (
        SqliteStore,
        plankton_core::PlanktonSettings,
        tempfile::TempDir,
    ) {
        let directory = tempfile::tempdir().unwrap();
        let mut settings = load_settings().unwrap();
        settings.database_url = format!("sqlite://{}", directory.path().join("test.db").display());
        let store = SqliteStore::new(&settings).await.unwrap();
        (store, settings, directory)
    }

    #[tokio::test]
    async fn notification_status_poll_does_not_read_evidence_and_includes_fast_results() {
        let (store, settings, _directory) = store().await;
        let request = store
            .submit_request(
                &settings,
                RequestContext::new(
                    "secret/test".into(),
                    "Deploy service".into(),
                    "test-agent".into(),
                ),
                PolicyMode::ManualOnly,
            )
            .await
            .unwrap();
        let baseline = store
            .list_approval_notification_statuses(None)
            .await
            .unwrap();
        let cursor = baseline[0].updated_at;
        store
            .record_decision(
                &settings,
                &request.id,
                Decision::Allow,
                "reviewer",
                Some("Expected deployment".into()),
            )
            .await
            .unwrap();
        sqlx::query("UPDATE access_requests SET context_json = 'invalid', provider_input_json = 'invalid', llm_suggestion_json = 'invalid' WHERE id = ?")
            .bind(&request.id).execute(&store.pool).await.unwrap();
        let changed = store
            .list_approval_notification_statuses(Some(cursor))
            .await
            .unwrap();
        assert_eq!(changed.len(), 1);
        assert_eq!(changed[0].approval_status, ApprovalStatus::Approved);
        assert_eq!(changed[0].reason, "Deploy service");
        let inclusive = store
            .list_approval_notification_statuses(Some(changed[0].updated_at))
            .await
            .unwrap();
        assert_eq!(inclusive, changed);
        let details = store
            .approval_notification_details(&request.id)
            .await
            .unwrap();
        assert_eq!(details.status.approval_status, ApprovalStatus::Approved);
        assert_eq!(details.resource_label, "secret/test");
        assert_eq!(details.decision_reason, "Expected deployment");
    }

    #[tokio::test]
    async fn notification_details_use_display_metadata_and_the_actual_manual_reason() {
        let (store, settings, _directory) = store().await;
        let mut context = RequestContext::new(
            "plankton://field/id/token".into(),
            "Deploy service".into(),
            "test-agent".into(),
        );
        context
            .resource_metadata
            .insert("item_title".into(), "Deployment".into());
        context
            .resource_metadata
            .insert("field_label".into(), "API token".into());
        let request = store
            .submit_request(&settings, context, PolicyMode::ManualOnly)
            .await
            .unwrap();
        store
            .record_decision(&settings, &request.id, Decision::Deny, "reviewer", None)
            .await
            .unwrap();
        sqlx::query("UPDATE access_requests SET llm_suggestion_json = ? WHERE id = ?")
            .bind(r#"{"rationale_summary":"Model wanted to allow"}"#)
            .bind(&request.id)
            .execute(&store.pool)
            .await
            .unwrap();
        let details = store
            .approval_notification_details(&request.id)
            .await
            .unwrap();
        assert_eq!(details.resource_label, "Deployment · API token");
        assert_eq!(
            details.decision_reason,
            "Access rejected; no reason was recorded."
        );
    }

    #[tokio::test]
    async fn notification_details_preserve_automatic_failure_reason() {
        let (store, settings, _directory) = store().await;
        let request = store
            .submit_request(
                &settings,
                RequestContext::new(
                    "secret/test".into(),
                    "Fetch release".into(),
                    "test-agent".into(),
                ),
                PolicyMode::ManualOnly,
            )
            .await
            .unwrap();
        sqlx::query("UPDATE access_requests SET policy_mode = 'llm_automatic', evaluation_state = 'failed', llm_suggestion_json = ? WHERE id = ?")
            .bind(r#"{"error":"Provider deadline exceeded"}"#).bind(&request.id).execute(&store.pool).await.unwrap();
        let details = store
            .approval_notification_details(&request.id)
            .await
            .unwrap();
        assert_eq!(details.decision_reason, "Provider deadline exceeded");
    }
}
