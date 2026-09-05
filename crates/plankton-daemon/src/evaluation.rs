use std::{
    collections::BTreeMap,
    fmt,
    future::Future,
    pin::Pin,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    time::Duration,
};

use chrono::Utc;
use plankton_core::{
    request_llm_suggestion_with_progress, LlmSuggestionProgress, PlanktonSettings, SettingsError,
};
use plankton_protocol::error::{ErrorCode, ErrorSeverity, ErrorSource, PlanktonError};
use plankton_store::{SqliteStore, StoreError};
use tokio::sync::mpsc;
use tokio::task::{AbortHandle, JoinHandle};
use tracing::{error, warn};
use uuid::Uuid;

use crate::{review_journal::ReviewJournal, runtime_settings::RuntimeSettings};

const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(1);

type EvaluationFuture = Pin<Box<dyn Future<Output = Result<(), StoreError>> + Send + 'static>>;
type EvaluationExecutor =
    Arc<dyn Fn(PlanktonSettings, SqliteStore, String) -> EvaluationFuture + Send + Sync + 'static>;

#[derive(Debug, thiserror::Error)]
enum EvaluationRunError {
    #[error("failed to reload daemon settings: {0}")]
    Settings(#[from] SettingsError),
    #[error(transparent)]
    Store(#[from] StoreError),
}

struct TrackedEvaluation {
    evaluation: AbortHandle,
    monitor: JoinHandle<()>,
}

struct EvaluationWorkerInner {
    settings: RuntimeSettings,
    store: SqliteStore,
    executor: EvaluationExecutor,
    shutting_down: Arc<AtomicBool>,
    tasks: Mutex<Vec<TrackedEvaluation>>,
}

#[derive(Clone)]
pub(crate) struct EvaluationWorker {
    inner: Arc<EvaluationWorkerInner>,
}

impl fmt::Debug for EvaluationWorker {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("EvaluationWorker")
            .finish_non_exhaustive()
    }
}

impl EvaluationWorker {
    pub(crate) fn new(settings: RuntimeSettings, store: SqliteStore) -> Self {
        let executor: EvaluationExecutor = Arc::new(|settings, store, request_id| {
            Box::pin(evaluate_request(settings, store, request_id))
        });
        Self::with_executor(settings, store, executor)
    }

    #[cfg(test)]
    fn new_with_executor(
        settings: PlanktonSettings,
        store: SqliteStore,
        executor: EvaluationExecutor,
    ) -> Self {
        Self::with_executor(RuntimeSettings::fixed(settings), store, executor)
    }

    fn with_executor(
        settings: RuntimeSettings,
        store: SqliteStore,
        executor: EvaluationExecutor,
    ) -> Self {
        Self {
            inner: Arc::new(EvaluationWorkerInner {
                settings,
                store,
                executor,
                shutting_down: Arc::new(AtomicBool::new(false)),
                tasks: Mutex::new(Vec::new()),
            }),
        }
    }

    pub(crate) async fn recover_stale(&self) -> Result<(), StoreError> {
        self.inner
            .store
            .recover_stale_operations(Utc::now(), chrono::Duration::zero())
            .await?;
        Ok(())
    }

    pub(crate) async fn resume_queued(&self) -> Result<(), StoreError> {
        for request_id in self
            .inner
            .store
            .list_queued_evaluation_request_ids()
            .await?
        {
            self.spawn(request_id);
        }
        Ok(())
    }

