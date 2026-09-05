use std::{collections::BTreeMap, sync::Arc, time::Duration};

use axum::{extract::State, routing::post, Json, Router};
use plankton_client::{ClientError, DaemonClient};
use plankton_core::passwords::{FileFormat, ParsedPasswordEntry, PasswordSourceDescriptor};
use plankton_core::{
    ApprovalStatus, EvaluationState, PlanktonSettings, PolicyMode, RequestContext,
};
use plankton_daemon::{start_with_settings, DaemonConfig};
use plankton_protocol::{
    daemon::{HealthRequest, RequestEnvelope, ResponseEnvelope},
    error::ErrorCode,
    passwords::{
        PasswordDestination, PasswordDraftInput, PasswordDraftState, SelectedPasswordEntry,
    },
    resources::{ResourceAccessRequest, ResourceAccessResponse, ResourceAccessState},
    PROTOCOL_VERSION,
};
use plankton_store::SqliteStore;
use tempfile::tempdir;
use tokio::sync::{oneshot, Mutex, Notify};

#[derive(Clone)]
struct BlockedProvider {
    arrived: Arc<Mutex<Option<oneshot::Sender<()>>>>,
    release: Arc<Notify>,
}

fn isolated_settings(temp: &tempfile::TempDir) -> PlanktonSettings {
    PlanktonSettings {
        database_url: format!("sqlite://{}", temp.path().join("plankton.db").display()),
        ..PlanktonSettings::default()
    }
}

async fn blocked_openai_completion(
    State(provider): State<BlockedProvider>,
) -> Json<serde_json::Value> {
    if let Some(arrived) = provider.arrived.lock().await.take() {
        let _ = arrived.send(());
    }
    provider.release.notified().await;
    Json(serde_json::json!({
        "id": "resp_blocked",
        "object": "chat.completion",
        "created": 1,
        "model": "blocked-model",
        "choices": [{
            "index": 0,
            "finish_reason": "stop",
            "message": {
                "role": "assistant",
                "content": "{\"suggested_decision\":\"allow\",\"rationale_summary\":\"test response\",\"risk_score\":10}"
            }
        }]
    }))
}

#[tokio::test]
async fn daemon_authenticates_and_preserves_correlation() {
    let temp = tempdir().expect("temp dir");
    let running = start_with_settings(
        DaemonConfig {
            state_path: temp.path().join("daemon.json"),
            port: 0,
        },
        isolated_settings(&temp),
    )
    .await
    .expect("daemon starts");
    let client = DaemonClient::from_state(running.state().clone()).expect("client");

    let health = client.health().await.expect("authenticated health");
    assert_eq!(health.protocol_version, PROTOCOL_VERSION);
    assert_eq!(health.pid, std::process::id());

    let unauthenticated = reqwest::Client::new()
        .post(format!("{}/v1/health", running.state().endpoint))
        .json(&RequestEnvelope::new(HealthRequest {}))
        .send()
        .await
        .expect("request");
    assert_eq!(unauthenticated.status(), reqwest::StatusCode::UNAUTHORIZED);

    running.shutdown().await.expect("clean shutdown");
}

#[tokio::test]
async fn daemon_fails_closed_on_protocol_mismatch() {
    let temp = tempdir().expect("temp dir");
    let running = start_with_settings(
        DaemonConfig {
            state_path: temp.path().join("daemon.json"),
            port: 0,
        },
        isolated_settings(&temp),
    )
    .await
    .expect("daemon starts");
    let request = RequestEnvelope {
        protocol_version: 2,
        correlation_id: uuid::Uuid::new_v4(),
        payload: HealthRequest {},
    };

    let response = reqwest::Client::new()
        .post(format!("{}/v1/health", running.state().endpoint))
        .bearer_auth(&running.state().bearer_token)
        .json(&request)
        .send()
        .await
        .expect("request")
        .json::<ResponseEnvelope<plankton_protocol::daemon::HealthResponse>>()
        .await
        .expect("typed response");

    match response {
        ResponseEnvelope::Failure {
            protocol_version,
            correlation_id,
            error,
            ..
        } => {
            assert_eq!(protocol_version, plankton_protocol::PROTOCOL_VERSION);
            assert_eq!(correlation_id, request.correlation_id);
            assert_eq!(error.code, ErrorCode::ProtocolMismatch);
        }
        ResponseEnvelope::Success { .. } => panic!("mismatched protocol must fail"),
    }

    running.shutdown().await.expect("clean shutdown");
}

