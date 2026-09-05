use std::path::Path;

use plankton_core::{AcpSessionConfig, PlanktonSettings, ProviderError};
use plankton_protocol::acp::{AcpProfile, AgentKind, VersionMode};

fn settings_for(profile: AcpProfile) -> PlanktonSettings {
    PlanktonSettings {
        acp_profile: profile,
        ..PlanktonSettings::default()
    }
}

fn preset(agent_kind: AgentKind, version_mode: VersionMode, version: Option<&str>) -> AcpProfile {
    AcpProfile {
        session_options: Default::default(),
        agent_kind,
        version_mode,
        version: version.map(str::to_string),
        program: None,
        args: Vec::new(),
    }
}

fn assert_resolved_npx(program: &str) {
    let program = Path::new(program);
    assert!(
        program.is_absolute(),
        "npx should resolve to an absolute path"
    );
    assert_eq!(
        program.file_name().and_then(|name| name.to_str()),
        Some("npx")
    );
}

#[test]
fn codex_latest_resolves_to_the_latest_selector_without_a_pinned_trace_version() {
    let config = AcpSessionConfig::from_settings(&settings_for(preset(
        AgentKind::Codex,
        VersionMode::Latest,
        None,
    )))
    .expect("Codex latest should resolve");

    assert_resolved_npx(&config.program);
    assert_eq!(
        config.args,
        vec![
            "-y".to_string(),
            "@agentclientprotocol/codex-acp@latest".to_string(),
        ]
    );
    assert_eq!(
        config.package_name.as_deref(),
        Some("@agentclientprotocol/codex-acp")
    );
    assert_eq!(config.package_version, None);
}

#[test]
fn codex_pinned_resolves_to_the_maintained_package_with_the_exact_version() {
    let config = AcpSessionConfig::from_settings(&settings_for(preset(
        AgentKind::Codex,
        VersionMode::Pinned,
        Some("0.12.3"),
    )))
    .expect("pinned Codex preset should resolve");

    assert_eq!(
        config.args,
        vec![
            "-y".to_string(),
            "@agentclientprotocol/codex-acp@0.12.3".to_string(),
        ]
    );
    assert_eq!(
        config.package_name.as_deref(),
        Some("@agentclientprotocol/codex-acp")
    );
    assert_eq!(config.package_version.as_deref(), Some("0.12.3"));
}

#[test]
fn preset_agents_resolve_latest_launch_commands() {
    let cases = [
        (
            AgentKind::ClaudeCode,
            vec!["-y", "@zed-industries/claude-code-acp@latest"],
        ),
        (AgentKind::OpenCode, vec!["-y", "opencode-ai@latest", "acp"]),
    ];

    for (agent_kind, expected_args) in cases {
        let config = AcpSessionConfig::from_settings(&settings_for(preset(
            agent_kind,
            VersionMode::Latest,
            None,
        )))
        .expect("latest preset should resolve");

        assert_resolved_npx(&config.program);
        assert_eq!(
            config.args,
            expected_args
                .into_iter()
                .map(str::to_string)
                .collect::<Vec<_>>()
        );
        assert_eq!(config.package_version, None);
    }
}

#[test]
fn pinned_preset_keeps_the_exact_semver_in_command_and_trace() {
    let config = AcpSessionConfig::from_settings(&settings_for(preset(
        AgentKind::OpenCode,
        VersionMode::Pinned,
        Some("1.2.3"),
    )))
    .expect("pinned preset should resolve");

    assert_eq!(
        config.args,
        vec![
            "-y".to_string(),
            "opencode-ai@1.2.3".to_string(),
            "acp".to_string(),
        ]
    );
    assert_eq!(config.package_name.as_deref(), Some("opencode-ai"));
    assert_eq!(config.package_version.as_deref(), Some("1.2.3"));
}

