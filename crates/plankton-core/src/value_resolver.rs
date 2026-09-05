use std::{
    collections::{BTreeMap, BTreeSet},
    env, fs,
    io::Write,
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

use chrono::{DateTime, Utc};
use directories::ProjectDirs;
use dotenvy::from_path_iter;
use minijinja::{Environment, UndefinedBehavior};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use tracing::{debug, warn};

const LOCAL_SECRET_CATALOG_RESOLVER_KIND: &str = "local_secret_catalog";
pub const ONEPASSWORD_CLI_PROVIDER_KIND: &str = "1password_cli";
pub const BITWARDEN_CLI_PROVIDER_KIND: &str = "bitwarden_cli";
pub const KEEPASSXC_CLI_PROVIDER_KIND: &str = "keepassxc_cli";
pub const DOTENV_FILE_PROVIDER_KIND: &str = "dotenv_file";
const DEFAULT_ONEPASSWORD_RESOURCE_TEMPLATE: &str =
    "secret/{{ account }}/{{ vault }}/{{ item }}/{{ field }}";
const DEFAULT_BITWARDEN_RESOURCE_TEMPLATE: &str = "secret/{{ container }}/{{ item }}/{{ field }}";
const DEFAULT_DOTENV_RESOURCE_TEMPLATE: &str = "secret/{{ source_name }}/{{ key }}";
const DEFAULT_KEEPASSXC_RESOURCE_TEMPLATE: &str = "secret/{{ vault }}/{{ item }}/{{ field }}";

const SECRET_CATALOG_BOOTSTRAP_TEMPLATE: &str = r#"# Plankton local secret catalog
# Map resource identifiers either to literal secret values or to imported source locators.
#
# Direct value example:
# [secrets]
# "secret/demo" = "replace-me"
#
# Imported reference example:
# [[imports]]
# resource = "secret/demo"
# display_name = "Demo token"
# provider_kind = "dotenv_file"
# file_path = "/abs/path/to/.env"
# key = "DEMO_TOKEN"
#
[secrets]
"#;

pub trait ValueResolver: Send + Sync {
    fn kind(&self) -> &'static str;

    fn list_resources(&self) -> Vec<String>;

    fn resolve(&self, resource: &str) -> Result<String, ValueResolverError>;
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SecretImportSpec {
    pub resource: String,
    pub display_name: Option<String>,
    pub description: Option<String>,
    pub tags: Vec<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub metadata: BTreeMap<String, String>,
    pub source_locator: SecretSourceLocator,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SecretImportBatchSpec {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resource_template: Option<String>,
    pub imports: Vec<SecretImportSpec>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ImportedSecretReference {
    pub resource: String,
    pub display_name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub metadata: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value: Option<String>,
    #[serde(flatten)]
    pub source_locator: SecretSourceLocator,
    pub imported_at: DateTime<Utc>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_verified_at: Option<DateTime<Utc>>,
}

impl ImportedSecretReference {
    pub fn provider_kind(&self) -> &'static str {
        self.source_locator.provider_kind()
    }

    pub fn container_label(&self) -> Option<&str> {
        self.source_locator.container_label()
    }

    pub fn field_selector(&self) -> &str {
        self.source_locator.field_selector()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "provider_kind", rename_all = "snake_case")]
pub enum SecretSourceLocator {
    #[serde(rename = "keepassxc_cli")]
    KeepassxcCli {
        database: PathBuf,
        entry: String,
        field: String,
        unlock_secret_file: PathBuf,
        executable: PathBuf,
        executable_sha256: String,
    },
    #[serde(rename = "1password_cli")]
    OnePasswordCli {
        account: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        account_id: Option<String>,
        vault: String,
        item: String,
        field: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        vault_id: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        item_id: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        field_id: Option<String>,
    },
    #[serde(rename = "bitwarden_cli")]
    BitwardenCli {
        account: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        organization: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        collection: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        folder: Option<String>,
        item: String,
        field: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        item_id: Option<String>,
    },
    #[serde(rename = "dotenv_file")]
    DotenvFile {
        file_path: PathBuf,
        #[serde(skip_serializing_if = "Option::is_none")]
        namespace: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        prefix: Option<String>,
        key: String,
    },
}

impl SecretSourceLocator {
    pub fn provider_kind(&self) -> &'static str {
        match self {
            Self::KeepassxcCli { .. } => KEEPASSXC_CLI_PROVIDER_KIND,
            Self::OnePasswordCli { .. } => ONEPASSWORD_CLI_PROVIDER_KIND,
            Self::BitwardenCli { .. } => BITWARDEN_CLI_PROVIDER_KIND,
            Self::DotenvFile { .. } => DOTENV_FILE_PROVIDER_KIND,
        }
    }

    pub fn container_label(&self) -> Option<&str> {
        match self {
            Self::KeepassxcCli { database, .. } => {
                database.file_stem().and_then(|value| value.to_str())
            }
            Self::OnePasswordCli { vault, .. } => Some(vault.as_str()),
            Self::BitwardenCli {
                collection,
                folder,
                organization,
                account,
                ..
            } => collection
                .as_deref()
                .or(folder.as_deref())
                .or(organization.as_deref())
                .or(Some(account.as_str())),
            Self::DotenvFile {
                namespace,
                prefix,
                file_path,
                ..
            } => namespace
                .as_deref()
                .or(prefix.as_deref())
                .or_else(|| file_path.file_name().and_then(|name| name.to_str())),
        }
    }

    pub fn field_selector(&self) -> &str {
        match self {
            Self::KeepassxcCli { field, .. } => field.as_str(),
            Self::OnePasswordCli { field, .. } => field.as_str(),
            Self::BitwardenCli { field, .. } => field.as_str(),
            Self::DotenvFile { key, .. } => key.as_str(),
        }
    }

    fn default_display_name(&self) -> String {
        match self {
            Self::KeepassxcCli { entry, field, .. } => format!("{entry}:{field}"),
            Self::OnePasswordCli { item, field, .. } => format!("{item}:{field}"),
            Self::BitwardenCli { item, field, .. } => format!("{item}:{field}"),
            Self::DotenvFile { key, .. } => key.clone(),
        }
    }

    fn default_resource_template(&self) -> &'static str {
        match self {
            Self::KeepassxcCli { .. } => DEFAULT_KEEPASSXC_RESOURCE_TEMPLATE,
            Self::OnePasswordCli { .. } => DEFAULT_ONEPASSWORD_RESOURCE_TEMPLATE,
            Self::BitwardenCli { .. } => DEFAULT_BITWARDEN_RESOURCE_TEMPLATE,
            Self::DotenvFile { .. } => DEFAULT_DOTENV_RESOURCE_TEMPLATE,
        }
    }

    fn resource_template_context(&self) -> BTreeMap<String, String> {
        let mut context = BTreeMap::from([(
            "provider_kind".to_string(),
            self.provider_kind().to_string(),
        )]);

        match self {
            Self::KeepassxcCli {
                database,
                entry,
                field,
                ..
            } => {
                let vault = database
                    .file_stem()
                    .and_then(|value| value.to_str())
                    .unwrap_or("plankton")
                    .to_string();
                context.insert("vault".to_string(), vault.clone());
                context.insert("source_name".to_string(), vault);
                context.insert("item".to_string(), entry.clone());
                context.insert("field".to_string(), field.clone());
            }
            Self::OnePasswordCli {
                account,
                account_id,
                vault,
                item,
                field,
                vault_id,
                item_id,
                field_id,
            } => {
                context.insert("account".to_string(), account.clone());
                context.insert("vault".to_string(), vault.clone());
                context.insert("container".to_string(), vault.clone());
                context.insert("item".to_string(), item.clone());
                context.insert("field".to_string(), field.clone());
                if let Some(account_id) = account_id.clone() {
                    context.insert("account_id".to_string(), account_id);
                }
                if let Some(vault_id) = vault_id.clone() {
                    context.insert("vault_id".to_string(), vault_id);
                }
                if let Some(item_id) = item_id.clone() {
                    context.insert("item_id".to_string(), item_id);
                }
                if let Some(field_id) = field_id.clone() {
                    context.insert("field_id".to_string(), field_id);
                }
            }
            Self::BitwardenCli {
                account,
                organization,
                collection,
                folder,
                item,
                field,
                item_id,
            } => {
                context.insert("account".to_string(), account.clone());
                context.insert(
                    "container".to_string(),
                    collection
                        .clone()
                        .or_else(|| folder.clone())
                        .or_else(|| organization.clone())
                        .unwrap_or_else(|| account.clone()),
                );
                context.insert("item".to_string(), item.clone());
                context.insert("field".to_string(), field.clone());
                if let Some(organization) = organization.clone() {
                    context.insert("organization".to_string(), organization);
                }
                if let Some(collection) = collection.clone() {
                    context.insert("collection".to_string(), collection);
                }
                if let Some(folder) = folder.clone() {
                    context.insert("folder".to_string(), folder);
                }
                if let Some(item_id) = item_id.clone() {
                    context.insert("item_id".to_string(), item_id);
                }
            }
            Self::DotenvFile {
                file_path,
                namespace,
                prefix,
                key,
            } => {
                context.insert("file_path".to_string(), file_path.display().to_string());
                context.insert("key".to_string(), key.clone());
                if let Some(file_name) = file_path.file_name().and_then(|value| value.to_str()) {
                    context.insert("file_name".to_string(), file_name.to_string());
                }
                if let Some(file_stem) = file_path.file_stem().and_then(|value| value.to_str()) {
                    context.insert("file_stem".to_string(), file_stem.to_string());
                }
                if let Some(namespace) = namespace.clone() {
                    context.insert("namespace".to_string(), namespace.clone());
                    context
                        .entry("source_name".to_string())
                        .or_insert(namespace);
                }
                if let Some(prefix) = prefix.clone() {
                    context.insert("prefix".to_string(), prefix.clone());
                    context.entry("source_name".to_string()).or_insert(prefix);
                }
                if !context.contains_key("source_name") {
                    let fallback = file_path
                        .file_stem()
                        .and_then(|value| value.to_str())
                        .or_else(|| file_path.file_name().and_then(|value| value.to_str()))
                        .unwrap_or("dotenv");
                    context.insert("source_name".to_string(), fallback.to_string());
                }
            }
        }

        context
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ImportedSecretReceipt {
    pub catalog_path: PathBuf,
    pub reference: ImportedSecretReference,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ImportedSecretBatchReceipt {
    pub catalog_path: PathBuf,
    pub receipts: Vec<ImportedSecretReceipt>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ImportedSecretCatalog {
    pub catalog_path: PathBuf,
    pub imports: Vec<ImportedSecretReference>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct LocalSecretLiteralMetadataRecord {
    pub(crate) resource: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) display_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) description: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) tags: Vec<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub(crate) metadata: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LocalSecretLiteralEntry {
    pub resource: String,
    pub value: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub metadata: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LocalSecretCatalog {
    pub catalog_path: PathBuf,
    pub literals: Vec<LocalSecretLiteralEntry>,
    pub imports: Vec<ImportedSecretReference>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ImportedSecretReferenceUpdate {
    pub resource: String,
    pub display_name: Option<String>,
    pub description: Option<String>,
    pub tags: Vec<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub metadata: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LocalSecretLiteralUpsert {
    pub resource: String,
    pub value: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub metadata: BTreeMap<String, String>,
}

#[derive(Debug, Clone)]
pub struct LocalSecretCatalogResolver {
    entries: BTreeMap<String, SecretCatalogEntry>,
}

#[derive(Debug, Clone)]
enum SecretCatalogEntry {
    Literal(String),
    Imported(Box<ImportedSecretReference>),
}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub(crate) struct SecretCatalogFile {
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub(crate) secrets: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub(crate) values: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) literal_entries: Vec<LocalSecretLiteralMetadataRecord>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) imports: Vec<ImportedSecretReference>,
}

#[derive(Debug, Clone)]
struct VendorCliPrograms {
    onepassword: PathBuf,
    bitwarden: PathBuf,
}

impl VendorCliPrograms {
    fn from_env() -> Self {
        Self {
            onepassword: env::var_os("PLANKTON_1PASSWORD_CLI_BIN")
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from("op")),
            bitwarden: env::var_os("PLANKTON_BITWARDEN_CLI_BIN")
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from("bw")),
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum SecretImportError {
    #[error("resource key is required")]
    MissingResource,
    #[error("secret value is required")]
    MissingSecretValue,
    #[error("batch import requires at least one source")]
    EmptyBatch,
    #[error("resource template is invalid: {message}")]
    InvalidResourceTemplate { message: String },
    #[error("resource template did not produce a valid resource identifier")]
    InvalidGeneratedResource,
    #[error("batch import generated duplicate resource identifier {resource}")]
    DuplicateResource { resource: String },
    #[error("failed to load local secret catalog from {path}: {message}")]
    LoadCatalog { path: String, message: String },
    #[error("failed to serialize local secret catalog to {path}: {message}")]
    SerializeCatalog { path: String, message: String },
    #[error("failed to write local secret catalog to {path}: {message}")]
    WriteCatalog { path: String, message: String },
    #[error("failed to verify imported source for {resource}: {source}")]
    VerifySource {
        resource: String,
        #[source]
        source: ValueResolverError,
    },
    #[error("resource {resource} was not found in the local secret catalog")]
    ResourceNotFound { resource: String },
    #[error("imported resource {resource} was not found in the local secret catalog")]
    ImportedResourceNotFound { resource: String },
    #[error("password item {item_id} was not found in the local secret catalog")]
    ItemNotFound { item_id: String },
    #[error("password item selector {item_id} matches more than one catalog item")]
    AmbiguousItem { item_id: String },
    #[error("a title is required when creating password item {item_id}")]
    MissingTargetItemTitle { item_id: String },
    #[error("password item {item_id} already contains field {label}")]
    DuplicateFieldLabel { item_id: String, label: String },
    #[error("resource {resource} has no stored value available for duplicate comparison")]
    MissingComparableValue { resource: String },
    #[error("resource {resource} does not match canonical resource {canonical_resource}")]
    DuplicateFieldMismatch {
        resource: String,
        canonical_resource: String,
    },
    #[error("password catalog changed while editing (expected {expected}, actual {actual})")]
    CatalogConflict { expected: String, actual: String },
    #[error("credential exposure policy is invalid: {message}")]
    InvalidExposurePolicy { message: String },
}

#[derive(Debug, thiserror::Error)]
pub enum ValueResolverError {
    #[error("secret catalog file not found at {path}")]
    CatalogMissing { path: String },
    #[error("local secret catalog setup is required at {path}")]
    CatalogBootstrapRequired { path: String, created: bool },
    #[error("failed to create local secret catalog at {path}: {message}")]
    CreateCatalog { path: String, message: String },
    #[error("failed to read secret catalog from {path}: {message}")]
    ReadCatalog { path: String, message: String },
    #[error("failed to parse secret catalog from {path}: {message}")]
    ParseCatalog { path: String, message: String },
    #[error("resource {resource} was not found in the local secret catalog")]
    ResourceNotFound { resource: String },
    #[error("resource {resource} resolved to an empty value")]
    EmptyValue { resource: String },
    #[error("{provider_kind} source locator for {resource} is invalid: {message}")]
    InvalidSourceLocator {
        provider_kind: String,
        resource: String,
        message: String,
    },
    #[error("{provider_kind} CLI is not available for {resource}: {message}")]
    ToolUnavailable {
        provider_kind: String,
        resource: String,
        message: String,
    },
    #[error("failed to read {provider_kind} source for {resource}: {message}")]
    SourceReadFailed {
        provider_kind: String,
        resource: String,
        message: String,
    },
    #[error("failed to parse {provider_kind} response for {resource}: {message}")]
    SourceParseFailed {
        provider_kind: String,
        resource: String,
        message: String,
    },
    #[error("{provider_kind} source for {resource} did not contain field {field}")]
    SourceFieldNotFound {
        provider_kind: String,
        resource: String,
        field: String,
    },
    #[error(
        "{provider_kind} source for {resource} contained field {field}, but its value was empty"
    )]
    SourceFieldEmpty {
        provider_kind: String,
        resource: String,
        field: String,
    },
}

impl LocalSecretCatalogResolver {
    pub fn load_default() -> Result<Self, ValueResolverError> {
        let path = local_secret_catalog_path();
        if !path.exists() {
            let created = bootstrap_secret_catalog(path.as_path())?;
            return Err(ValueResolverError::CatalogBootstrapRequired {
                path: path.display().to_string(),
                created,
            });
        }
        Self::load_from_path(path.as_path())
    }

    pub fn load_from_path(path: &Path) -> Result<Self, ValueResolverError> {
        let catalog = load_secret_catalog_file(path)?;
        Ok(Self {
            entries: catalog_entries(catalog),
        })
    }

    fn resolve_with_programs(
        &self,
        resource: &str,
        programs: &VendorCliPrograms,
    ) -> Result<String, ValueResolverError> {
        let resource = resource.trim();
        let entry =
            self.entries
                .get(resource)
                .ok_or_else(|| ValueResolverError::ResourceNotFound {
                    resource: resource.to_string(),
                })?;

        match entry {
            SecretCatalogEntry::Literal(value) => {
                if value.is_empty() {
                    Err(ValueResolverError::EmptyValue {
                        resource: resource.to_string(),
                    })
                } else {
                    Ok(value.clone())
                }
            }
            SecretCatalogEntry::Imported(reference) => match reference.value.as_deref() {
                Some(value) if !value.is_empty() => Ok(value.to_string()),
                Some(_) => Err(ValueResolverError::EmptyValue {
                    resource: resource.to_string(),
                }),
                None => resolve_imported_reference(reference, resource, programs),
            },
        }
    }
}

impl ValueResolver for LocalSecretCatalogResolver {
    fn kind(&self) -> &'static str {
        LOCAL_SECRET_CATALOG_RESOLVER_KIND
    }

    fn list_resources(&self) -> Vec<String> {
        self.entries.keys().cloned().collect()
    }

    fn resolve(&self, resource: &str) -> Result<String, ValueResolverError> {
        self.resolve_with_programs(resource, &VendorCliPrograms::from_env())
    }
}

pub fn local_secret_catalog_path() -> PathBuf {
    if let Ok(path) = env::var("PLANKTON_SECRET_FILE") {
        let trimmed = path.trim();
        if !trimmed.is_empty() {
            return PathBuf::from(trimmed);
        }
    }

    if let Some(project_dirs) = ProjectDirs::from("com", "OpenAquarium", "Plankton") {
        return project_dirs
            .config_local_dir()
            .join("plankton-secrets.toml");
    }

    std::env::temp_dir().join("plankton-secrets.toml")
}

pub fn default_value_resolver() -> Result<LocalSecretCatalogResolver, ValueResolverError> {
    LocalSecretCatalogResolver::load_default()
}

pub fn import_secret_reference(
    spec: SecretImportSpec,
) -> Result<ImportedSecretReceipt, SecretImportError> {
    let receipt = import_secret_references_at(
        local_secret_catalog_path().as_path(),
        SecretImportBatchSpec {
            resource_template: None,
            imports: vec![spec],
        },
    )?;
    receipt
        .receipts
        .into_iter()
        .next()
        .ok_or(SecretImportError::EmptyBatch)
}

/// Registers a live backend reference without copying its secret value into the catalog.
///
/// This is used after a human-confirmed backend write has already succeeded. Metadata remains
/// searchable while every read resolves from the encrypted/backend source of truth.
pub fn register_secret_reference(
    spec: SecretImportSpec,
) -> Result<ImportedSecretReceipt, SecretImportError> {
    let receipt = register_secret_references(vec![spec])?;
    receipt
        .receipts
        .into_iter()
        .next()
        .ok_or(SecretImportError::EmptyBatch)
}

/// Atomically registers live backend references without resolving or copying secret values.
pub fn register_secret_references(
    specs: Vec<SecretImportSpec>,
) -> Result<ImportedSecretBatchReceipt, SecretImportError> {
    register_secret_references_at(local_secret_catalog_path().as_path(), specs)
}

fn register_secret_references_at(
    path: &Path,
    specs: Vec<SecretImportSpec>,
) -> Result<ImportedSecretBatchReceipt, SecretImportError> {
    if specs.is_empty() {
        return Err(SecretImportError::EmptyBatch);
    }

    let mut catalog = load_secret_catalog_file_optional(path)?;
    let now = Utc::now();
    let mut references = Vec::with_capacity(specs.len());
    let mut seen_resources = BTreeSet::new();
    for spec in specs {
        let resource = resolve_import_resource(&spec, None)?;
        if !seen_resources.insert(resource.clone()) {
            return Err(SecretImportError::DuplicateResource { resource });
        }
        references.push(ImportedSecretReference {
            resource,
            display_name: spec
                .display_name
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(ToOwned::to_owned)
                .unwrap_or_else(|| spec.source_locator.default_display_name()),
            description: spec
                .description
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(ToOwned::to_owned),
            tags: sanitize_tags(spec.tags),
            metadata: sanitize_metadata(spec.metadata),
            value: None,
            source_locator: spec.source_locator,
            imported_at: now,
            last_verified_at: Some(now),
        });
    }

    for reference in &references {
        catalog.secrets.remove(&reference.resource);
        catalog.values.remove(&reference.resource);
        catalog
            .literal_entries
            .retain(|entry| entry.resource != reference.resource);
        if let Some(existing) = catalog
            .imports
            .iter_mut()
            .find(|existing| existing.resource == reference.resource)
        {
            *existing = reference.clone();
        } else {
            catalog.imports.push(reference.clone());
        }
    }
    save_secret_catalog_file(path, &catalog)?;

    Ok(ImportedSecretBatchReceipt {
        catalog_path: path.to_path_buf(),
        receipts: references
            .into_iter()
            .map(|reference| ImportedSecretReceipt {
                catalog_path: path.to_path_buf(),
                reference,
            })
            .collect(),
    })
}

pub fn import_secret_reference_at(
    path: &Path,
    spec: SecretImportSpec,
) -> Result<ImportedSecretReceipt, SecretImportError> {
    let receipt = import_secret_references_at(
        path,
        SecretImportBatchSpec {
            resource_template: None,
            imports: vec![spec],
        },
    )?;
    receipt
        .receipts
        .into_iter()
        .next()
        .ok_or(SecretImportError::EmptyBatch)
}

pub fn import_secret_references(
    spec: SecretImportBatchSpec,
) -> Result<ImportedSecretBatchReceipt, SecretImportError> {
    import_secret_references_at(local_secret_catalog_path().as_path(), spec)
}

pub fn import_secret_references_at(
    path: &Path,
    spec: SecretImportBatchSpec,
) -> Result<ImportedSecretBatchReceipt, SecretImportError> {
    import_secret_references_at_with_programs(path, spec, &VendorCliPrograms::from_env())
}

pub fn list_imported_secret_references() -> Result<ImportedSecretCatalog, SecretImportError> {
    list_imported_secret_references_at(local_secret_catalog_path().as_path())
}

pub fn list_local_secret_catalog() -> Result<LocalSecretCatalog, SecretImportError> {
    list_local_secret_catalog_at(local_secret_catalog_path().as_path())
}

pub fn get_local_secret_resource_description(
    resource: &str,
) -> Result<Option<String>, SecretImportError> {
    get_local_secret_resource_description_at(local_secret_catalog_path().as_path(), resource)
}

pub fn update_local_secret_resource_description(
    resource: &str,
    description: Option<String>,
) -> Result<Option<String>, SecretImportError> {
    update_local_secret_resource_description_at(
        local_secret_catalog_path().as_path(),
        resource,
        description,
    )
}

pub fn list_imported_secret_references_at(
    path: &Path,
) -> Result<ImportedSecretCatalog, SecretImportError> {
    let mut catalog = load_secret_catalog_file_optional(path)?;
    catalog
        .imports
        .sort_by(|left, right| left.resource.cmp(&right.resource));
    Ok(ImportedSecretCatalog {
        catalog_path: path.to_path_buf(),
        imports: catalog.imports,
    })
}

pub fn list_local_secret_catalog_at(path: &Path) -> Result<LocalSecretCatalog, SecretImportError> {
    let mut catalog = load_secret_catalog_file_optional(path)?;
    let literal_metadata = build_literal_metadata_map(&catalog.literal_entries);
    let mut literals = catalog
        .secrets
        .into_iter()
        .chain(catalog.values)
        .map(|(resource, value)| {
            let metadata = literal_metadata.get(&resource);
            LocalSecretLiteralEntry {
                resource,
                value,
                display_name: metadata.and_then(|entry| entry.display_name.clone()),
                description: metadata.and_then(|entry| entry.description.clone()),
                tags: metadata.map(|entry| entry.tags.clone()).unwrap_or_default(),
                metadata: metadata
                    .map(|entry| entry.metadata.clone())
                    .unwrap_or_default(),
            }
        })
        .collect::<Vec<_>>();
    literals.sort_by(|left, right| left.resource.cmp(&right.resource));
    catalog
        .imports
        .sort_by(|left, right| left.resource.cmp(&right.resource));
    Ok(LocalSecretCatalog {
        catalog_path: path.to_path_buf(),
        literals,
        imports: catalog.imports,
    })
}

pub fn get_local_secret_resource_description_at(
    path: &Path,
    resource: &str,
) -> Result<Option<String>, SecretImportError> {
    let resource = resource.trim();
    if resource.is_empty() {
        return Err(SecretImportError::MissingResource);
    }

    let catalog = load_secret_catalog_file_optional(path)?;
    if let Some(reference) = catalog
        .imports
        .iter()
        .find(|entry| entry.resource == resource)
    {
        return Ok(reference.description.clone());
    }

    let literal_metadata = build_literal_metadata_map(&catalog.literal_entries);
    Ok(literal_metadata
        .get(resource)
        .and_then(|entry| entry.description.clone()))
}

pub fn update_local_secret_resource_description_at(
    path: &Path,
    resource: &str,
    description: Option<String>,
) -> Result<Option<String>, SecretImportError> {
    let resource = resource.trim();
    if resource.is_empty() {
        return Err(SecretImportError::MissingResource);
    }

    let description = sanitize_optional_text(description);
    let mut catalog = load_secret_catalog_file_optional(path)?;

    if let Some(reference) = catalog
        .imports
        .iter_mut()
        .find(|entry| entry.resource == resource)
    {
        reference.description = description.clone();
        save_secret_catalog_file(path, &catalog)?;
        return Ok(description);
    }

    let literal_exists = catalog.secrets.contains_key(resource)
        || catalog.values.contains_key(resource)
        || catalog
            .literal_entries
            .iter()
            .any(|entry| entry.resource == resource);
    if !literal_exists {
        return Err(SecretImportError::ResourceNotFound {
            resource: resource.to_string(),
        });
    }

    let literal_metadata = build_literal_metadata_map(&catalog.literal_entries);
    let existing = literal_metadata.get(resource);
    upsert_literal_metadata_record(
        &mut catalog.literal_entries,
        LocalSecretLiteralMetadataRecord {
            resource: resource.to_string(),
            display_name: existing.and_then(|entry| entry.display_name.clone()),
            description: description.clone(),
            tags: existing.map(|entry| entry.tags.clone()).unwrap_or_default(),
            metadata: existing
                .map(|entry| entry.metadata.clone())
                .unwrap_or_default(),
        },
    );
    save_secret_catalog_file(path, &catalog)?;

    Ok(description)
}

pub fn upsert_local_secret_literal(
    entry: LocalSecretLiteralUpsert,
) -> Result<LocalSecretLiteralEntry, SecretImportError> {
    upsert_local_secret_literal_at(local_secret_catalog_path().as_path(), entry)
}

pub fn upsert_local_secret_literal_at(
    path: &Path,
    entry: LocalSecretLiteralUpsert,
) -> Result<LocalSecretLiteralEntry, SecretImportError> {
    let LocalSecretLiteralUpsert {
        resource,
        value,
        display_name,
        description,
        tags,
        metadata,
    } = entry;
    let resource = resource.trim();
    if resource.is_empty() {
        return Err(SecretImportError::MissingResource);
    }
    if value.is_empty() {
        return Err(SecretImportError::MissingSecretValue);
    }
    let display_name = sanitize_optional_text(display_name);
    let description = sanitize_optional_text(description);
    let tags = sanitize_tags(tags);
    let metadata = sanitize_metadata(metadata);

    let mut catalog = load_secret_catalog_file_optional(path)?;
    catalog.values.remove(resource);
    catalog.secrets.insert(resource.to_string(), value.clone());
    catalog
        .imports
        .retain(|reference| reference.resource != resource);
    upsert_literal_metadata_record(
        &mut catalog.literal_entries,
        LocalSecretLiteralMetadataRecord {
            resource: resource.to_string(),
            display_name: display_name.clone(),
            description: description.clone(),
            tags: tags.clone(),
            metadata: metadata.clone(),
        },
    );
    save_secret_catalog_file(path, &catalog)?;

    Ok(LocalSecretLiteralEntry {
        resource: resource.to_string(),
        value,
        display_name,
        description,
        tags,
        metadata,
    })
}

pub fn update_imported_secret_reference(
    update: ImportedSecretReferenceUpdate,
) -> Result<ImportedSecretReceipt, SecretImportError> {
    update_imported_secret_reference_at(local_secret_catalog_path().as_path(), update)
}

pub fn refresh_imported_secret_reference(
    resource: &str,
) -> Result<ImportedSecretReceipt, SecretImportError> {
    refresh_imported_secret_reference_at(local_secret_catalog_path().as_path(), resource)
}

pub fn update_imported_secret_snapshots(
    values: &BTreeMap<String, String>,
) -> Result<(), SecretImportError> {
    update_imported_secret_snapshots_at(local_secret_catalog_path().as_path(), values)
}

pub fn resolve_imported_secret_reference(
    reference: &ImportedSecretReference,
) -> Result<String, ValueResolverError> {
    resolve_imported_reference(
        reference,
        &reference.resource,
        &VendorCliPrograms::from_env(),
    )
}

pub fn update_imported_secret_snapshots_at(
    path: &Path,
    values: &BTreeMap<String, String>,
) -> Result<(), SecretImportError> {
    let mut catalog = load_secret_catalog_file_optional(path)?;
    for resource in values.keys() {
        if !catalog
            .imports
            .iter()
            .any(|reference| reference.resource == *resource)
        {
            return Err(SecretImportError::ImportedResourceNotFound {
                resource: resource.clone(),
            });
        }
    }
    let now = Utc::now();
    for reference in &mut catalog.imports {
        if let Some(value) = values.get(&reference.resource) {
            reference.value = Some(value.clone());
            reference.last_verified_at = Some(now);
        }
    }
    save_secret_catalog_file(path, &catalog)
}

pub fn update_imported_secret_reference_at(
    path: &Path,
    update: ImportedSecretReferenceUpdate,
) -> Result<ImportedSecretReceipt, SecretImportError> {
    let mut catalog = load_secret_catalog_file_optional(path)?;
    let reference = catalog
        .imports
        .iter_mut()
        .find(|reference| reference.resource == update.resource)
        .ok_or_else(|| SecretImportError::ImportedResourceNotFound {
            resource: update.resource.clone(),
        })?;

    reference.display_name = update
        .display_name
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| reference.source_locator.default_display_name());
    reference.description = update
        .description
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned);
    reference.tags = sanitize_tags(update.tags);
    reference.metadata = sanitize_metadata(update.metadata);

    let updated = reference.clone();
    save_secret_catalog_file(path, &catalog)?;

    Ok(ImportedSecretReceipt {
        catalog_path: path.to_path_buf(),
        reference: updated,
    })
}

pub fn refresh_imported_secret_reference_at(
    path: &Path,
    resource: &str,
) -> Result<ImportedSecretReceipt, SecretImportError> {
    refresh_imported_secret_reference_at_with_programs(
        path,
        resource,
        &VendorCliPrograms::from_env(),
    )
}

pub fn delete_imported_secret_reference(resource: &str) -> Result<bool, SecretImportError> {
    delete_imported_secret_reference_at(local_secret_catalog_path().as_path(), resource)
}

pub fn delete_local_secret_entry(resource: &str) -> Result<bool, SecretImportError> {
    delete_local_secret_entry_at(local_secret_catalog_path().as_path(), resource)
}

pub fn delete_local_secret_entries(resources: &[String]) -> Result<usize, SecretImportError> {
    delete_local_secret_entries_at(local_secret_catalog_path().as_path(), resources)
}

pub fn rename_local_secret_entry(
    resource: &str,
    next_resource: &str,
) -> Result<bool, SecretImportError> {
    rename_local_secret_entry_at(
        local_secret_catalog_path().as_path(),
        resource,
        next_resource,
    )
}

pub fn rename_local_secret_entry_at(
    path: &Path,
    resource: &str,
    next_resource: &str,
) -> Result<bool, SecretImportError> {
    let resource = resource.trim();
    let next_resource = next_resource.trim();
    if resource.is_empty() || next_resource.is_empty() {
        return Err(SecretImportError::MissingResource);
    }

    let mut catalog = load_secret_catalog_file_optional(path)?;
    let source_exists = catalog.secrets.contains_key(resource)
        || catalog.values.contains_key(resource)
        || catalog
            .imports
            .iter()
            .any(|reference| reference.resource == resource)
        || catalog
            .literal_entries
            .iter()
            .any(|entry| entry.resource == resource);
    if !source_exists {
        return Err(SecretImportError::ResourceNotFound {
            resource: resource.to_string(),
        });
    }
    if resource == next_resource {
        return Ok(false);
    }

    let target_exists = catalog.secrets.contains_key(next_resource)
        || catalog.values.contains_key(next_resource)
        || catalog
            .imports
            .iter()
            .any(|reference| reference.resource == next_resource)
        || catalog
            .literal_entries
            .iter()
            .any(|entry| entry.resource == next_resource);
    if target_exists {
        return Err(SecretImportError::DuplicateResource {
            resource: next_resource.to_string(),
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

    save_secret_catalog_file(path, &catalog)?;
    Ok(true)
}

pub fn delete_imported_secret_reference_at(
    path: &Path,
    resource: &str,
) -> Result<bool, SecretImportError> {
    delete_local_secret_entry_at(path, resource)
}

pub fn delete_local_secret_entry_at(
    path: &Path,
    resource: &str,
) -> Result<bool, SecretImportError> {
    Ok(delete_local_secret_entries_at(path, &[resource.to_string()])? > 0)
}

pub fn delete_local_secret_entries_at(
    path: &Path,
    resources: &[String],
) -> Result<usize, SecretImportError> {
    let resources = resources
        .iter()
        .map(|resource| resource.trim())
        .filter(|resource| !resource.is_empty())
        .collect::<std::collections::BTreeSet<_>>();
    if resources.is_empty() {
        return Ok(0);
    }
    let mut catalog = load_secret_catalog_file_optional(path)?;
    let deleted = resources
        .iter()
        .filter(|resource| {
            catalog.secrets.contains_key(**resource)
                || catalog.values.contains_key(**resource)
                || catalog
                    .imports
                    .iter()
                    .any(|reference| reference.resource == **resource)
                || catalog
                    .literal_entries
                    .iter()
                    .any(|entry| entry.resource == **resource)
        })
        .count();
    if deleted == 0 {
        return Ok(0);
    }
    catalog
        .secrets
        .retain(|resource, _| !resources.contains(resource.as_str()));
    catalog
        .values
        .retain(|resource, _| !resources.contains(resource.as_str()));
    catalog
        .imports
        .retain(|reference| !resources.contains(reference.resource.as_str()));
    catalog
        .literal_entries
        .retain(|entry| !resources.contains(entry.resource.as_str()));
    save_secret_catalog_file(path, &catalog)?;
    Ok(deleted)
}

#[cfg(test)]
fn import_secret_reference_at_with_programs(
    path: &Path,
    spec: SecretImportSpec,
    programs: &VendorCliPrograms,
) -> Result<ImportedSecretReceipt, SecretImportError> {
    let receipt = import_secret_references_at_with_programs(
        path,
        SecretImportBatchSpec {
            resource_template: None,
            imports: vec![spec],
        },
        programs,
    )?;
    receipt
        .receipts
        .into_iter()
        .next()
        .ok_or(SecretImportError::EmptyBatch)
}

#[cfg(test)]
fn refresh_imported_secret_reference_at_with_programs_for_test(
    path: &Path,
    resource: &str,
    programs: &VendorCliPrograms,
) -> Result<ImportedSecretReceipt, SecretImportError> {
    refresh_imported_secret_reference_at_with_programs(path, resource, programs)
}

fn import_secret_references_at_with_programs(
    path: &Path,
    spec: SecretImportBatchSpec,
    programs: &VendorCliPrograms,
) -> Result<ImportedSecretBatchReceipt, SecretImportError> {
    if spec.imports.is_empty() {
        return Err(SecretImportError::EmptyBatch);
    }

    let mut catalog = load_secret_catalog_file_optional(path)?;
    let now = Utc::now();

    let mut references = Vec::with_capacity(spec.imports.len());
    let mut seen_resources = BTreeSet::new();
    for import in spec.imports {
        let reference =
            build_import_reference(import, spec.resource_template.as_deref(), now, programs)?;
        if !seen_resources.insert(reference.resource.clone()) {
            return Err(SecretImportError::DuplicateResource {
                resource: reference.resource.clone(),
            });
        }
        references.push(reference);
    }

    for reference in &references {
        let resource = reference.resource.as_str();
        catalog.secrets.remove(resource);
        catalog.values.remove(resource);
        catalog
            .literal_entries
            .retain(|entry| entry.resource != resource);
        if let Some(existing) = catalog
            .imports
            .iter_mut()
            .find(|existing| existing.resource == resource)
        {
            *existing = reference.clone();
        } else {
            catalog.imports.push(reference.clone());
        }
    }

    save_secret_catalog_file(path, &catalog)?;

    Ok(ImportedSecretBatchReceipt {
        catalog_path: path.to_path_buf(),
        receipts: references
            .into_iter()
            .map(|reference| ImportedSecretReceipt {
                catalog_path: path.to_path_buf(),
                reference,
            })
            .collect(),
    })
}

fn refresh_imported_secret_reference_at_with_programs(
    path: &Path,
    resource: &str,
    programs: &VendorCliPrograms,
) -> Result<ImportedSecretReceipt, SecretImportError> {
    let resource = resource.trim();
    if resource.is_empty() {
        return Err(SecretImportError::MissingResource);
    }

    let mut catalog = load_secret_catalog_file_optional(path)?;
    let reference = catalog
        .imports
        .iter_mut()
        .find(|reference| reference.resource == resource)
        .ok_or_else(|| SecretImportError::ImportedResourceNotFound {
            resource: resource.to_string(),
        })?;

    let value = resolve_imported_reference(reference, resource, programs).map_err(|source| {
        SecretImportError::VerifySource {
            resource: resource.to_string(),
            source,
        }
    })?;
    let now = Utc::now();
    reference.value = Some(value);
    reference.last_verified_at = Some(now);

    let updated = reference.clone();
    save_secret_catalog_file(path, &catalog)?;

    Ok(ImportedSecretReceipt {
        catalog_path: path.to_path_buf(),
        reference: updated,
    })
}

fn build_import_reference(
    spec: SecretImportSpec,
    resource_template: Option<&str>,
    now: DateTime<Utc>,
    programs: &VendorCliPrograms,
) -> Result<ImportedSecretReference, SecretImportError> {
    let resource = resolve_import_resource(&spec, resource_template)?;
    let display_name = spec
        .display_name
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| spec.source_locator.default_display_name());
    let description = spec
        .description
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned);
    let tags = sanitize_tags(spec.tags);
    let metadata = sanitize_metadata(spec.metadata);

    let mut reference = ImportedSecretReference {
        resource: resource.clone(),
        display_name,
        description,
        tags,
        metadata,
        value: None,
        source_locator: spec.source_locator,
        imported_at: now,
        last_verified_at: None,
    };

    let value =
        resolve_imported_reference(&reference, resource.as_str(), programs).map_err(|source| {
            SecretImportError::VerifySource {
                resource: resource.clone(),
                source,
            }
        })?;
    reference.value = Some(value);
    reference.last_verified_at = Some(now);

    Ok(reference)
}

fn resolve_import_resource(
    spec: &SecretImportSpec,
    resource_template: Option<&str>,
) -> Result<String, SecretImportError> {
    if let Some(template) = resource_template
        .map(str::trim)
        .filter(|template| !template.is_empty())
    {
        return render_generated_resource(template, &spec.source_locator);
    }

    let explicit = spec.resource.trim();
    if !explicit.is_empty() {
        return Ok(explicit.to_string());
    }

    render_generated_resource(
        spec.source_locator.default_resource_template(),
        &spec.source_locator,
    )
}

fn render_generated_resource(
    template: &str,
    source_locator: &SecretSourceLocator,
) -> Result<String, SecretImportError> {
    let mut environment = Environment::new();
    environment.set_undefined_behavior(UndefinedBehavior::Strict);
    let rendered = environment
        .render_str(template, source_locator.resource_template_context())
        .map_err(|error| SecretImportError::InvalidResourceTemplate {
            message: error.to_string(),
        })?;
    normalize_generated_resource(&rendered).ok_or(SecretImportError::InvalidGeneratedResource)
}

fn normalize_generated_resource(value: &str) -> Option<String> {
    let segments = value
        .split('/')
        .filter_map(normalize_generated_resource_segment)
        .collect::<Vec<_>>();
    (!segments.is_empty()).then(|| segments.join("/"))
}

fn normalize_generated_resource_segment(value: &str) -> Option<String> {
    let mut normalized = String::new();
    let mut previous_was_dash = false;

    for character in value.trim().chars() {
        let next = match character {
            'a'..='z' | '0'..='9' | '_' | '.' => Some(character),
            'A'..='Z' => Some(character.to_ascii_lowercase()),
            '-' => Some('-'),
            _ => Some('-'),
        };

        let Some(next) = next else {
            continue;
        };

        if next == '-' {
            if normalized.is_empty() || previous_was_dash {
                continue;
            }
            previous_was_dash = true;
            normalized.push(next);
            continue;
        }

        previous_was_dash = false;
        normalized.push(next);
    }

    let trimmed = normalized
        .trim_matches(|character| matches!(character, '-' | '_' | '.'))
        .to_string();
    (!trimmed.is_empty()).then_some(trimmed)
}

pub(crate) fn sanitize_tags(tags: Vec<String>) -> Vec<String> {
    let mut seen = BTreeSet::new();
    let mut sanitized = Vec::new();

    for tag in tags
        .into_iter()
        .map(|tag| tag.trim().to_string())
        .filter(|tag| !tag.is_empty())
    {
        if seen.insert(tag.clone()) {
            sanitized.push(tag);
        }
    }

    sanitized
}

pub(crate) fn sanitize_optional_text(value: Option<String>) -> Option<String> {
    value
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
}

pub(crate) fn sanitize_metadata(metadata: BTreeMap<String, String>) -> BTreeMap<String, String> {
    metadata
        .into_iter()
        .filter_map(|(key, value)| {
            let sanitized_key = key.trim();
            let sanitized_value = value.trim();
            if sanitized_key.is_empty() || sanitized_value.is_empty() {
                return None;
            }

            Some((sanitized_key.to_string(), sanitized_value.to_string()))
        })
        .collect()
}

fn build_literal_metadata_map(
    entries: &[LocalSecretLiteralMetadataRecord],
) -> BTreeMap<String, LocalSecretLiteralMetadataRecord> {
    let mut map = BTreeMap::new();
    for entry in entries {
        let resource = entry.resource.trim();
        if resource.is_empty() {
            continue;
        }

        map.insert(
            resource.to_string(),
            LocalSecretLiteralMetadataRecord {
                resource: resource.to_string(),
                display_name: sanitize_optional_text(entry.display_name.clone()),
                description: sanitize_optional_text(entry.description.clone()),
                tags: sanitize_tags(entry.tags.clone()),
                metadata: sanitize_metadata(entry.metadata.clone()),
            },
        );
    }
    map
}

fn upsert_literal_metadata_record(
    entries: &mut Vec<LocalSecretLiteralMetadataRecord>,
    entry: LocalSecretLiteralMetadataRecord,
) {
    if let Some(existing) = entries
        .iter_mut()
        .find(|existing| existing.resource == entry.resource)
    {
        *existing = entry;
    } else {
        entries.push(entry);
    }
}

fn bootstrap_secret_catalog(path: &Path) -> Result<bool, ValueResolverError> {
    if path.exists() {
        return Ok(false);
    }

    ensure_catalog_parent_dir(path).map_err(|error| ValueResolverError::CreateCatalog {
        path: path.display().to_string(),
        message: error.to_string(),
    })?;

    fs::write(path, SECRET_CATALOG_BOOTSTRAP_TEMPLATE).map_err(|error| {
        ValueResolverError::CreateCatalog {
            path: path.display().to_string(),
            message: error.to_string(),
        }
    })?;

    Ok(true)
}

fn load_secret_catalog_file(path: &Path) -> Result<SecretCatalogFile, ValueResolverError> {
    let display_path = path.display().to_string();
    let content = fs::read_to_string(path).map_err(|error| {
        if error.kind() == std::io::ErrorKind::NotFound {
            ValueResolverError::CatalogMissing {
                path: display_path.clone(),
            }
        } else {
            ValueResolverError::ReadCatalog {
                path: display_path.clone(),
                message: error.to_string(),
            }
        }
    })?;

    parse_secret_catalog(path, &content)
}

pub(crate) fn load_secret_catalog_file_optional(
    path: &Path,
) -> Result<SecretCatalogFile, SecretImportError> {
    if !path.exists() {
        return Ok(SecretCatalogFile::default());
    }

    let content = fs::read_to_string(path).map_err(|error| SecretImportError::LoadCatalog {
        path: path.display().to_string(),
        message: error.to_string(),
    })?;

    parse_secret_catalog(path, &content).map_err(|error| SecretImportError::LoadCatalog {
        path: path.display().to_string(),
        message: error.to_string(),
    })
}

pub(crate) fn save_secret_catalog_file(
    path: &Path,
    catalog: &SecretCatalogFile,
) -> Result<(), SecretImportError> {
    ensure_catalog_parent_dir(path).map_err(|error| SecretImportError::WriteCatalog {
        path: path.display().to_string(),
        message: error.to_string(),
    })?;
    let content =
        toml::to_string_pretty(catalog).map_err(|error| SecretImportError::SerializeCatalog {
            path: path.display().to_string(),
            message: error.to_string(),
        })?;
    fs::write(path, content).map_err(|error| SecretImportError::WriteCatalog {
        path: path.display().to_string(),
        message: error.to_string(),
    })
}

fn ensure_catalog_parent_dir(path: &Path) -> Result<(), std::io::Error> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    Ok(())
}

fn parse_secret_catalog(
    path: &Path,
    content: &str,
) -> Result<SecretCatalogFile, ValueResolverError> {
    let display_path = path.display().to_string();
    let root: toml::Value =
        toml::from_str(content).map_err(|error| ValueResolverError::ParseCatalog {
            path: display_path.clone(),
            message: error.to_string(),
        })?;

    let table = root.as_table().cloned().unwrap_or_default();
    let mut catalog = SecretCatalogFile::default();

    merge_string_table(&mut catalog.secrets, table.get("secrets"));
    merge_string_table(&mut catalog.secrets, table.get("values"));

    if let Some(literal_entries_value) = table.get("literal_entries") {
        catalog.literal_entries = literal_entries_value.clone().try_into().map_err(|error| {
            ValueResolverError::ParseCatalog {
                path: display_path.clone(),
                message: error.to_string(),
            }
        })?;
    }

    if let Some(imports_value) = table.get("imports") {
        catalog.imports =
            imports_value
                .clone()
                .try_into()
                .map_err(|error| ValueResolverError::ParseCatalog {
                    path: display_path.clone(),
                    message: error.to_string(),
                })?;
    }

    for (key, value) in &table {
        if matches!(
            key.as_str(),
            "secrets" | "values" | "literal_entries" | "imports"
        ) {
            continue;
        }
        if let Some(value) = value.as_str() {
            catalog.secrets.insert(key.clone(), value.to_string());
        }
    }

    Ok(catalog)
}

fn merge_string_table(target: &mut BTreeMap<String, String>, value: Option<&toml::Value>) {
    let Some(table) = value.and_then(toml::Value::as_table) else {
        return;
    };
    for (key, value) in table {
        if let Some(value) = value.as_str() {
            target.insert(key.clone(), value.to_string());
        }
    }
}

fn catalog_entries(catalog: SecretCatalogFile) -> BTreeMap<String, SecretCatalogEntry> {
    let mut entries = BTreeMap::new();

    for (resource, value) in catalog.secrets.into_iter().chain(catalog.values) {
        entries.insert(resource, SecretCatalogEntry::Literal(value));
    }

    for reference in catalog.imports {
        entries.insert(
            reference.resource.clone(),
            SecretCatalogEntry::Imported(Box::new(reference)),
        );
    }

    entries
}

pub(crate) fn resolve_imported_reference_with_default_programs(
    reference: &ImportedSecretReference,
    resource: &str,
) -> Result<String, ValueResolverError> {
    resolve_imported_reference(reference, resource, &VendorCliPrograms::from_env())
}

fn resolve_imported_reference(
    reference: &ImportedSecretReference,
    resource: &str,
    programs: &VendorCliPrograms,
) -> Result<String, ValueResolverError> {
    match &reference.source_locator {
        SecretSourceLocator::KeepassxcCli {
            database,
            entry,
            field,
            unlock_secret_file,
            executable,
            executable_sha256,
        } => resolve_keepassxc_reference(
            executable,
            executable_sha256,
            database,
            unlock_secret_file,
            entry,
            field,
            resource,
        ),
        SecretSourceLocator::OnePasswordCli {
            account,
            account_id,
            vault,
            item,
            field,
            vault_id,
            item_id,
            field_id,
        } => resolve_onepassword_reference(
            programs.onepassword.as_path(),
            OnePasswordCliLocator {
                account,
                account_id: account_id.as_deref(),
                vault,
                item,
                field,
                vault_id: vault_id.as_deref(),
                item_id: item_id.as_deref(),
                field_id: field_id.as_deref(),
            },
            resource,
        ),
        SecretSourceLocator::BitwardenCli {
            item,
            field,
            item_id,
            ..
        } => resolve_bitwarden_reference(
            programs.bitwarden.as_path(),
            BitwardenCliLocator {
                item,
                field,
                item_id: item_id.as_deref(),
            },
            resource,
        ),
        SecretSourceLocator::DotenvFile {
            file_path,
            namespace: _,
            prefix,
            key,
        } => resolve_dotenv_reference(
            DotenvLocator {
                file_path: file_path.as_path(),
                prefix: prefix.as_deref(),
                key,
            },
            resource,
        ),
    }
}

fn resolve_keepassxc_reference(
    executable: &Path,
    expected_sha256: &str,
    database: &Path,
    unlock_secret_file: &Path,
    entry: &str,
    field: &str,
    resource: &str,
) -> Result<String, ValueResolverError> {
    if entry.trim().is_empty() || field.trim().is_empty() || expected_sha256.len() != 64 {
        return Err(ValueResolverError::InvalidSourceLocator {
            provider_kind: KEEPASSXC_CLI_PROVIDER_KIND.to_string(),
            resource: resource.to_string(),
            message: "entry, field, and a SHA-256 engine digest are required".to_string(),
        });
    }
    let engine = fs::read(executable).map_err(|error| ValueResolverError::ToolUnavailable {
        provider_kind: KEEPASSXC_CLI_PROVIDER_KIND.to_string(),
        resource: resource.to_string(),
        message: error.to_string(),
    })?;
    let actual_sha256 = format!("{:x}", Sha256::digest(engine));
    if actual_sha256 != expected_sha256 {
        return Err(ValueResolverError::ToolUnavailable {
            provider_kind: KEEPASSXC_CLI_PROVIDER_KIND.to_string(),
            resource: resource.to_string(),
            message: format!(
                "engine checksum mismatch: expected {expected_sha256}, received {actual_sha256}"
            ),
        });
    }
    let unlock_secret = fs::read_to_string(unlock_secret_file).map_err(|error| {
        ValueResolverError::SourceReadFailed {
            provider_kind: KEEPASSXC_CLI_PROVIDER_KIND.to_string(),
            resource: resource.to_string(),
            message: format!("failed to read vault unlock material: {error}"),
        }
    })?;
    let mut command = Command::new(executable);
    if executable
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("appimage"))
    {
        command.arg("cli");
    }
    let mut child = command
        .args(["show", "--quiet", "--show-protected", "--attributes", field])
        .arg(database)
        .arg(entry)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| ValueResolverError::ToolUnavailable {
            provider_kind: KEEPASSXC_CLI_PROVIDER_KIND.to_string(),
            resource: resource.to_string(),
            message: error.to_string(),
        })?;
    let stdin = child
        .stdin
        .as_mut()
        .ok_or_else(|| ValueResolverError::SourceReadFailed {
            provider_kind: KEEPASSXC_CLI_PROVIDER_KIND.to_string(),
            resource: resource.to_string(),
            message: "KeePassXC stdin was unavailable".to_string(),
        })?;
    stdin
        .write_all(format!("{}\n", unlock_secret.trim_end()).as_bytes())
        .map_err(|error| ValueResolverError::SourceReadFailed {
            provider_kind: KEEPASSXC_CLI_PROVIDER_KIND.to_string(),
            resource: resource.to_string(),
            message: error.to_string(),
        })?;
    let output =
        child
            .wait_with_output()
            .map_err(|error| ValueResolverError::SourceReadFailed {
                provider_kind: KEEPASSXC_CLI_PROVIDER_KIND.to_string(),
                resource: resource.to_string(),
                message: error.to_string(),
            })?;
    if !output.status.success() {
        return Err(ValueResolverError::SourceReadFailed {
            provider_kind: KEEPASSXC_CLI_PROVIDER_KIND.to_string(),
            resource: resource.to_string(),
            message: String::from_utf8_lossy(&output.stderr).trim().to_string(),
        });
    }
    let value = String::from_utf8(output.stdout).map_err(|error| {
        ValueResolverError::SourceParseFailed {
            provider_kind: KEEPASSXC_CLI_PROVIDER_KIND.to_string(),
            resource: resource.to_string(),
            message: error.to_string(),
        }
    })?;
    ensure_non_empty_value(
        KEEPASSXC_CLI_PROVIDER_KIND,
        resource,
        field,
        value.trim_end(),
    )
}

