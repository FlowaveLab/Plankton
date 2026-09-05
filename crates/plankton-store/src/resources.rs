use std::collections::BTreeMap;

use chrono::{DateTime, Utc};
use plankton_core::resources::{BackendKind, ResourceDocument};
use sqlx::Row;

use crate::{SqliteStore, StoreError};

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct BackendBindingRecord {
    pub id: String,
    pub backend_kind: BackendKind,
    pub display_name: String,
    pub enabled: bool,
    pub config: serde_json::Value,
    pub capabilities: Vec<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct VaultManifestRecord {
    pub id: String,
    pub backend_binding_id: String,
    pub display_name: String,
    pub format_version: u32,
    pub local_path: Option<String>,
    pub revision: u64,
    pub archived: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl SqliteStore {
    pub async fn upsert_backend_binding(
        &self,
        binding: &BackendBindingRecord,
    ) -> Result<(), StoreError> {
        sqlx::query(
            r#"
            INSERT INTO backend_bindings (
                id, backend_kind, display_name, enabled, config_json, capabilities_json,
                created_at, updated_at
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?)
            ON CONFLICT(id) DO UPDATE SET
                backend_kind = excluded.backend_kind,
                display_name = excluded.display_name,
                enabled = excluded.enabled,
                config_json = excluded.config_json,
                capabilities_json = excluded.capabilities_json,
                updated_at = excluded.updated_at
            "#,
        )
        .bind(&binding.id)
        .bind(backend_kind_to_str(binding.backend_kind))
        .bind(&binding.display_name)
        .bind(binding.enabled)
        .bind(serde_json::to_string(&binding.config)?)
        .bind(serde_json::to_string(&binding.capabilities)?)
        .bind(binding.created_at.to_rfc3339())
        .bind(binding.updated_at.to_rfc3339())
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn list_backend_bindings(
        &self,
        enabled_only: bool,
    ) -> Result<Vec<BackendBindingRecord>, StoreError> {
        let rows = sqlx::query(
            r#"
            SELECT id, backend_kind, display_name, enabled, config_json, capabilities_json,
                   created_at, updated_at
            FROM backend_bindings
            WHERE (? = 0 OR enabled = 1)
            ORDER BY display_name COLLATE NOCASE, id
            "#,
        )
        .bind(enabled_only)
        .fetch_all(&self.pool)
        .await?;

        rows.into_iter()
            .map(|row| {
                Ok(BackendBindingRecord {
                    id: row.try_get("id")?,
                    backend_kind: backend_kind_from_str(row.try_get("backend_kind")?)?,
                    display_name: row.try_get("display_name")?,
                    enabled: row.try_get("enabled")?,
                    config: serde_json::from_str(row.try_get("config_json")?)?,
                    capabilities: serde_json::from_str(row.try_get("capabilities_json")?)?,
                    created_at: parse_datetime(row.try_get("created_at")?)?,
                    updated_at: parse_datetime(row.try_get("updated_at")?)?,
                })
            })
            .collect()
    }

    pub async fn upsert_vault_manifest(
        &self,
        vault: &VaultManifestRecord,
    ) -> Result<(), StoreError> {
        sqlx::query(
            r#"
            INSERT INTO vault_manifests (
                id, backend_binding_id, display_name, format_version, local_path, revision,
                archived, created_at, updated_at
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)
            ON CONFLICT(id) DO UPDATE SET
                backend_binding_id = excluded.backend_binding_id,
                display_name = excluded.display_name,
                format_version = excluded.format_version,
                local_path = excluded.local_path,
                revision = excluded.revision,
                archived = excluded.archived,
                updated_at = excluded.updated_at
            "#,
        )
        .bind(&vault.id)
        .bind(&vault.backend_binding_id)
        .bind(&vault.display_name)
        .bind(i64::from(vault.format_version))
        .bind(&vault.local_path)
        .bind(i64::try_from(vault.revision).unwrap_or(i64::MAX))
        .bind(vault.archived)
        .bind(vault.created_at.to_rfc3339())
        .bind(vault.updated_at.to_rfc3339())
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Atomically replaces the metadata-only search index.
    ///
    /// `ResourceDocument` intentionally excludes secret field values. The caller must provide a
    /// monotonically increasing generation so cursors can fail closed after a refresh.
    pub async fn replace_resource_search_index(
        &self,
        generation: u64,
        documents: &[ResourceDocument],
    ) -> Result<(), StoreError> {
        let generation = i64::try_from(generation).unwrap_or(i64::MAX);
        let now = Utc::now().to_rfc3339();
        let mut tx = self.pool.begin().await?;
        sqlx::query("DELETE FROM resource_items")
            .execute(&mut *tx)
            .await?;
        sqlx::query("DELETE FROM resource_tags")
            .execute(&mut *tx)
            .await?;

        let mut section_positions = BTreeMap::<(String, String), i64>::new();
        let mut next_section_position = BTreeMap::<String, i64>::new();
        let mut next_field_position = BTreeMap::<String, i64>::new();

        for document in documents {
            let item_id = resource_item_id(&document.resource_id);
            let field_id = &document.resource_id;
            let section_key = (item_id.to_string(), document.section.clone());
            let section_position = *section_positions.entry(section_key).or_insert_with(|| {
                let position = next_section_position
                    .entry(item_id.to_string())
                    .or_default();
                let current = *position;
                *position += 1;
                current
            });
            let section_id = format!("{item_id}:section:{section_position}");
            let field_position = next_field_position.entry(item_id.to_string()).or_default();
            let current_field_position = *field_position;
            *field_position += 1;

            sqlx::query(
                r#"
                INSERT INTO resource_items (
                    id, vault_id, backend_locator_json, display_name, category, notes,
                    metadata_json, archived, revision, created_at, updated_at
                ) VALUES (?, ?, ?, ?, 'custom', ?, ?, 0, 0, ?, ?)
                ON CONFLICT(id) DO UPDATE SET
                    vault_id = excluded.vault_id,
                    backend_locator_json = excluded.backend_locator_json,
                    display_name = excluded.display_name,
                    notes = excluded.notes,
                    metadata_json = excluded.metadata_json,
                    updated_at = excluded.updated_at
                "#,
            )
            .bind(item_id)
            .bind(&document.backend_vault_id)
            .bind(serde_json::to_string(&serde_json::json!({
                "backend_binding_id": document.backend_binding_id,
                "backend_kind": backend_kind_to_str(document.backend_kind),
                "backend_vault_id": document.backend_vault_id,
            }))?)
            .bind(&document.display_name)
            .bind(&document.notes)
            .bind(serde_json::to_string(&document.metadata)?)
            .bind(&now)
            .bind(&now)
            .execute(&mut *tx)
            .await?;

            sqlx::query(
                r#"
                INSERT INTO resource_sections (id, item_id, label, position)
                VALUES (?, ?, ?, ?)
                ON CONFLICT(id) DO UPDATE SET label = excluded.label, position = excluded.position
                "#,
            )
            .bind(&section_id)
            .bind(item_id)
            .bind(&document.section)
            .bind(section_position)
            .execute(&mut *tx)
            .await?;

            sqlx::query(
                r#"
                INSERT INTO resource_fields (
                    id, item_id, section_id, field_key, label, field_kind, is_concealed,
                    position, resource_uri, created_at, updated_at
                ) VALUES (?, ?, ?, ?, ?, 'concealed', 1, ?, ?, ?, ?)
                "#,
            )
            .bind(field_id)
            .bind(item_id)
            .bind(&section_id)
            .bind(&document.field_key)
            .bind(&document.field_label)
            .bind(current_field_position)
            .bind(&document.resource_id)
            .bind(&now)
            .bind(&now)
            .execute(&mut *tx)
            .await?;

            for alias in &document.aliases {
                sqlx::query(
                    r#"
                    INSERT OR IGNORE INTO resource_aliases (item_id, alias, normalized_alias)
                    VALUES (?, ?, ?)
                    "#,
                )
                .bind(item_id)
                .bind(alias)
                .bind(alias.trim().to_lowercase())
                .execute(&mut *tx)
                .await?;
            }
            for tag in &document.tags {
                let normalized_tag = tag.trim().to_lowercase();
                sqlx::query(
                    r#"
                    INSERT OR IGNORE INTO resource_tags (id, normalized_name, display_name)
                    VALUES (?, ?, ?)
                    "#,
                )
                .bind(format!("tag:{normalized_tag}"))
                .bind(&normalized_tag)
                .bind(tag)
                .execute(&mut *tx)
                .await?;
                sqlx::query(
                    r#"
                    INSERT OR IGNORE INTO resource_item_tags (item_id, tag_id)
                    VALUES (?, ?)
                    "#,
                )
                .bind(item_id)
                .bind(format!("tag:{normalized_tag}"))
                .execute(&mut *tx)
                .await?;
            }

            sqlx::query(
                r#"
                INSERT INTO resource_search_documents (
                    field_id, item_id, document_json, normalized_text, index_generation, updated_at
                ) VALUES (?, ?, ?, ?, ?, ?)
                "#,
            )
            .bind(field_id)
            .bind(item_id)
            .bind(serde_json::to_string(&SerializableResourceDocument::from(
                document,
            ))?)
            .bind(normalized_document_text(document))
            .bind(generation)
            .bind(&now)
            .execute(&mut *tx)
            .await?;
        }
        tx.commit().await?;
        Ok(())
    }

    pub async fn load_resource_search_index(
        &self,
    ) -> Result<(u64, Vec<ResourceDocument>), StoreError> {
        let rows = sqlx::query(
            r#"
            SELECT document_json, index_generation
            FROM resource_search_documents
            ORDER BY field_id
            "#,
        )
        .fetch_all(&self.pool)
        .await?;
        let mut generation = None;
        let mut documents = Vec::with_capacity(rows.len());
        for row in rows {
            let row_generation: i64 = row.try_get("index_generation")?;
            if generation.is_some_and(|value| value != row_generation) {
                return Err(StoreError::InconsistentSearchGeneration);
            }
            generation = Some(row_generation);
            let stored: SerializableResourceDocument =
                serde_json::from_str(row.try_get("document_json")?)?;
            documents.push(stored.try_into()?);
        }
        Ok((generation.unwrap_or(0).try_into().unwrap_or(0), documents))
    }
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct SerializableResourceDocument {
    backend_kind: String,
    backend_binding_id: String,
    backend_vault_id: String,
    resource_id: String,
    display_name: String,
    aliases: Vec<String>,
    description: Option<String>,
    notes: String,
    tags: Vec<String>,
    field_key: String,
    field_label: String,
    section: String,
    metadata: BTreeMap<String, String>,
}

impl From<&ResourceDocument> for SerializableResourceDocument {
    fn from(document: &ResourceDocument) -> Self {
        Self {
            backend_kind: backend_kind_to_str(document.backend_kind).to_string(),
            backend_binding_id: document.backend_binding_id.clone(),
            backend_vault_id: document.backend_vault_id.clone(),
            resource_id: document.resource_id.clone(),
            display_name: document.display_name.clone(),
            aliases: document.aliases.clone(),
            description: document.description.clone(),
            notes: document.notes.clone(),
            tags: document.tags.clone(),
            field_key: document.field_key.clone(),
            field_label: document.field_label.clone(),
            section: document.section.clone(),
            metadata: document.metadata.clone(),
        }
    }
}

impl TryFrom<SerializableResourceDocument> for ResourceDocument {
    type Error = StoreError;

    fn try_from(document: SerializableResourceDocument) -> Result<Self, Self::Error> {
        Ok(Self {
            backend_kind: backend_kind_from_str(&document.backend_kind)?,
            backend_binding_id: document.backend_binding_id,
            backend_vault_id: document.backend_vault_id,
            resource_id: document.resource_id,
            display_name: document.display_name,
            aliases: document.aliases,
            description: document.description,
            notes: document.notes,
            tags: document.tags,
            field_key: document.field_key,
            field_label: document.field_label,
            section: document.section,
            metadata: document.metadata,
        })
    }
}

fn backend_kind_to_str(kind: BackendKind) -> &'static str {
    match kind {
        BackendKind::Local => "local",
        BackendKind::OnePassword => "one_password",
        BackendKind::Bitwarden => "bitwarden",
        BackendKind::Custom => "custom",
    }
}

fn backend_kind_from_str(value: &str) -> Result<BackendKind, StoreError> {
    match value {
        "local" => Ok(BackendKind::Local),
        "one_password" => Ok(BackendKind::OnePassword),
        "bitwarden" => Ok(BackendKind::Bitwarden),
        "custom" => Ok(BackendKind::Custom),
        other => Err(StoreError::InvalidStoredValue {
            field: "backend_kind",
            value: other.to_string(),
        }),
    }
}

fn parse_datetime(value: &str) -> Result<DateTime<Utc>, StoreError> {
    DateTime::parse_from_rfc3339(value)
        .map(|value| value.with_timezone(&Utc))
        .map_err(|_| StoreError::InvalidDateTime(value.to_string()))
}

fn resource_item_id(resource_uri: &str) -> &str {
    resource_uri
        .strip_prefix("plankton://field/")
        .and_then(|path| path.split_once('/'))
        .map_or(resource_uri, |(item_id, _)| item_id)
}

fn normalized_document_text(document: &ResourceDocument) -> String {
    [
        document.display_name.as_str(),
        document.notes.as_str(),
        document.field_key.as_str(),
        document.field_label.as_str(),
        document.section.as_str(),
        document.aliases.join(" ").as_str(),
        document.tags.join(" ").as_str(),
    ]
    .join(" ")
    .to_lowercase()
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use plankton_core::{
        load_settings,
        resources::{BackendKind, ResourceDocument},
    };
    use tempfile::tempdir;

    use super::{BackendBindingRecord, SqliteStore, Utc, VaultManifestRecord};

    #[tokio::test]
    async fn persists_bindings_vaults_and_metadata_only_search_documents() {
        let temp = tempdir().expect("temporary directory");
        let mut settings = load_settings().expect("settings");
        settings.database_url = format!("sqlite://{}", temp.path().join("store.db").display());
        let store = SqliteStore::new(&settings).await.expect("store");
        let now = Utc::now();
        store
            .upsert_backend_binding(&BackendBindingRecord {
                id: "local".into(),
                backend_kind: BackendKind::Local,
                display_name: "Personal".into(),
                enabled: true,
                config: serde_json::json!({"path": "vault.kdbx"}),
                capabilities: vec!["search".into(), "get".into()],
                created_at: now,
                updated_at: now,
            })
            .await
            .expect("binding");
        store
            .upsert_vault_manifest(&VaultManifestRecord {
                id: "vault".into(),
                backend_binding_id: "local".into(),
                display_name: "Personal".into(),
                format_version: 4,
                local_path: Some("vault.kdbx".into()),
                revision: 1,
                archived: false,
                created_at: now,
                updated_at: now,
            })
            .await
            .expect("vault");
        let document = ResourceDocument {
            backend_kind: BackendKind::Local,
            backend_binding_id: "local".into(),
            backend_vault_id: "vault".into(),
            resource_id: "plankton://field/item/password".into(),
            display_name: "Production API".into(),
            aliases: vec!["prod".into()],
            description: Some("service credential".into()),
            notes: "rotation every month".into(),
            tags: vec!["production".into()],
            field_key: "api_key".into(),
            field_label: "API Key".into(),
            section: "Credentials".into(),
            metadata: BTreeMap::from([("owner".into(), "platform".into())]),
        };
        store
            .replace_resource_search_index(7, std::slice::from_ref(&document))
            .await
            .expect("replace index");
        let (generation, loaded) = store
            .load_resource_search_index()
            .await
            .expect("load index");
        assert_eq!(generation, 7);
        assert_eq!(loaded, vec![document]);

        let stored = sqlx::query_scalar::<_, String>(
            "SELECT document_json FROM resource_search_documents LIMIT 1",
        )
        .fetch_one(&store.pool)
        .await
        .expect("raw document");
        assert!(!stored.contains("super-secret-value"));
    }

    #[tokio::test]
    async fn persists_search_documents_with_matching_field_segments_for_different_items() {
        let temp = tempdir().expect("temporary directory");
        let mut settings = load_settings().expect("settings");
        settings.database_url = format!("sqlite://{}", temp.path().join("store.db").display());
        let store = SqliteStore::new(&settings).await.expect("store");
        let now = Utc::now();
        store
            .upsert_backend_binding(&BackendBindingRecord {
                id: "local".into(),
                backend_kind: BackendKind::Local,
                display_name: "Personal".into(),
                enabled: true,
                config: serde_json::json!({}),
                capabilities: vec!["search".into()],
                created_at: now,
                updated_at: now,
            })
            .await
            .expect("binding");
        store
            .upsert_vault_manifest(&VaultManifestRecord {
                id: "vault".into(),
                backend_binding_id: "local".into(),
                display_name: "Personal".into(),
                format_version: 1,
                local_path: None,
                revision: 1,
                archived: false,
                created_at: now,
                updated_at: now,
            })
            .await
            .expect("vault");
        let documents = [
            ResourceDocument {
                backend_kind: BackendKind::Local,
                backend_binding_id: "local".into(),
                backend_vault_id: "vault".into(),
                resource_id: "plankton://field/first/password".into(),
                display_name: "First credential".into(),
                aliases: vec![],
                description: None,
                notes: String::new(),
                tags: vec![],
                field_key: "password".into(),
                field_label: "Password".into(),
                section: "Credentials".into(),
                metadata: BTreeMap::new(),
            },
            ResourceDocument {
                backend_kind: BackendKind::Local,
                backend_binding_id: "local".into(),
                backend_vault_id: "vault".into(),
                resource_id: "plankton://field/second/password".into(),
                display_name: "Second credential".into(),
                aliases: vec![],
                description: None,
                notes: String::new(),
                tags: vec![],
                field_key: "password".into(),
                field_label: "Password".into(),
                section: "Credentials".into(),
                metadata: BTreeMap::new(),
            },
        ];

        store
            .replace_resource_search_index(7, &documents)
            .await
            .expect("replace index");

        let (generation, loaded) = store
            .load_resource_search_index()
            .await
            .expect("load index");
        assert_eq!(generation, 7);
        assert_eq!(loaded, documents);
    }
}
