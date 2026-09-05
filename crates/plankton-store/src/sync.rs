use chrono::{DateTime, Utc};
use sqlx::Row;

use crate::{SqliteStore, StoreError};

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct SyncStateRecord {
    pub vault_id: String,
    pub adapter_id: String,
    pub remote_revision: Option<String>,
    pub base_hash: Option<String>,
    pub local_hash: Option<String>,
    pub last_attempt_at: Option<DateTime<Utc>>,
    pub last_success_at: Option<DateTime<Utc>>,
    pub status: String,
    pub error_id: Option<String>,
    pub config: serde_json::Value,
}

impl SqliteStore {
    pub async fn upsert_sync_state(&self, state: &SyncStateRecord) -> Result<(), StoreError> {
        sqlx::query(
            r#"
            INSERT INTO sync_states (
                vault_id, adapter_id, remote_revision, base_hash, local_hash, last_attempt_at,
                last_success_at, status, error_id, config_json
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            ON CONFLICT(vault_id, adapter_id) DO UPDATE SET
                remote_revision = excluded.remote_revision,
                base_hash = excluded.base_hash,
                local_hash = excluded.local_hash,
                last_attempt_at = excluded.last_attempt_at,
                last_success_at = excluded.last_success_at,
                status = excluded.status,
                error_id = excluded.error_id,
                config_json = excluded.config_json
            "#,
        )
        .bind(&state.vault_id)
        .bind(&state.adapter_id)
        .bind(&state.remote_revision)
        .bind(&state.base_hash)
        .bind(&state.local_hash)
        .bind(state.last_attempt_at.map(|value| value.to_rfc3339()))
        .bind(state.last_success_at.map(|value| value.to_rfc3339()))
        .bind(&state.status)
        .bind(&state.error_id)
        .bind(serde_json::to_string(&state.config)?)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn list_sync_states(&self) -> Result<Vec<SyncStateRecord>, StoreError> {
        let rows = sqlx::query(
            r#"
            SELECT vault_id, adapter_id, remote_revision, base_hash, local_hash,
                   last_attempt_at, last_success_at, status, error_id, config_json
            FROM sync_states
            ORDER BY vault_id, adapter_id
            "#,
        )
        .fetch_all(&self.pool)
        .await?;
        rows.into_iter().map(decode_sync_state).collect()
    }
}

fn decode_sync_state(row: sqlx::sqlite::SqliteRow) -> Result<SyncStateRecord, StoreError> {
    Ok(SyncStateRecord {
        vault_id: row.try_get("vault_id")?,
        adapter_id: row.try_get("adapter_id")?,
        remote_revision: row.try_get("remote_revision")?,
        base_hash: row.try_get("base_hash")?,
        local_hash: row.try_get("local_hash")?,
        last_attempt_at: parse_optional_datetime(row.try_get("last_attempt_at")?)?,
        last_success_at: parse_optional_datetime(row.try_get("last_success_at")?)?,
        status: row.try_get("status")?,
        error_id: row.try_get("error_id")?,
        config: serde_json::from_str(row.try_get("config_json")?)?,
    })
}

fn parse_optional_datetime(value: Option<String>) -> Result<Option<DateTime<Utc>>, StoreError> {
    value
        .map(|value| {
            DateTime::parse_from_rfc3339(&value)
                .map(|value| value.with_timezone(&Utc))
                .map_err(|_| StoreError::InvalidDateTime(value))
        })
        .transpose()
}

#[cfg(test)]
mod tests {
    use plankton_core::{load_settings, resources::BackendKind};
    use tempfile::tempdir;

    use super::*;
    use crate::{BackendBindingRecord, VaultManifestRecord};

    #[tokio::test]
    async fn round_trips_sync_configuration_and_status() {
        let temp = tempdir().expect("temporary directory");
        let mut settings = load_settings().expect("settings");
        settings.database_url = format!("sqlite://{}", temp.path().join("store.db").display());
        let store = SqliteStore::new(&settings).await.expect("store");
        let now = Utc::now();
        store
            .upsert_backend_binding(&BackendBindingRecord {
                id: "plankton".into(),
                backend_kind: BackendKind::Local,
                display_name: "Plankton".into(),
                enabled: true,
                config: serde_json::json!({}),
                capabilities: vec!["sync".into()],
                created_at: now,
                updated_at: now,
            })
            .await
            .expect("binding");
        store
            .upsert_vault_manifest(&VaultManifestRecord {
                id: "default".into(),
                backend_binding_id: "plankton".into(),
                display_name: "Default".into(),
                format_version: 4,
                local_path: Some("default.kdbx".into()),
                revision: 0,
                archived: false,
                created_at: now,
                updated_at: now,
            })
            .await
            .expect("vault");
        let record = SyncStateRecord {
            vault_id: "default".into(),
            adapter_id: "backup".into(),
            remote_revision: Some("7".into()),
            base_hash: Some("base".into()),
            local_hash: Some("local".into()),
            last_attempt_at: Some(now),
            last_success_at: Some(now),
            status: "idle".into(),
            error_id: None,
            config: serde_json::json!({
                "kind": "local_folder",
                "directory": "/tmp/backups"
            }),
        };
        store.upsert_sync_state(&record).await.expect("sync state");

        assert_eq!(
            store.list_sync_states().await.expect("list sync states"),
            vec![record]
        );
    }
}
