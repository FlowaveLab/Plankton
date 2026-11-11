use std::{
    collections::BTreeMap,
    process::Stdio,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex as TaskMutex,
    },
    time::{Duration, Instant},
};

use axum::{extract::State, Json};
use chrono::Utc;
use plankton_core::{
    default_value_resolver, execution::ExecutionPlan, ApprovalStatus, Decision, ValueResolver,
};
use plankton_protocol::{
    daemon::{RequestEnvelope, ResponseEnvelope},
    error::{ErrorCode, ErrorSeverity, ErrorSource, PlanktonError},
    resources::{
        ResourceAccessRequest, ResourceAccessStatusRequest, ResourceExecutionRequest,
        ResourceExecutionResponse, ResourceExecutionState,
    },
};
use plankton_store::{InterruptedOperation, OperationStatus};
use tokio::{
    io::{AsyncRead, AsyncReadExt, AsyncWriteExt},
    sync::Mutex,
};
use uuid::Uuid;

use super::resources::{
    admitted_access_resource, failure, resource_annotations, resource_backend_enabled, success,
};
use crate::server::ServerState;

const MAX_PLANS: usize = 128;
const MAX_RUNNING: usize = 4;
const PLAN_TTL: Duration = Duration::from_secs(30 * 60);
const OUTPUT_LIMIT: usize = 64 * 1024;

#[derive(Clone)]
pub(crate) struct ExecutionRegistry {
    entries: Arc<Mutex<BTreeMap<String, Entry>>>,
    tasks: Arc<TaskMutex<Vec<tokio::task::JoinHandle<()>>>>,
    shutting_down: Arc<AtomicBool>,
    preparations: Arc<tokio::sync::Semaphore>,
}

impl Default for ExecutionRegistry {
    fn default() -> Self {
        Self {
            entries: Default::default(),
            tasks: Default::default(),
            shutting_down: Default::default(),
            preparations: Arc::new(tokio::sync::Semaphore::new(MAX_RUNNING)),
        }
    }
}

impl std::fmt::Debug for ExecutionRegistry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("ExecutionRegistry")
    }
}

impl ExecutionRegistry {
    fn spawn(&self, future: impl std::future::Future<Output = ()> + Send + 'static) -> bool {
        let mut tasks = self
            .tasks
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if self.shutting_down.load(Ordering::Acquire) {
            return false;
        }
        tasks.retain(|task| !task.is_finished());
        tasks.push(tokio::spawn(future));
        true
    }

    pub(crate) fn abort_now(&self) {
        self.shutting_down.store(true, Ordering::Release);
        for task in self
            .tasks
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .iter()
        {
            task.abort();
        }
    }

    pub(crate) async fn shutdown(&self) {
        self.abort_now();
        let tasks = std::mem::take(
            &mut *self
                .tasks
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner()),
        );
        for task in tasks {
            let _ = task.await;
        }
        self.entries.lock().await.clear();
    }
}

#[derive(Clone)]
struct FieldReview {
    id: String,
    resource: String,
    env: Option<String>,
}

struct Entry {
    reviews: Vec<FieldReview>,
    plan: Option<ExecutionPlan>,
    response: ResourceExecutionResponse,
    created_at: Instant,
}

async fn expire_plans(state: &ServerState) {
    let expired = {
        let mut entries = state.executions.entries.lock().await;
        entries.retain(|_, entry| {
            matches!(
                entry.response.state,
                ResourceExecutionState::Pending | ResourceExecutionState::Running
            ) || entry.created_at.elapsed() < PLAN_TTL
        });
        entries
            .values_mut()
            .filter_map(|entry| {
                if entry.response.state != ResourceExecutionState::Pending
                    || entry.created_at.elapsed() < PLAN_TTL
                {
                    return None;
                }
                entry.plan = None;
                entry.response.state = ResourceExecutionState::Failed;
                entry.response.human_review_required = false;
                entry.response.pending_review_id = None;
                entry.response.error = Some("execution approval expired before execution".into());
                entry.created_at = Instant::now();
                Some(entry.reviews.clone())
            })
            .collect::<Vec<_>>()
    };
    for reviews in expired {
        discard_pending_reviews(state, &reviews, "Execution plan expired before execution").await;
    }
}

async fn discard_pending_reviews(state: &ServerState, reviews: &[FieldReview], reason: &str) {
    if let Ok(settings) = state.settings.current() {
        for review in reviews {
            // A resolved field retains its original audit decision.
            let _ = state
                .store
                .record_decision(
                    &settings,
                    &review.id,
                    Decision::Deny,
                    "execution-discard",
                    Some(reason.into()),
                )
                .await;
        }
    }
}

