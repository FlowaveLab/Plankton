use chrono::{DateTime, Utc};
use plankton_protocol::password_changes::{
    PasswordChangeDiff, PasswordChangeOperation, PasswordChangeState, PasswordChangeStatus,
};
use sqlx::Row;

use crate::{SqliteStore, StoreError};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredPasswordChange {
    pub status: PasswordChangeStatus,
    pub base_revision: String,
    pub operations: Vec<PasswordChangeOperation>,
    pub operation_ids: Vec<String>,
}

impl SqliteStore {
    pub async fn insert_password_change(
        &self,
        change: &StoredPasswordChange,
    ) -> Result<(), StoreError> {
        let status = &change.status;
        sqlx::query(
            r#"
            INSERT INTO password_changes (
                id, batch_id, reason, requested_by, state, version, confirmed_version,
                base_revision, operations_json, operation_ids_json, diff_json,
                successor_change_id, created_at, updated_at, confirmed_at, committed_at,
                error_message
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind(&status.change_id)
        .bind(&status.batch_id)
        .bind(&status.reason)
        .bind(&status.requested_by)
        .bind(state_string(status.state))
        .bind(status.version as i64)
        .bind(status.confirmed_version.map(|value| value as i64))
        .bind(&change.base_revision)
        .bind(serde_json::to_string(&change.operations)?)
        .bind(serde_json::to_string(&change.operation_ids)?)
        .bind(serde_json::to_string(&status.diff)?)
        .bind(&status.successor_change_id)
        .bind(status.created_at.to_rfc3339())
        .bind(status.updated_at.to_rfc3339())
        .bind(status.confirmed_at.map(|value| value.to_rfc3339()))
        .bind(status.committed_at.map(|value| value.to_rfc3339()))
        .bind(&status.error)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn save_password_change(
        &self,
        change: &StoredPasswordChange,
    ) -> Result<(), StoreError> {
        let status = &change.status;
        let result = sqlx::query(
            r#"
            UPDATE password_changes
            SET state = ?, version = ?, confirmed_version = ?, base_revision = ?,
                operations_json = ?, operation_ids_json = ?, diff_json = ?,
                successor_change_id = ?, updated_at = ?, confirmed_at = ?, committed_at = ?,
                error_message = ?
            WHERE id = ?
            "#,
        )
        .bind(state_string(status.state))
        .bind(status.version as i64)
        .bind(status.confirmed_version.map(|value| value as i64))
        .bind(&change.base_revision)
        .bind(serde_json::to_string(&change.operations)?)
        .bind(serde_json::to_string(&change.operation_ids)?)
        .bind(serde_json::to_string(&status.diff)?)
        .bind(&status.successor_change_id)
        .bind(status.updated_at.to_rfc3339())
        .bind(status.confirmed_at.map(|value| value.to_rfc3339()))
        .bind(status.committed_at.map(|value| value.to_rfc3339()))
        .bind(&status.error)
        .bind(&status.change_id)
        .execute(&self.pool)
        .await?;
        if result.rows_affected() == 0 {
            return Err(StoreError::NotFound(status.change_id.clone()));
        }
        Ok(())
    }

    pub async fn get_password_change(
        &self,
        change_id: &str,
    ) -> Result<StoredPasswordChange, StoreError> {
        let row = sqlx::query("SELECT * FROM password_changes WHERE id = ?")
            .bind(change_id)
            .fetch_optional(&self.pool)
            .await?
            .ok_or_else(|| StoreError::NotFound(change_id.to_string()))?;
        decode_password_change(&row)
    }

    pub async fn list_pending_password_changes(
        &self,
    ) -> Result<Vec<StoredPasswordChange>, StoreError> {
        let rows = sqlx::query(
            r#"
            SELECT * FROM password_changes
            WHERE state = 'pending_confirmation' AND version > 0
            ORDER BY created_at ASC, id ASC
            "#,
        )
        .fetch_all(&self.pool)
        .await?;
        rows.iter().map(decode_password_change).collect()
    }

    pub async fn find_pending_password_change_by_batch(
        &self,
        batch_id: &str,
        requested_by: &str,
    ) -> Result<Option<StoredPasswordChange>, StoreError> {
        let row = sqlx::query(
            r#"
            SELECT * FROM password_changes
            WHERE batch_id = ? AND requested_by = ?
              AND state = 'pending_confirmation' AND version > 0
            ORDER BY updated_at DESC, id DESC
            LIMIT 1
            "#,
        )
        .bind(batch_id)
        .bind(requested_by)
        .fetch_optional(&self.pool)
        .await?;
        row.as_ref().map(decode_password_change).transpose()
    }
}

fn decode_password_change(
    row: &sqlx::sqlite::SqliteRow,
) -> Result<StoredPasswordChange, StoreError> {
    let state_text = row.try_get::<String, _>("state")?;
    let state = parse_state(&state_text)?;
    let created_at = parse_datetime(row.try_get("created_at")?)?;
    let updated_at = parse_datetime(row.try_get("updated_at")?)?;
    let confirmed_at = row
        .try_get::<Option<String>, _>("confirmed_at")?
        .map(parse_datetime)
        .transpose()?;
    let committed_at = row
        .try_get::<Option<String>, _>("committed_at")?
        .map(parse_datetime)
        .transpose()?;
    let diff: PasswordChangeDiff = serde_json::from_str(row.try_get("diff_json")?)?;
    let operations = serde_json::from_str(row.try_get("operations_json")?)?;
    let operation_ids = serde_json::from_str(row.try_get("operation_ids_json")?)?;
    Ok(StoredPasswordChange {
        status: PasswordChangeStatus {
            batch_id: row.try_get("batch_id")?,
            change_id: row.try_get("id")?,
            version: row.try_get::<i64, _>("version")? as u64,
            confirmed_version: row
                .try_get::<Option<i64>, _>("confirmed_version")?
                .map(|value| value as u64),
            state,
            reason: row.try_get("reason")?,
            requested_by: row.try_get("requested_by")?,
            diff,
            successor_change_id: row.try_get("successor_change_id")?,
            created_at,
            updated_at,
            confirmed_at,
            committed_at,
            error: row.try_get("error_message")?,
        },
        base_revision: row.try_get("base_revision")?,
        operations,
        operation_ids,
    })
}

fn parse_datetime(value: String) -> Result<DateTime<Utc>, StoreError> {
    DateTime::parse_from_rfc3339(&value)
        .map(|value| value.with_timezone(&Utc))
        .map_err(|_| StoreError::InvalidDateTime(value))
}

fn state_string(state: PasswordChangeState) -> &'static str {
    match state {
        PasswordChangeState::PendingConfirmation => "pending_confirmation",
        PasswordChangeState::Confirmed => "confirmed",
        PasswordChangeState::Committing => "committing",
        PasswordChangeState::Committed => "committed",
        PasswordChangeState::Rejected => "rejected",
        PasswordChangeState::Conflict => "conflict",
        PasswordChangeState::Failed => "failed",
    }
}

fn parse_state(value: &str) -> Result<PasswordChangeState, StoreError> {
    match value {
        "pending_confirmation" => Ok(PasswordChangeState::PendingConfirmation),
        "confirmed" => Ok(PasswordChangeState::Confirmed),
        "committing" => Ok(PasswordChangeState::Committing),
        "committed" => Ok(PasswordChangeState::Committed),
        "rejected" => Ok(PasswordChangeState::Rejected),
        "conflict" => Ok(PasswordChangeState::Conflict),
        "failed" => Ok(PasswordChangeState::Failed),
        _ => Err(StoreError::InvalidStoredValue {
            field: "password_changes.state",
            value: value.to_string(),
        }),
    }
}

#[cfg(test)]
mod tests {
    use chrono::Utc;
    use plankton_core::PlanktonSettings;
    use plankton_protocol::password_changes::{
        PasswordChangeDiff, PasswordChangeOperation, PasswordChangeState, PasswordChangeStatus,
    };

    use super::StoredPasswordChange;
    use crate::SqliteStore;

    #[tokio::test]
    async fn pending_change_round_trips_operations_and_cumulative_diff() {
        let directory = tempfile::tempdir().expect("tempdir");
        let settings = PlanktonSettings {
            database_url: format!("sqlite://{}", directory.path().join("store.db").display()),
            ..PlanktonSettings::default()
        };
        let store = SqliteStore::new(&settings).await.expect("store");
        let now = Utc::now();
        let change = StoredPasswordChange {
            status: PasswordChangeStatus {
                batch_id: "batch_test".to_string(),
                change_id: "chg_test".to_string(),
                version: 1,
                confirmed_version: None,
                state: PasswordChangeState::PendingConfirmation,
                reason: "test catalog management".to_string(),
                requested_by: "test-agent".to_string(),
                diff: PasswordChangeDiff {
                    items: Vec::new(),
                    changed_items: 0,
                    changed_fields: 0,
                    breaking_changes: 0,
                },
                successor_change_id: None,
                created_at: now,
                updated_at: now,
                confirmed_at: None,
                committed_at: None,
                error: None,
            },
            base_revision: "revision".to_string(),
            operations: vec![PasswordChangeOperation::UpdateItem {
                item_id: "service".to_string(),
                next_item_id: None,
                title: Some("Service".to_string()),
                description: None,
                clear_description: false,
                tags: None,
                metadata: None,
            }],
            operation_ids: vec!["op_test".to_string()],
        };

        store
            .insert_password_change(&change)
            .await
            .expect("insert change");
        let mut empty_change = change.clone();
        empty_change.status.change_id = "chg_empty".to_string();
        empty_change.status.version = 0;
        empty_change.operations.clear();
        empty_change.operation_ids.clear();
        store
            .insert_password_change(&empty_change)
            .await
            .expect("insert legacy empty change");
        assert_eq!(
            store
                .get_password_change("chg_test")
                .await
                .expect("read change"),
            change
        );
        assert_eq!(
            store
                .list_pending_password_changes()
                .await
                .expect("list pending"),
            vec![change.clone()]
        );
        assert_eq!(
            store
                .find_pending_password_change_by_batch("batch_test", "test-agent")
                .await
                .expect("find pending batch"),
            Some(change)
        );
    }
}
