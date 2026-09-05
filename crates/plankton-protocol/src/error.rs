use std::collections::BTreeMap;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCode {
    InvalidRequest,
    ProtocolMismatch,
    NotFound,
    ApprovalRequired,
    ApprovalDenied,
    BackendUnavailable,
    BackendFailed,
    DaemonUnavailable,
    Timeout,
    Cancelled,
    Conflict,
    StorageFailed,
    ConfigurationFailed,
    Internal,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorSeverity {
    Warning,
    Error,
    Fatal,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ErrorSource {
    Daemon,
    Store,
    Acp,
    Backend { backend_id: String },
    Sync { adapter_id: String },
    Desktop,
    Cli,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlanktonError {
    pub code: ErrorCode,
    pub user_message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub internal_message: Option<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub public_context: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub internal_context: BTreeMap<String, String>,
    pub severity: ErrorSeverity,
    pub retryable: bool,
    pub timestamp: DateTime<Utc>,
    pub correlation_id: Uuid,
    pub source: ErrorSource,
}

impl PlanktonError {
    pub fn ai_safe(&self) -> AiError {
        AiError {
            code: self.code,
            user_message: self.user_message.clone(),
            context: self.public_context.clone(),
            severity: self.severity,
            retryable: self.retryable,
            timestamp: self.timestamp,
            correlation_id: self.correlation_id,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AiError {
    pub code: ErrorCode,
    pub user_message: String,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub context: BTreeMap<String, String>,
    pub severity: ErrorSeverity,
    pub retryable: bool,
    pub timestamp: DateTime<Utc>,
    pub correlation_id: Uuid,
}