struct OnePasswordCliLocator<'a> {
    account: &'a str,
    account_id: Option<&'a str>,
    vault: &'a str,
    item: &'a str,
    field: &'a str,
    vault_id: Option<&'a str>,
    item_id: Option<&'a str>,
    field_id: Option<&'a str>,
}

fn resolve_onepassword_reference(
    program: &Path,
    locator: OnePasswordCliLocator<'_>,
    resource: &str,
) -> Result<String, ValueResolverError> {
    if locator.account.trim().is_empty()
        || locator.vault.trim().is_empty()
        || locator.item.trim().is_empty()
        || locator.field.trim().is_empty()
    {
        return Err(ValueResolverError::InvalidSourceLocator {
            provider_kind: ONEPASSWORD_CLI_PROVIDER_KIND.to_string(),
            resource: resource.to_string(),
            message: "account, vault, item, and field are required".to_string(),
        });
    }

    let item_selector = locator.item_id.unwrap_or(locator.item);
    let account_selector = locator.account_id.unwrap_or(locator.account);
    let vault_selector = locator.vault_id.unwrap_or(locator.vault);
    let args = vec![
        "item".to_string(),
        "get".to_string(),
        item_selector.to_string(),
        "--vault".to_string(),
        vault_selector.to_string(),
        "--format".to_string(),
        "json".to_string(),
        "--account".to_string(),
        account_selector.to_string(),
    ];
    let stdout =
        run_command_capture_stdout(program, &args, ONEPASSWORD_CLI_PROVIDER_KIND, resource)?;
    let item: Value =
        serde_json::from_slice(&stdout).map_err(|error| ValueResolverError::SourceParseFailed {
            provider_kind: ONEPASSWORD_CLI_PROVIDER_KIND.to_string(),
            resource: resource.to_string(),
            message: error.to_string(),
        })?;
    let available_fields = item
        .get("fields")
        .and_then(Value::as_array)
        .map(|fields| describe_onepassword_fields(fields))
        .unwrap_or_default();

    let notes_selector = locator.field_id.unwrap_or(locator.field);
    debug!(
        resource,
        account = account_selector,
        vault = vault_selector,
        item_selector,
        field = locator.field,
        field_id = locator.field_id,
        notes_selector,
        has_top_level_notes = item
            .get("notesPlain")
            .and_then(|value| value.as_str())
            .map(|value| !value.trim().is_empty())
            .unwrap_or(false),
        available_fields = available_fields.join(", "),
        "resolving 1Password reference"
    );
    if normalize_match(locator.field, "notes")
        || normalize_match(locator.field, "notesplain")
        || normalize_match(notes_selector, "notes")
        || normalize_match(notes_selector, "notesplain")
    {
        if let Some(notes) = item.get("notesPlain").and_then(Value::as_str) {
            return ensure_non_empty_value(
                ONEPASSWORD_CLI_PROVIDER_KIND,
                resource,
                locator.field,
                notes,
            );
        }
    }

    let is_notes_selector = normalize_match(locator.field, "notes")
        || normalize_match(locator.field, "notesplain")
        || normalize_match(notes_selector, "notes")
        || normalize_match(notes_selector, "notesplain");

    let field_entry = item
        .get("fields")
        .and_then(Value::as_array)
        .and_then(|fields| {
            fields.iter().find(|field| {
                let id_matches = field
                    .get("id")
                    .and_then(Value::as_str)
                    .map(|value| normalize_match(locator.field_id.unwrap_or(locator.field), value))
                    .unwrap_or(false);
                let selector_matches = field
                    .get("id")
                    .and_then(Value::as_str)
                    .map(|value| normalize_match(locator.field, value))
                    .unwrap_or(false)
                    || field
                        .get("label")
                        .and_then(Value::as_str)
                        .map(|value| normalize_match(locator.field, value))
                        .unwrap_or(false)
                    || field
                        .get("purpose")
                        .and_then(Value::as_str)
                        .map(|value| normalize_match(locator.field, value))
                        .unwrap_or(false);
                let notes_matches = is_notes_selector
                    && (field
                        .get("id")
                        .and_then(Value::as_str)
                        .map(|value| normalize_match(value, "notesplain"))
                        .unwrap_or(false)
                        || field
                            .get("label")
                            .and_then(Value::as_str)
                            .map(|value| normalize_match(value, "notesplain"))
                            .unwrap_or(false)
                        || field
                            .get("purpose")
                            .and_then(Value::as_str)
                            .map(|value| normalize_match(value, "notes"))
                            .unwrap_or(false));
                id_matches || selector_matches || notes_matches
            })
        });

    let Some(field_entry) = field_entry else {
        warn!(
            resource,
            account = account_selector,
            vault = vault_selector,
            item_selector,
            field = locator.field,
            field_id = locator.field_id,
            notes_selector,
            available_fields = available_fields.join(", "),
            "1Password field lookup failed"
        );
        return Err(ValueResolverError::SourceFieldNotFound {
            provider_kind: ONEPASSWORD_CLI_PROVIDER_KIND.to_string(),
            resource: resource.to_string(),
            field: locator.field.to_string(),
        });
    };

    if let Some(value) = field_entry.get("value").and_then(Value::as_str) {
        return ensure_non_empty_value(
            ONEPASSWORD_CLI_PROVIDER_KIND,
            resource,
            locator.field,
            value,
        );
    }

    if let Some(reference) = field_entry.get("reference").and_then(Value::as_str) {
        let reference_args = vec![
            "read".to_string(),
            reference.to_string(),
            "--account".to_string(),
            account_selector.to_string(),
        ];
        let stdout = run_command_capture_stdout(
            program,
            &reference_args,
            ONEPASSWORD_CLI_PROVIDER_KIND,
            resource,
        )?;
        let value =
            String::from_utf8(stdout).map_err(|error| ValueResolverError::SourceParseFailed {
                provider_kind: ONEPASSWORD_CLI_PROVIDER_KIND.to_string(),
                resource: resource.to_string(),
                message: error.to_string(),
            })?;
        return ensure_non_empty_value(
            ONEPASSWORD_CLI_PROVIDER_KIND,
            resource,
            locator.field,
            value.trim_end_matches('\n'),
        );
    }

    Err(ValueResolverError::SourceFieldNotFound {
        provider_kind: ONEPASSWORD_CLI_PROVIDER_KIND.to_string(),
        resource: resource.to_string(),
        field: locator.field.to_string(),
    })
}