#[test]
fn custom_profile_preserves_program_and_args() {
    let config = AcpSessionConfig::from_settings(&settings_for(AcpProfile {
        session_options: Default::default(),
        agent_kind: AgentKind::Custom,
        version_mode: VersionMode::Custom,
        version: None,
        program: Some("/opt/acp/bin/agent".to_string()),
        args: vec!["serve".to_string(), "--stdio".to_string()],
    }))
    .expect("custom program should resolve");

    assert_eq!(config.program, "/opt/acp/bin/agent");
    assert_eq!(config.args, vec!["serve", "--stdio"]);
    assert_eq!(config.package_name, None);
    assert_eq!(config.package_version, None);
}

#[test]
fn normalized_legacy_zed_latest_profile_resolves_to_the_maintained_codex_package() {
    let config = AcpSessionConfig::from_settings(&settings_for(preset(
        AgentKind::Codex,
        VersionMode::Latest,
        None,
    )))
    .expect("normalized legacy latest profile should resolve");

    assert_eq!(
        config.args,
        vec![
            "-y".to_string(),
            "@agentclientprotocol/codex-acp@latest".to_string(),
        ]
    );
    assert_eq!(
        config.package_name.as_deref(),
        Some("@agentclientprotocol/codex-acp")
    );
    assert_eq!(config.package_version, None);
}

#[test]
fn normalized_legacy_zed_pinned_profile_stays_custom() {
    for version in ["0.10.0", "0.11.1"] {
        let package = format!("@zed-industries/codex-acp@{version}");
        let settings = settings_for(AcpProfile {
            session_options: Default::default(),
            agent_kind: AgentKind::Custom,
            version_mode: VersionMode::Custom,
            version: None,
            program: Some("npx".to_string()),
            args: vec!["-y".to_string(), package.clone()],
        });

        let config = AcpSessionConfig::from_settings(&settings)
            .expect("normalized legacy pinned profile should remain custom");

        assert_resolved_npx(&config.program);
        assert_eq!(config.args, vec!["-y".to_string(), package]);
        assert_eq!(config.package_name, None);
        assert_eq!(config.package_version, None);
    }
}

#[test]
fn structured_profile_wins_over_residual_legacy_zed_pinned_fields() {
    let settings = PlanktonSettings {
        acp_profile: preset(AgentKind::Codex, VersionMode::Pinned, Some("0.12.3")),
        acp_codex_program: "npx".to_string(),
        acp_codex_args: "-y @zed-industries/codex-acp@0.11.1".to_string(),
        ..PlanktonSettings::default()
    };

    let config =
        AcpSessionConfig::from_settings(&settings).expect("structured ACP profile should win");

    assert_eq!(
        config.args,
        vec![
            "-y".to_string(),
            "@agentclientprotocol/codex-acp@0.12.3".to_string(),
        ]
    );
    assert_eq!(
        config.package_name.as_deref(),
        Some("@agentclientprotocol/codex-acp")
    );
    assert_eq!(config.package_version.as_deref(), Some("0.12.3"));
}

#[test]
fn invalid_pinned_version_is_reported_as_a_configuration_error() {
    let error = AcpSessionConfig::from_settings(&settings_for(preset(
        AgentKind::Codex,
        VersionMode::Pinned,
        Some("latest"),
    )))
    .expect_err("latest is not an exact pinned semantic version");

    assert!(matches!(error, ProviderError::Config(_)));
    assert!(error.to_string().contains("invalid pinned ACP version"));
}

#[tokio::test]
async fn spawn_failures_are_returned_to_the_caller() {
    let config = AcpSessionConfig::from_settings(&settings_for(AcpProfile {
        session_options: Default::default(),
        agent_kind: AgentKind::Custom,
        version_mode: VersionMode::Custom,
        version: None,
        program: Some("/definitely/not/a/plankton-acp-agent".to_string()),
        args: Vec::new(),
    }))
    .expect("custom program configuration should resolve");

    let error = plankton_core::AcpSessionClient::new(config)
        .prompt_json_suggestion("return JSON".to_string())
        .await
        .expect_err("missing executable must not be swallowed");

    assert!(matches!(error, ProviderError::Transport(_)));
    assert!(error
        .to_string()
        .contains("failed to spawn ACP agent process"));
}
