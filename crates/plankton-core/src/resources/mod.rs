pub mod backend;
pub mod bitwarden_command;
pub mod keepassxc_command;
pub mod onepassword_command;
pub mod search;

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::passwords::Item;

pub use backend::{
    BackendCapabilities, BackendError, CredentialBackend, HumanConfirmation, ResourceValue,
};

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum CommandPolicyError {
    #[error("{backend} command arguments must be UTF-8")]
    NonUtf8Argument { backend: &'static str },
    #[error("{backend} write commands are unavailable to AI clients")]
    WriteCommand { backend: &'static str },
    #[error("{backend} command is unavailable to AI clients: {command}")]
    UnsupportedCommand {
        backend: &'static str,
        command: String,
    },
    #[error("{backend} flag is unavailable to AI clients: {flag}")]
    UnsupportedFlag { backend: &'static str, flag: String },
    #[error("{backend} file-output or session flags are unavailable to AI clients")]
    FileOrSessionFlag { backend: &'static str },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BackendKind {
    Local,
    OnePassword,
    Bitwarden,
    Custom,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResourceDocument {
    pub backend_kind: BackendKind,
    pub backend_binding_id: String,
    pub backend_vault_id: String,
    pub resource_id: String,
    pub display_name: String,
    pub aliases: Vec<String>,
    pub description: Option<String>,
    pub notes: String,
    pub tags: Vec<String>,
    pub field_key: String,
    pub field_label: String,
    pub section: String,
    pub metadata: BTreeMap<String, String>,
}

impl ResourceDocument {
    pub fn from_item(
        backend_kind: BackendKind,
        backend_binding_id: impl Into<String>,
        backend_vault_id: impl Into<String>,
        item: &Item,
    ) -> Self {
        let (section, field) = item
            .sections
            .iter()
            .find_map(|section| section.fields.first().map(|field| (section, field)))
            .expect("validated resource items must contain at least one field");
        Self {
            backend_kind,
            backend_binding_id: backend_binding_id.into(),
            backend_vault_id: backend_vault_id.into(),
            resource_id: item
                .resource_uri(&field.id)
                .expect("selected field must belong to item"),
            display_name: item.title.clone(),
            aliases: item.aliases.clone(),
            description: (!item.notes.is_empty()).then(|| item.notes.clone()),
            notes: item.notes.clone(),
            tags: item.tags.clone(),
            field_key: field.key.clone(),
            field_label: field.label.clone(),
            section: section.title.clone(),
            metadata: item.metadata.clone(),
        }
    }
}