    pub(crate) fn spawn(&self, request_id: String) {
        let settings = self.inner.settings.clone();
        let store = self.inner.store.clone();
        let shutting_down = self.inner.shutting_down.clone();
        let executor = self.inner.executor.clone();
        let evaluation_store = store.clone();
        let evaluation_request_id = request_id.clone();
        let evaluation = tokio::spawn(async move {
            let settings = settings.current()?;
            executor(settings, evaluation_store, evaluation_request_id)
                .await
                .map_err(EvaluationRunError::Store)
        });
        let evaluation_abort = evaluation.abort_handle();
        let monitor = tokio::spawn(async move {
            let failure = match evaluation.await {
                Ok(Ok(())) => None,
                Ok(Err(EvaluationRunError::Settings(error))) => Some((
                    ErrorCode::Internal,
                    format!("asynchronous request evaluation failed: {error}"),
                )),
                Ok(Err(EvaluationRunError::Store(error))) => Some((
                    ErrorCode::StorageFailed,
                    format!("asynchronous request evaluation failed: {error}"),
                )),
                Err(error) if error.is_panic() => Some((
                    ErrorCode::Internal,
                    format!("asynchronous request evaluation panicked: {error}"),
                )),
                Err(error) if error.is_cancelled() && shutting_down.load(Ordering::Acquire) => None,
                Err(error) => Some((
                    ErrorCode::Internal,
                    format!("asynchronous request evaluation was cancelled: {error}"),
                )),
            };
            if let Some((code, message)) = failure {
                finalize_worker_failure(&store, &request_id, code, message).await;
            }
        });
        let tracked = TrackedEvaluation {
            evaluation: evaluation_abort,
            monitor,
        };
        match self.inner.tasks.lock() {
            Ok(mut tasks) => {
                tasks.retain(|task| !task.monitor.is_finished());
                tasks.push(tracked);
            }
            Err(poisoned) => {
                let mut tasks = poisoned.into_inner();
                tasks.retain(|task| !task.monitor.is_finished());
                tasks.push(tracked);
                warn!("evaluation task registry mutex was poisoned");
            }
        }
    }

    pub(crate) async fn shutdown(&self) -> Result<(), StoreError> {
        self.inner.shutting_down.store(true, Ordering::Release);
        let tasks = match self.inner.tasks.lock() {
            Ok(mut tasks) => std::mem::take(&mut *tasks),
            Err(poisoned) => std::mem::take(&mut *poisoned.into_inner()),
        };
        for task in &tasks {
            task.evaluation.abort();
        }
        for task in tasks {
            let _ = task.monitor.await;
        }
        self.inner
            .store
            .recover_stale_operations(Utc::now(), chrono::Duration::zero())
            .await?;
        Ok(())
    }

