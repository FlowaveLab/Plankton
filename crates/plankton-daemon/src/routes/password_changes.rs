use std::collections::BTreeMap;

use axum::{extract::State, Json};
use chrono::Utc;
use plankton_protocol::{
    daemon::{RequestEnvelope, ResponseEnvelope},
    error::{ErrorCode, ErrorSeverity, ErrorSource, PlanktonError},
    password_changes::{
        ConfirmPasswordChangeRequest, PasswordChangeStatus, PasswordChangeStatusRequest,
        RejectPasswordChangeRequest, SubmitPasswordChangeRequest, SubmitPasswordChangeResponse,
    },
    PROTOCOL_VERSION,
};
use serde::Serialize;

use crate::server::ServerState;

pub async fn submit_password_change(
    State(state): State<ServerState>,
    Json(request): Json<RequestEnvelope<SubmitPasswordChangeRequest>>,
) -> Json<ResponseEnvelope<SubmitPasswordChangeResponse>> {
    let correlation_id = request.correlation_id;
    if let Err(error) = request.validate_version() {
        return Json(failure(
            correlation_id,
            ErrorCode::ProtocolMismatch,
            error.to_string(),
        ));
    }
    match state
        .password_changes
        .submit(&state.store, request.payload)
        .await
    {
        Ok(response) => Json(success(correlation_id, response)),
        Err(error) => Json(failure(
            correlation_id,
            ErrorCode::InvalidRequest,
            error.to_string(),
        )),
    }
}

pub async fn password_change_status(
    State(state): State<ServerState>,
    Json(request): Json<RequestEnvelope<PasswordChangeStatusRequest>>,
) -> Json<ResponseEnvelope<PasswordChangeStatus>> {
    let correlation_id = request.correlation_id;
    if let Err(error) = request.validate_version() {
        return Json(failure(
            correlation_id,
            ErrorCode::ProtocolMismatch,
            error.to_string(),
        ));
    }
    match state
        .store
        .get_password_change(&request.payload.change_id)
        .await
    {
        Ok(change) => Json(success(correlation_id, change.status)),
        Err(error) => Json(failure(
            correlation_id,
            ErrorCode::NotFound,
            error.to_string(),
        )),
    }
}

pub async fn confirm_password_change(
    State(state): State<ServerState>,
    Json(request): Json<RequestEnvelope<ConfirmPasswordChangeRequest>>,
) -> Json<ResponseEnvelope<PasswordChangeStatus>> {
    let correlation_id = request.correlation_id;
    if let Err(error) = request.validate_version() {
        return Json(failure(
            correlation_id,
            ErrorCode::ProtocolMismatch,
            error.to_string(),
        ));
    }
    match state
        .password_changes
        .confirm(&state.store, request.payload)
        .await
    {
        Ok(status) => Json(success(correlation_id, status)),
        Err(error) => Json(failure(
            correlation_id,
            ErrorCode::Conflict,
            error.to_string(),
        )),
    }
}

pub async fn reject_password_change(
    State(state): State<ServerState>,
    Json(request): Json<RequestEnvelope<RejectPasswordChangeRequest>>,
) -> Json<ResponseEnvelope<PasswordChangeStatus>> {
    let correlation_id = request.correlation_id;
    if let Err(error) = request.validate_version() {
        return Json(failure(
            correlation_id,
            ErrorCode::ProtocolMismatch,
            error.to_string(),
        ));
    }
    match state
        .password_changes
        .reject(&state.store, request.payload)
        .await
    {
        Ok(status) => Json(success(correlation_id, status)),
        Err(error) => Json(failure(
            correlation_id,
            ErrorCode::InvalidRequest,
            error.to_string(),
        )),
    }
}

fn success<T>(correlation_id: uuid::Uuid, data: T) -> ResponseEnvelope<T> {
    ResponseEnvelope::Success {
        protocol_version: PROTOCOL_VERSION,
        correlation_id,
        data,
    }
}

fn failure<T: Serialize>(
    correlation_id: uuid::Uuid,
    code: ErrorCode,
    message: String,
) -> ResponseEnvelope<T> {
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
