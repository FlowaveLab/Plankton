//! Live adapter discovery/readiness check; never accesses a Plankton credential.
use plankton_core::{AcpSessionClient, PlanktonSettings};
use plankton_protocol::acp::AgentKind;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut settings = PlanktonSettings::default();
    settings.acp_profile.agent_kind = match args.first().map(String::as_str) {
        Some("claude_code") => AgentKind::ClaudeCode,
        Some("open_code") => AgentKind::OpenCode,
        Some("codex") => AgentKind::Codex,
        _ => {
            return Err(
                "usage: acp_options_probe codex|claude_code|open_code [--ready] [id=value ...]"
                    .into(),
            )
        }
    };
    settings.acp_timeout_secs = 60;
    for arg in args.iter().skip(1).filter(|arg| !arg.starts_with("--")) {
        let (id, value) = arg.split_once('=').ok_or("expected id=value")?;
        settings
            .acp_profile
            .session_options
            .insert(id.into(), value.into());
    }
    let client = AcpSessionClient::from_settings(&settings)?;
    if args.iter().any(|arg| arg == "--parallel") {
        let catalog = client.discover_options().await?;
        let model = catalog
            .config_options
            .iter()
            .find(|option| option.category.as_deref() == Some("model"))
            .ok_or("no model selector")?;
        let mut other_settings = settings.clone();
        other_settings
            .acp_profile
            .session_options
            .insert(model.id.clone(), model.current_value.clone());
        let other = AcpSessionClient::from_settings(&other_settings)?;
        let first = client
            .continue_chat_with_files(None, "Reply with exactly FIRST.".into(), vec![])
            .await?;
        tokio::time::sleep(std::time::Duration::from_millis(300)).await;
        let second = other
            .continue_chat_with_files(None, "Reply with exactly SECOND.".into(), vec![])
            .await?;
        let (first, second) = tokio::join!(first.finish(), second.finish());
        let first = first?;
        let second = second?;
        if !first.content.contains("FIRST") || !second.content.contains("SECOND") {
            return Err("parallel response check failed".into());
        }
        println!(
            "{}",
            serde_json::json!({"different_configurations_completed":true,"first_session":first.trace.session_id,"second_session":second.trace.session_id})
        );
        return Ok(());
    }
    if args.iter().any(|arg| arg == "--conversation") {
        let directory = tempfile::tempdir()?;
        let path = directory.path().join("evidence.txt");
        let marker = format!("ACP-EVIDENCE-{}", uuid::Uuid::new_v4());
        std::fs::write(&path, &marker)?;
        let first = client.continue_chat_with_files(None, format!("Protocol smoke test. Read the local fixture file {} using a tool, then reply with its content only. This fixture is not a credential.", path.display()), vec![path.display().to_string()]).await?.finish().await?;
        if !first.content.contains(&marker) {
            return Err("file tool round trip did not return fixture marker".into());
        }
        let session = first.trace.session_id.clone().ok_or("missing session")?;
        let second = client
            .continue_chat_with_files(
                Some(session.clone()),
                "Without reading files again, repeat the fixture marker from our previous turn."
                    .into(),
                vec![],
            )
            .await?
            .finish()
            .await?;
        if second.trace.session_id.as_deref() != Some(&session) || !second.content.contains(&marker)
        {
            return Err("session continuity check failed".into());
        }
        println!(
            "{}",
            serde_json::json!({"file_read":true,"same_session":true,"session_id":session,"first_response":first.content,"continued_response":second.content})
        );
        return Ok(());
    }
    let result = if args.iter().any(|arg| arg == "--ready") {
        client.test_connection().await?
    } else {
        client.discover_options().await?
    };
    println!("{}", serde_json::to_string_pretty(&result)?);
    Ok(())
}
