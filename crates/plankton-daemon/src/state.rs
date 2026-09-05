use std::{
    fs::{self, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
};

use chrono::Utc;
use plankton_core::passwords::ConfirmationLedger;
use plankton_core::{load_settings, shutdown_acp_supervisor, PlanktonSettings};
use plankton_protocol::{daemon::DaemonState, PROTOCOL_VERSION};
use plankton_store::SqliteStore;
use std::sync::Arc;
use tokio::sync::Mutex;
use tokio::{net::TcpListener, sync::oneshot, task::JoinHandle};
use tracing::error;
use uuid::Uuid;

use crate::{evaluation::EvaluationWorker, runtime_settings::RuntimeSettings, server};

#[derive(Debug, Clone)]
pub struct PasswordDraftController {
    confirmations: Arc<Mutex<ConfirmationLedger>>,
}

impl PasswordDraftController {
    pub async fn preview(
        &self,
        draft_id: Uuid,
    ) -> Result<
        plankton_core::passwords::ParsedPasswordSource,
        plankton_core::passwords::ConfirmationError,
    > {
        self.confirmations.lock().await.preview(draft_id)
    }

    pub async fn confirm(
        &self,
        draft_id: Uuid,
        destination: plankton_protocol::passwords::PasswordDestination,
    ) -> Result<
        plankton_core::passwords::ConfirmedPasswordWrite,
        plankton_core::passwords::ConfirmationError,
    > {
        self.confirmations
            .lock()
            .await
            .confirm_and_consume(draft_id, destination)
    }

    pub async fn restore(
        &self,
        draft_id: Uuid,
        source: plankton_core::passwords::ParsedPasswordSource,
    ) {
        self.confirmations
            .lock()
            .await
            .restore_draft(draft_id, source);
    }

    pub async fn replace(
        &self,
        draft_id: Uuid,
        source: plankton_core::passwords::ParsedPasswordSource,
    ) -> Result<(), plankton_core::passwords::ConfirmationError> {
        self.confirmations
            .lock()
            .await
            .replace_draft(draft_id, source)
    }

    pub async fn complete(
        &self,
        draft_id: Uuid,
        destination: String,
        resource_ids: Vec<String>,
    ) -> Result<(), plankton_core::passwords::ConfirmationError> {
        self.confirmations
            .lock()
            .await
            .complete_draft(draft_id, destination, resource_ids)
    }
}

#[derive(Debug, Clone)]
pub struct DaemonConfig {
    pub state_path: PathBuf,
    pub port: u16,
}

impl Default for DaemonConfig {
    fn default() -> Self {
        let state_path = directories::ProjectDirs::from("com", "OpenAquarium", "Plankton")
            .map(|directories| {
                directories
                    .runtime_dir()
                    .unwrap_or(directories.data_local_dir())
                    .to_path_buf()
            })
            .unwrap_or_else(std::env::temp_dir)
            .join("daemon.json");
        Self {
            state_path,
            port: 0,
        }
    }
}

pub async fn start(config: DaemonConfig) -> Result<RunningDaemon, DaemonStartError> {
    let settings = load_settings().map_err(DaemonStartError::Settings)?;
    start_with_runtime_settings(config, settings, RuntimeSettings::reloading()).await
}

pub async fn start_with_settings(
    config: DaemonConfig,
    settings: PlanktonSettings,
) -> Result<RunningDaemon, DaemonStartError> {
    let runtime_settings = RuntimeSettings::fixed(settings.clone());
    start_with_runtime_settings(config, settings, runtime_settings).await
}

async fn start_with_runtime_settings(
    config: DaemonConfig,
    initial_settings: PlanktonSettings,
    runtime_settings: RuntimeSettings,
) -> Result<RunningDaemon, DaemonStartError> {
    if config.state_path.exists() {
        return Err(DaemonStartError::StateAlreadyExists(config.state_path));
    }
    let store = SqliteStore::new(&initial_settings)
        .await
        .map_err(DaemonStartError::Store)?;
    let evaluations = EvaluationWorker::new(runtime_settings.clone(), store.clone());
    evaluations
        .recover_stale()
        .await
        .map_err(DaemonStartError::Store)?;
    let listener = TcpListener::bind(("127.0.0.1", config.port))
        .await
        .map_err(DaemonStartError::Bind)?;
    let address = listener.local_addr().map_err(DaemonStartError::Bind)?;
    let state = DaemonState {
        protocol_version: PROTOCOL_VERSION,
        endpoint: format!("http://{address}"),
        bearer_token: format!("{}{}", Uuid::new_v4().simple(), Uuid::new_v4().simple()),
        pid: std::process::id(),
        started_at: Utc::now(),
    };
    write_state_atomic(&config.state_path, &state)?;
    if let Err(error) = evaluations.resume_queued().await {
        let _ = fs::remove_file(&config.state_path);
        return Err(DaemonStartError::Store(error));
    }

    let (shutdown_tx, shutdown_rx) = oneshot::channel();
    let confirmations = Arc::new(Mutex::new(ConfirmationLedger::new(
        std::time::Duration::from_secs(15 * 60),
    )));
    let app = server::router(
        state.clone(),
        confirmations.clone(),
        runtime_settings,
        store,
        evaluations.clone(),
    );
    let task = tokio::spawn(async move {
        axum::serve(listener, app)
            .with_graceful_shutdown(async {
                let _result = shutdown_rx.await;
            })
            .await
    });
    Ok(RunningDaemon {
        state,
        password_drafts: PasswordDraftController { confirmations },
        state_path: Some(config.state_path),
        shutdown_tx: Some(shutdown_tx),
        task: Some(task),
        evaluations: Some(evaluations),
    })
}

#[derive(Debug)]
pub struct RunningDaemon {
    state: DaemonState,
    password_drafts: PasswordDraftController,
    state_path: Option<PathBuf>,
    shutdown_tx: Option<oneshot::Sender<()>>,
    task: Option<JoinHandle<std::io::Result<()>>>,
    evaluations: Option<EvaluationWorker>,
}

impl RunningDaemon {
    pub fn state(&self) -> &DaemonState {
        &self.state
    }

    pub fn password_drafts(&self) -> PasswordDraftController {
        self.password_drafts.clone()
    }

    pub async fn shutdown(mut self) -> Result<(), DaemonStartError> {
        let shutdown_result = self.shutdown_tx.take().map_or(Ok(()), |shutdown| {
            shutdown
                .send(())
                .map_err(|()| DaemonStartError::ShutdownChannel)
        });
        let serve_result = if let Some(task) = self.task.take() {
            match task.await {
                Ok(result) => result.map_err(DaemonStartError::Serve),
                Err(error) => Err(DaemonStartError::Join(error)),
            }
        } else {
            Ok(())
        };
        let evaluation_result = if let Some(evaluations) = self.evaluations.take() {
            evaluations
                .shutdown()
                .await
                .map_err(DaemonStartError::Store)
        } else {
            Ok(())
        };
        shutdown_acp_supervisor();
        let remove_state_result = self.state_path.take().map_or(Ok(()), |path| {
            fs::remove_file(path).map_err(DaemonStartError::RemoveState)
        });

        shutdown_result?;
        serve_result?;
        evaluation_result?;
        remove_state_result?;
        Ok(())
    }
}

impl Drop for RunningDaemon {
    fn drop(&mut self) {
        if let Some(evaluations) = self.evaluations.take() {
            evaluations.abort_now();
        }
        shutdown_acp_supervisor();
        if let Some(shutdown) = self.shutdown_tx.take() {
            if shutdown.send(()).is_err() {
                error!("daemon shutdown receiver was already closed");
            }
        }
        if let Some(path) = self.state_path.take() {
            if let Err(remove_error) = fs::remove_file(&path) {
                if remove_error.kind() != std::io::ErrorKind::NotFound {
                    error!(path = %path.display(), error = %remove_error, "failed to remove daemon state");
                }
            }
        }
    }
}

fn write_state_atomic(path: &Path, state: &DaemonState) -> Result<(), DaemonStartError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(DaemonStartError::CreateStateDirectory)?;
    }
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| DaemonStartError::InvalidStatePath(path.to_path_buf()))?;
    let temporary = path.with_file_name(format!(".{file_name}.{}.tmp", std::process::id()));
    let result = (|| {
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options
            .open(&temporary)
            .map_err(DaemonStartError::CreateState)?;
        let bytes = serde_json::to_vec_pretty(state).map_err(DaemonStartError::SerializeState)?;
        file.write_all(&bytes)
            .map_err(DaemonStartError::WriteState)?;
        file.sync_all().map_err(DaemonStartError::SyncState)?;
        fs::rename(&temporary, path).map_err(DaemonStartError::PersistState)
    })();
    if result.is_err() {
        match fs::remove_file(&temporary) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => {
                return Err(DaemonStartError::CleanupTemporaryState {
                    path: temporary,
                    source: error,
                });
            }
        }
    }
    result
}

