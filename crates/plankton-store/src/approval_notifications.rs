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
    /// True only when the latest final approval audit records a real automatic
    /// allow/deny. An earlier escalation trace does not make a human decision automatic.
    pub decision_is_automatic: bool,
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
        const COLUMNS: &str = r#"SELECT id, resource, requested_by, reason, policy_mode, approval_status, evaluation_state, updated_at,
            COALESCE((SELECT actor = 'system_auto' AND
                CASE WHEN json_valid(payload_json) THEN json_extract(payload_json, '$.auto_disposition') IN ('allow', 'deny') ELSE 0 END
                FROM audit_records INDEXED BY idx_audit_records_request_id_event_type_created_at WHERE request_id = access_requests.id AND event_type = 'approval_recorded' AND action = 'approval_recorded'
                ORDER BY created_at DESC, rowid DESC LIMIT 1), 0) AS decision_is_automatic
            FROM access_requests"#;
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
                SELECT actor, note, actor = 'system_auto' AND
                    CASE WHEN json_valid(payload_json) THEN json_extract(payload_json, '$.auto_disposition') IN ('allow', 'deny') ELSE 0 END
                    AS decision_is_automatic
                FROM audit_records INDEXED BY idx_audit_records_request_id_event_type_created_at
                WHERE request_id = ? AND event_type = 'approval_recorded' AND action = 'approval_recorded'
                ORDER BY created_at DESC, rowid DESC LIMIT 1
            )
            SELECT id, resource, requested_by, reason, policy_mode, approval_status, evaluation_state, updated_at,
                json_extract(context, '$.resource_metadata.item_title') AS item_title,
                json_extract(context, '$.resource_metadata.display_name') AS display_name,
                json_extract(context, '$.resource_metadata.field_label') AS field_label,
                json_extract(context, '$.resource_metadata.field_key') AS field_key,
                COALESCE((SELECT decision_is_automatic FROM latest_approval), 0) AS decision_is_automatic,
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
        decision_is_automatic: row.try_get("decision_is_automatic")?,
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
        settings.provider_kind = "mock".into();
        settings.llm_approval_allow_enabled = true;
        settings.llm_approval_deny_enabled = true;
        settings.llm_approval_escalate_enabled = true;
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
        assert!(!changed[0].decision_is_automatic);
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
        assert!(!details.status.decision_is_automatic);
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

    async fn evaluate(
        store: &SqliteStore,
        settings: &plankton_core::PlanktonSettings,
        request: &plankton_core::AccessRequest,
    ) -> plankton_core::AccessRequest {
        let running = store
            .claim_evaluation(&request.id, Utc::now())
            .await
            .unwrap()
            .unwrap();
        let suggestion = plankton_core::request_llm_suggestion(
            settings,
            running.policy_mode,
            running.provider_input.as_ref().unwrap(),
        )
        .await;
        store
            .finalize_evaluation(&running.id, suggestion)
            .await
            .unwrap()
    }

    #[tokio::test]
    async fn notification_projection_distinguishes_automatic_allow_and_deny_from_human_decisions() {
        let (store, settings, _directory) = store().await;
        for (resource, expected) in [
            ("secret/dev-token", ApprovalStatus::Approved),
            ("secret/prod-token", ApprovalStatus::Rejected),
        ] {
            let request = store
                .submit_request(
                    &settings,
                    RequestContext::new(resource.into(), "test".into(), "test-agent".into()),
                    PolicyMode::LlmAutomatic,
                )
                .await
                .unwrap();
            let finished = evaluate(&store, &settings, &request).await;
            assert_eq!(finished.approval_status, expected);
            let details = store
                .approval_notification_details(&request.id)
                .await
                .unwrap();
            assert!(details.status.decision_is_automatic);
            let polled = store
                .list_approval_notification_statuses(None)
                .await
                .unwrap()
                .into_iter()
                .find(|status| status.id == request.id)
                .unwrap();
            assert_eq!(polled, details.status);
        }
    }

    #[tokio::test]
    async fn notification_projection_keeps_a_human_result_manual_after_automatic_escalation() {
        let (store, mut settings, _directory) = store().await;
        settings.llm_approval_allow_enabled = false;
        let request = store
            .submit_request(
                &settings,
                RequestContext::new(
                    "secret/dev-token".into(),
                    "test".into(),
                    "test-agent".into(),
                ),
                PolicyMode::LlmAutomatic,
            )
            .await
            .unwrap();
        let escalated = evaluate(&store, &settings, &request).await;
        assert_eq!(escalated.approval_status, ApprovalStatus::Pending);
        assert_eq!(
            escalated.automatic_decision.unwrap().auto_disposition,
            plankton_core::AutomaticDisposition::Escalate
        );
        let resolved = store
            .record_decision(
                &settings,
                &request.id,
                Decision::Allow,
                "desktop-user",
                None,
            )
            .await
            .unwrap();
        assert!(resolved.automatic_decision.is_some());
        let details = store
            .approval_notification_details(&request.id)
            .await
            .unwrap();
        assert!(!details.status.decision_is_automatic);
        assert!(
            !store
                .list_approval_notification_statuses(None)
                .await
                .unwrap()[0]
                .decision_is_automatic
        );
    }

    #[tokio::test]
    async fn notification_projection_does_not_guess_unknown_or_spoofed_decision_origins() {
        let (store, settings, _directory) = store().await;
        let request = store
            .submit_request(
                &settings,
                RequestContext::new("secret/test".into(), "test".into(), "test-agent".into()),
                PolicyMode::LlmAutomatic,
            )
            .await
            .unwrap();
        // A caller-provided actor name alone is not proof of an automatic decision.
        store
            .record_decision(&settings, &request.id, Decision::Allow, "system_auto", None)
            .await
            .unwrap();
        assert!(
            !store
                .approval_notification_details(&request.id)
                .await
                .unwrap()
                .status
                .decision_is_automatic
        );
        sqlx::query(
            "DELETE FROM audit_records WHERE request_id = ? AND event_type = 'approval_recorded' AND action = 'approval_recorded'",
        )
        .bind(&request.id)
        .execute(&store.pool)
        .await
        .unwrap();
        assert!(
            !store
                .list_approval_notification_statuses(None)
                .await
                .unwrap()[0]
                .decision_is_automatic
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
