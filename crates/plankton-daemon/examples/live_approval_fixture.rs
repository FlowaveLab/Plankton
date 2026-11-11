//! Real-provider fixture server for scripts/live-approval-cases.py.
//! Model configuration is inherited; all credentials and application data are synthetic.
use std::{collections::BTreeMap, path::PathBuf};

use anyhow::{ensure, Context, Result};
use plankton_core::{load_settings, store_exposure_policy, PolicyMode};
use plankton_daemon::{start_with_settings, DaemonConfig};
use plankton_protocol::exposure::{
    CredentialExposurePolicy, CredentialExposureSurface, ExposureBreachAction,
};

#[tokio::main]
async fn main() -> Result<()> {
    let directory = PathBuf::from(
        std::env::args()
            .nth(1)
            .context("fixture directory required")?,
    )
    .canonicalize()?;
    std::env::set_current_dir(&directory)?;
    // No project configuration or real credential catalog is loaded in this directory.
    let mut settings = load_settings()?;
    ensure!(
        settings.provider_kind == "acp",
        "live fixture requires the configured ACP provider"
    );
    let catalog = directory.join("synthetic-catalog.toml");
    std::fs::write(&catalog, "[secrets]\n")?;
    std::env::set_var("PLANKTON_SECRET_FILE", &catalog);
    for (resource, process_limit) in [
        ("secret/live-a", 1),
        ("secret/live-b", 1),
        ("secret/live-blocked", 0),
    ] {
        let mut policy = CredentialExposurePolicy {
            breach_action: ExposureBreachAction::Deny,
            ..Default::default()
        };
        policy
            .surfaces
            .iter_mut()
            .find(|entry| entry.surface == CredentialExposureSurface::ProcessPropagation)
            .unwrap()
            .max_level = process_limit;
        let mut metadata = BTreeMap::new();
        store_exposure_policy(&mut metadata, &policy)?;
        plankton_core::upsert_local_secret_literal(plankton_core::LocalSecretLiteralUpsert {
            resource: resource.into(),
            value: format!("synthetic-live-{}", uuid::Uuid::new_v4()),
            display_name: Some(resource.into()),
            description: None,
            tags: Vec::new(),
            metadata,
        })?;
    }
    settings.database_url = format!("sqlite://{}", directory.join("fixture.db").display());
    settings.default_policy_mode = PolicyMode::LlmAutomatic;
    let model = settings.acp_profile.session_options.get("model").cloned();
    let agent = settings.acp_profile.agent_kind;
    let daemon = start_with_settings(
        DaemonConfig {
            state_path: directory.join("daemon.json"),
            port: 0,
        },
        settings,
    )
    .await?;
    std::fs::write(
        directory.join("ready.json"),
        serde_json::to_vec_pretty(&serde_json::json!({
            "synthetic_only":true,"agent":agent,"model":model,"protocol_version":plankton_protocol::PROTOCOL_VERSION,
        }))?,
    )?;
    tokio::signal::ctrl_c().await?;
    daemon.shutdown().await?;
    Ok(())
}