pub(crate) fn start_cleanup(state: ServerState) {
    state.executions.clone().spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(60));
        loop {
            interval.tick().await;
            expire_plans(&state).await;
        }
    });
}

pub(crate) async fn recover_abandoned_plans(
    store: &plankton_store::SqliteStore,
    settings: &plankton_core::PlanktonSettings,
) -> Result<(), plankton_store::StoreError> {
    for request in store.list_pending_requests().await? {
        if request
            .context
            .metadata
            .get("execution_admission")
            .map(String::as_str)
            .is_some_and(|admission| {
                matches!(admission, "daemon_python_file_v1" | "daemon_command_v1")
            })
        {
            store.record_decision(settings, &request.id, Decision::Deny, "execution-recovery", Some("Daemon restarted before this execution plan was consumed; the plan was discarded".into())).await?;
        }
    }
    Ok(())
}

pub async fn execute_resource(
    State(state): State<ServerState>,
    Json(request): Json<RequestEnvelope<ResourceExecutionRequest>>,
) -> Json<ResponseEnvelope<ResourceExecutionResponse>> {
    let correlation_id = request.correlation_id;
    if let Err(error) = request.validate_version() {
        return Json(failure(
            correlation_id,
            ErrorCode::ProtocolMismatch,
            error.to_string(),
            false,
        ));
    }
    if state.executions.shutting_down.load(Ordering::Acquire) {
        return Json(failure(
            correlation_id,
            ErrorCode::Conflict,
            "daemon is shutting down".into(),
            false,
        ));
    }
    expire_plans(&state).await;
    let payload = request.payload;
    let preparation = payload.clone();
    let permit = match state.executions.preparations.clone().try_acquire_owned() {
        Ok(permit) => permit,
        Err(_) => {
            return Json(failure(
                correlation_id,
                ErrorCode::Conflict,
                "consumer evidence collection is busy; try again later".into(),
                false,
            ))
        }
    };
    let plan = match tokio::task::spawn_blocking(move || {
        let _permit = permit;
        ExecutionPlan::prepare(&preparation)
    })
    .await
    {
        Ok(Ok(plan)) => plan,
        Ok(Err(error)) => {
            return Json(failure(
                correlation_id,
                ErrorCode::InvalidRequest,
                error,
                false,
            ))
        }
        Err(_) => {
            return Json(failure(
                correlation_id,
                ErrorCode::Internal,
                "consumer evidence collection failed".into(),
                false,
            ))
        }
    };
    let mut entries = state.executions.entries.lock().await;
    if entries.len() >= MAX_PLANS {
        let oldest_finished = entries
            .iter()
            .filter(|(_, entry)| {
                !matches!(
                    entry.response.state,
                    ResourceExecutionState::Pending | ResourceExecutionState::Running
                )
            })
            .min_by_key(|(_, entry)| entry.created_at)
            .map(|(id, _)| id.clone());
        if let Some(id) = oldest_finished {
            entries.remove(&id);
        } else {
            return Json(failure(
                correlation_id,
                ErrorCode::Conflict,
                "execution queue is full; finish existing requests first".into(),
                false,
            ));
        }
    }
    let plan_id = Uuid::new_v4().to_string();
    let bindings = std::iter::once((payload.resource_id.clone(), payload.credential_env))
        .chain(
            payload
                .additional_credentials
                .into_iter()
                .map(|field| (field.resource_id, Some(field.env))),
        )
        .collect::<Vec<_>>();
    let mut evidence = plan.evidence(&plan_id);
    if bindings.len() > 1 {
        evidence.insert(
            "execution_credential_bindings".into(),
            serde_json::to_string(&bindings).expect("bindings serialize"),
        );
    }
    let mut reviews = Vec::new();
    let mut pending_review_id = None;
    for (resource, env) in bindings {
        if let Some(name) = &env {
            evidence.insert("execution_credential_env".into(), name.clone());
        }
        let access = ResourceAccessRequest {
            resource_id: resource.clone(),
            reason: payload.reason.clone(),
            requested_by: payload.requested_by.clone(),
            script_path: plan.source_path().map(str::to_string),
            call_chain: Vec::new(),
            call_chain_details: Vec::new(),
            metadata: evidence.clone(),
        };
        let Json(submission) = admitted_access_resource(
            State(state.clone()),
            Json(RequestEnvelope {
                protocol_version: plankton_protocol::PROTOCOL_VERSION,
                correlation_id,
                payload: access,
            }),
            plan.call_chain_nodes(),
        )
        .await;
        match submission {
            ResponseEnvelope::Success { data, .. } => {
                if data.human_review_required && pending_review_id.is_none() {
                    pending_review_id = Some(data.request_id.clone());
                }
                reviews.push(FieldReview {
                    id: data.request_id,
                    resource,
                    env,
                });
            }
            ResponseEnvelope::Failure {
                protocol_version,
                correlation_id,
                error,
            } => {
                discard_pending_reviews(
                    &state,
                    &reviews,
                    "Another credential could not be admitted",
                )
                .await;
                return Json(ResponseEnvelope::Failure {
                    protocol_version,
                    correlation_id,
                    error,
                });
            }
        }
    }
    let request_id = reviews[0].id.clone();
    let response = ResourceExecutionResponse {
        request_id: request_id.clone(),
        resource_id: payload.resource_id,
        state: ResourceExecutionState::Pending,
        human_review_required: pending_review_id.is_some(),
        pending_review_id: pending_review_id.filter(|id| id != &request_id),
        exit_code: None,
        stdout: String::new(),
        stderr: String::new(),
        error: None,
    };
    entries.insert(
        request_id,
        Entry {
            reviews,
            plan: Some(plan),
            response: response.clone(),
            created_at: Instant::now(),
        },
    );
    Json(success(correlation_id, response))
}

