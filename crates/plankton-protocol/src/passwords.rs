use std::{collections::BTreeMap, path::PathBuf};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::exposure::CredentialExposurePolicy;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum PasswordSourceDescriptor {
    Manual {
        keys: Vec<String>,
    },
    Environment {
        names: Vec<String>,
    },
    OnePassword {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        account: Option<String>,
        fields: Vec<OnePasswordFieldReference>,
    },
    File {
        path: PathBuf,
        #[serde(default)]
        format: FileFormat,
        #[serde(default)]
        keys: Vec<String>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OnePasswordFieldReference {
    pub key: String,
    pub reference: String,
}

impl OnePasswordFieldReference {
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.key.trim().is_empty() || self.key.chars().any(char::is_control) {
            return Err("1Password field keys must be non-empty and contain no control characters");
        }
        if self.reference.chars().any(char::is_control) {
            return Err("1Password references must not contain control characters");
        }
        let path = self
            .reference
            .strip_prefix("op://")
            .ok_or("1Password references must start with op://")?;
        let parts = path
            .split('?')
            .next()
            .unwrap_or_default()
            .split('/')
            .collect::<Vec<_>>();
        if !(3..=4).contains(&parts.len()) || parts.iter().any(|part| part.trim().is_empty()) {
            return Err("Use op://VAULT/ITEM/FIELD or op://VAULT/ITEM/SECTION/FIELD");
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PasswordDraftLayoutSuggestion {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub field_labels: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub field_resources: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_exposure_policy: Option<CredentialExposurePolicy>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub field_exposure_policies: BTreeMap<String, CredentialExposurePolicy>,
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PasswordDraftInput {
    pub descriptor: PasswordSourceDescriptor,
    pub entries: Vec<SelectedPasswordEntry>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub suggested_item_title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub suggested_destination: Option<PasswordDestination>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub suggested_layout: Option<PasswordDraftLayoutSuggestion>,
}

impl PasswordDraftInput {
    pub fn descriptor(&self) -> PasswordSourceDescriptor {
        self.descriptor.clone()
    }
}

impl std::fmt::Debug for PasswordDraftInput {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("PasswordDraftInput")
            .field("descriptor", &self.descriptor)
            .field("entries", &self.entries)
            .field("suggested_item_title", &self.suggested_item_title)
            .field("suggested_destination", &self.suggested_destination)
            .field("suggested_layout", &self.suggested_layout)
            .finish()
    }
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SelectedPasswordEntry {
    pub key: String,
    pub value: String,
}

impl std::fmt::Debug for SelectedPasswordEntry {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("SelectedPasswordEntry")
            .field("key", &self.key)
            .finish()
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FileFormat {
    #[default]
    Auto,
    Dotenv,
    Json,
    Yaml,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum PasswordDestination {
    Plankton {
        vault_id: String,
    },
    External {
        binding_id: String,
        vault_id: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PasswordDraftCreated {
    pub draft_id: Uuid,
    pub keys: Vec<String>,
    pub expires_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PasswordDraftState {
    PendingHumanInput,
    Committing,
    Committed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PasswordDraftStatusRequest {
    pub draft_id: Uuid,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PasswordDraftStatus {
    pub draft_id: Uuid,
    pub state: PasswordDraftState,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub destination: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub resource_ids: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::{PasswordDraftInput, PasswordSourceDescriptor, SelectedPasswordEntry};

    #[test]
    fn explicit_environment_input_omits_values_from_debug_and_metadata_descriptor() {
        let input = PasswordDraftInput {
            descriptor: PasswordSourceDescriptor::Environment {
                names: vec!["API_TOKEN".into()],
            },
            entries: vec![SelectedPasswordEntry {
                key: "API_TOKEN".into(),
                value: "client-only-secret".into(),
            }],
            suggested_item_title: Some("Production API".into()),
            suggested_destination: Some(super::PasswordDestination::Plankton {
                vault_id: "work".into(),
            }),
            suggested_layout: None,
        };

        let debug = format!("{input:?}");
        assert!(debug.contains("API_TOKEN"));
        assert!(debug.contains("Production API"));
        assert!(!debug.contains("client-only-secret"));

        assert_eq!(
            input.descriptor(),
            PasswordSourceDescriptor::Environment {
                names: vec!["API_TOKEN".into()],
            }
        );
        let metadata = serde_json::to_string(&input.descriptor()).expect("metadata serializes");
        assert!(!metadata.contains("client-only-secret"));
    }
}
