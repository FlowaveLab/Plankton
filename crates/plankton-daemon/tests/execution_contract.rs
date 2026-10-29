#![cfg(unix)]
use std::{collections::BTreeMap, time::Duration};

use plankton_core::{Decision, PlanktonSettings, PolicyMode};
use plankton_daemon::{start_with_settings, DaemonConfig, RunningDaemon};
use plankton_protocol::{
    daemon::{RequestEnvelope, ResponseEnvelope},
    resources::{ResourceExecutionRequest, ResourceExecutionResponse, ResourceExecutionState},
};
use plankton_store::SqliteStore;

async fn post(daemon: &RunningDaemon, path: &str, payload: serde_json::Value) -> reqwest::Response {
    reqwest::Client::new()
        .post(format!("{}{path}", daemon.state().endpoint))
        .bearer_auth(&daemon.state().bearer_token)
        .json(&RequestEnvelope::new(payload))
        .send()
        .await
        .unwrap()
}

async fn submit(
    daemon: &RunningDaemon,
    request: &ResourceExecutionRequest,
) -> ResourceExecutionResponse {
    match post(
        daemon,
        "/v1/resources/execute",
        serde_json::to_value(request).unwrap(),
    )
    .await
    .json::<ResponseEnvelope<ResourceExecutionResponse>>()
    .await
    .unwrap()
    {
        ResponseEnvelope::Success { data, .. } => data,
        response => panic!("execution admission failed: {response:?}"),
    }
}

async fn poll(daemon: &RunningDaemon, id: &str) -> ResourceExecutionResponse {
    match post(
        daemon,
        "/v1/resources/execute/status",
        serde_json::json!({"request_id":id}),
    )
    .await
    .json::<ResponseEnvelope<ResourceExecutionResponse>>()
    .await
    .unwrap()
    {
        ResponseEnvelope::Success { data, .. } => data,
        response => panic!("execution status failed: {response:?}"),
    }
}