pub async fn execution_status(
    State(state): State<ServerState>,
    Json(request): Json<RequestEnvelope<ResourceAccessStatusRequest>>,
) -> Json<ResponseEnvelope<ResourceExecutionResponse>> {
    let correlation_id = request.correlation_id;
    if let Err(error) = request.validate_version() {
        return Json(failure(
            correlation_id,
            ErrorCode::ProtocolMismatch,
            error.to_string(),
            false,
        ));
    }
    expire_plans(&state).await;
    let id = request.payload.request_id;
    let mut entries = state.executions.entries.lock().await;
    let running = entries
        .values()
        .filter(|entry| entry.response.state == ResourceExecutionState::Running)
        .count();
    let Some(entry) = entries.get_mut(&id) else {
        return Json(failure(correlation_id, ErrorCode::NotFound, "execution plan expired or the daemon restarted; execution is never replayed. Check the previous execution outcome before submitting again".into(), false));
    };
    if entry.response.state != ResourceExecutionState::Pending {
        return Json(success(correlation_id, entry.response.clone()));
    }
    let mut pending = false;
    entry.response.error = None;
    entry.response.human_review_required = false;
    entry.response.pending_review_id = None;
    for field in &entry.reviews {
        let review = match state.store.get_request(&field.id).await {
            Ok(review) => review.request,
            Err(_) => {
                return Json(failure(
                    correlation_id,
                    ErrorCode::StorageFailed,
                    "execution review is unavailable".into(),
                    false,
                ))
            }
        };
        match review.approval_status {
            ApprovalStatus::Rejected => {
                entry.plan = None;
                entry.response.state = ResourceExecutionState::Denied;
                entry.response.error = Some("credential execution was denied".into());
                entry.response.human_review_required = false;
                entry.response.pending_review_id = None;
                entry.created_at = Instant::now();
                discard_pending_reviews(
                    &state,
                    &entry.reviews,
                    "Another credential in this execution was denied",
                )
                .await;
                return Json(success(correlation_id, entry.response.clone()));
            }
            ApprovalStatus::Approved => {}
            ApprovalStatus::Pending => {
                pending = true;
                if review.human_review_required() && !entry.response.human_review_required {
                    entry.response.human_review_required = true;
                    if let Some(message) = review.review_failure_message("en") {
                        entry.response.error = Some(format!("{}: {message}", field.resource));
                    }
                    entry.response.pending_review_id = (field.id != id).then(|| field.id.clone());
                }
            }
        }
    }
    if !pending && running < MAX_RUNNING {
        let Some(plan) = entry.plan.take() else {
            return Json(failure(
                correlation_id,
                ErrorCode::Conflict,
                "execution plan has already been consumed".into(),
                false,
            ));
        };
        entry.response.state = ResourceExecutionState::Running;
        entry.response.human_review_required = false;
        let reviews = entry.reviews.clone();
        let execution_id = id.clone();
        let execution_state = state.clone();
        if !state.executions.spawn(async move {
            complete_execution(execution_state, execution_id, reviews, plan).await;
        }) {
            entry.response.state = ResourceExecutionState::Failed;
            entry.response.error = Some("daemon stopped before execution started".into());
        }
    }
    Json(success(correlation_id, entry.response.clone()))
}