#[tokio::test]
async fn client_reports_daemon_absence_without_retrying_forever() {
    let state = plankton_protocol::daemon::DaemonState {
        protocol_version: PROTOCOL_VERSION,
        endpoint: "http://127.0.0.1:9".into(),
        bearer_token: "unreachable".into(),
        pid: 0,
        started_at: chrono::Utc::now(),
    };
    let client = DaemonClient::builder(state)
        .timeout(Duration::from_millis(250))
        .build()
        .expect("client");

    assert!(matches!(
        client.health().await,
        Err(ClientError::Unavailable(_))
    ));
}

#[tokio::test]
async fn daemon_response_exposes_human_review_required() {
    let temp = tempdir().expect("temp dir");
    let running = start_with_settings(
        DaemonConfig {
            state_path: temp.path().join("daemon.json"),
            port: 0,
        },
        isolated_settings(&temp),
    )
    .await
    .expect("daemon starts");
    let client = DaemonClient::from_state(running.state().clone()).expect("client");

    let response = client
        .request_resource_access(ResourceAccessRequest {
            resource_id: "secret/manual-review-contract".into(),
            reason: "verify derived review transport".into(),
            requested_by: "integration-test".into(),
            script_path: None,
            call_chain_details: Vec::new(),
            call_chain: Vec::new(),
            metadata: BTreeMap::new(),
        })
        .await
        .expect("manual access response");

    assert_eq!(response.state, ResourceAccessState::Pending);
    assert!(response.human_review_required);

    running.shutdown().await.expect("shutdown");
}

#[tokio::test]
async fn client_submits_password_file_as_human_confirmation_draft() {
    let temp = tempdir().expect("temp dir");
    let source = temp.path().join(".env");
    std::fs::write(&source, "TOKEN=secret\nUNSELECTED=nope\n").expect("fixture");
    let running = start_with_settings(
        DaemonConfig {
            state_path: temp.path().join("daemon.json"),
            port: 0,
        },
        isolated_settings(&temp),
    )
    .await
    .expect("daemon starts");
    let client = DaemonClient::from_state(running.state().clone()).expect("client");

    let descriptor = PasswordSourceDescriptor::File {
        path: source.clone(),
        format: FileFormat::Auto,
        keys: vec!["TOKEN".into()],
    };
    let parsed = plankton_core::passwords::parse_password_source_descriptor(descriptor.clone())
        .expect("client parses selected file");
    std::fs::remove_file(&source).expect("daemon must not need the client file");
    let draft = client
        .create_password_draft(PasswordDraftInput {
            descriptor,
            entries: parsed
                .entries
                .into_iter()
                .map(|entry| SelectedPasswordEntry {
                    key: entry.key,
                    value: entry.value,
                })
                .collect(),
            suggested_item_title: Some("Selected tokens".into()),
            suggested_destination: None,
            suggested_layout: None,
        })
        .await
        .expect("draft");
    assert_eq!(draft.keys, vec!["TOKEN"]);

    running.shutdown().await.expect("shutdown");
}

#[tokio::test]
async fn daemon_uses_client_supplied_environment_entry_when_its_environment_lacks_the_name() {
    let temp = tempdir().expect("temp dir");
    let sentinel = format!("PLANKTON_CLIENT_ONLY_{}", uuid::Uuid::new_v4().simple());
    assert!(
        std::env::var_os(&sentinel).is_none(),
        "the daemon process must not inherit the client-only sentinel"
    );
    let running = start_with_settings(
        DaemonConfig {
            state_path: temp.path().join("daemon.json"),
            port: 0,
        },
        isolated_settings(&temp),
    )
    .await
    .expect("daemon starts");
    let client = DaemonClient::from_state(running.state().clone()).expect("client");
    let draft = client
        .create_password_draft(PasswordDraftInput {
            descriptor: PasswordSourceDescriptor::Environment {
                names: vec![sentinel.clone()],
            },
            entries: vec![SelectedPasswordEntry {
                key: sentinel.clone(),
                value: "client-only-value".into(),
            }],
            suggested_item_title: None,
            suggested_destination: None,
            suggested_layout: None,
        })
        .await
        .expect("daemon accepts explicitly supplied environment entry");

    assert_eq!(draft.keys, vec![sentinel]);
    running.shutdown().await.expect("shutdown");
}