struct BitwardenCliLocator<'a> {
    item: &'a str,
    field: &'a str,
    item_id: Option<&'a str>,
}

fn resolve_bitwarden_reference(
    program: &Path,
    locator: BitwardenCliLocator<'_>,
    resource: &str,
) -> Result<String, ValueResolverError> {
    if locator.item.trim().is_empty() || locator.field.trim().is_empty() {
        return Err(ValueResolverError::InvalidSourceLocator {
            provider_kind: BITWARDEN_CLI_PROVIDER_KIND.to_string(),
            resource: resource.to_string(),
            message: "item and field are required".to_string(),
        });
    }

    let item_selector = locator.item_id.unwrap_or(locator.item);
    let args = vec![
        "get".to_string(),
        "item".to_string(),
        item_selector.to_string(),
    ];
    let stdout = run_command_capture_stdout(program, &args, BITWARDEN_CLI_PROVIDER_KIND, resource)?;
    let item: Value =
        serde_json::from_slice(&stdout).map_err(|error| ValueResolverError::SourceParseFailed {
            provider_kind: BITWARDEN_CLI_PROVIDER_KIND.to_string(),
            resource: resource.to_string(),
            message: error.to_string(),
        })?;

    if normalize_match(locator.field, "notes") {
        if let Some(notes) = item.get("notes").and_then(Value::as_str) {
            return ensure_non_empty_value(
                BITWARDEN_CLI_PROVIDER_KIND,
                resource,
                locator.field,
                notes,
            );
        }
    }

    if normalize_match(locator.field, "username") {
        if let Some(username) = item
            .get("login")
            .and_then(|login| login.get("username"))
            .and_then(Value::as_str)
        {
            return ensure_non_empty_value(
                BITWARDEN_CLI_PROVIDER_KIND,
                resource,
                locator.field,
                username,
            );
        }
    }

    if normalize_match(locator.field, "password") {
        if let Some(password) = item
            .get("login")
            .and_then(|login| login.get("password"))
            .and_then(Value::as_str)
        {
            return ensure_non_empty_value(
                BITWARDEN_CLI_PROVIDER_KIND,
                resource,
                locator.field,
                password,
            );
        }
    }

    let field_entry = item
        .get("fields")
        .and_then(Value::as_array)
        .and_then(|fields| {
            fields.iter().find(|field| {
                field
                    .get("name")
                    .and_then(Value::as_str)
                    .map(|value| normalize_match(locator.field, value))
                    .unwrap_or(false)
            })
        });

    if let Some(value) = field_entry
        .and_then(|field| field.get("value"))
        .and_then(Value::as_str)
    {
        return ensure_non_empty_value(BITWARDEN_CLI_PROVIDER_KIND, resource, locator.field, value);
    }

    Err(ValueResolverError::SourceFieldNotFound {
        provider_kind: BITWARDEN_CLI_PROVIDER_KIND.to_string(),
        resource: resource.to_string(),
        field: locator.field.to_string(),
    })
}

