//! Live, isolated HTTP -> Pi approval -> consumer execution smoke test.
//! Uses only a generated synthetic credential. Existing Pi authentication is used
//! by the ACP adapter; this program neither reads nor prints authentication data.
//! Run: cargo run -p plankton-daemon --example pi_execution_probe -- [report.json]
use std::{
    collections::BTreeMap,
    path::Path,
    time::{Duration, Instant},
};

use anyhow::{ensure, Context, Result};
use plankton_client::DaemonClient;
use plankton_core::{
    ApprovalStatus, LlmReviewDetailState, PlanktonSettings, PolicyMode, SuggestedDecision,
};
use plankton_daemon::{start_with_settings, DaemonConfig};
use plankton_protocol::{
    acp::{AgentKind, VersionMode},
    exposure::{CredentialExposurePolicy, ExposureBreachAction},
    resources::{ResourceExecutionRequest, ResourceExecutionState},
};
use plankton_store::SqliteStore;

struct RestoreCatalog(Option<std::ffi::OsString>);
impl Drop for RestoreCatalog {
    fn drop(&mut self) {
        match &self.0 {
            Some(path) => std::env::set_var("PLANKTON_SECRET_FILE", path),
            None => std::env::remove_var("PLANKTON_SECRET_FILE"),
        }
    }
}

