//! Typed client for the local Plankton daemon.

use std::{
    path::{Path, PathBuf},
    time::Duration,
};

use plankton_protocol::{
    daemon::{
        DaemonState, HealthRequest, HealthResponse, ProtocolVersionError, RequestEnvelope,
        ResponseEnvelope,
    },
    error::AiError,
    password_changes::{
        ConfirmPasswordChangeRequest, PasswordChangeStatus, PasswordChangeStatusRequest,
        RejectPasswordChangeRequest, SubmitPasswordChangeRequest, SubmitPasswordChangeResponse,
    },
    passwords::{
        PasswordDraftCreated, PasswordDraftInput, PasswordDraftStatus, PasswordDraftStatusRequest,
    },
    resources::{
        ResourceAccessRequest, ResourceAccessResponse, ResourceAccessStatusRequest,
        ResourceSearchRequest, ResourceSearchResponse,
    },
    PROTOCOL_VERSION,
};

#[derive(Debug, Clone)]
pub struct DaemonClient {
    state: DaemonState,
    http: reqwest::Client,
}

impl DaemonClient {
    pub async fn connect_default() -> Result<Self, ClientError> {
        Self::from_state_file(default_state_path()).await
    }

    pub async fn from_state_file(path: impl AsRef<Path>) -> Result<Self, ClientError> {
        let path = path.as_ref();
        let bytes = tokio::fs::read(path)
            .await
            .map_err(|source| ClientError::ReadState {
                path: path.to_path_buf(),
                source,
            })?;
        let state = serde_json::from_slice(&bytes).map_err(|source| ClientError::ParseState {
            path: path.to_path_buf(),
            source,
        })?;
        Self::from_state(state)
    }

    pub fn from_state(state: DaemonState) -> Result<Self, ClientError> {
        Self::builder(state).build()
    }

    pub fn builder(state: DaemonState) -> DaemonClientBuilder {
        DaemonClientBuilder {
            state,
            timeout: Duration::from_secs(10),
        }
    }

    pub async fn health(&self) -> Result<HealthResponse, ClientError> {
        self.post("/v1/health", HealthRequest {}).await
    }

    pub async fn create_password_draft(
        &self,
        input: PasswordDraftInput,
    ) -> Result<PasswordDraftCreated, ClientError> {
        self.post("/v1/passwords/drafts", input).await
    }

    pub async fn password_draft_status(
        &self,
        draft_id: uuid::Uuid,
    ) -> Result<PasswordDraftStatus, ClientError> {
        self.post(
            "/v1/passwords/drafts/status",
            PasswordDraftStatusRequest { draft_id },
        )
        .await
    }

    pub async fn submit_password_change(
        &self,
        request: SubmitPasswordChangeRequest,
    ) -> Result<SubmitPasswordChangeResponse, ClientError> {
        self.post("/v1/passwords/changes", request).await
    }

    pub async fn password_change_status(
        &self,
        change_id: impl Into<String>,
    ) -> Result<PasswordChangeStatus, ClientError> {
        self.post(
            "/v1/passwords/changes/status",
            PasswordChangeStatusRequest {
                change_id: change_id.into(),
            },
        )
        .await
    }

    pub async fn confirm_password_change(
        &self,
        request: ConfirmPasswordChangeRequest,
    ) -> Result<PasswordChangeStatus, ClientError> {
        self.post("/v1/passwords/changes/confirm", request).await
    }

    pub async fn reject_password_change(
        &self,
        request: RejectPasswordChangeRequest,
    ) -> Result<PasswordChangeStatus, ClientError> {
        self.post("/v1/passwords/changes/reject", request).await
    }

    pub async fn search_resources(
        &self,
        request: ResourceSearchRequest,
    ) -> Result<ResourceSearchResponse, ClientError> {
        self.post("/v1/resources/search", request).await
    }

    pub async fn request_resource_access(
        &self,
        request: ResourceAccessRequest,
    ) -> Result<ResourceAccessResponse, ClientError> {
        self.post("/v1/resources/access", request).await
    }

    pub async fn resource_access_status(
        &self,
        request_id: impl Into<String>,
    ) -> Result<ResourceAccessResponse, ClientError> {
        self.post(
            "/v1/resources/access/status",
            ResourceAccessStatusRequest {
                request_id: request_id.into(),
            },
        )
        .await
    }

    async fn post<Request, Response>(
        &self,
        path: &str,
        payload: Request,
    ) -> Result<Response, ClientError>
    where
        Request: serde::Serialize,
        Response: serde::de::DeserializeOwned,
    {
        let request = RequestEnvelope::new(payload);
        let correlation_id = request.correlation_id;
        let response = self
            .http
            .post(format!("{}{path}", self.state.endpoint))
            .bearer_auth(&self.state.bearer_token)
            .json(&request)
            .send()
            .await
            .map_err(map_transport)?
            .error_for_status()
            .map_err(map_transport)?
            .bytes()
            .await
            .map_err(ClientError::Transport)?;

        decode_response(&response, correlation_id)
    }
}

#[derive(serde::Deserialize)]
struct ResponseMetadata {
    protocol_version: u16,
    correlation_id: uuid::Uuid,
}