async fn complete_execution(
    state: ServerState,
    id: String,
    reviews: Vec<FieldReview>,
    plan: ExecutionPlan,
) {
    let operation_id = format!("execution:{id}");
    let now = Utc::now();
    let operation = InterruptedOperation {
        id: operation_id.clone(),
        operation_kind: "credential_execution".into(),
        operation_key: id.clone(),
        state: serde_json::json!({"request_id": id, "phase": "executing"}),
        status: OperationStatus::Running,
        started_at: now,
        heartbeat_at: now,
        finished_at: None,
    };
    let outcome = async {
        state
            .store
            .start_operation(&operation)
            .await
            .map_err(|_| "could not record execution start".to_string())?;
        let check = plan.clone();
        tokio::task::spawn_blocking(move || check.verify_unchanged())
            .await
            .map_err(|_| "could not verify reviewed files")??;
        let mut credentials = Vec::with_capacity(reviews.len());
        for field in &reviews {
            if !resource_backend_enabled(&state.store, &field.resource)
                .await
                .map_err(|_| "credential backend availability check failed")?
            {
                return Err("credential backend is disabled".into());
            }
            let resource = field.resource.clone();
            let allow_output = output_surfaces_allow_secret(&state, &field.id, &resource).await?;
            let value =
                tokio::task::spawn_blocking(move || default_value_resolver()?.resolve(&resource))
                    .await
                    .map_err(|_| "credential resolution failed")?
                    .map_err(|_| "credential resolution failed")?;
            credentials.push(ResolvedCredential {
                env: field.env.clone(),
                value,
                allow_output,
            });
        }
        // Resolving an external backend can take time. Recheck immediately before
        // launch as well; this does not isolate mutable imports or same-user races.
        let check = plan.clone();
        tokio::task::spawn_blocking(move || check.verify_unchanged())
            .await
            .map_err(|_| "could not verify reviewed files")??;
        run_consumer(&plan, &credentials).await
    }
    .await;
    let (exit_code, stdout, stderr, error) = match outcome {
        Ok(output) => (
            output.exit_code,
            output.stdout,
            output.stderr,
            output.error.or_else(|| match output.exit_code {
                Some(0) => None,
                Some(code) => Some(format!("consumer exited with status {code}")),
                None => Some("consumer terminated without an exit code".into()),
            }),
        ),
        Err(error) => (None, String::new(), String::new(), Some(error)),
    };
    let failed = error.is_some();
    let audit = state
        .store
        .finish_operation(
            &operation_id,
            if failed {
                OperationStatus::Failed
            } else {
                OperationStatus::Completed
            },
            Utc::now(),
        )
        .await;
    if let Some(message) = error.as_ref() {
        let _ = state
            .store
            .record_diagnostic_error(&PlanktonError {
                code: ErrorCode::BackendFailed,
                user_message: message.clone(),
                internal_message: None,
                public_context: BTreeMap::from([
                    ("request_id".into(), id.clone()),
                    ("phase".into(), "consumer_execution".into()),
                ]),
                internal_context: BTreeMap::new(),
                severity: ErrorSeverity::Error,
                retryable: false,
                timestamp: Utc::now(),
                correlation_id: Uuid::new_v4(),
                source: ErrorSource::Daemon,
            })
            .await;
    }
    let mut entries = state.executions.entries.lock().await;
    if let Some(entry) = entries.get_mut(&id) {
        entry.response.state = if failed || audit.is_err() {
            ResourceExecutionState::Failed
        } else {
            ResourceExecutionState::Completed
        };
        entry.response.exit_code = exit_code;
        entry.response.stdout = stdout;
        entry.response.stderr = stderr;
        entry.response.error = error.or_else(|| {
            audit.err().map(|_| {
                "execution finished but audit completion failed; do not retry blindly".into()
            })
        });
        entry.created_at = Instant::now();
    }
}

async fn output_surfaces_allow_secret(
    state: &ServerState,
    id: &str,
    resource: &str,
) -> Result<bool, String> {
    use plankton_protocol::exposure::CredentialExposureSurface::{LlmContext, TerminalLog};
    let request = state
        .store
        .get_request(id)
        .await
        .map_err(|_| "execution policy snapshot is unavailable")?
        .request;
    let mut catalog = tokio::task::spawn_blocking(plankton_core::list_local_secret_catalog)
        .await
        .map_err(|_| "execution policy is unavailable")?
        .map_err(|_| "execution policy is unavailable")?;
    let current_fingerprint = plankton_core::exposure::catalog_exposure_snapshots(&catalog)
        .into_iter()
        .find(|entry| entry.resource_id == resource)
        .map(|entry| entry.fingerprint);
    if request
        .context
        .metadata
        .get(plankton_core::exposure::EXPOSURE_CATALOG_FINGERPRINT_KEY)
        != current_fingerprint.as_ref()
    {
        return Err(
            "credential source or configured policy changed after review; execution was refused"
                .into(),
        );
    }
    state
        .store
        .apply_remembered_exposure_policies(&mut catalog)
        .await
        .map_err(|_| "execution policy is unavailable")?;
    let (_, metadata) = resource_annotations(catalog, resource);
    let policy = plankton_core::exposure_policy_from_metadata(&metadata);
    Ok(policy.surface(LlmContext).max_level == 1 && policy.surface(TerminalLog).max_level == 1)
}