struct DotenvLocator<'a> {
    file_path: &'a Path,
    prefix: Option<&'a str>,
    key: &'a str,
}

fn resolve_dotenv_reference(
    locator: DotenvLocator<'_>,
    resource: &str,
) -> Result<String, ValueResolverError> {
    let file_display = locator.file_path.display().to_string();
    let mut parsed = BTreeMap::new();
    let iter = from_path_iter(locator.file_path).map_err(|error| {
        ValueResolverError::SourceReadFailed {
            provider_kind: DOTENV_FILE_PROVIDER_KIND.to_string(),
            resource: resource.to_string(),
            message: format!("{file_display}: {error}"),
        }
    })?;

    for entry in iter {
        let (key, value) = entry.map_err(|error| ValueResolverError::SourceParseFailed {
            provider_kind: DOTENV_FILE_PROVIDER_KIND.to_string(),
            resource: resource.to_string(),
            message: format!("{file_display}: {error}"),
        })?;
        parsed.insert(key, value);
    }

    for candidate in dotenv_candidate_keys(locator.prefix, locator.key) {
        if let Some(value) = parsed.get(candidate.as_str()) {
            return ensure_non_empty_value(
                DOTENV_FILE_PROVIDER_KIND,
                resource,
                candidate.as_str(),
                value,
            );
        }
    }

    Err(ValueResolverError::SourceFieldNotFound {
        provider_kind: DOTENV_FILE_PROVIDER_KIND.to_string(),
        resource: resource.to_string(),
        field: locator.key.to_string(),
    })
}