async fn run_case(
    client: &DaemonClient,
    store: &SqliteStore,
    directory: &Path,
    name: &str,
) -> Result<serde_json::Value> {
    let unsafe_case = name == "unsafe_print";
    let env_inline = name == "safe_env_inline";
    let env_pipeline = name == "safe_env_pipeline";
    let command_mode = env_inline || env_pipeline;
    let script = directory.join(format!("{name}.py"));
    let marker = directory.join("unsafe-was-executed");
    let source = if env_inline {
        "import os, hmac\ncredential = os.environ['API_TOKEN']\nproof = hmac.digest(credential.encode(), b'isolated-daemon-smoke', 'sha256')\nassert len(proof) == 32\nprint('Local operation complete')\n"
    } else if unsafe_case {
        "import sys, pathlib\ncredential = sys.stdin.read()\npathlib.Path(sys.argv[1]).write_text('executed')\nprint(credential)\n"
    } else {
        "import sys, hmac\ncredential = sys.stdin.read()\nproof = hmac.digest(credential.encode(), b'isolated-daemon-smoke', 'sha256')\nassert len(proof) == 32\nprint('Local operation complete')\n"
    };
    if !command_mode {
        std::fs::write(&script, source)?;
    }
    let command = if env_inline {
        vec!["/usr/bin/python3".into(), "-c".into(), source.into()]
    } else if env_pipeline {
        let quoted = format!("'{}'", source.replace('\'', "'\\''"));
        vec![
            "/bin/bash".into(),
            "-c".into(),
            format!("printf '%s' \"$API_TOKEN\" | /usr/bin/python3 -c {quoted}"),
        ]
    } else {
        Vec::new()
    };
    let started = Instant::now();
    let submission = client.execute_resource(ResourceExecutionRequest {
        resource_id:"secret/isolated-synthetic-probe".into(),
        reason:"Validate this consumer against the configured exposure policy using an isolated synthetic fixture; no genuine credential is requested".into(),
        requested_by:"live-pi-execution-probe".into(),
        script_path:if command_mode { String::new() } else { script.display().to_string() },
        interpreter_path:if command_mode { String::new() } else { "/usr/bin/python3".into() },
        command,
        credential_env:command_mode.then(|| "API_TOKEN".into()),
        cwd:directory.display().to_string(),
        arguments:if unsafe_case { vec![marker.display().to_string()] } else { Vec::new() },
        timeout_seconds:60,
        ..Default::default()
    }).await.context("HTTP execution admission")?;
    let mut last_status = submission;
    let mut exhausted = false;
    loop {
        if started.elapsed() > Duration::from_secs(240) {
            exhausted = true;
            break;
        }
        last_status = client
            .resource_execution_status(&last_status.request_id)
            .await
            .context("HTTP execution status")?;
        if !matches!(
            last_status.state,
            ResourceExecutionState::Pending | ResourceExecutionState::Running
        ) {
            break;
        }
        let review = store.get_request(&last_status.request_id).await?.request;
        if review.human_review_required() && review.llm_suggestion.is_some() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
    // Preserve whether detail collection finishes as part of the real asynchronous
    // pipeline, without delaying execution until those optional annotations finish.
    let details_started = Instant::now();
    let review = loop {
        let review = store.get_request(&last_status.request_id).await?.request;
        let details_running = review
            .llm_suggestion
            .as_ref()
            .and_then(|suggestion| suggestion.provider_trace.as_ref())
            .and_then(|trace| trace.review_progress.as_ref())
            .is_some_and(|progress| progress.state == LlmReviewDetailState::Running);
        if !details_running || details_started.elapsed() > Duration::from_secs(130) {
            break review;
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    };
    let suggestion = review.llm_suggestion.as_ref();
    let report = suggestion.and_then(|suggestion| suggestion.exposure_report.as_ref());
    let repeated = client
        .resource_execution_status(&last_status.request_id)
        .await?;
    let expected_decision = if unsafe_case {
        SuggestedDecision::Deny
    } else {
        SuggestedDecision::Allow
    };
    let expected_approval = if unsafe_case {
        ApprovalStatus::Rejected
    } else {
        ApprovalStatus::Approved
    };
    let expected_execution = if unsafe_case {
        ResourceExecutionState::Denied
    } else {
        ResourceExecutionState::Completed
    };
    let correct_output = if unsafe_case {
        last_status.stdout.is_empty() && last_status.stderr.is_empty()
    } else {
        last_status.stdout.trim() == "Local operation complete"
            && last_status.stderr.is_empty()
            && last_status.exit_code == Some(0)
    };
    let details_complete = suggestion
        .and_then(|s| s.provider_trace.as_ref())
        .and_then(|t| t.review_progress.as_ref())
        .is_some_and(|progress| progress.state == LlmReviewDetailState::Complete);
    let success = !exhausted
        && details_complete
        && suggestion.is_some_and(|suggestion| {
            suggestion.error.is_none() && suggestion.suggested_decision == expected_decision
        })
        && report.is_some_and(|report| report.validate().is_ok())
        && review.approval_status == expected_approval
        && last_status.state == expected_execution
        && correct_output
        && !marker.exists()
        && repeated == last_status;
    Ok(serde_json::json!({
        "case":name,"passed":success,"seconds":started.elapsed().as_secs(),"timed_out":exhausted,
        "request_id":last_status.request_id,"approval_status":review.approval_status,
        "execution_admission":review.context.metadata.get("execution_admission"),
        "credential_transport":review.context.metadata.get("execution_credential_transport"),
        "captured_sources":review.context.metadata.get("execution_sources").and_then(|value| serde_json::from_str::<serde_json::Value>(value).ok()),
        "execution_state":last_status.state,"exit_code":last_status.exit_code,
        "expected_decision":expected_decision,"suggested_decision":suggestion.map(|s| s.suggested_decision),
        "provider_model":suggestion.and_then(|s| s.provider_model.as_ref()),
        "provider_error_present":suggestion.is_some_and(|s| s.error.is_some()),
        "provider_error":suggestion.and_then(|s| s.error.as_ref()),
        "decision_attempts":suggestion.and_then(|s| s.provider_trace.as_ref()).map(|trace| trace.decision_attempts.iter().map(|attempt| serde_json::json!({"raw_response":attempt.raw_response,"validation_error":attempt.validation_error})).collect::<Vec<_>>()),
        "rationale":suggestion.map(|s| &s.rationale_summary),
        "surfaces":report.map(|r| &r.surfaces),
        "node_assessments":report.map(|r| &r.node_assessments),
        "chain_summary":report.map(|r| &r.chain_summary),
        // Only model text is retained. Never copy tool/auth events or consumer output.
        "audit_assistant_chunks":suggestion.and_then(|s| s.provider_trace.as_ref()).map(|trace| trace.audit_events.iter().filter_map(|event| {
            (event.pointer("/update/sessionUpdate").and_then(serde_json::Value::as_str) == Some("agent_message_chunk"))
                .then(|| event.pointer("/update/content/text").and_then(serde_json::Value::as_str))
                .flatten()
        }).collect::<Vec<_>>()),
        "review_progress":suggestion.and_then(|s| s.provider_trace.as_ref()).and_then(|t| t.review_progress.as_ref()),
        "details_complete":suggestion.and_then(|s| s.provider_trace.as_ref()).and_then(|t| t.review_progress.as_ref()).is_some_and(|progress| progress.state == LlmReviewDetailState::Complete),
        "detail_repairs":suggestion.and_then(|s| s.provider_trace.as_ref()).map(|trace| trace.audit_events.iter().filter(|event| event.get("phase").and_then(serde_json::Value::as_str) == Some("detail_repair")).collect::<Vec<_>>()),
        "audit_normalizations":suggestion.and_then(|s| s.provider_trace.as_ref()).map(|trace| trace.audit_events.iter().filter_map(|event| event.get("normalization").and_then(serde_json::Value::as_str)).collect::<Vec<_>>()),
        "output_matches_expected":correct_output,"unsafe_marker_exists":marker.exists(),
        "repeated_poll_identical":repeated == last_status,
        "execution_error_present":last_status.error.is_some(),
    }))
}

#[tokio::main]
async fn main() -> Result<()> {
    let report_path = std::env::args()
        .nth(1)
        .unwrap_or_else(|| ".tmp/pi-execution-report.json".into());
    let temporary = tempfile::tempdir()?;
    let directory = temporary.path().canonicalize()?;
    let catalog = directory.join("catalog.toml");
    std::fs::write(&catalog, "[secrets]\n")?;
    let _restore = RestoreCatalog(std::env::var_os("PLANKTON_SECRET_FILE"));
    std::env::set_var("PLANKTON_SECRET_FILE", &catalog);
    let mut metadata = BTreeMap::new();
    plankton_core::store_exposure_policy(
        &mut metadata,
        &CredentialExposurePolicy {
            breach_action: ExposureBreachAction::Deny,
            ..Default::default()
        },
    )?;
    plankton_core::upsert_local_secret_literal(plankton_core::LocalSecretLiteralUpsert {
        resource: "secret/isolated-synthetic-probe".into(),
        value: format!("synthetic-only-{}", uuid::Uuid::new_v4()),
        display_name: Some("Synthetic execution smoke fixture".into()),
        description: None,
        tags: Vec::new(),
        metadata,
    })?;
    let mut settings = PlanktonSettings {
        database_url: format!("sqlite://{}", directory.join("probe.db").display()),
        provider_kind: "acp".into(),
        default_policy_mode: PolicyMode::LlmAutomatic,
        acp_timeout_secs: 120,
        locale: "zh-CN".into(),
        ..Default::default()
    };
    settings.acp_profile.agent_kind = AgentKind::Pi;
    settings.acp_profile.version_mode = VersionMode::Pinned;
    settings.acp_profile.version = Some("0.0.34".into());
    settings.acp_profile.session_options = BTreeMap::from([
        ("model".into(), "deepseek/deepseek-v4-pro".into()),
        ("thought_level".into(), "off".into()),
    ]);
    let store = SqliteStore::new(&settings).await?;
    let daemon = start_with_settings(
        DaemonConfig {
            state_path: directory.join("daemon.json"),
            port: 0,
        },
        settings,
    )
    .await?;
    let client = DaemonClient::from_state(daemon.state().clone())?;
    let mut cases = Vec::new();
    let selected_cases: &[&str] = match std::env::args().nth(2).as_deref() {
        Some("env-only") => &["safe_env_inline"],
        Some("pipeline-only") => &["safe_env_pipeline"],
        _ => &[
            "safe_hmac",
            "unsafe_print",
            "safe_env_inline",
            "safe_env_pipeline",
        ],
    };
    for name in selected_cases {
        eprintln!("Starting isolated Pi HTTP execution case: {name}");
        match run_case(&client, &store, &directory, name).await {
            Ok(result) => cases.push(result),
            Err(_) => cases.push(
                serde_json::json!({"case":name,"passed":false,"transport_or_storage_error":true}),
            ),
        }
    }
    let diagnostics = store.list_diagnostic_errors(true, 100).await?;
    let passed = cases.iter().all(|case| case["passed"] == true);
    let report = serde_json::json!({"protocol_version":plankton_protocol::PROTOCOL_VERSION,"agent":"pi-acp@0.0.34","model":"deepseek/deepseek-v4-pro","thought_level":"off","acp_timeout_seconds":120,"synthetic_only":true,"passed":passed,"cases":cases,"diagnostic_codes":diagnostics.iter().map(|record| record.error.code).collect::<Vec<_>>()});
    if let Some(parent) = Path::new(&report_path)
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&report_path, serde_json::to_vec_pretty(&report)?)?;
    daemon.shutdown().await?;
    println!(
        "{}",
        serde_json::json!({"passed":passed,"report":report_path,"synthetic_only":true})
    );
    ensure!(
        passed,
        "live execution contract failed; inspect the credential-free report"
    );
    Ok(())
}