    pub(crate) fn abort_now(&self) {
        self.inner.shutting_down.store(true, Ordering::Release);
        let mut tasks = match self.inner.tasks.lock() {
            Ok(tasks) => tasks,
            Err(poisoned) => poisoned.into_inner(),
        };
        for task in tasks.drain(..) {
            task.evaluation.abort();
            task.monitor.abort();
        }
    }
}

async fn finalize_worker_failure(
    store: &SqliteStore,
    request_id: &str,
    code: ErrorCode,
    message: String,
) {
    let interruption = store.interrupt_evaluation(request_id, Utc::now()).await;
    if let Err(error) = &interruption {
        error!(
            request_id,
            error = %error,
            "failed to persist interrupted evaluation state"
        );
    }
    let internal_message = match interruption {
        Ok(_) => message,
        Err(error) => format!("{message}; interruption persistence failed: {error}"),
    };
    let diagnostic = PlanktonError {
        code,
        user_message:
            "automatic request evaluation was interrupted; human review remains available".into(),
        internal_message: Some(internal_message),
        public_context: BTreeMap::from([("request_id".into(), request_id.to_string())]),
        internal_context: BTreeMap::new(),
        severity: ErrorSeverity::Error,
        retryable: true,
        timestamp: Utc::now(),
        correlation_id: Uuid::new_v4(),
        source: ErrorSource::Daemon,
    };
    if let Err(error) = store.record_diagnostic_error(&diagnostic).await {
        error!(
            request_id,
            error = %error,
            "failed to persist asynchronous evaluation diagnostic"
        );
    }
}

async fn evaluate_request(
    settings: PlanktonSettings,
    store: SqliteStore,
    request_id: String,
) -> Result<(), StoreError> {
    let Some(request) = store.claim_evaluation(&request_id, Utc::now()).await? else {
        return Ok(());
    };
    let provider_input =
        request
            .provider_input
            .as_ref()
            .ok_or_else(|| StoreError::InvalidStoredValue {
                field: "provider_input_json",
                value: format!("queued evaluation {request_id} has no provider input"),
            })?;
    let (progress_tx, mut progress_rx) = mpsc::unbounded_channel();
    let suggestion = request_llm_suggestion_with_progress(
        &settings,
        request.policy_mode,
        provider_input,
        Some(progress_tx),
    );
    tokio::pin!(suggestion);
    let mut heartbeat = tokio::time::interval(HEARTBEAT_INTERVAL);
    let mut decision_finalized = false;
    let mut journal = match ReviewJournal::create(&settings, &request_id).await {
        Ok(journal) => Some(journal),
        Err(error) => {
            warn!(request_id, error = %error, "failed to create durable review journal");
            None
        }
    };

    loop {
        tokio::select! {
            suggestion = &mut suggestion => {
                if decision_finalized {
                    store.update_evaluation_details(&request_id, suggestion.clone()).await?;
                } else {
                    store.finalize_evaluation(&request_id, suggestion.clone()).await?;
                }
                if let Some(journal) = &journal {
                    if let Err(error) = journal.complete(&suggestion).await {
                        warn!(request_id, error = %error, "failed to complete durable review journal");
                    }
                }
                break;
            },
            progress = progress_rx.recv() => {
                match progress {
                    Some(LlmSuggestionProgress::DecisionReady(suggestion)) if !decision_finalized => {
                        store.finalize_evaluation(&request_id, suggestion.clone()).await?;
                        if let Some(journal) = &journal {
                            if let Err(error) = journal.record_decision(&suggestion).await {
                                warn!(request_id, error = %error, "failed to persist review decision journal");
                            }
                        }
                        decision_finalized = true;
                    }
                    Some(LlmSuggestionProgress::DecisionReady(suggestion) | LlmSuggestionProgress::DetailsUpdated(suggestion)) if decision_finalized => {
                        store.update_evaluation_details(&request_id, suggestion.clone()).await?;
                        if let Some(journal) = &mut journal {
                            if let Err(error) = journal.record_details(&suggestion).await {
                                warn!(request_id, error = %error, "failed to persist review details journal");
                            }
                        }
                    }
                    Some(LlmSuggestionProgress::DecisionReady(_))
                    | Some(LlmSuggestionProgress::DetailsUpdated(_))
                    | None => {}
                }
            },
            _ = heartbeat.tick(), if !decision_finalized => {
                if let Err(error) = store.heartbeat_evaluation(&request_id, Utc::now()).await {
                    warn!(
                        request_id,
                        error = %error,
                        "failed to heartbeat asynchronous request evaluation"
                    );
                }
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use plankton_core::{
        load_settings, EvaluationState, PlanktonSettings, PolicyMode, RequestContext,
    };
    use plankton_store::{DiagnosticRecord, SqliteStore, StoreError};
    use tempfile::tempdir;

    use super::{EvaluationFuture, EvaluationWorker};

    async fn test_worker(
        executor: fn(PlanktonSettings, SqliteStore, String) -> EvaluationFuture,
    ) -> (tempfile::TempDir, EvaluationWorker, SqliteStore, String) {
        let temp = tempdir().expect("temporary directory");
        let mut settings = load_settings().expect("settings");
        settings.database_url = format!("sqlite://{}", temp.path().join("store.db").display());
        settings.provider_kind = "mock".into();
        let store = SqliteStore::new(&settings).await.expect("store");
        let request = store
            .submit_request(
                &settings,
                RequestContext::new(
                    "secret/worker-failure".into(),
                    "exercise supervised cleanup".into(),
                    "daemon-test".into(),
                ),
                PolicyMode::Assisted,
            )
            .await
            .expect("queued request");
        let worker =
            EvaluationWorker::new_with_executor(settings, store.clone(), Arc::new(executor));
        (temp, worker, store, request.id)
    }

    fn claim_then_missing_provider_input(
        _settings: PlanktonSettings,
        store: SqliteStore,
        request_id: String,
    ) -> EvaluationFuture {
        Box::pin(async move {
            store
                .claim_evaluation(&request_id, chrono::Utc::now())
                .await?
                .expect("queued request");
            Err(StoreError::InvalidStoredValue {
                field: "provider_input_json",
                value: "missing after claim".into(),
            })
        })
    }

    fn claim_then_panic(
        _settings: PlanktonSettings,
        store: SqliteStore,
        request_id: String,
    ) -> EvaluationFuture {
        Box::pin(async move {
            store
                .claim_evaluation(&request_id, chrono::Utc::now())
                .await?
                .expect("queued request");
            panic!("injected evaluator panic");
        })
    }

    fn claim_then_wait_forever(
        _settings: PlanktonSettings,
        store: SqliteStore,
        request_id: String,
    ) -> EvaluationFuture {
        Box::pin(async move {
            store
                .claim_evaluation(&request_id, chrono::Utc::now())
                .await?
                .expect("queued request");
            std::future::pending::<Result<(), StoreError>>().await
        })
    }

    async fn wait_for_state(store: &SqliteStore, request_id: &str, expected: EvaluationState) {
        tokio::time::timeout(std::time::Duration::from_secs(2), async {
            loop {
                let request = store
                    .get_request(request_id)
                    .await
                    .expect("request")
                    .request;
                if request.evaluation_state == expected {
                    break;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("worker should reach expected state");
    }

    async fn wait_until_interrupted(store: &SqliteStore, request_id: &str) {
        wait_for_state(store, request_id, EvaluationState::Interrupted).await;
    }

    async fn wait_for_diagnostics(store: &SqliteStore) -> Vec<DiagnosticRecord> {
        tokio::time::timeout(std::time::Duration::from_secs(2), async {
            loop {
                let diagnostics = store
                    .list_diagnostic_errors(false, 20)
                    .await
                    .expect("diagnostics");
                if !diagnostics.is_empty() {
                    break diagnostics;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("worker diagnostic should become visible")
    }

    #[tokio::test]
    async fn worker_error_after_claim_interrupts_request_and_records_diagnostic() {
        let (_temp, worker, store, request_id) =
            test_worker(claim_then_missing_provider_input).await;

        worker.spawn(request_id.clone());
        wait_until_interrupted(&store, &request_id).await;

        let diagnostics = wait_for_diagnostics(&store).await;
        assert_eq!(diagnostics.len(), 1);
        assert!(diagnostics[0]
            .error
            .internal_message
            .as_deref()
            .is_some_and(|message| message.contains("provider_input_json")));
        worker.shutdown().await.expect("shutdown");
    }

    #[tokio::test]
    async fn worker_panic_after_claim_interrupts_request_and_records_diagnostic() {
        let (_temp, worker, store, request_id) = test_worker(claim_then_panic).await;

        worker.spawn(request_id.clone());
        wait_until_interrupted(&store, &request_id).await;

        let diagnostics = wait_for_diagnostics(&store).await;
        assert_eq!(diagnostics.len(), 1);
        assert!(diagnostics[0]
            .error
            .internal_message
            .as_deref()
            .is_some_and(|message| message.contains("panic")));
        worker.shutdown().await.expect("shutdown");
    }

    #[tokio::test]
    async fn worker_cancellation_interrupts_request_before_shutdown_returns() {
        let (_temp, worker, store, request_id) = test_worker(claim_then_wait_forever).await;
        worker.spawn(request_id.clone());
        wait_for_state(&store, &request_id, EvaluationState::Running).await;

        worker.shutdown().await.expect("shutdown");

        let request = store
            .get_request(&request_id)
            .await
            .expect("request")
            .request;
        assert_eq!(request.evaluation_state, EvaluationState::Interrupted);
        let diagnostics = store
            .list_diagnostic_errors(false, 20)
            .await
            .expect("diagnostics");
        assert!(
            diagnostics.is_empty(),
            "clean shutdown cancellation should use recovery without an error diagnostic"
        );
    }
}
