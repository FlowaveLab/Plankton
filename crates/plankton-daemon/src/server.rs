use axum::{
    body::Body,
    extract::{Request, State},
    http::{header, StatusCode},
    middleware::{self, Next},
    response::Response,
    routing::post,
    Router,
};
use plankton_core::passwords::ConfirmationLedger;
use plankton_protocol::daemon::DaemonState;
use plankton_store::SqliteStore;
use std::sync::Arc;
use tokio::sync::Mutex;

use crate::password_changes::PasswordChangeCoordinator;
use crate::{evaluation::EvaluationWorker, routes, runtime_settings::RuntimeSettings};

#[derive(Debug, Clone)]
pub(crate) struct ServerState {
    pub daemon: DaemonState,
    pub confirmations: Arc<Mutex<ConfirmationLedger>>,
    pub settings: RuntimeSettings,
    pub store: SqliteStore,
    pub evaluations: EvaluationWorker,
    pub password_changes: PasswordChangeCoordinator,
}

pub(crate) fn router(
    state: DaemonState,
    confirmations: Arc<Mutex<ConfirmationLedger>>,
    settings: RuntimeSettings,
    store: SqliteStore,
    evaluations: EvaluationWorker,
) -> Router {
    let server_state = ServerState {
        daemon: state.clone(),
        confirmations,
        settings,
        store,
        evaluations,
        password_changes: PasswordChangeCoordinator::default(),
    };
    Router::new()
        .route("/v1/health", post(routes::health))
        .route("/v1/passwords/drafts", post(routes::create_draft))
        .route("/v1/passwords/drafts/status", post(routes::draft_status))
        .route(
            "/v1/passwords/changes",
            post(routes::submit_password_change),
        )
        .route(
            "/v1/passwords/changes/status",
            post(routes::password_change_status),
        )
        .route(
            "/v1/passwords/changes/confirm",
            post(routes::confirm_password_change),
        )
        .route(
            "/v1/passwords/changes/reject",
            post(routes::reject_password_change),
        )
        .route("/v1/resources/search", post(routes::search_resources))
        .route("/v1/resources/access", post(routes::access_resource))
        .route("/v1/resources/access/status", post(routes::access_status))
        .with_state(server_state)
        .layer(middleware::from_fn_with_state(state, authenticate))
}

async fn authenticate(
    State(state): State<DaemonState>,
    request: Request<Body>,
    next: Next,
) -> Result<Response, StatusCode> {
    let expected = format!("Bearer {}", state.bearer_token);
    let authenticated = request
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value == expected);
    if authenticated {
        Ok(next.run(request).await)
    } else {
        Err(StatusCode::UNAUTHORIZED)
    }
}
