#![cfg(unix)]

use plankton_core::execution::{requires_source_stdin, ExecutionPlan};
use plankton_protocol::resources::ResourceExecutionRequest;
use std::{fs, path::Path};

fn request(directory: &Path, command: &[&str]) -> ResourceExecutionRequest {
    ResourceExecutionRequest {
        resource_id: "secret/review-fixture".into(),
        reason: "Independent execution binding regression".into(),
        requested_by: "test".into(),
        cwd: directory.display().to_string(),
        command: command.iter().map(|value| (*value).into()).collect(),
        ..Default::default()
    }
}

fn shell_request(directory: &Path, source: &str) -> ResourceExecutionRequest {
    request(directory, &["/bin/sh", "-c", source])
}

#[test]
fn login_and_interactive_shells_cannot_add_unreviewed_startup_sources() {
    let directory = tempfile::tempdir().unwrap();
    for flags in ["-lc", "-ic", "-lic"] {
        assert!(
            ExecutionPlan::prepare(&request(directory.path(), &["/bin/sh", flags, "true"]))
                .is_err(),
            "startup-changing flags must fail admission: {flags}"
        );
    }
    for flag in ["-l", "-i"] {
        let mut stdin_source = request(directory.path(), &["/bin/sh", "-s", flag]);
        stdin_source.credential_env = Some("API_TOKEN".into());
        stdin_source.command_stdin = Some("true\n".into());
        assert!(
            ExecutionPlan::prepare(&stdin_source).is_err(),
            "startup flags after -s must also fail: {flag}"
        );
    }
    if Path::new("/bin/zsh").exists() {
        assert!(
            ExecutionPlan::prepare(&request(directory.path(), &["/bin/zsh", "-c", "true"]))
                .is_err()
        );
        // -f disables user startup files. System startup handling is a separately
        // recorded boundary; the plan may require an absent or collected system file.
    }
}

#[test]
fn shell_tilde_paths_are_not_confused_with_literal_relative_paths() {
    let directory = tempfile::tempdir().unwrap();
    fs::create_dir(directory.path().join("~")).unwrap();
    fs::write(
        directory.path().join("~/consumer.py"),
        "print('literal path')\n",
    )
    .unwrap();
    assert!(
        ExecutionPlan::prepare(&shell_request(directory.path(), "python3 ~/consumer.py")).is_err()
    );
    let literal =
        ExecutionPlan::prepare(&shell_request(directory.path(), "python3 '~/consumer.py'"))
            .unwrap();
    literal.verify_unchanged().unwrap();
}

#[test]
fn last_stdin_redirection_controls_the_source_that_is_collected() {
    let directory = tempfile::tempdir().unwrap();
    fs::write(
        directory.path().join("original.py"),
        "print('file source')\n",
    )
    .unwrap();
    let source = "python3 - < original.py <<'PY'\nprint('heredoc source')\nPY\n";
    let plan = ExecutionPlan::prepare(&shell_request(directory.path(), source)).unwrap();
    let nodes = plan.call_chain_nodes();
    assert_eq!(
        nodes.last().unwrap().preview_text.as_deref(),
        Some("print('heredoc source')\n")
    );

    let reverse = "python3 - <<'PY' < original.py\nprint('heredoc source')\nPY\n";
    let plan = ExecutionPlan::prepare(&shell_request(directory.path(), reverse)).unwrap();
    assert_eq!(
        plan.call_chain_nodes()
            .last()
            .unwrap()
            .preview_text
            .as_deref(),
        Some("print('file source')\n")
    );
}

#[test]
fn another_file_descriptor_cannot_be_misreported_as_source_stdin() {
    let directory = tempfile::tempdir().unwrap();
    fs::write(directory.path().join("original.py"), "print('not stdin')\n").unwrap();
    for source in [
        "python3 - 3< original.py",
        "python3 - 3<<'PY'\nprint('not stdin')\nPY\n",
    ] {
        assert!(ExecutionPlan::prepare(&shell_request(directory.path(), source)).is_err());
    }
}

#[test]
fn shell_cannot_overwrite_a_collected_entrypoint_before_using_it() {
    let directory = tempfile::tempdir().unwrap();
    fs::write(directory.path().join("consumer.py"), "print('reviewed')\n").unwrap();
    let source = "printf 'print(2)\\n' > consumer.py; python3 consumer.py";
    assert!(ExecutionPlan::prepare(&shell_request(directory.path(), source)).is_err());
}

#[test]
fn plain_environment_references_work_but_prompt_reexpansion_is_not_admitted() {
    let directory = tempfile::tempdir().unwrap();
    let mut plain = shell_request(
        directory.path(),
        "curl -H \"Authorization: Bearer ${API_TOKEN}\" https://example.invalid",
    );
    plain.credential_env = Some("API_TOKEN".into());
    ExecutionPlan::prepare(&plain).unwrap();
    let mut prompt = request(
        directory.path(),
        &["/bin/bash", "-c", "echo \"${API_TOKEN@P}\""],
    );
    prompt.credential_env = Some("API_TOKEN".into());
    assert!(ExecutionPlan::prepare(&prompt).is_err());
}

#[test]
fn native_dash_arguments_do_not_trigger_implicit_cli_stdin_capture() {
    for command in [vec!["curl", "-"], vec!["curl", "--", "-"]] {
        assert!(!requires_source_stdin(
            &command
                .iter()
                .map(|value| (*value).into())
                .collect::<Vec<_>>()
        ));
    }
}
