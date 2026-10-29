use std::{
    collections::{HashSet, VecDeque},
    sync::Mutex,
};

use anyhow::{Context, Result};
use plankton_core::{
    exposure_policy_from_metadata, AccessRequest, ApprovalStatus, CallChainNode,
    CredentialExposureReport, EvaluationState, LlmReviewProgress, SanitizedInlineSource,
    SuggestedDecision,
};
use plankton_protocol::exposure::CredentialExposurePolicy;
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, Runtime, WebviewUrl, WebviewWindowBuilder};

pub const APPROVAL_LABEL: &str = "approval";
pub const APPROVAL_QUEUE_EVENT: &str = "plankton://approval-queue";
const APPROVAL_DEFAULT_WIDTH: f64 = 900.0;
const APPROVAL_DEFAULT_HEIGHT: f64 = 860.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApprovalSurface {
    Compact,
    None,
}

pub fn select_surface(human_review_required: bool) -> ApprovalSurface {
    if human_review_required {
        ApprovalSurface::Compact
    } else {
        ApprovalSurface::None
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum QueueTransition {
    Unchanged,
    QueueUpdated,
    ShowCurrent(String),
    CloseCompact,
}

#[derive(Debug, Default)]
pub struct ApprovalQueue {
    presented: HashSet<String>,
    compact: VecDeque<String>,
}

impl ApprovalQueue {
    pub fn register_presentation(
        &mut self,
        request_id: impl Into<String>,
        surface: ApprovalSurface,
    ) -> bool {
        let request_id = request_id.into();
        if surface == ApprovalSurface::None || !self.presented.insert(request_id.clone()) {
            return false;
        }
        if surface == ApprovalSurface::Compact {
            self.compact.push_back(request_id);
        }
        true
    }

    pub fn compact_request_ids(&self) -> Vec<String> {
        self.compact.iter().cloned().collect()
    }

    pub fn current_compact_request_id(&self) -> Option<&str> {
        self.compact.front().map(String::as_str)
    }

    pub fn resolve(&mut self, request_id: &str) -> QueueTransition {
        self.presented.remove(request_id);
        self.remove_compact(request_id)
    }

    pub fn open_full_details(&mut self, request_id: &str) -> QueueTransition {
        self.remove_compact(request_id)
    }

    fn remove_compact(&mut self, request_id: &str) -> QueueTransition {
        let was_current = self.current_compact_request_id() == Some(request_id);
        let Some(index) = self.compact.iter().position(|id| id == request_id) else {
            return QueueTransition::Unchanged;
        };
        self.compact.remove(index);
        if !was_current {
            return QueueTransition::QueueUpdated;
        }
        self.current_compact_request_id()
            .map_or(QueueTransition::CloseCompact, |next| {
                QueueTransition::ShowCurrent(next.to_string())
            })
    }

    fn dismiss_popup(&mut self) {
        // Retain `presented`: polling must not reopen a dismissed human approval.
        self.compact.clear();
    }

    fn forget_failed_presentation(&mut self, request_id: &str) {
        self.presented.remove(request_id);
        if let Some(index) = self.compact.iter().position(|id| id == request_id) {
            self.compact.remove(index);
        }
    }

    fn reconcile_active_presentations(&mut self, active: &HashSet<String>) -> bool {
        let previous_presented_len = self.presented.len();
        let previous_compact_len = self.compact.len();
        self.presented
            .retain(|request_id| active.contains(request_id));
        self.compact
            .retain(|request_id| active.contains(request_id));
        self.presented.len() != previous_presented_len || self.compact.len() != previous_compact_len
    }
}

#[derive(Debug)]
pub struct ApprovalRuntime {
    queue: Mutex<ApprovalQueue>,
}

impl Default for ApprovalRuntime {
    fn default() -> Self {
        Self {
            queue: Mutex::new(ApprovalQueue::default()),
        }
    }
}

impl ApprovalRuntime {
    pub fn compact_request_ids(&self) -> Result<Vec<String>> {
        self.queue
            .lock()
            .map(|queue| queue.compact_request_ids())
            .map_err(|_| anyhow::anyhow!("failed to lock approval presentation state"))
    }

    fn register(&self, request_id: &str, surface: ApprovalSurface) -> Result<bool> {
        self.queue
            .lock()
            .map(|mut queue| queue.register_presentation(request_id, surface))
            .map_err(|_| anyhow::anyhow!("failed to lock approval presentation state"))
    }

    fn forget_failed_presentation(&self, request_id: &str) -> Result<()> {
        self.queue
            .lock()
            .map(|mut queue| queue.forget_failed_presentation(request_id))
            .map_err(|_| anyhow::anyhow!("failed to lock approval presentation state"))
    }

    fn resolve(&self, request_id: &str) -> Result<QueueTransition> {
        self.queue
            .lock()
            .map(|mut queue| queue.resolve(request_id))
            .map_err(|_| anyhow::anyhow!("failed to lock approval presentation state"))
    }

    fn open_full_details(&self, request_id: &str) -> Result<QueueTransition> {
        self.queue
            .lock()
            .map(|mut queue| queue.open_full_details(request_id))
            .map_err(|_| anyhow::anyhow!("failed to lock approval presentation state"))
    }

    fn retain_active(&self, active: &HashSet<String>) -> Result<bool> {
        self.queue
            .lock()
            .map(|mut queue| queue.reconcile_active_presentations(active))
            .map_err(|_| anyhow::anyhow!("failed to lock approval presentation state"))
    }

    fn notify_or_rollback<F>(&self, request_id: &str, notify: F) -> Result<()>
    where
        F: FnOnce() -> Result<()>,
    {
        if let Err(error) = notify() {
            self.forget_failed_presentation(request_id)?;
            return Err(error);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PresentationResult {
    Duplicate,
    Compact,
    None,
}

pub fn present_request<R: Runtime>(
    app: &AppHandle<R>,
    request_id: &str,
    human_review_required: bool,
) -> Result<PresentationResult> {
    let runtime = app
        .try_state::<ApprovalRuntime>()
        .context("approval presentation runtime is unavailable")?;
    let surface = select_surface(human_review_required);
    if surface == ApprovalSurface::None {
        return Ok(PresentationResult::None);
    }
    if !runtime.register(request_id, surface)? {
        return Ok(PresentationResult::Duplicate);
    }

    match surface {
        ApprovalSurface::Compact => {
            if let Err(error) = show_compact_window(app) {
                runtime.forget_failed_presentation(request_id)?;
                return Err(error);
            }
            runtime.notify_or_rollback(request_id, || emit_queue_changed(app))?;
            Ok(PresentationResult::Compact)
        }
        ApprovalSurface::None => Ok(PresentationResult::None),
    }
}

pub fn compact_requests<R: Runtime>(
    app: &AppHandle<R>,
    pending_requests: &[AccessRequest],
) -> Result<Vec<CompactApprovalRequest>> {
    let runtime = app
        .try_state::<ApprovalRuntime>()
        .context("approval presentation runtime is unavailable")?;
    let request_ids = runtime.compact_request_ids()?;
    Ok(request_ids
        .into_iter()
        .filter_map(|request_id| {
            pending_requests
                .iter()
                .find(|request| request.id == request_id)
                .map(CompactApprovalRequest::from)
        })
        .collect())
}

pub fn compact_request_ids<R: Runtime>(app: &AppHandle<R>) -> Result<Vec<String>> {
    app.try_state::<ApprovalRuntime>()
        .context("approval presentation runtime is unavailable")?
        .compact_request_ids()
}

pub fn resolve_request<R: Runtime>(app: &AppHandle<R>, request_id: &str) -> Result<()> {
    let runtime = app
        .try_state::<ApprovalRuntime>()
        .context("approval presentation runtime is unavailable")?;
    let transition = runtime.resolve(request_id)?;
    apply_resolution_transition(app, transition)
}

pub fn open_full_details<R: Runtime>(app: &AppHandle<R>, request_id: &str) -> Result<()> {
    let runtime = app
        .try_state::<ApprovalRuntime>()
        .context("approval presentation runtime is unavailable")?;
    if runtime.open_full_details(request_id)? != QueueTransition::Unchanged {
        hide_compact_window(app)?;
    }
    emit_queue_changed(app)?;
    Ok(())
}

pub fn reconcile_active_requests<R: Runtime>(
    app: &AppHandle<R>,
    active_request_ids: impl IntoIterator<Item = String>,
) -> Result<()> {
    let runtime = app
        .try_state::<ApprovalRuntime>()
        .context("approval presentation runtime is unavailable")?;
    let active = active_request_ids.into_iter().collect::<HashSet<_>>();
    if !runtime.retain_active(&active)? {
        return Ok(());
    }
    if runtime.compact_request_ids()?.is_empty() {
        hide_compact_window(app)?;
    }
    emit_queue_changed(app)?;
    Ok(())
}

fn apply_resolution_transition<R: Runtime>(
    app: &AppHandle<R>,
    transition: QueueTransition,
) -> Result<()> {
    match transition {
        QueueTransition::Unchanged => {}
        QueueTransition::QueueUpdated => emit_queue_changed(app)?,
        QueueTransition::ShowCurrent(_) => {
            show_compact_window(app)?;
            emit_queue_changed(app)?;
        }
        QueueTransition::CloseCompact => {
            hide_compact_window(app)?;
            emit_queue_changed(app)?;
        }
    }
    Ok(())
}

pub fn dismiss_popup<R: Runtime>(app: &AppHandle<R>) -> Result<()> {
    app.try_state::<ApprovalRuntime>()
        .context("approval presentation runtime is unavailable")?
        .queue
        .lock()
        .map_err(|_| anyhow::anyhow!("failed to lock approval presentation state"))?
        .dismiss_popup();
    hide_compact_window(app)?;
    emit_queue_changed(app)
}

fn show_compact_window<R: Runtime>(app: &AppHandle<R>) -> Result<()> {
    let window = match app.get_webview_window(APPROVAL_LABEL) {
        Some(window) => window,
        None => {
            WebviewWindowBuilder::new(app, APPROVAL_LABEL, WebviewUrl::App("index.html".into()))
                .title("Plankton")
                .inner_size(APPROVAL_DEFAULT_WIDTH, APPROVAL_DEFAULT_HEIGHT)
                .min_inner_size(640.0, 520.0)
                .resizable(true)
                .always_on_top(true)
                .visible(false)
                .build()
                .context("failed to create human approval window")?
        }
    };
    if !window.is_visible()? {
        if let Some(monitor) = window.current_monitor()?.or(window.primary_monitor()?) {
            let area = monitor.work_area();
            let scale = monitor.scale_factor();
            window.set_size(tauri::LogicalSize::new(
                APPROVAL_DEFAULT_WIDTH.min(f64::from(area.size.width) / scale - 32.0),
                APPROVAL_DEFAULT_HEIGHT.min(f64::from(area.size.height) / scale - 32.0),
            ))?;
        }
        window.center()?;
        window.show()?;
    }
    window.unminimize()?;
    crate::background::synchronize_dock_visibility(app)?;
    window.set_focus()?;
    Ok(())
}

fn hide_compact_window<R: Runtime>(app: &AppHandle<R>) -> Result<()> {
    if let Some(window) = app.get_webview_window(APPROVAL_LABEL) {
        window
            .hide()
            .context("failed to hide compact approval window")?;
    }
    crate::background::synchronize_dock_visibility(app)?;
    Ok(())
}

fn emit_queue_changed<R: Runtime>(app: &AppHandle<R>) -> Result<()> {
    app.emit_to(APPROVAL_LABEL, APPROVAL_QUEUE_EVENT, ())
        .context("failed to notify compact approval queue")
}

#[derive(Debug, Clone, Serialize)]
pub struct CompactApprovalRequest {
    pub locale: String,
    pub policy_mode: plankton_core::PolicyMode,
    created_at: chrono::DateTime<chrono::Utc>,
    resource_metadata: std::collections::BTreeMap<String, String>,
    id: String,
    resource: String,
    requested_by: String,
    reason: String,
    context: String,
    call_chain: Vec<CallChainNode>,
    suggestion: String,
    suggested_decision: String,
    risk_score: Option<u8>,
    exposure_report: Option<CredentialExposureReport>,
    inline_sources: Vec<SanitizedInlineSource>,
    execution_sources: Option<String>,
    exposure_policy: CredentialExposurePolicy,
    approval_status: ApprovalStatus,
    evaluation_state: EvaluationState,
    review_progress: Option<LlmReviewProgress>,
}

impl From<&AccessRequest> for CompactApprovalRequest {
    fn from(request: &AccessRequest) -> Self {
        let context = request
            .context
            .script_path
            .clone()
            .or_else(|| {
                let chain = request
                    .context
                    .call_chain
                    .iter()
                    .filter_map(|node| node.prompt_display_path())
                    .collect::<Vec<_>>();
                (!chain.is_empty()).then(|| chain.join(" → "))
            })
            .unwrap_or_else(|| "No script or call-chain context provided.".to_string());
        let suggestion = request
            .llm_suggestion
            .as_ref()
            .map(|suggestion| suggestion.rationale_summary.clone())
            .or_else(|| {
                request
                    .automatic_decision
                    .as_ref()
                    .map(|decision| decision.auto_rationale_summary.clone())
            })
            .unwrap_or_else(|| "No automatic recommendation is available.".to_string());
        let suggested_decision = request
            .llm_suggestion
            .as_ref()
            .map(|suggestion| suggestion.suggested_decision)
            .map(|decision| match decision {
                SuggestedDecision::Allow => "allow",
                SuggestedDecision::Deny => "deny",
                SuggestedDecision::Escalate => "escalate",
            })
            .unwrap_or("human_review")
            .to_string();
        let risk_score = request
            .llm_suggestion
            .as_ref()
            .map(|suggestion| suggestion.risk_score)
            .or_else(|| {
                request
                    .automatic_decision
                    .as_ref()
                    .and_then(|decision| decision.risk_score)
            });
        Self {
            locale: "en".into(),
            policy_mode: request.policy_mode,
            created_at: request.created_at,
            resource_metadata: request.context.resource_metadata.clone(),
            id: request.id.clone(),
            resource: request.context.resource.clone(),
            requested_by: request.context.requested_by.clone(),
            reason: request.context.reason.clone(),
            context,
            call_chain: request.context.call_chain.clone(),
            suggestion,
            suggested_decision,
            risk_score,
            exposure_report: request
                .llm_suggestion
                .as_ref()
                .and_then(|suggestion| suggestion.exposure_report.clone()),
            inline_sources: request
                .provider_input
                .as_ref()
                .map(|input| input.sanitized_context.inline_sources.clone())
                .unwrap_or_default(),
            execution_sources: request.context.metadata.get("execution_sources").cloned(),
            exposure_policy: exposure_policy_from_metadata(&request.context.resource_metadata),
            approval_status: request.approval_status,
            evaluation_state: request.evaluation_state,
            review_progress: request
                .llm_suggestion
                .as_ref()
                .and_then(|suggestion| suggestion.provider_trace.as_ref())
                .and_then(|trace| trace.review_progress.clone()),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::{BTreeMap, HashSet};

    use plankton_core::{
        AccessRequest, LlmApprovalDecisionPolicy, PolicyMode, ProviderInputSnapshot,
        RequestContext, SanitizedInlineSource, SanitizedPromptContext, SanitizedSourceLine,
    };

    use super::{
        select_surface, ApprovalQueue, ApprovalRuntime, ApprovalSurface, CompactApprovalRequest,
        QueueTransition,
    };

    #[test]
    fn compact_request_preserves_sanitized_inline_sources_for_evidence_references() {
        let inline_source = SanitizedInlineSource {
            source_id: "inline:python".to_string(),
            node_index: 0,
            argument_index: 2,
            language: "python".to_string(),
            lines: vec![SanitizedSourceLine {
                line: 1,
                text: "urlopen(endpoint)".to_string(),
            }],
        };
        let provider_input = ProviderInputSnapshot {
            template_id: "request-advice".to_string(),
            template_version: "1".to_string(),
            prompt_contract_version: "1".to_string(),
            prompt_sha256: "test-sha256".to_string(),
            prompt: "sanitized prompt".to_string(),
            decision_policy: LlmApprovalDecisionPolicy::default(),
            allowed_read_files: Vec::new(),
            sanitized_context: SanitizedPromptContext {
                resource: "plankton://field/service/token".to_string(),
                resource_tags: Vec::new(),
                metadata: BTreeMap::new(),
                request_metadata: BTreeMap::new(),
                reason: "Run the network client".to_string(),
                requested_by: "test-agent".to_string(),
                script_path: None,
                call_chain: Vec::new(),
                call_chain_details: Vec::new(),
                inline_sources: vec![inline_source.clone()],
                env_vars: BTreeMap::new(),
                env_var_names: Vec::new(),
            },
        };
        let request = AccessRequest::new_pending(
            RequestContext::new(
                "plankton://field/service/token".to_string(),
                "Run the network client".to_string(),
                "test-agent".to_string(),
            ),
            PolicyMode::ManualOnly,
            None,
            String::new(),
            Some(provider_input),
            None,
        );

        let compact = CompactApprovalRequest::from(&request);

        assert_eq!(compact.inline_sources, vec![inline_source]);
    }

    #[test]
    fn human_review_has_its_own_surface_independent_of_notifications() {
        assert_eq!(select_surface(true), ApprovalSurface::Compact);
        assert_eq!(select_surface(false), ApprovalSurface::None);
    }

    #[test]
    fn dismissed_human_request_does_not_reopen_on_every_poll() {
        let mut queue = ApprovalQueue::default();
        assert!(queue.register_presentation("new", ApprovalSurface::Compact));
        queue.dismiss_popup();
        assert!(queue.compact_request_ids().is_empty());
        assert!(!queue.register_presentation("new", ApprovalSurface::Compact));
        assert!(queue.register_presentation("next", ApprovalSurface::Compact));
    }

    #[test]
    fn deduplicates_presentation_by_request_id_across_all_sources() {
        let mut queue = ApprovalQueue::default();

        assert!(queue.register_presentation("request-1", ApprovalSurface::Compact));
        assert!(!queue.register_presentation("request-1", ApprovalSurface::Compact));
        assert!(!queue.register_presentation("request-1", ApprovalSurface::Compact));
        assert_eq!(queue.compact_request_ids(), vec!["request-1"]);
    }

    #[test]
    fn compact_requests_share_one_ordered_queue() {
        let mut queue = ApprovalQueue::default();

        assert!(queue.register_presentation("request-1", ApprovalSurface::Compact));
        assert!(queue.register_presentation("request-2", ApprovalSurface::Compact));

        assert_eq!(queue.compact_request_ids(), vec!["request-1", "request-2"]);
        assert_eq!(queue.current_compact_request_id(), Some("request-1"));
    }

    #[test]
    fn resolving_compact_requests_advances_then_closes_on_the_last_request() {
        let mut queue = ApprovalQueue::default();
        queue.register_presentation("request-1", ApprovalSurface::Compact);
        queue.register_presentation("request-2", ApprovalSurface::Compact);

        assert_eq!(
            queue.resolve("request-1"),
            QueueTransition::ShowCurrent("request-2".to_string())
        );
        assert_eq!(queue.current_compact_request_id(), Some("request-2"));
        assert_eq!(queue.resolve("request-2"), QueueTransition::CloseCompact);
        assert!(queue.compact_request_ids().is_empty());
    }

    #[test]
    fn resolving_a_later_request_preserves_the_current_request() {
        let mut queue = ApprovalQueue::default();
        queue.register_presentation("old", ApprovalSurface::Compact);
        queue.register_presentation("latest", ApprovalSurface::Compact);
        assert_eq!(queue.resolve("latest"), QueueTransition::QueueUpdated);
        assert_eq!(queue.current_compact_request_id(), Some("old"));
        assert_eq!(queue.compact_request_ids(), vec!["old"]);
    }

    #[test]
    fn opening_full_details_removes_only_the_compact_item_without_representing_it() {
        let mut queue = ApprovalQueue::default();
        queue.register_presentation("request-1", ApprovalSurface::Compact);
        queue.register_presentation("request-2", ApprovalSurface::Compact);

        assert_eq!(
            queue.open_full_details("request-1"),
            QueueTransition::ShowCurrent("request-2".to_string())
        );
        assert!(!queue.register_presentation("request-1", ApprovalSurface::Compact));
        assert_eq!(queue.compact_request_ids(), vec!["request-2"]);
    }

    #[test]
    fn opening_unrelated_details_does_not_close_human_approval() {
        let mut queue = ApprovalQueue::default();
        queue.register_presentation("human", ApprovalSurface::Compact);
        assert_eq!(
            queue.open_full_details("automatic"),
            QueueTransition::Unchanged
        );
        assert_eq!(queue.compact_request_ids(), vec!["human"]);
    }

    #[test]
    fn inactive_then_active_again_is_a_new_presentation_transition() {
        let mut queue = ApprovalQueue::default();
        queue.register_presentation("request-1", ApprovalSurface::Compact);

        queue.reconcile_active_presentations(&HashSet::new());

        assert!(queue.register_presentation("request-1", ApprovalSurface::Compact));
        assert_eq!(queue.compact_request_ids(), vec!["request-1"]);
    }

    #[test]
    fn closing_while_still_active_remains_suppressed_from_representation() {
        let mut queue = ApprovalQueue::default();
        queue.register_presentation("request-1", ApprovalSurface::Compact);

        queue.reconcile_active_presentations(&HashSet::from(["request-1".to_string()]));

        assert!(!queue.register_presentation("request-1", ApprovalSurface::Compact));
        assert_eq!(queue.compact_request_ids(), vec!["request-1"]);
    }

    #[test]
    fn terminal_requests_leave_presented_and_compact_state() {
        let mut queue = ApprovalQueue::default();
        queue.register_presentation("request-1", ApprovalSurface::Compact);

        queue.reconcile_active_presentations(&HashSet::new());

        assert!(queue.compact_request_ids().is_empty());
        assert!(queue.register_presentation("request-1", ApprovalSurface::Compact));
    }

    #[test]
    fn failed_queue_notification_rolls_back_and_allows_retry() {
        let runtime = ApprovalRuntime::default();
        assert!(runtime
            .register("request-1", ApprovalSurface::Compact)
            .expect("register presentation"));

        let result = runtime.notify_or_rollback("request-1", || {
            anyhow::bail!("deterministic emitter failure")
        });

        assert_eq!(
            result.expect_err("notification must fail").to_string(),
            "deterministic emitter failure"
        );
        assert!(runtime
            .register("request-1", ApprovalSurface::Compact)
            .expect("presentation should be retryable"));
    }
}
