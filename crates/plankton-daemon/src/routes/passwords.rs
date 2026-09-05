use std::collections::BTreeMap;

use axum::{extract::State, Json};
use chrono::{Duration, Utc};
use plankton_core::passwords::parse_password_draft_input;
use plankton_protocol::{
    daemon::{RequestEnvelope, ResponseEnvelope},
    error::{ErrorCode, ErrorSeverity, ErrorSource, PlanktonError},
    passwords::{
        PasswordDraftCreated, PasswordDraftInput, PasswordDraftStatus, PasswordDraftStatusRequest,
    },
    PROTOCOL_VERSION,
};

use crate::server::ServerState;

pub async fn create_draft(
    State(state): State<ServerState>,
    Json(request): Json<RequestEnvelope<PasswordDraftInput>>,
) -> Json<ResponseEnvelope<PasswordDraftCreated>> {
    if let Err(version) = request.validate_version() {
        return Json(failure(
            request.correlation_id,
            ErrorCode::ProtocolMismatch,
            version.to_string(),
        ));
    }
    let correlation_id = request.correlation_id;
    let source = match parse_password_draft_input(request.payload) {
        Ok(source) => source,
        Err(error) => {
            return Json(failure(
                correlation_id,
                ErrorCode::InvalidRequest,
                error.to_string(),
            ))
        }
    };
    let keys = source
        .entries
        .iter()
        .map(|entry| entry.key.clone())
        .collect();
    let draft_id = state.confirmations.lock().await.create_draft(source);
    Json(ResponseEnvelope::Success {
        protocol_version: PROTOCOL_VERSION,
        correlation_id,
        data: PasswordDraftCreated {
            draft_id,
            keys,
            expires_at: Utc::now() + Duration::minutes(15),
        },
    })
}

pub async fn draft_status(
    State(state): State<ServerState>,
    Json(request): Json<RequestEnvelope<PasswordDraftStatusRequest>>,
) -> Json<ResponseEnvelope<PasswordDraftStatus>> {
    if let Err(version) = request.validate_version() {
        return Json(status_failure(
            request.correlation_id,
            ErrorCode::ProtocolMismatch,
            version.to_string(),
        ));
    }
    let correlation_id = request.correlation_id;
    match state
        .confirmations
        .lock()
        .await
        .draft_status(request.payload.draft_id)
    {
        Ok(status) => Json(ResponseEnvelope::Success {
            protocol_version: PROTOCOL_VERSION,
            correlation_id,
            data: status,
        }),
        Err(error) => Json(status_failure(
            correlation_id,
            ErrorCode::NotFound,
            error.to_string(),
        )),
    }
}

fn failure(
    correlation_id: uuid::Uuid,
    code: ErrorCode,
    message: String,
) -> ResponseEnvelope<PasswordDraftCreated> {
    let error = PlanktonError {
        code,
        user_message: message,
        internal_message: None,
        public_context: BTreeMap::new(),
        internal_context: BTreeMap::new(),
        severity: ErrorSeverity::Error,
        retryable: false,
        timestamp: Utc::now(),
        correlation_id,
        source: ErrorSource::Daemon,
    };
    ResponseEnvelope::Failure {
        protocol_version: PROTOCOL_VERSION,
        correlation_id,
        error: error.ai_safe(),
    }
}

fn status_failure(
    correlation_id: uuid::Uuid,
    code: ErrorCode,
    message: String,
) -> ResponseEnvelope<PasswordDraftStatus> {
    let error = PlanktonError {
        code,
        user_message: message,
        internal_message: None,
        public_context: BTreeMap::new(),
        internal_context: BTreeMap::new(),
        severity: ErrorSeverity::Error,
        retryable: false,
        timestamp: Utc::now(),
        correlation_id,
        source: ErrorSource::Daemon,
    };
    ResponseEnvelope::Failure {
        protocol_version: PROTOCOL_VERSION,
        correlation_id,
        error: error.ai_safe(),
    }
}
