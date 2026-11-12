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
    request_llm_batch_suggestions, request_llm_suggestion_with_progress, BatchReviewInput,
    LlmSuggestionProgress, PlanktonSettings, SettingsError, MAX_REVIEW_BATCH_SIZE,
};
use plankton_protocol::error::{ErrorCode, ErrorSeverity, ErrorSource, PlanktonError};
use plankton_store::{SqliteStore, StoreError};
use tokio::sync::{mpsc, Semaphore};
use tokio::task::{AbortHandle, JoinHandle};
use tracing::{error, warn};
use uuid::Uuid;

use crate::{review_journal::ReviewJournal, runtime_settings::RuntimeSettings};

const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(1);
const BATCH_WINDOW: Duration = Duration::from_secs(1);

type EvaluationFuture = Pin<Box<dyn Future<Output = Result<(), StoreError>> + Send + 'static>>;
type EvaluationExecutor = Arc<
    dyn Fn(PlanktonSettings, SqliteStore, Vec<String>) -> EvaluationFuture + Send + Sync + 'static,
>;

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
    pending: Mutex<Vec<String>>,
    windows: Mutex<Vec<JoinHandle<()>>>,
    permits: Arc<Semaphore>,
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
        let executor: EvaluationExecutor = Arc::new(|settings, store, request_ids| {
            Box::pin(evaluate_requests(settings, store, request_ids))
        });
        Self::with_executor(settings, store, executor)
    }

    #[cfg(test)]
    fn new_with_executor(
        settings: PlanktonSettings,
        store: SqliteStore,
        executor: Arc<
            dyn Fn(PlanktonSettings, SqliteStore, String) -> EvaluationFuture
                + Send
                + Sync
                + 'static,
        >,
    ) -> Self {
        let batch_executor: EvaluationExecutor = Arc::new(move |settings, store, requests| {
            let executor = executor.clone();
            Box::pin(async move {
                for request in requests {
                    executor(settings.clone(), store.clone(), request).await?;
                }
                Ok(())
            })
        });
        Self::with_executor(RuntimeSettings::fixed(settings), store, batch_executor)
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
                pending: Mutex::new(Vec::new()),
                windows: Mutex::new(Vec::new()),
                permits: Arc::new(Semaphore::new(2)),
            }),
        }
    }

    pub(crate) async fn recover_stale(&self) -> Result<(), StoreError> {
        self.inner
            .store
            .recover_stale_operations(Utc::now(), chrono::Duration::zero())
            .await?;
        self.inner
            .store
            .recover_interrupted_review_details(Utc::now())
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
        if self.inner.shutting_down.load(Ordering::Acquire) {
            return;
        }
        let mut pending = self
            .inner
            .pending
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        if pending.contains(&request_id) {
            return;
        }
        let first = pending.is_empty();
        pending.push(request_id);
        if !first {
            return;
        }
        let worker = self.clone();
        let deadline = tokio::time::Instant::now() + BATCH_WINDOW;
        let window = tokio::spawn(async move {
            // Fixed from the first arrival. Later arrivals never reset this timer.
            tokio::time::sleep_until(deadline).await;
            let requests = std::mem::take(
                &mut *worker
                    .inner
                    .pending
                    .lock()
                    .unwrap_or_else(|error| error.into_inner()),
            );
            for batch in requests.chunks(MAX_REVIEW_BATCH_SIZE) {
                worker.spawn_batch(batch.to_vec());
            }
        });
        let mut windows = self
            .inner
            .windows
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        windows.retain(|window| !window.is_finished());
        if self.inner.shutting_down.load(Ordering::Acquire) {
            window.abort();
        } else {
            windows.push(window);
        }
    }

    fn spawn_batch(&self, request_ids: Vec<String>) {
        if self.inner.shutting_down.load(Ordering::Acquire) {
            return;
        }
        let settings = self.inner.settings.clone();
        let store = self.inner.store.clone();
        let shutting_down = self.inner.shutting_down.clone();
        let executor = self.inner.executor.clone();
        let evaluation_store = store.clone();
        let evaluation_request_ids = request_ids.clone();
        let permits = self.inner.permits.clone();
        let evaluation = tokio::spawn(async move {
            let _permit = permits
                .acquire_owned()
                .await
                .expect("evaluation semaphore stays open");
            let settings = settings.current()?;
            executor(settings, evaluation_store, evaluation_request_ids)
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
                for request_id in request_ids {
                    finalize_worker_failure(&store, &request_id, code, message.clone()).await;
                }
            }
        });
        let tracked = TrackedEvaluation {
            evaluation: evaluation_abort,
            monitor,
        };
        let mut tasks = self
            .inner
            .tasks
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        tasks.retain(|task| !task.monitor.is_finished());
        if self.inner.shutting_down.load(Ordering::Acquire) {
            tracked.evaluation.abort();
            tracked.monitor.abort();
        } else {
            tasks.push(tracked);
        }
    }

    pub(crate) async fn shutdown(&self) -> Result<(), StoreError> {
        self.inner.shutting_down.store(true, Ordering::Release);
        let windows = std::mem::take(
            &mut *self
                .inner
                .windows
                .lock()
                .unwrap_or_else(|error| error.into_inner()),
        );
        for window in &windows {
            window.abort();
        }
        for window in windows {
            let _ = window.await;
        }
        self.inner
            .pending
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .clear();
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
        self.inner
            .store
            .recover_interrupted_review_details(Utc::now())
            .await?;
        Ok(())
    }

    pub(crate) fn abort_now(&self) {
        self.inner.shutting_down.store(true, Ordering::Release);
        for window in self
            .inner
            .windows
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .drain(..)
        {
            window.abort();
        }
        self.inner
            .pending
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .clear();
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

async fn evaluate_requests(
    settings: PlanktonSettings,
    store: SqliteStore,
    request_ids: Vec<String>,
) -> Result<(), StoreError> {
    // Admitted consumers already identify every credential explicitly. A single
    // credential needs the same evidence-bearing result as a multi-credential batch,
    // not a mandatory second audit. Retain legacy selector discovery for old requests.
    let legacy_single = if let [id] = request_ids.as_slice() {
        let request = store.get_request(id).await?.request;
        !request.provider_input.as_ref().is_some_and(|input| {
            matches!(
                input
                    .sanitized_context
                    .request_metadata
                    .get("execution_admission")
                    .map(String::as_str),
                Some("daemon_command_v1" | "daemon_python_file_v1")
            )
        })
    } else {
        false
    };
    if legacy_single || !matches!(settings.provider_kind.as_str(), "acp" | "acp_codex") {
        for id in request_ids {
            evaluate_request(settings.clone(), store.clone(), id).await?;
        }
        return Ok(());
    }
    let mut inputs = Vec::new();
    let mut journals = BTreeMap::new();
    for id in request_ids {
        let Some(request) = store.claim_evaluation(&id, Utc::now()).await? else {
            continue;
        };
        let input = request
            .provider_input
            .ok_or_else(|| StoreError::InvalidStoredValue {
                field: "provider_input_json",
                value: format!("queued evaluation {id} has no provider input"),
            })?;
        match ReviewJournal::create(&settings, &id).await {
            Ok(journal) => {
                journals.insert(id.clone(), journal);
            }
            Err(error) => warn!(request_id = id, %error, "failed to create batch review journal"),
        }
        inputs.push(BatchReviewInput {
            request_id: id,
            policy_mode: request.policy_mode,
            input,
        });
    }
    if inputs.is_empty() {
        return Ok(());
    }
    let suggestions = request_llm_batch_suggestions(&settings, &inputs);
    tokio::pin!(suggestions);
    let mut heartbeat = tokio::time::interval(HEARTBEAT_INTERVAL);
    let suggestions = loop {
        tokio::select! {
            suggestions = &mut suggestions => break suggestions,
            _ = heartbeat.tick() => {
                for input in &inputs { store.heartbeat_evaluation(&input.request_id, Utc::now()).await?; }
            }
        }
    };
    for input in &inputs {
        let suggestion =
            suggestions
                .get(&input.request_id)
                .ok_or_else(|| StoreError::InvalidStoredValue {
                    field: "batch_review",
                    value: format!("missing result for {}", input.request_id),
                })?;
        // The existing store rechecks this field's own policy and concurrent
        // human resolution. No approval is copied from a neighboring request.
        store
            .finalize_evaluation(&input.request_id, suggestion.clone())
            .await?;
        if let Some(journal) = journals.get(&input.request_id) {
            if let Err(error) = journal.complete(suggestion).await {
                warn!(request_id = input.request_id, %error, "failed to persist batch review journal");
            }
        }
    }
    Ok(())
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

    #[cfg(unix)]
    #[tokio::test]
    async fn production_batch_repairs_once_and_preserves_independent_database_results() {
        for mode in [
            "complete",
            "direct",
            "missing",
            "single-file",
            "single-direct",
            "misplaced-empty",
            "repair-direct",
            "repair-file",
            "partial-invalid",
            "missing-file",
            "transport",
            "empty-direct",
            "unknown",
        ] {
            let temp = tempdir().unwrap();
            let script = temp.path().join("batch_acp.py");
            let log = temp.path().join("prompts.jsonl");
            std::fs::write(&script, include_str!("../tests/fixtures/batch_acp.py")).unwrap();
            let settings = PlanktonSettings {
                database_url: format!("sqlite://{}", temp.path().join("batch.db").display()),
                provider_kind: "acp".into(),
                acp_profile: plankton_protocol::acp::AcpProfile {
                    agent_kind: plankton_protocol::acp::AgentKind::Custom,
                    version_mode: plankton_protocol::acp::VersionMode::Custom,
                    version: None,
                    program: Some("/usr/bin/python3".into()),
                    args: vec![
                        "-u".into(),
                        script.display().to_string(),
                        log.display().to_string(),
                        mode.into(),
                    ],
                    session_options: Default::default(),
                },
                acp_timeout_secs: 5,
                ..PlanktonSettings::default()
            };
            let store = SqliteStore::new(&settings).await.unwrap();
            let worker = EvaluationWorker::new(
                crate::runtime_settings::RuntimeSettings::fixed(settings.clone()),
                store.clone(),
            );
            let mut requests = Vec::new();
            let resources: &[&str] = if mode.starts_with("single") {
                &["secret/allow"]
            } else {
                &["secret/allow", "secret/deny"]
            };
            for resource in resources {
                let mut context =
                    RequestContext::new((*resource).into(), "batch fixture".into(), "test".into());
                context
                    .metadata
                    .insert("execution_admission".into(), "daemon_command_v1".into());
                let request = store
                    .submit_request(&settings, context, PolicyMode::LlmAutomatic)
                    .await
                    .unwrap();
                worker.spawn(request.id.clone());
                requests.push(request);
            }
            let finished = tokio::time::timeout(std::time::Duration::from_secs(10), async {
                loop {
                    let mut finished = Vec::new();
                    for request in &requests {
                        finished.push(store.get_request(&request.id).await.unwrap().request);
                    }
                    if finished
                        .iter()
                        .all(|request| request.llm_suggestion.is_some())
                    {
                        break finished;
                    }
                    tokio::time::sleep(std::time::Duration::from_millis(20)).await;
                }
            })
            .await
            .unwrap();
            let calls = std::fs::read_to_string(log).unwrap();
            let expected_calls = if matches!(
                mode,
                "missing" | "partial-invalid" | "missing-file" | "empty-direct"
            ) || mode.starts_with("repair")
            {
                2
            } else {
                1
            };
            let calls: Vec<serde_json::Value> = calls
                .lines()
                .map(|line| serde_json::from_str(line).unwrap())
                .collect();
            assert_eq!(calls.len(), expected_calls, "{mode}");
            for (index, call) in calls.iter().enumerate() {
                assert_eq!(
                    call["ids"],
                    serde_json::json!(requests
                        .iter()
                        .map(|request| request.id.clone())
                        .collect::<Vec<_>>())
                );
                assert_eq!(
                    call["session"], calls[0]["session"],
                    "repair must reuse the session"
                );
                assert_eq!(call["repair"], index > 0);
            }
            for request in &finished {
                let trace = request
                    .llm_suggestion
                    .as_ref()
                    .unwrap()
                    .provider_trace
                    .as_ref()
                    .unwrap();
                assert_eq!(
                    trace.decision_attempts.len(),
                    expected_calls,
                    "{mode}: audit attempts"
                );
                if expected_calls == 2 {
                    assert!(trace.decision_attempts[0].validation_error.is_some());
                    assert!(
                        mode == "empty-direct"
                            || !trace.decision_attempts[0].raw_response.is_empty()
                    );
                }
            }
            if !matches!(mode, "missing" | "transport" | "empty-direct") {
                assert_eq!(
                    finished[0].approval_status,
                    plankton_core::ApprovalStatus::Approved,
                    "{mode}: {:?}",
                    finished[0].llm_suggestion
                );
                if mode == "partial-invalid" {
                    assert_eq!(
                        finished[1].approval_status,
                        plankton_core::ApprovalStatus::Pending
                    );
                    assert!(finished[1].llm_suggestion.as_ref().unwrap().error.is_some());
                } else if finished.len() == 2 {
                    assert_eq!(
                        finished[1].approval_status,
                        if mode == "unknown" {
                            plankton_core::ApprovalStatus::Pending
                        } else {
                            plankton_core::ApprovalStatus::Rejected
                        },
                        "{mode}"
                    );
                    assert!(finished[1].llm_suggestion.as_ref().unwrap().error.is_none());
                    assert_eq!(
                        finished[0]
                            .llm_suggestion
                            .as_ref()
                            .unwrap()
                            .provider_trace
                            .as_ref()
                            .unwrap()
                            .session_id,
                        finished[1]
                            .llm_suggestion
                            .as_ref()
                            .unwrap()
                            .provider_trace
                            .as_ref()
                            .unwrap()
                            .session_id
                    );
                }
            } else {
                for request in finished {
                    assert_ne!(
                        request.approval_status,
                        plankton_core::ApprovalStatus::Approved
                    );
                    let error = request.llm_suggestion.unwrap().error.unwrap();
                    let expected = match mode {
                        "missing" => "omitted",
                        "transport" => "fixture provider unavailable",
                        "empty-direct" => "did not include any message content",
                        _ => unreachable!(),
                    };
                    assert!(error.to_lowercase().contains(expected), "{mode}: {error}");
                }
            }
            worker.shutdown().await.unwrap();
        }
    }

    async fn batch_worker(
        executor: super::EvaluationExecutor,
    ) -> (tempfile::TempDir, EvaluationWorker) {
        let temp = tempdir().unwrap();
        let settings = PlanktonSettings {
            database_url: format!("sqlite://{}", temp.path().join("batch.db").display()),
            provider_kind: "mock".into(),
            ..PlanktonSettings::default()
        };
        let store = SqliteStore::new(&settings).await.unwrap();
        let worker = EvaluationWorker::with_executor(
            crate::runtime_settings::RuntimeSettings::fixed(settings),
            store,
            executor,
        );
        // SQLite runs on blocking threads; pause only after its real IO finishes.
        tokio::time::pause();
        (temp, worker)
    }

    #[tokio::test]
    async fn one_second_window_is_fixed_deduplicated_and_preserves_later_requests() {
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let executor: super::EvaluationExecutor = Arc::new(move |_, _, ids| {
            let tx = tx.clone();
            Box::pin(async move {
                tx.send(ids).unwrap();
                Ok(())
            })
        });
        let (_temp, worker) = batch_worker(executor).await;
        worker.spawn("a".into());
        tokio::time::advance(std::time::Duration::from_millis(900)).await;
        assert!(rx.try_recv().is_err());
        worker.spawn("b".into());
        worker.spawn("a".into());
        tokio::time::advance(std::time::Duration::from_millis(100)).await;
        assert_eq!(rx.recv().await.unwrap(), vec!["a", "b"]);
        worker.spawn("c".into());
        tokio::time::advance(std::time::Duration::from_millis(999)).await;
        assert!(rx.try_recv().is_err());
        tokio::time::advance(std::time::Duration::from_millis(1)).await;
        assert_eq!(rx.recv().await.unwrap(), vec!["c"]);
        tokio::time::resume();
        worker.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn batches_overlap_with_two_workers_and_never_exceed_capacity() {
        let (started_tx, mut started_rx) = tokio::sync::mpsc::unbounded_channel();
        let release = Arc::new(tokio::sync::Semaphore::new(0));
        let executor: super::EvaluationExecutor = Arc::new({
            let release = release.clone();
            move |_, _, ids| {
                let release = release.clone();
                let started = started_tx.clone();
                Box::pin(async move {
                    started.send(ids).unwrap();
                    release.acquire().await.unwrap().forget();
                    Ok(())
                })
            }
        });
        let (_temp, worker) = batch_worker(executor).await;
        for index in 0..17 {
            worker.spawn(format!("request-{index}"));
        }
        tokio::time::advance(std::time::Duration::from_secs(1)).await;
        let first = started_rx.recv().await.unwrap();
        let second = started_rx.recv().await.unwrap();
        assert_eq!(first.len(), 8);
        assert_eq!(second.len(), 8);
        assert!(
            started_rx.try_recv().is_err(),
            "third batch must wait for capacity"
        );
        release.add_permits(1);
        assert_eq!(started_rx.recv().await.unwrap(), vec!["request-16"]);
        release.add_permits(2);
        tokio::time::resume();
        worker.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn shutdown_during_collecting_window_starts_no_review() {
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let executor: super::EvaluationExecutor = Arc::new(move |_, _, ids| {
            let tx = tx.clone();
            Box::pin(async move {
                tx.send(ids).unwrap();
                Ok(())
            })
        });
        let (_temp, worker) = batch_worker(executor).await;
        worker.spawn("a".into());
        tokio::time::resume();
        worker.shutdown().await.unwrap();
        tokio::time::pause();
        tokio::time::advance(std::time::Duration::from_secs(2)).await;
        assert!(rx.try_recv().is_err());
    }

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