async fn finish(daemon: &RunningDaemon, id: &str) -> ResourceExecutionResponse {
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let response = poll(daemon, id).await;
            if !matches!(
                response.state,
                ResourceExecutionState::Pending | ResourceExecutionState::Running
            ) {
                return response;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .unwrap()
}

struct RestoreSecretFile(Option<std::ffi::OsString>);
impl Drop for RestoreSecretFile {
    fn drop(&mut self) {
        match &self.0 {
            Some(value) => std::env::set_var("PLANKTON_SECRET_FILE", value),
            None => std::env::remove_var("PLANKTON_SECRET_FILE"),
        }
    }
}

#[tokio::test(flavor = "current_thread")]
async fn daemon_admits_bound_consumers_and_never_releases_unbound_credentials() {
    let directory = tempfile::tempdir().unwrap();
    let catalog = directory.path().join("secrets.toml");
    let fixture = "SYNTHETIC_EXECUTION_ONLY_123";
    std::fs::write(
        &catalog,
        format!("[secrets]\n\"secret/test\" = \"{fixture}\"\n"),
    )
    .unwrap();
    let _restore = RestoreSecretFile(std::env::var_os("PLANKTON_SECRET_FILE"));
    std::env::set_var("PLANKTON_SECRET_FILE", &catalog);
    let mut metadata = BTreeMap::new();
    plankton_core::store_exposure_policy(
        &mut metadata,
        &plankton_protocol::exposure::CredentialExposurePolicy::direct(),
    )
    .unwrap();
    plankton_core::upsert_local_secret_literal(plankton_core::LocalSecretLiteralUpsert {
        resource: "secret/direct".into(),
        value: fixture.into(),
        display_name: None,
        description: None,
        tags: Vec::new(),
        metadata,
    })
    .unwrap();
    let settings = PlanktonSettings {
        database_url: format!("sqlite://{}", directory.path().join("store.db").display()),
        default_policy_mode: PolicyMode::ManualOnly,
        ..Default::default()
    };
    let config = DaemonConfig {
        state_path: directory.path().join("daemon.json"),
        port: 0,
    };
    let daemon = start_with_settings(config.clone(), settings.clone())
        .await
        .unwrap();
    let store = SqliteStore::new(&settings).await.unwrap();
    let script = directory.path().join("consumer.py");
    let marker = directory.path().join("count");
    let source = format!("import sys, pathlib, os\nassert 'PLANKTON_SECRET_FILE' not in os.environ\nvalue = sys.stdin.read()\nassert len(value) == {}\np = pathlib.Path(sys.argv[1])\np.write_text(str(int(p.read_text()) + 1) if p.exists() else '1')\nprint('consumer-ok')\n", fixture.len());
    std::fs::write(&script, &source).unwrap();
    let mut request = ResourceExecutionRequest {
        resource_id: "secret/test".into(),
        reason: "Consume synthetic fixture in a reviewed script".into(),
        requested_by: "execution-contract-test".into(),
        script_path: script.display().to_string(),
        interpreter_path: "/usr/bin/python3".into(),
        cwd: directory.path().display().to_string(),
        arguments: vec![marker.display().to_string()],
        timeout_seconds: 60,
        ..Default::default()
    };

    // Even direct policy and forged daemon metadata cannot open the old raw-value route.
    for resource in ["secret/test", "secret/direct"] {
        let raw = post(&daemon, "/v1/resources/access", serde_json::json!({"resource_id":resource,"reason":"test","requested_by":"test","script_path":script,"metadata":{"execution_admission":"daemon_python_file_v1","script_path_source":"declared_consumer"}})).await.text().await.unwrap();
        assert!(raw.contains("Unbound credential retrieval is disabled"));
        assert!(!raw.contains(fixture));
    }
    for (path, payload) in [
        (
            "/v1/resources/access",
            serde_json::json!({"resource_id":"secret/test","reason":"old client","requested_by":"test","metadata":{}}),
        ),
        (
            "/v1/resources/access/status",
            serde_json::json!({"request_id":"old-id"}),
        ),
    ] {
        let old_request = RequestEnvelope {
            protocol_version: 5,
            correlation_id: uuid::Uuid::new_v4(),
            payload,
        };
        let response = reqwest::Client::new()
            .post(format!("{}{path}", daemon.state().endpoint))
            .bearer_auth(&daemon.state().bearer_token)
            .json(&old_request)
            .send()
            .await
            .unwrap()
            .text()
            .await
            .unwrap();
        assert!(response.contains("protocol_mismatch"), "{response}");
    }
    for script_path in ["-", "-c", "/dev/stdin"] {
        let mut unsupported = request.clone();
        unsupported.script_path = script_path.into();
        let rejected = post(
            &daemon,
            "/v1/resources/execute",
            serde_json::to_value(unsupported).unwrap(),
        )
        .await
        .text()
        .await
        .unwrap();
        assert!(rejected.contains("failure"));
    }
    let mut forged = serde_json::to_value(&request).unwrap();
    forged["metadata"] = serde_json::json!({"execution_admission":"daemon_python_file_v1"});
    assert_eq!(
        post(&daemon, "/v1/resources/execute", forged)
            .await
            .status(),
        reqwest::StatusCode::UNPROCESSABLE_ENTITY
    );

    let approved = submit(&daemon, &request).await;
    assert!(approved.human_review_required);
    let evidence = store
        .get_request(&approved.request_id)
        .await
        .unwrap()
        .request
        .context;
    assert_eq!(
        evidence.call_chain[0].source,
        plankton_core::CallChainNodeSource::ControlledExecution
    );
    assert_eq!(
        evidence.call_chain[0].preview_text.as_deref(),
        Some(source.as_str())
    );
    assert!(!serde_json::to_string(&evidence).unwrap().contains(fixture));
    store
        .record_decision(
            &settings,
            &approved.request_id,
            Decision::Allow,
            "test-human",
            None,
        )
        .await
        .unwrap();
    let (a, b) = tokio::join!(
        poll(&daemon, &approved.request_id),
        poll(&daemon, &approved.request_id)
    );
    assert!(!matches!(a.state, ResourceExecutionState::Denied));
    assert!(!matches!(b.state, ResourceExecutionState::Denied));
    let complete = finish(&daemon, &approved.request_id).await;
    assert_eq!(
        complete.state,
        ResourceExecutionState::Completed,
        "{complete:?}"
    );
    assert_eq!(complete.stdout.trim(), "consumer-ok");
    assert_eq!(poll(&daemon, &approved.request_id).await, complete);
    assert_eq!(std::fs::read_to_string(&marker).unwrap(), "1");
    let raw = post(
        &daemon,
        "/v1/resources/access/status",
        serde_json::json!({"request_id":approved.request_id}),
    )
    .await
    .text()
    .await
    .unwrap();
    assert!(raw.contains("Unbound credential retrieval is disabled"));
    assert!(!raw.contains(fixture));

    let denied = submit(&daemon, &request).await;
    store
        .record_decision(
            &settings,
            &denied.request_id,
            Decision::Deny,
            "test-human",
            None,
        )
        .await
        .unwrap();
    assert_eq!(
        finish(&daemon, &denied.request_id).await.state,
        ResourceExecutionState::Denied
    );
    assert_eq!(std::fs::read_to_string(&marker).unwrap(), "1");

    let swapped = submit(&daemon, &request).await;
    std::fs::write(&script, "print('replacement')\n").unwrap();
    store
        .record_decision(
            &settings,
            &swapped.request_id,
            Decision::Allow,
            "test-human",
            None,
        )
        .await
        .unwrap();
    let failed = finish(&daemon, &swapped.request_id).await;
    assert_eq!(failed.state, ResourceExecutionState::Failed);
    assert!(failed.error.unwrap().contains("script changed"));
    assert_eq!(std::fs::read_to_string(&marker).unwrap(), "1");

    // Direct policy still uses the exact same verified execution path, never raw-value delivery.
    std::fs::write(&script, &source).unwrap();
    request.resource_id = "secret/direct".into();
    let direct = submit(&daemon, &request).await;
    assert_eq!(
        finish(&daemon, &direct.request_id).await.state,
        ResourceExecutionState::Completed
    );
    assert_eq!(std::fs::read_to_string(&marker).unwrap(), "2");

    // Generic command execution preserves file-module semantics and binds inline,
    // code-stdin, business-stdin, environment and shell-pipeline evidence.
    let modules = directory.path().join("modules");
    std::fs::create_dir(&modules).unwrap();
    std::fs::write(modules.join("helper.py"), "VALUE = 42\n").unwrap();
    let python_file = modules.join("consumer.py");
    std::fs::write(&python_file, "import helper, os\nassert helper.VALUE == 42\nassert os.environ['API_TOKEN']\nprint('generic-ok')\n").unwrap();
    let mut modes = vec![
        (vec!["/usr/bin/python3".into(), python_file.display().to_string()], None),
        (vec!["python3".into(), "-c".into(), "import os; assert os.environ['API_TOKEN']; print('generic-ok')".into()], None),
        (vec!["python3".into(), "-".into()], Some("import os\nassert os.environ['API_TOKEN']\nprint('generic-ok')\n".into())),
        (vec!["python3".into(), "-c".into(), "import os,sys; assert os.environ['API_TOKEN']; assert sys.stdin.read() == 'business input'; print('generic-ok')".into()], Some("business input".into())),
        (vec!["/bin/sh".into(), "-c".into(), "python3 - <<'PY' | python3 -c 'import sys; assert sys.stdin.read().strip(); print(\"generic-ok\")'\nimport os\nassert os.environ['API_TOKEN']\nprint('fixed intermediate')\nPY\n".into()], None),
        (vec!["/bin/sh".into(), "-s".into()], Some("python3 -c 'import os; assert os.environ[\"API_TOKEN\"]; print(\"generic-ok\")'\n".into())),
    ];
    let node = [
        "/opt/homebrew/bin/node",
        "/usr/local/bin/node",
        "/usr/bin/node",
    ]
    .into_iter()
    .find(|path| std::path::Path::new(path).is_file());
    if let Some(node) = node {
        std::fs::write(modules.join("helper.cjs"), "module.exports = 42;\n").unwrap();
        std::fs::write(modules.join("helper.mjs"), "export default 42;\n").unwrap();
        let cjs = modules.join("consumer.cjs");
        let esm = modules.join("consumer.mjs");
        std::fs::write(&cjs, "if(require('./helper.cjs') !== 42 || !process.env.API_TOKEN) throw Error('invalid'); console.log('generic-ok');\n").unwrap();
        std::fs::write(&esm, "import helper from './helper.mjs'; if(helper !== 42 || !process.env.API_TOKEN) throw Error('invalid'); console.log('generic-ok');\n").unwrap();
        modes.extend([
            (vec![node.into(), cjs.display().to_string()], None),
            (vec![node.into(), esm.display().to_string()], None),
            (vec![node.into(), "-e".into(), "if(!process.env.API_TOKEN) throw Error('missing'); console.log('generic-ok');".into()], None),
            (vec![node.into(), "-".into()], Some("if(!process.env.API_TOKEN) throw Error('missing'); console.log('generic-ok');\n".into())),
        ]);
    }
    for (command, command_stdin) in modes {
        let generic = ResourceExecutionRequest {
            resource_id: "secret/direct".into(),
            reason: "Run a collected synthetic consumer".into(),
            requested_by: "execution-contract-test".into(),
            cwd: directory.path().display().to_string(),
            command,
            credential_env: Some("API_TOKEN".into()),
            command_stdin,
            ..Default::default()
        };
        let submitted = submit(&daemon, &generic).await;
        let result = finish(&daemon, &submitted.request_id).await;
        assert_eq!(
            result.state,
            ResourceExecutionState::Completed,
            "command {:?}: {result:?}",
            generic.command
        );
        assert_eq!(result.stdout.trim(), "generic-ok");
        let context = store
            .get_request(&submitted.request_id)
            .await
            .unwrap()
            .request
            .context;
        assert_eq!(
            context.metadata.get("execution_admission").unwrap(),
            "daemon_command_v1"
        );
        assert!(!serde_json::to_string(&context).unwrap().contains(fixture));
        let snapshots: Vec<plankton_core::execution::ExecutionSource> =
            serde_json::from_str(context.metadata.get("execution_sources").unwrap()).unwrap();
        assert!(!snapshots.is_empty());
        assert!(snapshots
            .iter()
            .all(|snapshot| snapshot.node_index < context.call_chain.len()));
    }
    let mut unknown = ResourceExecutionRequest {
        resource_id: "secret/direct".into(),
        reason: "unknown entrypoint must be rejected".into(),
        requested_by: "execution-contract-test".into(),
        cwd: directory.path().display().to_string(),
        command: vec!["unrecognized-program".into()],
        ..Default::default()
    };
    let rejected = post(
        &daemon,
        "/v1/resources/execute",
        serde_json::to_value(&unknown).unwrap(),
    )
    .await
    .text()
    .await
    .unwrap();
    assert!(rejected.contains("no execution-evidence recognizer"));
    let impostor = directory.path().join("curl");
    std::fs::write(&impostor, "#!/bin/sh\nprintf unexpected\n").unwrap();
    unknown.command = vec![impostor.display().to_string()];
    let rejected = post(
        &daemon,
        "/v1/resources/execute",
        serde_json::to_value(&unknown).unwrap(),
    )
    .await
    .text()
    .await
    .unwrap();
    assert!(rejected.contains("does not match an installed recognized runtime"));

    for (source, expected_error, code) in [
        (
            "import sys\nsys.stdout.write(sys.stdin.read())\n",
            "output was withheld",
            Some(0),
        ),
        (
            "import sys\nsys.stdin.read()\nsys.exit(7)\n",
            "consumer exited unsuccessfully",
            Some(7),
        ),
        (
            "import sys\nsys.stdin.read()\nsys.stdout.write('x' * 70000)\n",
            "exceeded 64 KiB",
            None,
        ),
        (
            "import sys, time\nsys.stdin.read()\ntime.sleep(60)\n",
            "approved timeout",
            None,
        ),
    ] {
        std::fs::write(&script, source).unwrap();
        request.timeout_seconds = 1;
        let submitted = submit(&daemon, &request).await;
        let result = finish(&daemon, &submitted.request_id).await;
        assert_eq!(result.state, ResourceExecutionState::Failed, "{result:?}");
        assert!(
            result.error.as_deref().unwrap().contains(expected_error),
            "{result:?}"
        );
        assert_eq!(result.exit_code, code);
        assert!(result.stdout.is_empty() && result.stderr.is_empty());
    }

    // Both output surfaces must be enabled; enabling only one is insufficient.
    use plankton_protocol::exposure::{CredentialExposurePolicy, CredentialExposureSurface};
    for allow_both in [false, true] {
        let mut policy = CredentialExposurePolicy::direct();
        for surface in &mut policy.surfaces {
            if surface.surface == CredentialExposureSurface::TerminalLog
                || (allow_both && surface.surface == CredentialExposureSurface::LlmContext)
            {
                surface.max_level = 1;
            }
        }
        let mut metadata = BTreeMap::new();
        plankton_core::store_exposure_policy(&mut metadata, &policy).unwrap();
        plankton_core::upsert_local_secret_literal(plankton_core::LocalSecretLiteralUpsert {
            resource: "secret/output".into(),
            value: fixture.into(),
            display_name: None,
            description: None,
            tags: Vec::new(),
            metadata,
        })
        .unwrap();
        request.resource_id = "secret/output".into();
        std::fs::write(&script, "import sys\nsys.stdout.write(sys.stdin.read())\n").unwrap();
        let submitted = submit(&daemon, &request).await;
        let result = finish(&daemon, &submitted.request_id).await;
        if allow_both {
            assert_eq!(result.state, ResourceExecutionState::Completed);
            assert_eq!(result.stdout, fixture);
        } else {
            assert_eq!(result.state, ResourceExecutionState::Failed);
            assert!(result.stdout.is_empty());
        }
    }

    // Shutdown must cancel the detached consumer before a later side effect.
    let started = directory.path().join("started");
    let after_shutdown = directory.path().join("after-shutdown");
    std::fs::write(&script, "import sys, time, pathlib\nsys.stdin.read()\npathlib.Path(sys.argv[1]).write_text('started')\ntime.sleep(2)\npathlib.Path(sys.argv[2]).write_text('must not happen')\n").unwrap();
    request.timeout_seconds = 60;
    request.arguments = vec![
        started.display().to_string(),
        after_shutdown.display().to_string(),
    ];
    request.resource_id = "secret/test".into();
    let orphan = submit(&daemon, &request).await;
    request.resource_id = "secret/direct".into();
    let interrupted = submit(&daemon, &request).await;
    assert_eq!(
        poll(&daemon, &interrupted.request_id).await.state,
        ResourceExecutionState::Running
    );
    tokio::time::timeout(Duration::from_secs(5), async {
        while !started.exists() {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .unwrap();

    daemon.shutdown().await.unwrap();
    tokio::time::sleep(Duration::from_millis(2200)).await;
    assert!(
        !after_shutdown.exists(),
        "consumer survived daemon shutdown"
    );
    let restarted = start_with_settings(config, settings).await.unwrap();
    let expired = post(
        &restarted,
        "/v1/resources/execute/status",
        serde_json::json!({"request_id":approved.request_id}),
    )
    .await
    .text()
    .await
    .unwrap();
    assert!(expired.contains("daemon restarted"));
    assert_eq!(std::fs::read_to_string(&marker).unwrap(), "2");
    assert_eq!(
        store
            .get_request(&orphan.request_id)
            .await
            .unwrap()
            .request
            .approval_status,
        plankton_core::ApprovalStatus::Rejected
    );
    let diagnostics = store.list_diagnostic_errors(true, 100).await.unwrap();
    assert!(!format!("{diagnostics:?}").contains(fixture));
    restarted.shutdown().await.unwrap();
}