#[tokio::test]
async fn onepassword_import_stays_pending_and_never_returns_values_to_the_cli() {
    let temp = tempdir().unwrap();
    let state_path = temp.path().join("daemon.json");
    let running = start_with_settings(
        DaemonConfig {
            state_path: state_path.clone(),
            port: 0,
        },
        isolated_settings(&temp),
    )
    .await
    .unwrap();
    let client = DaemonClient::from_state(running.state().clone()).unwrap();
    let draft = client
        .create_password_draft(PasswordDraftInput {
            descriptor: PasswordSourceDescriptor::OnePassword {
                account: Some("team".into()),
                fields: vec![plankton_protocol::passwords::OnePasswordFieldReference {
                    key: "TOKEN".into(),
                    reference: "op://Work/Service/password".into(),
                }],
            },
            entries: vec![SelectedPasswordEntry {
                key: "TOKEN".into(),
                value: "OP_IMPORT_SECRET_SENTINEL".into(),
            }],
            suggested_item_title: Some("Service".into()),
            suggested_destination: None,
            suggested_layout: None,
        })
        .await
        .unwrap();
    assert_eq!(draft.keys, vec!["TOKEN"]);
    assert!(!serde_json::to_string(&draft)
        .unwrap()
        .contains("OP_IMPORT_SECRET_SENTINEL"));
    let status = client.password_draft_status(draft.draft_id).await.unwrap();
    assert_eq!(status.state, PasswordDraftState::PendingHumanInput);
    assert!(status.resource_ids.is_empty());
    assert!(!serde_json::to_string(&status)
        .unwrap()
        .contains("OP_IMPORT_SECRET_SENTINEL"));
    assert!(!std::fs::read_to_string(state_path)
        .unwrap()
        .contains("OP_IMPORT_SECRET_SENTINEL"));
    let controller = running.password_drafts();
    let preview = controller.preview(draft.draft_id).await.unwrap();
    assert_eq!(preview.entries[0].value, "OP_IMPORT_SECRET_SENTINEL");
    assert!(matches!(
        preview.descriptor,
        PasswordSourceDescriptor::OnePassword { .. }
    ));
    running.shutdown().await.unwrap();
}

#[tokio::test]
async fn manual_batch_draft_status_reports_resources_only_after_human_save() {
    let temp = tempdir().expect("temp dir");
    let running = start_with_settings(
        DaemonConfig {
            state_path: temp.path().join("daemon.json"),
            port: 0,
        },
        isolated_settings(&temp),
    )
    .await
    .expect("daemon starts");
    let client = DaemonClient::from_state(running.state().clone()).expect("client");
    let draft = client
        .create_password_draft(PasswordDraftInput {
            descriptor: PasswordSourceDescriptor::Manual {
                keys: vec!["CLIENT_ID".into(), "CLIENT_SECRET".into()],
            },
            entries: Vec::new(),
            suggested_item_title: Some("Example credentials".into()),
            suggested_destination: None,
            suggested_layout: None,
        })
        .await
        .expect("manual draft");
    assert_eq!(draft.keys, vec!["CLIENT_ID", "CLIENT_SECRET"]);
    assert_eq!(
        client
            .password_draft_status(draft.draft_id)
            .await
            .expect("pending status")
            .state,
        PasswordDraftState::PendingHumanInput
    );

    let controller = running.password_drafts();
    let mut source = controller.preview(draft.draft_id).await.expect("preview");
    source.entries = vec![
        ParsedPasswordEntry {
            key: "CLIENT_ID".into(),
            value: "human-id".into(),
        },
        ParsedPasswordEntry {
            key: "CLIENT_SECRET".into(),
            value: "human-secret".into(),
        },
    ];
    controller
        .replace(draft.draft_id, source)
        .await
        .expect("replace with human values");
    controller
        .confirm(
            draft.draft_id,
            PasswordDestination::Plankton {
                vault_id: "default".into(),
            },
        )
        .await
        .expect("confirm");
    controller
        .complete(
            draft.draft_id,
            "plankton:default".into(),
            vec!["plankton://field/draft/client-id".into()],
        )
        .await
        .expect("complete");

    let committed = client
        .password_draft_status(draft.draft_id)
        .await
        .expect("committed status");
    assert_eq!(committed.state, PasswordDraftState::Committed);
    assert_eq!(
        committed.resource_ids,
        vec!["plankton://field/draft/client-id"]
    );
    let encoded = serde_json::to_string(&committed).expect("status serializes");
    assert!(!encoded.contains("human-id"));
    assert!(!encoded.contains("human-secret"));

    running.shutdown().await.expect("shutdown");
}