#[derive(Debug, thiserror::Error)]
pub enum DaemonStartError {
    #[error("daemon state already exists at {0}")]
    StateAlreadyExists(PathBuf),
    #[error("invalid daemon state path: {0}")]
    InvalidStatePath(PathBuf),
    #[error("failed to load daemon settings: {0}")]
    Settings(#[source] plankton_core::SettingsError),
    #[error("daemon persistence failed: {0}")]
    Store(#[source] plankton_store::StoreError),
    #[error("failed to bind daemon listener: {0}")]
    Bind(#[source] std::io::Error),
    #[error("failed to create daemon state directory: {0}")]
    CreateStateDirectory(#[source] std::io::Error),
    #[error("failed to create daemon state: {0}")]
    CreateState(#[source] std::io::Error),
    #[error("failed to serialize daemon state: {0}")]
    SerializeState(#[source] serde_json::Error),
    #[error("failed to write daemon state: {0}")]
    WriteState(#[source] std::io::Error),
    #[error("failed to sync daemon state: {0}")]
    SyncState(#[source] std::io::Error),
    #[error("failed to persist daemon state: {0}")]
    PersistState(#[source] std::io::Error),
    #[error("failed to clean temporary daemon state {path}: {source}")]
    CleanupTemporaryState {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("daemon shutdown channel was already closed")]
    ShutdownChannel,
    #[error("daemon task failed: {0}")]
    Join(#[source] tokio::task::JoinError),
    #[error("daemon server failed: {0}")]
    Serve(#[source] std::io::Error),
    #[error("failed to remove daemon state: {0}")]
    RemoveState(#[source] std::io::Error),
}