fn dotenv_candidate_keys(prefix: Option<&str>, key: &str) -> Vec<String> {
    let key = key.trim();
    let mut candidates = Vec::new();

    if let Some(prefix) = prefix.map(str::trim).filter(|prefix| !prefix.is_empty()) {
        if key.starts_with(prefix) {
            candidates.push(key.to_string());
        } else {
            candidates.push(format!("{prefix}{key}"));
        }
    }

    if candidates.iter().all(|candidate| candidate != key) {
        candidates.push(key.to_string());
    }

    candidates
}

fn ensure_non_empty_value(
    provider_kind: &str,
    resource: &str,
    field: &str,
    value: &str,
) -> Result<String, ValueResolverError> {
    if value.is_empty() {
        return Err(ValueResolverError::SourceFieldEmpty {
            provider_kind: provider_kind.to_string(),
            resource: resource.to_string(),
            field: field.to_string(),
        });
    }

    Ok(value.to_string())
}

fn run_command_capture_stdout(
    program: &Path,
    args: &[String],
    provider_kind: &str,
    resource: &str,
) -> Result<Vec<u8>, ValueResolverError> {
    let output = Command::new(program).args(args).output().map_err(|error| {
        let message = if error.kind() == std::io::ErrorKind::NotFound {
            format!("{} was not found in PATH", program.display())
        } else {
            error.to_string()
        };
        ValueResolverError::ToolUnavailable {
            provider_kind: provider_kind.to_string(),
            resource: resource.to_string(),
            message,
        }
    })?;

    if !output.status.success() {
        return Err(ValueResolverError::SourceReadFailed {
            provider_kind: provider_kind.to_string(),
            resource: resource.to_string(),
            message: compact_process_message(&output),
        });
    }

    Ok(output.stdout)
}