fn decode_response<Response>(
    body: &[u8],
    expected_correlation: uuid::Uuid,
) -> Result<Response, ClientError>
where
    Response: serde::de::DeserializeOwned,
{
    let metadata: ResponseMetadata =
        serde_json::from_slice(body).map_err(ClientError::DecodeResponse)?;
    validate_response(
        metadata.protocol_version,
        metadata.correlation_id,
        expected_correlation,
    )?;

    let response: ResponseEnvelope<Response> =
        serde_json::from_slice(body).map_err(ClientError::DecodeResponse)?;

    match response {
        ResponseEnvelope::Success { data, .. } => Ok(data),
        ResponseEnvelope::Failure { error, .. } => Err(ClientError::Daemon(error)),
    }
}

pub fn default_state_path() -> PathBuf {
    directories::ProjectDirs::from("com", "OpenAquarium", "Plankton")
        .map(|directories| {
            directories
                .runtime_dir()
                .unwrap_or(directories.data_local_dir())
                .to_path_buf()
        })
        .unwrap_or_else(std::env::temp_dir)
        .join("daemon.json")
}

#[derive(Debug)]
pub struct DaemonClientBuilder {
    state: DaemonState,
    timeout: Duration,
}

impl DaemonClientBuilder {
    pub fn timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    pub fn build(self) -> Result<DaemonClient, ClientError> {
        if self.state.protocol_version != PROTOCOL_VERSION {
            return Err(ClientError::ProtocolMismatch(ProtocolVersionError {
                expected: PROTOCOL_VERSION,
                received: self.state.protocol_version,
            }));
        }
        reqwest::Url::parse(&self.state.endpoint)
            .map_err(|error| ClientError::InvalidEndpoint(error.to_string()))?;
        let http = reqwest::Client::builder()
            .timeout(self.timeout)
            .build()
            .map_err(ClientError::Transport)?;
        Ok(DaemonClient {
            state: self.state,
            http,
        })
    }
}

fn validate_response(
    protocol_version: u16,
    received_correlation: uuid::Uuid,
    expected_correlation: uuid::Uuid,
) -> Result<(), ClientError> {
    if protocol_version != PROTOCOL_VERSION {
        return Err(ClientError::ProtocolMismatch(ProtocolVersionError {
            expected: PROTOCOL_VERSION,
            received: protocol_version,
        }));
    }
    if received_correlation != expected_correlation {
        return Err(ClientError::CorrelationMismatch {
            expected: expected_correlation,
            received: received_correlation,
        });
    }
    Ok(())
}

fn map_transport(error: reqwest::Error) -> ClientError {
    if error.is_connect() || error.is_timeout() {
        ClientError::Unavailable(error.to_string())
    } else {
        ClientError::Transport(error)
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ClientError {
    #[error("failed to read daemon state {path}: {source}")]
    ReadState {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("failed to parse daemon state {path}: {source}")]
    ParseState {
        path: PathBuf,
        source: serde_json::Error,
    },
    #[error("invalid daemon endpoint: {0}")]
    InvalidEndpoint(String),
    #[error("daemon is unavailable: {0}")]
    Unavailable(String),
    #[error("daemon transport failed: {0}")]
    Transport(#[source] reqwest::Error),
    #[error("failed to decode daemon response: {0}")]
    DecodeResponse(#[source] serde_json::Error),
    #[error(transparent)]
    ProtocolMismatch(#[from] ProtocolVersionError),
    #[error("daemon response correlation mismatch: expected {expected}, received {received}")]
    CorrelationMismatch {
        expected: uuid::Uuid,
        received: uuid::Uuid,
    },
    #[error("daemon rejected request: {}", .0.user_message)]
    Daemon(AiError),
}

#[cfg(test)]
mod tests {
    use plankton_protocol::resources::ResourceAccessResponse;

    use super::*;

    #[test]
    fn old_resource_response_reports_protocol_mismatch_before_payload_schema_error() {
        let correlation_id = uuid::Uuid::new_v4();
        let body = serde_json::to_vec(&serde_json::json!({
            "status": "success",
            "protocol_version": 2,
            "correlation_id": correlation_id,
            "data": {
                "request_id": "request-v2",
                "resource_id": "secret/legacy",
                "state": "pending",
                "value": null,
                "decision_note": null
            }
        }))
        .expect("response fixture");

        let error = decode_response::<ResourceAccessResponse>(&body, correlation_id)
            .expect_err("legacy response must fail on version before decoding current fields");

        assert!(matches!(
            error,
            ClientError::ProtocolMismatch(ProtocolVersionError {
                expected: PROTOCOL_VERSION,
                received: 2
            })
        ));
    }

    #[test]
    fn old_failure_response_reports_protocol_mismatch_before_error_schema_error() {
        let correlation_id = uuid::Uuid::new_v4();
        let body = serde_json::to_vec(&serde_json::json!({
            "status": "failure",
            "protocol_version": 2,
            "correlation_id": correlation_id,
            "error": {
                "legacy_error": "old schema"
            }
        }))
        .expect("response fixture");

        let error = decode_response::<ResourceAccessResponse>(&body, correlation_id)
            .expect_err("legacy failure must fail on version before decoding its error");

        assert!(matches!(
            error,
            ClientError::ProtocolMismatch(ProtocolVersionError {
                expected: PROTOCOL_VERSION,
                received: 2
            })
        ));
    }
}