struct ConsumerOutput {
    exit_code: Option<i32>,
    stdout: String,
    stderr: String,
    error: Option<String>,
}

async fn read_bounded(mut stream: impl AsyncRead + Unpin) -> Result<Vec<u8>, String> {
    let mut bytes = Vec::new();
    (&mut stream)
        .take((OUTPUT_LIMIT + 1) as u64)
        .read_to_end(&mut bytes)
        .await
        .map_err(|_| "consumer output could not be read")?;
    if bytes.len() > OUTPUT_LIMIT {
        return Err("consumer output exceeded 64 KiB per stream".into());
    }
    Ok(bytes)
}

#[cfg(unix)]
struct ProcessGroup(u32);
#[cfg(unix)]
impl Drop for ProcessGroup {
    fn drop(&mut self) {
        // The child owns a newly created process group; terminate remaining descendants.
        unsafe {
            libc::kill(-(self.0 as i32), libc::SIGKILL);
        }
    }
}

struct ResolvedCredential {
    env: Option<String>,
    value: String,
    allow_output: bool,
}

async fn run_consumer(
    plan: &ExecutionPlan,
    credentials: &[ResolvedCredential],
) -> Result<ConsumerOutput, String> {
    let mut command = tokio::process::Command::new(&plan.interpreter_path);
    command
        .args(&plan.arguments)
        .current_dir(&plan.cwd)
        .env_clear()
        .envs(plankton_core::execution::CONSUMER_ENVIRONMENT)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    for credential in credentials {
        if let Some(name) = &credential.env {
            command.env(name, &credential.value);
        }
    }
    #[cfg(unix)]
    command.process_group(0);
    let mut child = command
        .spawn()
        .map_err(|_| "reviewed consumer executable could not be started")?;
    #[cfg(unix)]
    let _group = ProcessGroup(child.id().ok_or("consumer process id is unavailable")?);
    let mut stdin = child.stdin.take().ok_or("consumer stdin is unavailable")?;
    let stdout = child
        .stdout
        .take()
        .ok_or("consumer stdout is unavailable")?;
    let stderr = child
        .stderr
        .take()
        .ok_or("consumer stderr is unavailable")?;
    let input = if plan.credential_env.is_some() {
        plan.command_stdin.as_deref().unwrap_or_default()
    } else {
        &credentials[0].value
    };
    let run = async {
        tokio::try_join!(
            async {
                stdin
                    .write_all(input.as_bytes())
                    .await
                    .map_err(|_| "consumer did not accept its input".to_string())?;
                stdin
                    .shutdown()
                    .await
                    .map_err(|_| "consumer input could not be closed".to_string())?;
                drop(stdin);
                Ok(())
            },
            read_bounded(stdout),
            read_bounded(stderr),
            async {
                child
                    .wait()
                    .await
                    .map_err(|_| "consumer exit status is unavailable".to_string())
            }
        )
    };
    let result =
        tokio::time::timeout(Duration::from_secs(u64::from(plan.timeout_seconds)), run).await;
    match result {
        Ok(Ok(((), stdout, stderr, status))) => {
            let withheld = credentials.iter().any(|credential| {
                let value = credential.value.as_bytes();
                !credential.allow_output
                    && !value.is_empty()
                    && [&stdout, &stderr]
                        .iter()
                        .any(|bytes| bytes.windows(value.len()).any(|window| window == value))
            });
            Ok(ConsumerOutput {
                exit_code: status.code(),
                stdout: if withheld { String::new() } else { String::from_utf8_lossy(&stdout).into_owned() },
                stderr: if withheld { String::new() } else { String::from_utf8_lossy(&stderr).into_owned() },
                error: withheld.then(|| "consumer output contained credential bytes without both terminal_log and llm_context permission; output was withheld".into()),
            })
        }
        Ok(Err(error)) => {
            let _ = child.kill().await;
            Err(error)
        }
        Err(_) => {
            let _ = child.kill().await;
            Err("consumer execution exceeded its approved timeout".into())
        }
    }
}