fn compact_process_message(output: &std::process::Output) -> String {
    let stderr = String::from_utf8_lossy(&output.stderr);
    let stderr = stderr
        .lines()
        .find(|line| !line.trim().is_empty())
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(ToOwned::to_owned);

    stderr.unwrap_or_else(|| format!("process exited with status {}", output.status))
}

fn normalize_match(left: &str, right: &str) -> bool {
    left.trim().eq_ignore_ascii_case(right.trim())
}

fn describe_onepassword_fields(fields: &[Value]) -> Vec<String> {
    fields
        .iter()
        .map(|field| {
            let id = field.get("id").and_then(Value::as_str).unwrap_or("-");
            let label = field.get("label").and_then(Value::as_str).unwrap_or("-");
            let purpose = field.get("purpose").and_then(Value::as_str).unwrap_or("-");
            format!("{id}:{label}:{purpose}")
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use std::{collections::BTreeMap, fs, path::Path};

    use tempfile::tempdir;

    use super::{
        default_value_resolver, delete_imported_secret_reference_at,
        delete_local_secret_entries_at, import_secret_reference_at_with_programs,
        import_secret_references_at_with_programs, list_imported_secret_references_at,
        list_local_secret_catalog_at, refresh_imported_secret_reference_at_with_programs_for_test,
        register_secret_references_at, rename_local_secret_entry_at,
        update_imported_secret_reference_at, upsert_local_secret_literal_at,
        ImportedSecretReferenceUpdate, LocalSecretCatalogResolver, LocalSecretLiteralUpsert,
        SecretImportBatchSpec, SecretImportSpec, SecretSourceLocator, ValueResolver,
        ValueResolverError, VendorCliPrograms, BITWARDEN_CLI_PROVIDER_KIND,
        DOTENV_FILE_PROVIDER_KIND, ONEPASSWORD_CLI_PROVIDER_KIND,
    };

    #[test]
    fn resolves_value_from_secrets_table() {
        let temp = tempdir().expect("temp directory should be created");
        let path = temp.path().join("secrets.toml");
        fs::write(&path, "[secrets]\n\"secret/demo\" = \"demo-value\"\n")
            .expect("catalog should be written");

        let resolver = LocalSecretCatalogResolver::load_from_path(path.as_path())
            .expect("resolver should load");

        assert_eq!(
            resolver
                .resolve("secret/demo")
                .expect("value should resolve"),
            "demo-value"
        );
        assert_eq!(resolver.kind(), "local_secret_catalog");
    }

    #[test]
    fn deletes_multiple_local_entries_in_one_catalog_update() {
        let temp = tempdir().expect("temp directory should be created");
        let path = temp.path().join("secrets.toml");
        for resource in ["secret/one", "secret/two", "secret/keep"] {
            upsert_local_secret_literal_at(
                path.as_path(),
                LocalSecretLiteralUpsert {
                    resource: resource.to_string(),
                    value: format!("value-{resource}"),
                    display_name: None,
                    description: None,
                    tags: Vec::new(),
                    metadata: BTreeMap::new(),
                },
            )
            .expect("literal should be stored");
        }

        let deleted = delete_local_secret_entries_at(
            path.as_path(),
            &["secret/one".to_string(), "secret/two".to_string()],
        )
        .expect("batch deletion should succeed");
        let catalog =
            list_local_secret_catalog_at(path.as_path()).expect("catalog should remain readable");

        assert_eq!(deleted, 2);
        assert_eq!(catalog.literals.len(), 1);
        assert_eq!(catalog.literals[0].resource, "secret/keep");
    }

    #[test]
    fn resolves_value_from_root_mapping() {
        let temp = tempdir().expect("temp directory should be created");
        let path = temp.path().join("secrets.toml");
        fs::write(&path, "\"secret/demo\" = \"demo-value\"\n").expect("catalog should be written");

        let resolver = LocalSecretCatalogResolver::load_from_path(path.as_path())
            .expect("resolver should load");

        assert_eq!(
            resolver
                .resolve("secret/demo")
                .expect("value should resolve"),
            "demo-value"
        );
    }

    #[test]
    fn lists_resource_keys_from_catalog() {
        let temp = tempdir().expect("temp directory should be created");
        let path = temp.path().join("secrets.toml");
        fs::write(
            &path,
            "[secrets]\n\"secret/z-token\" = \"z\"\n\"secret/a-token\" = \"a\"\n",
        )
        .expect("catalog should be written");

        let resolver = LocalSecretCatalogResolver::load_from_path(path.as_path())
            .expect("resolver should load");

        assert_eq!(
            resolver.list_resources(),
            vec!["secret/a-token".to_string(), "secret/z-token".to_string()]
        );
    }

    #[test]
    fn registers_multiple_live_references_in_one_catalog_write() {
        let temp = tempdir().expect("temp directory should be created");
        let path = temp.path().join("secrets.toml");
        let database = temp.path().join("vault.kdbx");
        let unlock_secret_file = temp.path().join(".vault.unlock");
        let executable = temp.path().join("keepassxc-cli");

        let specs = ["username", "password"]
            .into_iter()
            .map(|field| SecretImportSpec {
                resource: format!("plankton://field/item/{field}"),
                display_name: Some(format!("Demo:{field}")),
                description: None,
                tags: vec!["demo".to_string()],
                metadata: BTreeMap::from([("field_key".to_string(), field.to_string())]),
                source_locator: SecretSourceLocator::KeepassxcCli {
                    database: database.clone(),
                    entry: format!("item-{field}"),
                    field: "password".to_string(),
                    unlock_secret_file: unlock_secret_file.clone(),
                    executable: executable.clone(),
                    executable_sha256: "digest".to_string(),
                },
            })
            .collect();

        let receipt = register_secret_references_at(&path, specs)
            .expect("metadata-only references should register atomically");
        let catalog = list_imported_secret_references_at(&path)
            .expect("registered references should be readable");

        assert_eq!(receipt.receipts.len(), 2);
        assert_eq!(catalog.imports.len(), 2);
        assert!(catalog.imports.iter().all(|entry| entry.value.is_none()));
    }

    #[test]
    fn dotenv_import_persists_secret_value_snapshot() {
        let temp = tempdir().expect("temp directory should be created");
        let env_path = temp.path().join(".env");
        let catalog_path = temp.path().join("catalog.toml");
        fs::write(&env_path, "APP_SECRET_DEMO=demo-value\n").expect(".env should be written");

        let receipt = import_secret_reference_at_with_programs(
            catalog_path.as_path(),
            SecretImportSpec {
                resource: "secret/demo".to_string(),
                display_name: Some("Demo secret".to_string()),
                description: Some("imported from dotenv".to_string()),
                tags: vec!["demo".to_string()],
                metadata: BTreeMap::from([("owner".to_string(), "alice".to_string())]),
                source_locator: SecretSourceLocator::DotenvFile {
                    file_path: env_path.clone(),
                    namespace: Some("app".to_string()),
                    prefix: Some("APP_".to_string()),
                    key: "SECRET_DEMO".to_string(),
                },
            },
            &VendorCliPrograms::from_env(),
        )
        .expect("dotenv import should succeed");

        let resolver = LocalSecretCatalogResolver::load_from_path(catalog_path.as_path())
            .expect("resolver should load");
        let catalog_content =
            fs::read_to_string(catalog_path.as_path()).expect("catalog should be readable");

        assert_eq!(receipt.reference.provider_kind(), DOTENV_FILE_PROVIDER_KIND);
        assert_eq!(receipt.reference.value.as_deref(), Some("demo-value"));
        assert!(receipt.reference.last_verified_at.is_some());
        assert_eq!(
            receipt.reference.metadata.get("owner").map(String::as_str),
            Some("alice")
        );
        assert_eq!(
            resolver
                .resolve("secret/demo")
                .expect("dotenv value should resolve"),
            "demo-value"
        );
        assert_eq!(resolver.list_resources(), vec!["secret/demo".to_string()]);
        assert!(catalog_content.contains("provider_kind = \"dotenv_file\""));
        assert!(catalog_content.contains("value = \"demo-value\""));
    }

    #[test]
    fn dotenv_import_generates_default_resource_when_resource_is_blank() {
        let temp = tempdir().expect("temp directory should be created");
        let env_path = temp.path().join(".env");
        let catalog_path = temp.path().join("catalog.toml");
        fs::write(&env_path, "APP_SECRET_DEMO=demo-value\n").expect(".env should be written");

        let receipt = import_secret_reference_at_with_programs(
            catalog_path.as_path(),
            SecretImportSpec {
                resource: String::new(),
                display_name: None,
                description: None,
                tags: Vec::new(),
                metadata: BTreeMap::new(),
                source_locator: SecretSourceLocator::DotenvFile {
                    file_path: env_path,
                    namespace: Some("app".to_string()),
                    prefix: Some("APP_".to_string()),
                    key: "SECRET_DEMO".to_string(),
                },
            },
            &VendorCliPrograms::from_env(),
        )
        .expect("dotenv import should succeed");

        assert_eq!(receipt.reference.resource, "secret/app/secret_demo");
    }

    #[test]
    fn batch_import_uses_shared_template_and_persists_secret_values() {
        let temp = tempdir().expect("temp directory should be created");
        let env_path = temp.path().join(".env");
        let catalog_path = temp.path().join("catalog.toml");
        fs::write(&env_path, "APP_ALPHA=alpha-secret\nAPP_BETA=beta-secret\n")
            .expect(".env should be written");

        let receipt = import_secret_references_at_with_programs(
            catalog_path.as_path(),
            SecretImportBatchSpec {
                resource_template: Some("config/{{ source_name }}/{{ key }}".to_string()),
                imports: vec![
                    SecretImportSpec {
                        resource: String::new(),
                        display_name: None,
                        description: None,
                        tags: vec!["alpha".to_string()],
                        metadata: BTreeMap::new(),
                        source_locator: SecretSourceLocator::DotenvFile {
                            file_path: env_path.clone(),
                            namespace: Some("svc".to_string()),
                            prefix: Some("APP_".to_string()),
                            key: "ALPHA".to_string(),
                        },
                    },
                    SecretImportSpec {
                        resource: String::new(),
                        display_name: None,
                        description: None,
                        tags: vec!["beta".to_string()],
                        metadata: BTreeMap::new(),
                        source_locator: SecretSourceLocator::DotenvFile {
                            file_path: env_path,
                            namespace: Some("svc".to_string()),
                            prefix: Some("APP_".to_string()),
                            key: "BETA".to_string(),
                        },
                    },
                ],
            },
            &VendorCliPrograms::from_env(),
        )
        .expect("batch import should succeed");

        let catalog_content =
            fs::read_to_string(catalog_path.as_path()).expect("catalog should be readable");
        let resolver = LocalSecretCatalogResolver::load_from_path(catalog_path.as_path())
            .expect("resolver should load");

        assert_eq!(receipt.receipts.len(), 2);
        assert_eq!(
            receipt
                .receipts
                .iter()
                .map(|item| item.reference.resource.clone())
                .collect::<Vec<_>>(),
            vec![
                "config/svc/alpha".to_string(),
                "config/svc/beta".to_string(),
            ]
        );
        assert_eq!(
            resolver.list_resources(),
            vec![
                "config/svc/alpha".to_string(),
                "config/svc/beta".to_string(),
            ]
        );
        assert!(catalog_content.contains("value = \"alpha-secret\""));
        assert!(catalog_content.contains("value = \"beta-secret\""));
    }

    #[test]
    fn reports_missing_resource_without_leaking_value() {
        let temp = tempdir().expect("temp directory should be created");
        let path = temp.path().join("secrets.toml");
        fs::write(&path, "[secrets]\n\"secret/demo\" = \"super-secret\"\n")
            .expect("catalog should be written");

        let resolver = LocalSecretCatalogResolver::load_from_path(path.as_path())
            .expect("resolver should load");
        let error = resolver
            .resolve("secret/missing")
            .expect_err("missing key should fail");

        assert!(error.to_string().contains("secret/missing"));
        assert!(!error.to_string().contains("super-secret"));
    }

    #[test]
    fn default_value_resolver_honors_env_override() {
        let temp = tempdir().expect("temp directory should be created");
        let path = temp.path().join("catalog.toml");
        fs::write(&path, "[secrets]\n\"secret/demo\" = \"demo-value\"\n")
            .expect("catalog should be written");

        unsafe {
            std::env::set_var("PLANKTON_SECRET_FILE", path.as_os_str());
        }
        let resolver = default_value_resolver().expect("resolver should load");
        unsafe {
            std::env::remove_var("PLANKTON_SECRET_FILE");
        }

        assert_eq!(
            resolver
                .resolve("secret/demo")
                .expect("value should resolve"),
            "demo-value"
        );
    }

    #[test]
    fn default_value_resolver_bootstraps_missing_catalog() {
        let temp = tempdir().expect("temp directory should be created");
        let path = temp.path().join("catalog.toml");

        unsafe {
            std::env::set_var("PLANKTON_SECRET_FILE", path.as_os_str());
        }
        let error = default_value_resolver().expect_err("missing catalog should bootstrap");
        unsafe {
            std::env::remove_var("PLANKTON_SECRET_FILE");
        }

        match error {
            ValueResolverError::CatalogBootstrapRequired {
                path: error_path,
                created,
            } => {
                assert!(created);
                assert_eq!(error_path, path.display().to_string());
            }
            other => panic!("unexpected error: {other}"),
        }

        let content = fs::read_to_string(&path).expect("bootstrap catalog should be created");
        assert!(content.contains("[secrets]"));
        assert!(content.contains("\"secret/demo\" = \"replace-me\""));
    }

    #[test]
    fn loads_imported_references_from_catalog_for_listing() {
        let temp = tempdir().expect("temp directory should be created");
        let env_path = temp.path().join(".env");
        let path = temp.path().join("catalog.toml");
        fs::write(&env_path, "SERVICE_TOKEN=service-value\n").expect(".env should be written");
        fs::write(
            &path,
            format!(
                "[secrets]\n\"secret/literal\" = \"literal-value\"\n\n[[imports]]\nresource = \"secret/imported\"\ndisplay_name = \"Imported\"\nprovider_kind = \"dotenv_file\"\nfile_path = \"{}\"\nkey = \"SERVICE_TOKEN\"\nimported_at = \"2026-04-12T00:00:00Z\"\n",
                env_path.display()
            ),
        )
        .expect("catalog should be written");

        let resolver = LocalSecretCatalogResolver::load_from_path(path.as_path())
            .expect("resolver should load");

        assert_eq!(
            resolver.list_resources(),
            vec!["secret/imported".to_string(), "secret/literal".to_string()]
        );
        assert_eq!(
            resolver
                .resolve("secret/imported")
                .expect("imported value should resolve"),
            "service-value"
        );
    }

    #[test]
    fn local_literal_catalog_preserves_display_metadata() {
        let temp = tempdir().expect("temp directory should be created");
        let catalog_path = temp.path().join("catalog.toml");

        let entry = upsert_local_secret_literal_at(
            catalog_path.as_path(),
            LocalSecretLiteralUpsert {
                resource: "secret/manual/demo".to_string(),
                value: "demo-value".to_string(),
                display_name: Some("Manual Demo".to_string()),
                description: Some("local note".to_string()),
                tags: vec!["prod".to_string(), "db".to_string()],
                metadata: BTreeMap::from([
                    ("owner".to_string(), "alice".to_string()),
                    ("team".to_string(), "platform".to_string()),
                ]),
            },
        )
        .expect("local literal should save");

        assert_eq!(entry.display_name.as_deref(), Some("Manual Demo"));
        assert_eq!(entry.description.as_deref(), Some("local note"));
        assert_eq!(entry.tags, vec!["prod".to_string(), "db".to_string()]);
        assert_eq!(
            entry.metadata.get("owner").map(String::as_str),
            Some("alice")
        );

        let listed = list_local_secret_catalog_at(catalog_path.as_path())
            .expect("catalog should list local literals");
        assert_eq!(listed.literals.len(), 1);
        assert_eq!(listed.literals[0].resource, "secret/manual/demo");
        assert_eq!(
            listed.literals[0].display_name.as_deref(),
            Some("Manual Demo")
        );
        assert_eq!(
            listed.literals[0].description.as_deref(),
            Some("local note")
        );
        assert_eq!(
            listed.literals[0].metadata.get("team").map(String::as_str),
            Some("platform")
        );

        let catalog_content =
            fs::read_to_string(catalog_path.as_path()).expect("catalog should be readable");
        assert!(catalog_content.contains("[[literal_entries]]"));
        assert!(catalog_content.contains("display_name = \"Manual Demo\""));
        assert!(!catalog_content.contains("owner = \"\""));
    }

    #[test]
    fn renames_literal_and_imported_resources_without_changing_values_or_locators() {
        let temp = tempdir().expect("temp directory should be created");
        let env_path = temp.path().join(".env");
        let catalog_path = temp.path().join("catalog.toml");
        fs::write(&env_path, "SERVICE_TOKEN=service-value\n").expect(".env should be written");
        fs::write(
            &catalog_path,
            format!(
                "[secrets]\n\"secret/local/old\" = \"local-value\"\n\n[[literal_entries]]\nresource = \"secret/local/old\"\ndisplay_name = \"Local\"\n\n[[imports]]\nresource = \"secret/imported/old\"\ndisplay_name = \"Imported\"\nprovider_kind = \"dotenv_file\"\nfile_path = \"{}\"\nkey = \"SERVICE_TOKEN\"\nimported_at = \"2026-04-12T00:00:00Z\"\n",
                env_path.display()
            ),
        )
        .expect("catalog should be written");

        assert!(rename_local_secret_entry_at(
            catalog_path.as_path(),
            "secret/local/old",
            "secret/local/new",
        )
        .expect("literal resource should move"));
        assert!(rename_local_secret_entry_at(
            catalog_path.as_path(),
            "secret/imported/old",
            "secret/imported/new",
        )
        .expect("imported resource should move"));

        let resolver = LocalSecretCatalogResolver::load_from_path(catalog_path.as_path())
            .expect("renamed catalog should load");
        assert_eq!(
            resolver
                .resolve("secret/local/new")
                .expect("renamed literal should resolve"),
            "local-value"
        );
        assert_eq!(
            resolver
                .resolve("secret/imported/new")
                .expect("renamed import should keep its locator"),
            "service-value"
        );
        let listed = list_local_secret_catalog_at(catalog_path.as_path())
            .expect("renamed catalog should list");
        assert_eq!(listed.literals[0].resource, "secret/local/new");
        assert_eq!(listed.imports[0].resource, "secret/imported/new");
    }

    #[test]
    fn rejects_resource_rename_collisions_without_modifying_the_catalog() {
        let temp = tempdir().expect("temp directory should be created");
        let catalog_path = temp.path().join("catalog.toml");
        fs::write(
            &catalog_path,
            "[secrets]\n\"secret/old\" = \"old-value\"\n\"secret/existing\" = \"existing-value\"\n",
        )
        .expect("catalog should be written");

        let error =
            rename_local_secret_entry_at(catalog_path.as_path(), "secret/old", "secret/existing")
                .expect_err("an existing target should reject the move");
        assert!(error.to_string().contains("secret/existing"));

        let resolver = LocalSecretCatalogResolver::load_from_path(catalog_path.as_path())
            .expect("unchanged catalog should load");
        assert_eq!(
            resolver
                .resolve("secret/old")
                .expect("old key should remain"),
            "old-value"
        );
        assert_eq!(
            resolver
                .resolve("secret/existing")
                .expect("existing key should remain"),
            "existing-value"
        );
    }

    #[test]
    fn imported_catalog_supports_listing_update_and_delete() {
        let temp = tempdir().expect("temp directory should be created");
        let env_path = temp.path().join(".env");
        let catalog_path = temp.path().join("catalog.toml");
        fs::write(&env_path, "APP_SECRET_DEMO=demo-value\n").expect(".env should be written");

        import_secret_reference_at_with_programs(
            catalog_path.as_path(),
            SecretImportSpec {
                resource: "secret/demo".to_string(),
                display_name: Some("Demo secret".to_string()),
                description: Some("before update".to_string()),
                tags: vec!["prod".to_string()],
                metadata: BTreeMap::from([
                    ("team".to_string(), "backend".to_string()),
                    ("owner".to_string(), "alice".to_string()),
                ]),
                source_locator: SecretSourceLocator::DotenvFile {
                    file_path: env_path,
                    namespace: Some("app".to_string()),
                    prefix: Some("APP_".to_string()),
                    key: "SECRET_DEMO".to_string(),
                },
            },
            &VendorCliPrograms::from_env(),
        )
        .expect("import should succeed");

        let listed = list_imported_secret_references_at(catalog_path.as_path())
            .expect("catalog should list imports");
        assert_eq!(listed.imports.len(), 1);
        assert_eq!(listed.imports[0].resource, "secret/demo");
        assert_eq!(listed.imports[0].value.as_deref(), Some("demo-value"));
        assert_eq!(
            listed.imports[0].metadata.get("team").map(String::as_str),
            Some("backend")
        );

        let updated = update_imported_secret_reference_at(
            catalog_path.as_path(),
            ImportedSecretReferenceUpdate {
                resource: "secret/demo".to_string(),
                display_name: Some("Renamed demo".to_string()),
                description: Some("after update".to_string()),
                tags: vec![
                    "prod".to_string(),
                    "rotated".to_string(),
                    "prod".to_string(),
                ],
                metadata: BTreeMap::from([
                    ("owner".to_string(), "bob".to_string()),
                    ("team".to_string(), "platform".to_string()),
                ]),
            },
        )
        .expect("update should succeed");

        assert_eq!(updated.reference.display_name, "Renamed demo");
        assert_eq!(
            updated.reference.tags,
            vec!["prod".to_string(), "rotated".to_string()]
        );
        assert_eq!(
            updated.reference.metadata.get("owner").map(String::as_str),
            Some("bob")
        );

        let reloaded = list_imported_secret_references_at(catalog_path.as_path())
            .expect("catalog should reload imports");
        assert_eq!(
            reloaded.imports[0].description.as_deref(),
            Some("after update")
        );
        assert_eq!(
            reloaded.imports[0].metadata.get("team").map(String::as_str),
            Some("platform")
        );

        fs::write(
            catalog_path.parent().expect("catalog parent").join(".env"),
            "APP_SECRET_DEMO=rotated-value\n",
        )
        .expect(".env should be updated");
        let refreshed = refresh_imported_secret_reference_at_with_programs_for_test(
            catalog_path.as_path(),
            "secret/demo",
            &VendorCliPrograms::from_env(),
        )
        .expect("refresh should succeed");
        assert_eq!(refreshed.reference.value.as_deref(), Some("rotated-value"));

        assert!(
            delete_imported_secret_reference_at(catalog_path.as_path(), "secret/demo")
                .expect("delete should succeed")
        );
        let after_delete = list_imported_secret_references_at(catalog_path.as_path())
            .expect("catalog should load after delete");
        assert!(after_delete.imports.is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn resolves_1password_reference_via_cli_contract() {
        let temp = tempdir().expect("temp directory should be created");
        let op_path = temp.path().join("op");
        write_executable(
            op_path.as_path(),
            r#"#!/bin/sh
cmd1="$1"
cmd2="$2"
account=""
while [ "$#" -gt 0 ]; do
  if [ "$1" = "--account" ]; then
    account="$2"
    break
  fi
  shift
done

if [ "$account" != "acct-1" ]; then
  echo "expected --account acct-1, got $account" >&2
  exit 1
fi

if [ "$cmd1" = "item" ] && [ "$cmd2" = "get" ]; then
  cat <<'JSON'
{"fields":[{"label":"password","value":"op-secret"}],"notesPlain":"op notes"}
JSON
  exit 0
fi
echo "unexpected op invocation" >&2
exit 1
"#,
        );
        let catalog_path = temp.path().join("catalog.toml");

        let receipt = import_secret_reference_at_with_programs(
            catalog_path.as_path(),
            SecretImportSpec {
                resource: "secret/op-demo".to_string(),
                display_name: None,
                description: None,
                tags: Vec::new(),
                metadata: BTreeMap::new(),
                source_locator: SecretSourceLocator::OnePasswordCli {
                    account: "demo@example.com".to_string(),
                    account_id: Some("acct-1".to_string()),
                    vault: "Engineering".to_string(),
                    item: "API Token".to_string(),
                    field: "password".to_string(),
                    vault_id: None,
                    item_id: None,
                    field_id: None,
                },
            },
            &VendorCliPrograms {
                onepassword: op_path.clone(),
                bitwarden: temp.path().join("bw"),
            },
        )
        .expect("1Password import should succeed");

        assert_eq!(
            receipt.reference.provider_kind(),
            ONEPASSWORD_CLI_PROVIDER_KIND
        );
        let resolver = LocalSecretCatalogResolver::load_from_path(catalog_path.as_path())
            .expect("resolver should load");
        assert_eq!(
            resolver
                .resolve_with_programs(
                    "secret/op-demo",
                    &VendorCliPrograms {
                        onepassword: op_path,
                        bitwarden: temp.path().join("bw"),
                    },
                )
                .expect("1Password value should resolve"),
            "op-secret"
        );
    }

    #[cfg(unix)]
    #[test]
    fn resolves_1password_notesplain_reference_via_cli_contract() {
        let temp = tempdir().expect("temp directory should be created");
        let op_path = temp.path().join("op");
        write_executable(
            op_path.as_path(),
            r#"#!/bin/sh
cmd1="$1"
cmd2="$2"
account=""
while [ "$#" -gt 0 ]; do
  if [ "$1" = "--account" ]; then
    account="$2"
    break
  fi
  shift
done

if [ "$account" != "acct-1" ]; then
  echo "expected --account acct-1, got $account" >&2
  exit 1
fi

if [ "$cmd1" = "item" ] && [ "$cmd2" = "get" ]; then
  cat <<'JSON'
{"fields":[{"id":"notesPlain","label":"notesPlain","reference":"op://vault/item/notesPlain"}],"notesPlain":"op notes"}
JSON
  exit 0
fi
echo "unexpected op invocation" >&2
exit 1
"#,
        );
        let catalog_path = temp.path().join("catalog.toml");

        import_secret_reference_at_with_programs(
            catalog_path.as_path(),
            SecretImportSpec {
                resource: "secret/op-notes".to_string(),
                display_name: None,
                description: None,
                tags: Vec::new(),
                metadata: BTreeMap::new(),
                source_locator: SecretSourceLocator::OnePasswordCli {
                    account: "demo@example.com".to_string(),
                    account_id: Some("acct-1".to_string()),
                    vault: "Engineering".to_string(),
                    item: "API Token".to_string(),
                    field: "notesPlain".to_string(),
                    vault_id: None,
                    item_id: None,
                    field_id: Some("notesPlain".to_string()),
                },
            },
            &VendorCliPrograms {
                onepassword: op_path.clone(),
                bitwarden: temp.path().join("bw"),
            },
        )
        .expect("1Password notes import should succeed");

        let resolver = LocalSecretCatalogResolver::load_from_path(catalog_path.as_path())
            .expect("resolver should load");
        assert_eq!(
            resolver
                .resolve_with_programs(
                    "secret/op-notes",
                    &VendorCliPrograms {
                        onepassword: op_path,
                        bitwarden: temp.path().join("bw"),
                    },
                )
                .expect("1Password notes value should resolve"),
            "op notes"
        );
    }

    #[cfg(unix)]
    #[test]
    fn resolves_1password_reference_only_notes_field_via_cli_contract() {
        let temp = tempdir().expect("temp directory should be created");
        let op_path = temp.path().join("op");
        write_executable(
            op_path.as_path(),
            r#"#!/bin/sh
cmd1="$1"
cmd2="$2"
account=""
while [ "$#" -gt 0 ]; do
  if [ "$1" = "--account" ]; then
    account="$2"
    break
  fi
  shift
done

if [ "$account" != "acct-1" ]; then
  echo "expected --account acct-1, got $account" >&2
  exit 1
fi

if [ "$cmd1" = "item" ] && [ "$cmd2" = "get" ]; then
  cat <<'JSON'
{"fields":[{"id":"notesPlain","label":"notesPlain","purpose":"NOTES","reference":"op://vault/item/notesPlain"}]}
JSON
  exit 0
fi

if [ "$cmd1" = "read" ]; then
  printf 'op notes'
  exit 0
fi

echo "unexpected op invocation" >&2
exit 1
"#,
        );
        let catalog_path = temp.path().join("catalog.toml");

        import_secret_reference_at_with_programs(
            catalog_path.as_path(),
            SecretImportSpec {
                resource: "secret/op-notes-ref".to_string(),
                display_name: None,
                description: None,
                tags: Vec::new(),
                metadata: BTreeMap::new(),
                source_locator: SecretSourceLocator::OnePasswordCli {
                    account: "demo@example.com".to_string(),
                    account_id: Some("acct-1".to_string()),
                    vault: "Engineering".to_string(),
                    item: "API Token".to_string(),
                    field: "notes".to_string(),
                    vault_id: None,
                    item_id: None,
                    field_id: Some("notesPlain".to_string()),
                },
            },
            &VendorCliPrograms {
                onepassword: op_path.clone(),
                bitwarden: temp.path().join("bw"),
            },
        )
        .expect("1Password reference-only notes import should succeed");

        let resolver = LocalSecretCatalogResolver::load_from_path(catalog_path.as_path())
            .expect("resolver should load");
        assert_eq!(
            resolver
                .resolve_with_programs(
                    "secret/op-notes-ref",
                    &VendorCliPrograms {
                        onepassword: op_path,
                        bitwarden: temp.path().join("bw"),
                    },
                )
                .expect("1Password reference-only notes value should resolve"),
            "op notes"
        );
    }

    #[cfg(unix)]
    #[test]
    fn resolves_bitwarden_reference_via_cli_contract() {
        let temp = tempdir().expect("temp directory should be created");
        let bw_path = temp.path().join("bw");
        write_executable(
            bw_path.as_path(),
            r#"#!/bin/sh
if [ "$1" = "get" ] && [ "$2" = "item" ]; then
  cat <<'JSON'
{"login":{"password":"bw-secret","username":"demo-user"},"notes":"bw notes","fields":[{"name":"custom_token","value":"custom-secret"}]}
JSON
  exit 0
fi
echo "unexpected bw invocation" >&2
exit 1
"#,
        );
        let catalog_path = temp.path().join("catalog.toml");

        let receipt = import_secret_reference_at_with_programs(
            catalog_path.as_path(),
            SecretImportSpec {
                resource: "secret/bw-demo".to_string(),
                display_name: None,
                description: None,
                tags: Vec::new(),
                metadata: BTreeMap::new(),
                source_locator: SecretSourceLocator::BitwardenCli {
                    account: "personal".to_string(),
                    organization: None,
                    collection: None,
                    folder: Some("Dev".to_string()),
                    item: "API Token".to_string(),
                    field: "password".to_string(),
                    item_id: None,
                },
            },
            &VendorCliPrograms {
                onepassword: temp.path().join("op"),
                bitwarden: bw_path.clone(),
            },
        )
        .expect("Bitwarden import should succeed");

        assert_eq!(
            receipt.reference.provider_kind(),
            BITWARDEN_CLI_PROVIDER_KIND
        );
        let resolver = LocalSecretCatalogResolver::load_from_path(catalog_path.as_path())
            .expect("resolver should load");
        assert_eq!(
            resolver
                .resolve_with_programs(
                    "secret/bw-demo",
                    &VendorCliPrograms {
                        onepassword: temp.path().join("op"),
                        bitwarden: bw_path,
                    },
                )
                .expect("Bitwarden value should resolve"),
            "bw-secret"
        );
    }

    #[cfg(unix)]
    fn write_executable(path: &Path, content: &str) {
        use std::os::unix::fs::PermissionsExt;

        fs::write(path, content).expect("script should be written");
        let mut permissions = fs::metadata(path)
            .expect("metadata should load")
            .permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(path, permissions).expect("permissions should be updated");
    }
}
