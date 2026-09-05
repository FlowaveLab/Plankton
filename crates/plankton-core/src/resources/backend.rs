use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use super::ResourceDocument;
use crate::passwords::Item;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BackendCapabilities {
    pub search: bool,
    pub read: bool,
    pub create: bool,
    pub update: bool,
    pub move_item: bool,
    pub archive: bool,
    pub delete: bool,
    pub history: bool,
    pub sync: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HumanConfirmation {
    pub operation_id: String,
    pub content_hash: String,
    pub single_use_token: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResourceValue {
    pub resource_id: String,
    pub value: String,
}

#[async_trait]
pub trait CredentialBackend: Send + Sync {
    fn id(&self) -> &str;
    fn capabilities(&self) -> BackendCapabilities;
    async fn index(&self) -> Result<Vec<ResourceDocument>, BackendError>;
    async fn get(&self, resource_id: &str) -> Result<ResourceValue, BackendError>;
    async fn create(
        &self,
        item: &Item,
        confirmation: &HumanConfirmation,
    ) -> Result<String, BackendError>;
}

#[derive(Debug, thiserror::Error)]
pub enum BackendError {
    #[error("backend is disabled")]
    Disabled,
    #[error("backend does not support {0}")]
    Unsupported(&'static str),
    #[error("resource was not found")]
    NotFound,
    #[error("human confirmation is required")]
    ConfirmationRequired,
    #[error("backend command failed: {0}")]
    Command(String),
    #[error("backend returned malformed data: {0}")]
    Malformed(String),
}
