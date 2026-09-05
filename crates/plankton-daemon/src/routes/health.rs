use std::collections::BTreeMap;

use axum::{extract::State, Json};
use chrono::Utc;
use plankton_protocol::{
    daemon::{DaemonHealth, HealthRequest, HealthResponse, RequestEnvelope, ResponseEnvelope},
    error::{ErrorCode, ErrorSeverity, ErrorSource, PlanktonError},
    PROTOCOL_VERSION,
};

use crate::server::ServerState;

pub async fn health(
    State(state): State<ServerState>,
    Json(request): Json<RequestEnvelope<HealthRequest>>,
) -> Json<ResponseEnvelope<HealthResponse>> {
    if let Err(version) = request.validate_version() {
        let error = PlanktonError {
            code: ErrorCode::ProtocolMismatch,
            user_message: version.to_string(),
            internal_message: None,
            public_context: BTreeMap::from([
                ("expected".into(), version.expected.to_string()),
                ("received".into(), version.received.to_string()),
            ]),
            internal_context: BTreeMap::new(),
            severity: ErrorSeverity::Error,
            retryable: false,
            timestamp: Utc::now(),
            correlation_id: request.correlation_id,
            source: ErrorSource::Daemon,
        };
        return Json(ResponseEnvelope::Failure {
            protocol_version: PROTOCOL_VERSION,
            correlation_id: request.correlation_id,
            error: error.ai_safe(),
        });
    }

    Json(ResponseEnvelope::Success {
        protocol_version: PROTOCOL_VERSION,
        correlation_id: request.correlation_id,
        data: HealthResponse {
            protocol_version: PROTOCOL_VERSION,
            health: DaemonHealth::Ready,
            pid: state.daemon.pid,
            started_at: state.daemon.started_at,
        },
    })
}