#[tokio::test]
async fn access_request_returns_pending_before_provider_is_released() {
    let temp = tempdir().expect("temp dir");
    let provider_listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
        .await
        .expect("provider listener");
    let provider_address = provider_listener.local_addr().expect("provider address");
    let (arrived_tx, arrived_rx) = oneshot::channel();
    let release = Arc::new(Notify::new());
    let provider = BlockedProvider {
        arrived: Arc::new(Mutex::new(Some(arrived_tx))),
        release: release.clone(),
    };
    let provider_task = tokio::spawn(async move {
        axum::serve(
            provider_listener,
            Router::new()
                .route("/chat/completions", post(blocked_openai_completion))
                .with_state(provider),
        )
        .await
    });
    let settings = PlanktonSettings {
        database_url: format!("sqlite://{}", temp.path().join("plankton.db").display()),
        default_policy_mode: PolicyMode::Assisted,
        provider_kind: "openai_compatible".into(),
        openai_api_base: format!("http://{provider_address}"),
        openai_api_key: "test-key".into(),
        openai_model: "blocked-model".into(),
        ..PlanktonSettings::default()
    };
    let running = start_with_settings(
        DaemonConfig {
            state_path: temp.path().join("daemon.json"),
            port: 0,
        },
        settings.clone(),
    )
    .await
    .expect("daemon starts");
    let request = RequestEnvelope::new(ResourceAccessRequest {
        resource_id: "secret/blocked-provider".into(),
        reason: "verify durable asynchronous evaluation".into(),
        requested_by: "integration-test".into(),
        script_path: None,
        call_chain_details: Vec::new(),
        call_chain: Vec::new(),
        metadata: BTreeMap::new(),
    });
    let endpoint = running.state().endpoint.clone();
    let token = running.state().bearer_token.clone();
    let mut access_task = tokio::spawn(async move {
        reqwest::Client::new()
            .post(format!("{endpoint}/v1/resources/access"))
            .bearer_auth(token)
            .json(&request)
            .send()
            .await
            .expect("access response")
            .json::<ResponseEnvelope<ResourceAccessResponse>>()
            .await
            .expect("typed access response")
    });

    let response = tokio::time::timeout(Duration::from_secs(10), &mut access_task)
        .await
        .expect("initial access response blocked on provider evaluation")
        .expect("access task");
    tokio::time::timeout(Duration::from_secs(10), arrived_rx)
        .await
        .expect("provider should be called")
        .expect("provider arrival signal");
    let store = SqliteStore::new(&settings).await.expect("store");
    let initial = store
        .list_running_operations(Some("llm_evaluation"))
        .await
        .expect("running evaluation")
        .into_iter()
        .next()
        .expect("blocked evaluation should be running")
        .heartbeat_at;
    tokio::time::sleep(Duration::from_millis(1_100)).await;
    let later = store
        .list_running_operations(Some("llm_evaluation"))
        .await
        .expect("running evaluation")
        .into_iter()
        .next()
        .expect("blocked evaluation should still be running")
        .heartbeat_at;
    release.notify_waiters();

    running.shutdown().await.expect("shutdown");
    provider_task.abort();

    assert!(
        later > initial,
        "blocked provider evaluation was not heartbeated"
    );
    assert!(matches!(
        response,
        ResponseEnvelope::Success {
            data: ResourceAccessResponse {
                state: ResourceAccessState::Pending,
                human_review_required: false,
                value: None,
                ..
            },
            ..
        }
    ));
}

#[tokio::test]
async fn daemon_start_resumes_queued_work_and_interrupts_abandoned_running_work() {
    let temp = tempdir().expect("temp dir");
    let settings = PlanktonSettings {
        database_url: format!("sqlite://{}", temp.path().join("plankton.db").display()),
        default_policy_mode: PolicyMode::Assisted,
        provider_kind: "mock".into(),
        ..PlanktonSettings::default()
    };
    let store = SqliteStore::new(&settings).await.expect("store");
    let queued = store
        .submit_request(
            &settings,
            RequestContext::new(
                "secret/queued".into(),
                "resume after startup".into(),
                "integration-test".into(),
            ),
            PolicyMode::Assisted,
        )
        .await
        .expect("queued request");
    let abandoned = store
        .submit_request(
            &settings,
            RequestContext::new(
                "secret/abandoned".into(),
                "retain human review after interruption".into(),
                "integration-test".into(),
            ),
            PolicyMode::Assisted,
        )
        .await
        .expect("abandoned request");
    store
        .claim_evaluation(
            &abandoned.id,
            chrono::Utc::now() - chrono::Duration::minutes(5),
        )
        .await
        .expect("claim abandoned request")
        .expect("request should be queued");

    let running = start_with_settings(
        DaemonConfig {
            state_path: temp.path().join("daemon.json"),
            port: 0,
        },
        settings,
    )
    .await
    .expect("daemon starts");

    let queued_result = tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            let request = store
                .get_request(&queued.id)
                .await
                .expect("queued request")
                .request;
            if request.evaluation_state == EvaluationState::Completed {
                break request;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("queued evaluation should resume");
    let interrupted = store
        .get_request(&abandoned.id)
        .await
        .expect("abandoned request")
        .request;

    assert_eq!(queued_result.approval_status, ApprovalStatus::Pending);
    assert!(queued_result.llm_suggestion.is_some());
    assert_eq!(interrupted.evaluation_state, EvaluationState::Interrupted);
    assert_eq!(interrupted.approval_status, ApprovalStatus::Pending);
    assert!(interrupted.llm_suggestion.is_none());

    running.shutdown().await.expect("shutdown");
}
