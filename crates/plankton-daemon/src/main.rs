use anyhow::Context;
use plankton_daemon::{start, DaemonConfig};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();
    let running = start(DaemonConfig::default())
        .await
        .context("failed to start planktond")?;
    tracing::info!(
        endpoint = %running.state().endpoint,
        pid = running.state().pid,
        "planktond ready"
    );
    tokio::signal::ctrl_c()
        .await
        .context("failed to listen for shutdown signal")?;
    running
        .shutdown()
        .await
        .context("failed to stop planktond cleanly")
}
