use chrono::{DateTime, Duration, Utc};
use plankton_protocol::error::{ErrorCode, ErrorSeverity, PlanktonError};
use sqlx::Row;
use uuid::Uuid;

use crate::{SqliteStore, StoreError};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiagnosticRecord {
    pub error: PlanktonError,
    pub acknowledged_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OperationStatus {
    Queued,
    Running,
    Completed,
    Failed,
    Interrupted,
    Superseded,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InterruptedOperation {
    pub id: String,
    pub operation_kind: String,
    pub operation_key: String,
    pub state: serde_json::Value,
    pub status: OperationStatus,
    pub started_at: DateTime<Utc>,
    pub heartbeat_at: DateTime<Utc>,
    pub finished_at: Option<DateTime<Utc>>,
}

impl SqliteStore {
    pub async fn record_diagnostic_error(&self, error: &PlanktonError) -> Result<(), StoreError> {
        sqlx::query(
            r#"
            INSERT INTO diagnostic_errors (
                id, correlation_id, code, severity, source_json, user_message, internal_message,
                public_context_json, internal_context_json, retryable, occurred_at, acknowledged_at
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, NULL)
            "#,
        )
        .bind(Uuid::new_v4().to_string())
        .bind(error.correlation_id.to_string())
        .bind(
            serde_json::to_value(error.code)?
                .as_str()
                .unwrap_or("internal"),
        )
        .bind(
            serde_json::to_value(error.severity)?
                .as_str()
                .unwrap_or("error"),
        )
        .bind(serde_json::to_string(&error.source)?)
        .bind(&error.user_message)
        .bind(&error.internal_message)
        .bind(serde_json::to_string(&error.public_context)?)
        .bind(serde_json::to_string(&error.internal_context)?)
        .bind(error.retryable)
        .bind(error.timestamp.to_rfc3339())
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn list_diagnostic_errors(
        &self,
        include_acknowledged: bool,
        limit: u16,
    ) -> Result<Vec<DiagnosticRecord>, StoreError> {
        self.list_diagnostic_errors_page((!include_acknowledged).then_some(false), None, limit, 0)
            .await
    }

    pub async fn list_diagnostic_errors_page(
        &self,
        acknowledgement: Option<bool>,
        severity: Option<&str>,
        limit: u16,
        offset: u32,
    ) -> Result<Vec<DiagnosticRecord>, StoreError> {
        if limit == 0 || limit > 500 {
            return Err(StoreError::InvalidLimit {
                maximum: 500,
                received: limit,
            });
        }
        let rows = sqlx::query(
            r#"
            SELECT correlation_id, code, severity, source_json, user_message, internal_message,
                   public_context_json, internal_context_json, retryable, occurred_at,
                   acknowledged_at
            FROM diagnostic_errors
            WHERE (
                ? IS NULL
                OR (? = 1 AND acknowledged_at IS NOT NULL)
                OR (? = 0 AND acknowledged_at IS NULL)
            )
              AND (? IS NULL OR severity = ?)
            ORDER BY occurred_at DESC, id DESC
            LIMIT ? OFFSET ?
            "#,
        )
        .bind(acknowledgement)
        .bind(acknowledgement)
        .bind(acknowledgement)
        .bind(severity)
        .bind(severity)
        .bind(i64::from(limit))
        .bind(i64::from(offset))
        .fetch_all(&self.pool)
        .await?;
        rows.into_iter().map(decode_diagnostic).collect()
    }

    pub async fn count_diagnostic_errors(
        &self,
        acknowledgement: Option<bool>,
        severity: Option<&str>,
    ) -> Result<u64, StoreError> {
        let count: i64 = sqlx::query_scalar(
            r#"
            SELECT COUNT(*)
            FROM diagnostic_errors
            WHERE (
                ? IS NULL
                OR (? = 1 AND acknowledged_at IS NOT NULL)
                OR (? = 0 AND acknowledged_at IS NULL)
            )
              AND (? IS NULL OR severity = ?)
            "#,
        )
        .bind(acknowledgement)
        .bind(acknowledgement)
        .bind(acknowledgement)
        .bind(severity)
        .bind(severity)
        .fetch_one(&self.pool)
        .await?;
        u64::try_from(count).map_err(|_| StoreError::InvalidStoredValue {
            field: "diagnostic_error_count",
            value: count.to_string(),
        })
    }

    pub async fn acknowledge_diagnostic_error(
        &self,
        correlation_id: Uuid,
        acknowledged_at: DateTime<Utc>,
    ) -> Result<bool, StoreError> {
        let result = sqlx::query(
            "UPDATE diagnostic_errors SET acknowledged_at = ? WHERE correlation_id = ? AND acknowledged_at IS NULL",
        )
        .bind(acknowledged_at.to_rfc3339())
        .bind(correlation_id.to_string())
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected() > 0)
    }

    pub async fn acknowledge_sync_diagnostic_errors(
        &self,
        vault_id: &str,
        adapter_id: &str,
        acknowledged_at: DateTime<Utc>,
    ) -> Result<u64, StoreError> {
        let result = sqlx::query(
            r#"
            UPDATE diagnostic_errors
            SET acknowledged_at = ?
            WHERE acknowledged_at IS NULL
              AND json_extract(source_json, '$.kind') = 'sync'
              AND json_extract(source_json, '$.adapter_id') = ?
              AND json_extract(public_context_json, '$.vault_id') = ?
              AND json_extract(public_context_json, '$.adapter_id') = ?
            "#,
        )
        .bind(acknowledged_at.to_rfc3339())
        .bind(adapter_id)
        .bind(vault_id)
        .bind(adapter_id)
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected())
    }

    pub async fn start_operation(
        &self,
        operation: &InterruptedOperation,
    ) -> Result<(), StoreError> {
        sqlx::query(
            r#"
            INSERT INTO interrupted_operations (
                id, operation_kind, operation_key, state_json, status, started_at, heartbeat_at,
                finished_at
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind(&operation.id)
        .bind(&operation.operation_kind)
        .bind(&operation.operation_key)
        .bind(serde_json::to_string(&operation.state)?)
        .bind(operation_status_to_str(operation.status))
        .bind(operation.started_at.to_rfc3339())
        .bind(operation.heartbeat_at.to_rfc3339())
        .bind(operation.finished_at.map(|value| value.to_rfc3339()))
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn heartbeat_operation(
        &self,
        id: &str,
        state: &serde_json::Value,
        heartbeat_at: DateTime<Utc>,
    ) -> Result<(), StoreError> {
        let result = sqlx::query(
            r#"
            UPDATE interrupted_operations
            SET state_json = ?, heartbeat_at = ?
            WHERE id = ? AND status = 'running'
            "#,
        )
        .bind(serde_json::to_string(state)?)
        .bind(heartbeat_at.to_rfc3339())
        .bind(id)
        .execute(&self.pool)
        .await?;
        if result.rows_affected() == 0 {
            return Err(StoreError::NotFound(id.to_string()));
        }
        Ok(())
    }

    pub async fn finish_operation(
        &self,
        id: &str,
        status: OperationStatus,
        finished_at: DateTime<Utc>,
    ) -> Result<(), StoreError> {
        if matches!(status, OperationStatus::Queued | OperationStatus::Running) {
            return Err(StoreError::InvalidStoredValue {
                field: "operation_status",
                value: "queued/running cannot be terminal".into(),
            });
        }
        let result = sqlx::query(
            r#"
            UPDATE interrupted_operations
            SET status = ?, heartbeat_at = ?, finished_at = ?
            WHERE id = ? AND status IN ('queued', 'running')
            "#,
        )
        .bind(operation_status_to_str(status))
        .bind(finished_at.to_rfc3339())
        .bind(finished_at.to_rfc3339())
        .bind(id)
        .execute(&self.pool)
        .await?;
        if result.rows_affected() == 0 {
            let existing = sqlx::query_scalar::<_, String>(
                "SELECT status FROM interrupted_operations WHERE id = ?",
            )
            .bind(id)
            .fetch_optional(&self.pool)
            .await?;
            match existing {
                Some(existing) if existing == operation_status_to_str(status) => return Ok(()),
                Some(existing) => {
                    return Err(StoreError::InvalidStoredValue {
                        field: "operation_status",
                        value: format!(
                            "operation {id} is {existing}, cannot finish as {}",
                            operation_status_to_str(status)
                        ),
                    });
                }
                None => return Err(StoreError::NotFound(id.to_string())),
            }
        }
        Ok(())
    }

    pub async fn list_running_operations(
        &self,
        operation_kind: Option<&str>,
    ) -> Result<Vec<InterruptedOperation>, StoreError> {
        let rows = sqlx::query(
            r#"
            SELECT id, operation_kind, operation_key, state_json, status, started_at,
                   heartbeat_at, finished_at
            FROM interrupted_operations
            WHERE status = 'running' AND (? IS NULL OR operation_kind = ?)
            ORDER BY started_at, id
            "#,
        )
        .bind(operation_kind)
        .bind(operation_kind)
        .fetch_all(&self.pool)
        .await?;
        rows.into_iter().map(decode_operation).collect()
    }

    pub async fn list_active_operations(
        &self,
        operation_kind: Option<&str>,
    ) -> Result<Vec<InterruptedOperation>, StoreError> {
        let rows = sqlx::query(
            r#"
            SELECT id, operation_kind, operation_key, state_json, status, started_at,
                   heartbeat_at, finished_at
            FROM interrupted_operations
            WHERE status IN ('queued', 'running') AND (? IS NULL OR operation_kind = ?)
            ORDER BY started_at, id
            "#,
        )
        .bind(operation_kind)
        .bind(operation_kind)
        .fetch_all(&self.pool)
        .await?;
        rows.into_iter().map(decode_operation).collect()
    }

    /// Marks stale running work as interrupted and returns it for deterministic recovery.
    pub async fn recover_stale_operations(
        &self,
        now: DateTime<Utc>,
        stale_after: Duration,
    ) -> Result<Vec<InterruptedOperation>, StoreError> {
        let threshold = (now - stale_after).to_rfc3339();
        let mut tx = self.pool.begin().await?;
        let rows = sqlx::query(
            r#"
            SELECT id, operation_kind, operation_key, state_json, status, started_at,
                   heartbeat_at, finished_at
            FROM interrupted_operations
            WHERE status = 'running' AND heartbeat_at < ?
            ORDER BY heartbeat_at, id
            "#,
        )
        .bind(&threshold)
        .fetch_all(&mut *tx)
        .await?;
        let mut operations = rows
            .into_iter()
            .map(decode_operation)
            .collect::<Result<Vec<_>, _>>()?;
        for operation in &operations {
            if operation.operation_kind == "llm_evaluation" {
                sqlx::query(
                    r#"
                    UPDATE access_requests
                    SET evaluation_state = 'interrupted', updated_at = ?
                    WHERE id = ? AND evaluation_state = 'running'
                    "#,
                )
                .bind(now.to_rfc3339())
                .bind(&operation.operation_key)
                .execute(&mut *tx)
                .await?;
            }
        }
        sqlx::query(
            r#"
            UPDATE interrupted_operations
            SET status = 'interrupted', finished_at = ?
            WHERE status = 'running' AND heartbeat_at < ?
            "#,
        )
        .bind(now.to_rfc3339())
        .bind(&threshold)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;

        for operation in &mut operations {
            operation.status = OperationStatus::Interrupted;
            operation.finished_at = Some(now);
        }
        Ok(operations)
    }
}

fn decode_diagnostic(row: sqlx::sqlite::SqliteRow) -> Result<DiagnosticRecord, StoreError> {
    let code: ErrorCode = serde_json::from_value(serde_json::Value::String(row.try_get("code")?))?;
    let severity: ErrorSeverity =
        serde_json::from_value(serde_json::Value::String(row.try_get("severity")?))?;
    Ok(DiagnosticRecord {
        error: PlanktonError {
            code,
            user_message: row.try_get("user_message")?,
            internal_message: row.try_get("internal_message")?,
            public_context: serde_json::from_str(row.try_get("public_context_json")?)?,
            internal_context: serde_json::from_str(row.try_get("internal_context_json")?)?,
            severity,
            retryable: row.try_get("retryable")?,
            timestamp: parse_datetime(row.try_get("occurred_at")?)?,
            correlation_id: Uuid::parse_str(row.try_get("correlation_id")?).map_err(|_| {
                StoreError::InvalidStoredValue {
                    field: "correlation_id",
                    value: row.get("correlation_id"),
                }
            })?,
            source: serde_json::from_str(row.try_get("source_json")?)?,
        },
        acknowledged_at: row
            .try_get::<Option<String>, _>("acknowledged_at")?
            .map(|value| parse_datetime(&value))
            .transpose()?,
    })
}

fn decode_operation(row: sqlx::sqlite::SqliteRow) -> Result<InterruptedOperation, StoreError> {
    Ok(InterruptedOperation {
        id: row.try_get("id")?,
        operation_kind: row.try_get("operation_kind")?,
        operation_key: row.try_get("operation_key")?,
        state: serde_json::from_str(row.try_get("state_json")?)?,
        status: operation_status_from_str(row.try_get("status")?)?,
        started_at: parse_datetime(row.try_get("started_at")?)?,
        heartbeat_at: parse_datetime(row.try_get("heartbeat_at")?)?,
        finished_at: row
            .try_get::<Option<String>, _>("finished_at")?
            .map(|value| parse_datetime(&value))
            .transpose()?,
    })
}

fn operation_status_to_str(status: OperationStatus) -> &'static str {
    match status {
        OperationStatus::Queued => "queued",
        OperationStatus::Running => "running",
        OperationStatus::Completed => "completed",
        OperationStatus::Failed => "failed",
        OperationStatus::Interrupted => "interrupted",
        OperationStatus::Superseded => "superseded",
    }
}

fn operation_status_from_str(value: &str) -> Result<OperationStatus, StoreError> {
    match value {
        "queued" => Ok(OperationStatus::Queued),
        "running" => Ok(OperationStatus::Running),
        "completed" => Ok(OperationStatus::Completed),
        "failed" => Ok(OperationStatus::Failed),
        "interrupted" => Ok(OperationStatus::Interrupted),
        "superseded" => Ok(OperationStatus::Superseded),
        other => Err(StoreError::InvalidStoredValue {
            field: "operation_status",
            value: other.to_string(),
        }),
    }
}

fn parse_datetime(value: &str) -> Result<DateTime<Utc>, StoreError> {
    DateTime::parse_from_rfc3339(value)
        .map(|value| value.with_timezone(&Utc))
        .map_err(|_| StoreError::InvalidDateTime(value.to_string()))
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use chrono::{Duration, Utc};
    use plankton_core::load_settings;
    use plankton_protocol::error::{ErrorCode, ErrorSeverity, ErrorSource, PlanktonError};
    use tempfile::tempdir;
    use uuid::Uuid;

    use super::{InterruptedOperation, OperationStatus, SqliteStore};

    #[tokio::test]
    async fn diagnostics_are_visible_until_acknowledged() {
        let temp = tempdir().expect("temporary directory");
        let mut settings = load_settings().expect("settings");
        settings.database_url = format!("sqlite://{}", temp.path().join("store.db").display());
        let store = SqliteStore::new(&settings).await.expect("store");
        let correlation_id = Uuid::new_v4();
        store
            .record_diagnostic_error(&PlanktonError {
                code: ErrorCode::BackendFailed,
                user_message: "Password provider failed".into(),
                internal_message: Some("exit status 12".into()),
                public_context: BTreeMap::new(),
                internal_context: BTreeMap::new(),
                severity: ErrorSeverity::Error,
                retryable: true,
                timestamp: Utc::now(),
                correlation_id,
                source: ErrorSource::Backend {
                    backend_id: "external".into(),
                },
            })
            .await
            .expect("record");
        assert_eq!(
            store
                .list_diagnostic_errors(false, 20)
                .await
                .expect("list")
                .len(),
            1
        );
        assert!(store
            .acknowledge_diagnostic_error(correlation_id, Utc::now())
            .await
            .expect("acknowledge"));
        assert!(store
            .list_diagnostic_errors(false, 20)
            .await
            .expect("list")
            .is_empty());
    }

    #[tokio::test]
    async fn successful_sync_acknowledges_only_its_own_previous_failures() {
        let temp = tempdir().expect("temporary directory");
        let mut settings = load_settings().expect("settings");
        settings.database_url = format!("sqlite://{}", temp.path().join("store.db").display());
        let store = SqliteStore::new(&settings).await.expect("store");
        for (vault_id, adapter_id) in [("work", "origin"), ("other", "origin")] {
            store
                .record_diagnostic_error(&PlanktonError {
                    code: ErrorCode::Conflict,
                    user_message: "Encrypted vault synchronization failed".into(),
                    internal_message: Some("remote changed".into()),
                    public_context: BTreeMap::from([
                        ("vault_id".into(), vault_id.into()),
                        ("adapter_id".into(), adapter_id.into()),
                    ]),
                    internal_context: BTreeMap::new(),
                    severity: ErrorSeverity::Error,
                    retryable: true,
                    timestamp: Utc::now(),
                    correlation_id: Uuid::new_v4(),
                    source: ErrorSource::Sync {
                        adapter_id: adapter_id.into(),
                    },
                })
                .await
                .expect("record sync diagnostic");
        }

        assert_eq!(
            store
                .acknowledge_sync_diagnostic_errors("work", "origin", Utc::now())
                .await
                .expect("acknowledge matching sync diagnostics"),
            1
        );
        let remaining = store
            .list_diagnostic_errors(false, 20)
            .await
            .expect("list remaining diagnostics");
        assert_eq!(remaining.len(), 1);
        assert_eq!(
            remaining[0].error.public_context.get("vault_id"),
            Some(&"other".to_string())
        );
    }

    #[tokio::test]
    async fn diagnostic_pages_report_the_complete_filtered_count_beyond_one_hundred() {
        let temp = tempdir().expect("temporary directory");
        let mut settings = load_settings().expect("settings");
        settings.database_url = format!("sqlite://{}", temp.path().join("store.db").display());
        let store = SqliteStore::new(&settings).await.expect("store");
        let base = Utc::now();
        for index in 0..125 {
            store
                .record_diagnostic_error(&PlanktonError {
                    code: ErrorCode::BackendFailed,
                    user_message: format!("Diagnostic {index}"),
                    internal_message: None,
                    public_context: BTreeMap::new(),
                    internal_context: BTreeMap::new(),
                    severity: if index % 2 == 0 {
                        ErrorSeverity::Error
                    } else {
                        ErrorSeverity::Warning
                    },
                    retryable: true,
                    timestamp: base + Duration::seconds(index),
                    correlation_id: Uuid::new_v4(),
                    source: ErrorSource::Backend {
                        backend_id: "external".into(),
                    },
                })
                .await
                .expect("record diagnostic");
        }

        assert_eq!(
            store
                .count_diagnostic_errors(Some(false), Some("error"))
                .await
                .expect("count error diagnostics"),
            63
        );
        let page = store
            .list_diagnostic_errors_page(Some(false), Some("error"), 20, 40)
            .await
            .expect("third error page");
        assert_eq!(page.len(), 20);
        assert!(page
            .windows(2)
            .all(|pair| pair[0].error.timestamp >= pair[1].error.timestamp));
    }

    #[tokio::test]
    async fn stale_running_operations_are_marked_interrupted() {
        let temp = tempdir().expect("temporary directory");
        let mut settings = load_settings().expect("settings");
        settings.database_url = format!("sqlite://{}", temp.path().join("store.db").display());
        let store = SqliteStore::new(&settings).await.expect("store");
        let now = Utc::now();
        store
            .start_operation(&InterruptedOperation {
                id: "sync-1".into(),
                operation_kind: "sync".into(),
                operation_key: "vault-1".into(),
                state: serde_json::json!({"phase": "download"}),
                status: OperationStatus::Running,
                started_at: now - Duration::minutes(5),
                heartbeat_at: now - Duration::minutes(5),
                finished_at: None,
            })
            .await
            .expect("start");
        let recovered = store
            .recover_stale_operations(now, Duration::minutes(1))
            .await
            .expect("recover");
        assert_eq!(recovered.len(), 1);
        assert_eq!(recovered[0].status, OperationStatus::Interrupted);
    }

    #[tokio::test]
    async fn lists_only_running_operations_of_the_requested_kind() {
        let temp = tempdir().expect("temporary directory");
        let mut settings = load_settings().expect("settings");
        settings.database_url = format!("sqlite://{}", temp.path().join("store.db").display());
        let store = SqliteStore::new(&settings).await.expect("store");
        let now = Utc::now();
        for (id, kind, terminal_status) in [
            ("llm-running", "llm_evaluation", None),
            ("sync-running", "vault_sync", None),
            (
                "llm-done",
                "llm_evaluation",
                Some(OperationStatus::Completed),
            ),
        ] {
            store
                .start_operation(&InterruptedOperation {
                    id: id.into(),
                    operation_kind: kind.into(),
                    operation_key: id.into(),
                    state: serde_json::json!({}),
                    status: OperationStatus::Running,
                    started_at: now,
                    heartbeat_at: now,
                    finished_at: None,
                })
                .await
                .expect("start operation");
            if let Some(status) = terminal_status {
                store
                    .finish_operation(id, status, now)
                    .await
                    .expect("finish operation");
            }
        }

        let operations = store
            .list_running_operations(Some("llm_evaluation"))
            .await
            .expect("list running LLM operations");
        assert_eq!(
            operations
                .iter()
                .map(|operation| operation.id.as_str())
                .collect::<Vec<_>>(),
            vec!["llm-running"]
        );
    }

    #[tokio::test]
    async fn lists_queued_and_running_operations_as_active() {
        let temp = tempdir().expect("temporary directory");
        let mut settings = load_settings().expect("settings");
        settings.database_url = format!("sqlite://{}", temp.path().join("store.db").display());
        let store = SqliteStore::new(&settings).await.expect("store");
        let now = Utc::now();
        for (id, status) in [
            ("queued", OperationStatus::Queued),
            ("running", OperationStatus::Running),
            ("completed", OperationStatus::Running),
        ] {
            store
                .start_operation(&InterruptedOperation {
                    id: id.into(),
                    operation_kind: "llm_evaluation".into(),
                    operation_key: id.into(),
                    state: serde_json::json!({}),
                    status,
                    started_at: now,
                    heartbeat_at: now,
                    finished_at: None,
                })
                .await
                .expect("start");
        }
        store
            .finish_operation("completed", OperationStatus::Completed, now)
            .await
            .expect("finish");

        let active = store
            .list_active_operations(Some("llm_evaluation"))
            .await
            .expect("list active evaluations");

        assert_eq!(
            active
                .iter()
                .map(|operation| operation.id.as_str())
                .collect::<Vec<_>>(),
            vec!["queued", "running"]
        );
    }

    #[tokio::test]
    async fn finishing_an_operation_is_idempotent() {
        let temp = tempdir().expect("temporary directory");
        let mut settings = load_settings().expect("settings");
        settings.database_url = format!("sqlite://{}", temp.path().join("store.db").display());
        let store = SqliteStore::new(&settings).await.expect("store");
        let now = Utc::now();
        store
            .start_operation(&InterruptedOperation {
                id: "evaluation-1".into(),
                operation_kind: "llm_evaluation".into(),
                operation_key: "request-1".into(),
                state: serde_json::json!({"phase": "provider_evaluation"}),
                status: OperationStatus::Running,
                started_at: now,
                heartbeat_at: now,
                finished_at: None,
            })
            .await
            .expect("start");

        store
            .finish_operation("evaluation-1", OperationStatus::Completed, now)
            .await
            .expect("first finish");
        store
            .finish_operation("evaluation-1", OperationStatus::Completed, now)
            .await
            .expect("repeated finish should be idempotent");
    }
}
