use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{error::AiError, PROTOCOL_VERSION};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HealthRequest {}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DaemonHealth {
    Ready,
    Degraded,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HealthResponse {
    pub protocol_version: u16,
    pub health: DaemonHealth,
    pub pid: u32,
    pub started_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RequestEnvelope<T> {
    pub protocol_version: u16,
    pub correlation_id: Uuid,
    pub payload: T,
}

impl<T> RequestEnvelope<T> {
    pub fn new(payload: T) -> Self {
        Self {
            protocol_version: PROTOCOL_VERSION,
            correlation_id: Uuid::new_v4(),
            payload,
        }
    }

    pub fn validate_version(&self) -> Result<(), ProtocolVersionError> {
        if self.protocol_version == PROTOCOL_VERSION {
            Ok(())
        } else {
            Err(ProtocolVersionError {
                expected: PROTOCOL_VERSION,
                received: self.protocol_version,
            })
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum ResponseEnvelope<T> {
    Success {
        protocol_version: u16,
        correlation_id: Uuid,
        data: T,
    },
    Failure {
        protocol_version: u16,
        correlation_id: Uuid,
        error: AiError,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DaemonState {
    pub protocol_version: u16,
    pub endpoint: String,
    pub bearer_token: String,
    pub pid: u32,
    pub started_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("protocol version mismatch: expected {expected}, received {received}")]
pub struct ProtocolVersionError {
    pub expected: u16,
    pub received: u16,
}
