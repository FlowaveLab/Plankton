use std::collections::BTreeMap;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::exposure::CredentialExposurePolicy;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "operation", rename_all = "snake_case", deny_unknown_fields)]
pub enum PasswordChangeOperation {
    UpdateItem {
        item_id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        next_item_id: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        title: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        description: Option<String>,
        #[serde(default)]
        clear_description: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        tags: Option<Vec<String>>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        metadata: Option<BTreeMap<String, String>>,
    },
    RenameResource {
        resource_id: String,
        next_resource_id: String,
    },
    RenameFieldLabel {
        resource_id: String,
        label: String,
    },
    SetItemExposurePolicy {
        item_id: String,
        policy: CredentialExposurePolicy,
    },
    InheritFieldExposurePolicy {
        resource_id: String,
    },
    SetFieldExposurePolicy {
        resource_id: String,
        policy: CredentialExposurePolicy,
    },
    UpdateField {
        resource_id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        label: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        exposure_policy: Option<CredentialExposurePolicy>,
    },
    MoveField {
        resource_id: String,
        target_item_id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        target_title: Option<String>,
    },
    MergeItems {
        source_item_id: String,
        target_item_id: String,
    },
    DeleteField {
        resource_id: String,
    },
    DeleteDuplicateField {
        resource_id: String,
        canonical_resource_id: String,
    },
    RefreshItem {
        item_id: String,
    },
    DeleteItem {
        item_id: String,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PasswordChangeImpact {
    Metadata,
    ExposurePolicy,
    References,
    Locator,
    Refresh,
    Delete,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PasswordChangeDiffEntry {
    pub path: String,
    pub label: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub before: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub after: Option<String>,
    pub impact: PasswordChangeImpact,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PasswordItemDiff {
    pub record_id: String,
    pub item_id: String,
    pub title: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub vaults: Vec<String>,
    pub entries: Vec<PasswordChangeDiffEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PasswordChangeDiff {
    pub items: Vec<PasswordItemDiff>,
    pub changed_items: u32,
    pub changed_fields: u32,
    pub breaking_changes: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PasswordFieldSummary {
    pub resource_id: String,
    pub label: String,
    pub provider_kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vault: Option<String>,
    pub has_value: bool,
    #[serde(default)]
    pub exposure_policy: CredentialExposurePolicy,
    #[serde(default)]
    pub inherits_exposure_policy: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PasswordItemSummary {
    pub record_id: String,
    pub item_id: String,
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub metadata: BTreeMap<String, String>,
    #[serde(default)]
    pub default_exposure_policy: CredentialExposurePolicy,
    pub fields: Vec<PasswordFieldSummary>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PasswordCatalogMetadata {
    pub revision: String,
    pub items: Vec<PasswordItemSummary>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PasswordChangeState {
    PendingConfirmation,
    Confirmed,
    Committing,
    Committed,
    Rejected,
    Conflict,
    Failed,
}

impl PasswordChangeState {
    pub fn is_terminal(self) -> bool {
        matches!(
            self,
            Self::Committed | Self::Rejected | Self::Conflict | Self::Failed
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SubmitPasswordChangeRequest {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub change_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub batch_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    pub requested_by: String,
    pub operation_id: String,
    pub operation: PasswordChangeOperation,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PasswordChangeStatusRequest {
    pub change_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConfirmPasswordChangeRequest {
    pub change_id: String,
    pub confirmed_version: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RejectPasswordChangeRequest {
    pub change_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PasswordChangeStatus {
    pub batch_id: String,
    pub change_id: String,
    pub version: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub confirmed_version: Option<u64>,
    pub state: PasswordChangeState,
    pub reason: String,
    pub requested_by: String,
    pub diff: PasswordChangeDiff,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub successor_change_id: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub confirmed_at: Option<DateTime<Utc>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub committed_at: Option<DateTime<Utc>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SubmitPasswordChangeResponse {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub requested_change_id: Option<String>,
    pub effective_change_id: String,
    pub status: PasswordChangeStatus,
}
