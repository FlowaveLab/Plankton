use std::{
    collections::{HashMap, VecDeque},
    sync::Mutex,
    time::Duration,
};

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use plankton_core::{ApprovalStatus, EvaluationState, PolicyMode};
use plankton_store::{ApprovalNotificationDetails, ApprovalNotificationStatus, SqliteStore};
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, Runtime, WebviewUrl, WebviewWindowBuilder};

pub const NOTIFICATION_LABEL: &str = "approval-notification";
pub const NOTIFICATION_EVENT: &str = "plankton://approval-notification";
const NOTIFICATION_WIDTH: f64 = 320.0;
const NOTIFICATION_HEIGHT: f64 = 170.0;
const NOTIFICATION_EDGE_INSET: f64 = 16.0;
const RESULT_LIFETIME: Duration = Duration::from_secs(5);

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum NotificationStage {
    Evaluating,
    Approved,
    Rejected,
    Failed,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ApprovalNotification {
    pub id: String,
    pub request_id: String,
    pub stage: NotificationStage,
    pub resource: String,
    pub resource_label: String,
    pub requested_by: String,
    pub reason: String,
    pub decision_reason: String,
}

fn notification_stage(status: &ApprovalNotificationStatus) -> Option<NotificationStage> {
    match status.approval_status {
        ApprovalStatus::Approved => Some(NotificationStage::Approved),
        ApprovalStatus::Rejected => Some(NotificationStage::Rejected),
        ApprovalStatus::Pending if status.policy_mode == PolicyMode::ManualOnly => None,
        ApprovalStatus::Pending => match status.evaluation_state {
            EvaluationState::Queued | EvaluationState::Running => {
                Some(NotificationStage::Evaluating)
            }
            EvaluationState::Failed | EvaluationState::Interrupted => {
                Some(NotificationStage::Failed)
            }
            // Escalation hands off to the separate, actionable approval window.
            _ => None,
        },
    }
}

fn notification_lifetime(stage: NotificationStage) -> Option<Duration> {
    (stage != NotificationStage::Evaluating).then_some(RESULT_LIFETIME)
}

#[derive(Debug, Clone)]
struct ObservedStatus {
    stage: Option<NotificationStage>,
    updated_at: DateTime<Utc>,
}

#[derive(Debug)]
struct NotificationState {
    initialized: bool,
    enabled: bool,
    seen: HashMap<String, ObservedStatus>,
    current: Option<ApprovalNotification>,
    pending: VecDeque<ApprovalNotification>,
    displayed_id: Option<String>,
}

impl Default for NotificationState {
    fn default() -> Self {
        Self {
            initialized: false,
            enabled: true,
            seen: HashMap::new(),
            current: None,
            pending: VecDeque::new(),
            displayed_id: None,
        }
    }
}

impl NotificationState {
    fn seed_baseline(&mut self, statuses: &[ApprovalNotificationStatus]) {
        self.initialized = true;
        for status in statuses {
            self.remember(status);
        }
    }

    fn remember(&mut self, status: &ApprovalNotificationStatus) {
        self.seen.insert(
            status.id.clone(),
            ObservedStatus {
                stage: notification_stage(status),
                updated_at: status.updated_at,
            },
        );
    }

    fn changed(&self, status: &ApprovalNotificationStatus) -> bool {
        self.seen.get(&status.id).is_none_or(|previous| {
            status.updated_at >= previous.updated_at && previous.stage != notification_stage(status)
        })
    }

    fn observe(
        &mut self,
        status: &ApprovalNotificationStatus,
        details: Option<&ApprovalNotificationDetails>,
    ) -> bool {
        if !self.changed(status) {
            if self.seen.get(&status.id).is_some_and(|previous| {
                status.updated_at > previous.updated_at
                    && previous.stage == notification_stage(status)
            }) {
                self.remember(status);
            }
            return false;
        }
        let stage = notification_stage(status);
        if self.enabled && stage.is_some() && details.is_none() {
            // Failed detail reads remain eligible on the next poll.
            return false;
        }
        self.remember(status);
        if !self.enabled {
            return false;
        }
        let Some(stage) = stage else {
            return self.remove_request(&status.id);
        };
        let details = details.expect("visible notifications require details");
        let notification = ApprovalNotification {
            id: uuid::Uuid::new_v4().to_string(),
            request_id: status.id.clone(),
            stage,
            resource: status.resource.clone(),
            resource_label: details.resource_label.clone(),
            requested_by: status.requested_by.clone(),
            reason: if status.reason.trim().is_empty() {
                "No request reason was recorded.".into()
            } else {
                status.reason.clone()
            },
            decision_reason: details.decision_reason.clone(),
        };
        self.enqueue(notification)
    }

    fn enqueue(&mut self, notification: ApprovalNotification) -> bool {
        self.pending
            .retain(|pending| pending.request_id != notification.request_id);
        // Progress represents the latest active request and must not obstruct
        // another request's result. Final results are queued so fast parallel
        // requests each get a visible result, even if their progress was skipped.
        if self.current.as_ref().is_none_or(|current| {
            current.request_id == notification.request_id
                || current.stage == NotificationStage::Evaluating
        }) {
            self.current = Some(notification);
            return true;
        }
        if notification.stage == NotificationStage::Evaluating {
            self.pending
                .retain(|pending| pending.stage != NotificationStage::Evaluating);
            self.pending.push_back(notification);
        } else {
            let index = self
                .pending
                .iter()
                .position(|pending| pending.stage == NotificationStage::Evaluating)
                .unwrap_or(self.pending.len());
            self.pending.insert(index, notification);
        }
        false
    }

    fn remove_request(&mut self, request_id: &str) -> bool {
        self.pending
            .retain(|pending| pending.request_id != request_id);
        if self
            .current
            .as_ref()
            .is_some_and(|current| current.request_id == request_id)
        {
            self.current = self.pending.pop_front();
            return true;
        }
        false
    }

    fn dismiss(&mut self, notification_id: &str) -> bool {
        if self
            .current
            .as_ref()
            .is_some_and(|current| current.id == notification_id)
        {
            self.current = self.pending.pop_front();
            true
        } else {
            false
        }
    }

    fn set_enabled(&mut self, enabled: bool) -> bool {
        self.enabled = enabled;
        if enabled {
            return false;
        }
        self.pending.clear();
        self.current.take().is_some() || self.displayed_id.is_some()
    }
}

#[derive(Debug, Default)]
pub struct ApprovalNotificationRuntime {
    state: Mutex<NotificationState>,
}

impl ApprovalNotificationRuntime {
    fn with_state<T>(&self, operation: impl FnOnce(&mut NotificationState) -> T) -> Result<T> {
        self.state
            .lock()
            .map(|mut state| operation(&mut state))
            .map_err(|_| anyhow::anyhow!("failed to lock approval notification state"))
    }
}

pub fn seed_baseline<R: Runtime>(
    app: &AppHandle<R>,
    statuses: &[ApprovalNotificationStatus],
) -> Result<()> {
    app.try_state::<ApprovalNotificationRuntime>()
        .context("approval notification runtime is unavailable")?
        .with_state(|state| state.seed_baseline(statuses))
}

pub fn set_enabled<R: Runtime>(app: &AppHandle<R>, enabled: bool) -> Result<()> {
    let changed = app
        .try_state::<ApprovalNotificationRuntime>()
        .context("approval notification runtime is unavailable")?
        .with_state(|state| state.set_enabled(enabled))?;
    if changed {
        synchronize_window(app)?;
    }
    Ok(())
}

/// This only changes runtime state. The command handler persists the preference
/// using the same settings write as the Settings toggle.
pub fn disable<R: Runtime>(app: &AppHandle<R>) -> Result<()> {
    set_enabled(app, false)
}

pub fn current<R: Runtime>(app: &AppHandle<R>) -> Result<Option<ApprovalNotification>> {
    app.try_state::<ApprovalNotificationRuntime>()
        .context("approval notification runtime is unavailable")?
        .with_state(|state| state.current.clone())
}

pub fn dismiss<R: Runtime>(app: &AppHandle<R>, notification_id: &str) -> Result<()> {
    let changed = app
        .try_state::<ApprovalNotificationRuntime>()
        .context("approval notification runtime is unavailable")?
        .with_state(|state| state.dismiss(notification_id))?;
    if changed {
        synchronize_window(app)?;
    }
    Ok(())
}

pub fn dismiss_current<R: Runtime>(app: &AppHandle<R>) -> Result<()> {
    if let Some(notification) = current(app)? {
        dismiss(app, &notification.id)?;
    }
    Ok(())
}

pub async fn observe_statuses<R: Runtime>(
    app: &AppHandle<R>,
    store: &SqliteStore,
    statuses: &[ApprovalNotificationStatus],
) -> Result<()> {
    let runtime = app
        .try_state::<ApprovalNotificationRuntime>()
        .context("approval notification runtime is unavailable")?;
    let initialized = runtime.with_state(|state| {
        if state.initialized {
            true
        } else {
            state.seed_baseline(statuses);
            false
        }
    })?;
    if !initialized {
        return Ok(());
    }
    for status in statuses {
        let needs_details = runtime.with_state(|state| {
            state.changed(status) && state.enabled && notification_stage(status).is_some()
        })?;
        let changed = if needs_details {
            // Read status and details together: a very fast result may already
            // have replaced the queued/running state observed by the poll.
            let details = store.approval_notification_details(&status.id).await?;
            runtime.with_state(|state| state.observe(&details.status, Some(&details)))?
        } else {
            runtime.with_state(|state| state.observe(status, None))?
        };
        if changed {
            synchronize_window(app)?;
        }
    }
    // Also retries native presentation after a window creation failure without
    // replaying already displayed stages or fetching their evidence again.
    let needs_render = runtime.with_state(|state| {
        state.current.as_ref().map(|current| current.id.as_str()) != state.displayed_id.as_deref()
    })?;
    if needs_render {
        synchronize_window(app)?;
    }
    Ok(())
}

fn synchronize_window<R: Runtime>(app: &AppHandle<R>) -> Result<()> {
    let handle = app.clone();
    app.run_on_main_thread(move || {
        if let Err(error) = render_current(&handle) {
            tracing::error!(%error, "failed to update approval notification window");
        }
    })
    .context("failed to schedule approval notification window update")
}

fn render_current<R: Runtime>(app: &AppHandle<R>) -> Result<()> {
    let runtime = app
        .try_state::<ApprovalNotificationRuntime>()
        .context("approval notification runtime is unavailable")?;
    let (notification, displayed_id) =
        runtime.with_state(|state| (state.current.clone(), state.displayed_id.clone()))?;
    if notification
        .as_ref()
        .map(|notification| notification.id.as_str())
        == displayed_id.as_deref()
    {
        return Ok(());
    }
    if let Some(notification) = notification.as_ref() {
        show_window(app)?;
        runtime.with_state(|state| state.displayed_id = Some(notification.id.clone()))?;
        if let Some(lifetime) = notification_lifetime(notification.stage) {
            let handle = app.clone();
            let notification_id = notification.id.clone();
            tauri::async_runtime::spawn(async move {
                tokio::time::sleep(lifetime).await;
                // A timer from an older stage never closes a newer result.
                if let Err(error) = dismiss(&handle, &notification_id) {
                    tracing::error!(%error, "failed to dismiss expired approval notification");
                }
            });
        }
        // The native lifetime starts even if the webview has not loaded yet or
        // event delivery fails. The frontend can recover through `current`.
        app.emit_to(NOTIFICATION_LABEL, NOTIFICATION_EVENT, notification)?;
    } else {
        let mut emitted = Ok(());
        if let Some(window) = app.get_webview_window(NOTIFICATION_LABEL) {
            emitted = app.emit_to(
                NOTIFICATION_LABEL,
                NOTIFICATION_EVENT,
                Option::<ApprovalNotification>::None,
            );
            window.hide()?;
        }
        runtime.with_state(|state| state.displayed_id = None)?;
        crate::background::synchronize_dock_visibility(app)?;
        emitted?;
    }
    Ok(())
}

fn notification_position(
    position: tauri::PhysicalPosition<i32>,
    size: tauri::PhysicalSize<u32>,
    scale: f64,
) -> tauri::PhysicalPosition<i32> {
    let width = (NOTIFICATION_WIDTH * scale).round() as i32;
    let inset = (NOTIFICATION_EDGE_INSET * scale).round() as i32;
    tauri::PhysicalPosition::new(
        position.x + (size.width as i32 - width - inset).max(0),
        position.y + inset,
    )
}

fn show_window<R: Runtime>(app: &AppHandle<R>) -> Result<()> {
    let window = match app.get_webview_window(NOTIFICATION_LABEL) {
        Some(window) => window,
        None => WebviewWindowBuilder::new(
            app,
            NOTIFICATION_LABEL,
            WebviewUrl::App("index.html".into()),
        )
        .title("Plankton")
        .inner_size(NOTIFICATION_WIDTH, NOTIFICATION_HEIGHT)
        .resizable(false)
        .maximizable(false)
        .minimizable(false)
        .decorations(false)
        .always_on_top(true)
        .skip_taskbar(true)
        .focused(false)
        .visible(false)
        .build()
        .context("failed to create approval result notification")?,
    };
    let monitor = match app.get_webview_window("main") {
        Some(main) => main.current_monitor()?,
        None => None,
    }
    .or(window.primary_monitor()?);
    if let Some(monitor) = monitor {
        let work = monitor.work_area();
        window.set_position(notification_position(
            work.position,
            work.size,
            monitor.scale_factor(),
        ))?;
    }
    if !window.is_visible()? {
        // macOS `show` otherwise makes the window key even with focused(false).
        window.set_focusable(false)?;
        let shown = window.show();
        let focusability_restored = window.set_focusable(true);
        shown.context("failed to show approval notification")?;
        focusability_restored.context("failed to restore notification interaction")?;
    }
    crate::background::synchronize_dock_visibility(app)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn status(
        id: &str,
        approval_status: ApprovalStatus,
        evaluation_state: EvaluationState,
    ) -> ApprovalNotificationStatus {
        ApprovalNotificationStatus {
            id: id.into(),
            resource: "secret/test".into(),
            requested_by: "test-agent".into(),
            reason: "Run deployment".into(),
            policy_mode: PolicyMode::LlmAutomatic,
            approval_status,
            evaluation_state,
            updated_at: Utc::now(),
        }
    }

    fn observe(state: &mut NotificationState, status: &ApprovalNotificationStatus) -> bool {
        let details = ApprovalNotificationDetails {
            status: status.clone(),
            resource_label: "Deploy · Token".into(),
            decision_reason: "Within the configured scope".into(),
        };
        state.observe(status, Some(&details))
    }

    #[test]
    fn startup_baseline_never_replays_history_but_future_results_are_visible() {
        let old = status("old", ApprovalStatus::Approved, EvaluationState::Completed);
        let running = status("running", ApprovalStatus::Pending, EvaluationState::Running);
        let mut state = NotificationState::default();
        state.seed_baseline(&[old.clone(), running.clone()]);
        assert!(!observe(&mut state, &old));
        assert!(!observe(&mut state, &running));
        assert!(state.current.is_none());
        assert!(observe(
            &mut state,
            &status(
                "running",
                ApprovalStatus::Approved,
                EvaluationState::Completed
            )
        ));
        assert_eq!(state.current.unwrap().stage, NotificationStage::Approved);
    }

    #[test]
    fn fast_requests_show_their_result_without_an_observed_running_state() {
        let mut state = NotificationState::default();
        state.seed_baseline(&[]);
        assert!(observe(
            &mut state,
            &status("fast", ApprovalStatus::Rejected, EvaluationState::Completed)
        ));
        let current = state.current.as_ref().unwrap();
        assert_eq!(current.stage, NotificationStage::Rejected);
        assert_eq!(current.resource_label, "Deploy · Token");
        assert_eq!(current.reason, "Run deployment");
        assert!(!current.decision_reason.is_empty());
    }

    #[test]
    fn queued_to_running_is_deduplicated_and_progress_never_expires() {
        let mut state = NotificationState::default();
        observe(
            &mut state,
            &status("a", ApprovalStatus::Pending, EvaluationState::Queued),
        );
        let id = state.current.as_ref().unwrap().id.clone();
        assert!(!observe(
            &mut state,
            &status("a", ApprovalStatus::Pending, EvaluationState::Running)
        ));
        assert_eq!(state.current.as_ref().unwrap().id, id);
        assert_eq!(notification_lifetime(NotificationStage::Evaluating), None);
        for stage in [
            NotificationStage::Approved,
            NotificationStage::Rejected,
            NotificationStage::Failed,
        ] {
            assert_eq!(notification_lifetime(stage), Some(Duration::from_secs(5)));
        }
    }

    #[test]
    fn old_timer_and_close_cannot_hide_a_new_stage() {
        let mut state = NotificationState::default();
        observe(
            &mut state,
            &status("a", ApprovalStatus::Pending, EvaluationState::Failed),
        );
        let failed_id = state.current.as_ref().unwrap().id.clone();
        observe(
            &mut state,
            &status("a", ApprovalStatus::Approved, EvaluationState::Completed),
        );
        assert_ne!(state.current.as_ref().unwrap().id, failed_id);
        assert!(!state.dismiss(&failed_id));
        assert_eq!(
            state.current.as_ref().unwrap().stage,
            NotificationStage::Approved
        );
    }

    #[test]
    fn handoff_to_human_clears_progress_and_manual_results_still_notify() {
        let mut state = NotificationState::default();
        observe(
            &mut state,
            &status("a", ApprovalStatus::Pending, EvaluationState::Running),
        );
        assert!(observe(
            &mut state,
            &status("a", ApprovalStatus::Pending, EvaluationState::Completed)
        ));
        assert!(state.current.is_none());
        let mut manual = status(
            "manual",
            ApprovalStatus::Pending,
            EvaluationState::NotRequired,
        );
        manual.policy_mode = PolicyMode::ManualOnly;
        assert!(!observe(&mut state, &manual));
        manual.approval_status = ApprovalStatus::Approved;
        assert!(observe(&mut state, &manual));
        assert_eq!(
            state.current.as_ref().unwrap().stage,
            NotificationStage::Approved
        );
    }

    #[test]
    fn independent_results_queue_without_waiting_for_unrelated_evaluation() {
        let mut state = NotificationState::default();
        observe(
            &mut state,
            &status("slow", ApprovalStatus::Pending, EvaluationState::Running),
        );
        observe(
            &mut state,
            &status(
                "fast-a",
                ApprovalStatus::Approved,
                EvaluationState::Completed,
            ),
        );
        observe(
            &mut state,
            &status(
                "fast-b",
                ApprovalStatus::Rejected,
                EvaluationState::Completed,
            ),
        );
        observe(
            &mut state,
            &status("slow", ApprovalStatus::Approved, EvaluationState::Completed),
        );
        assert_eq!(state.current.as_ref().unwrap().request_id, "fast-a");
        let id = state.current.as_ref().unwrap().id.clone();
        state.dismiss(&id);
        assert_eq!(state.current.as_ref().unwrap().request_id, "fast-b");
        let id = state.current.as_ref().unwrap().id.clone();
        state.dismiss(&id);
        assert_eq!(state.current.as_ref().unwrap().request_id, "slow");
    }

    #[test]
    fn disabled_or_dismissed_notifications_do_not_replay() {
        let mut state = NotificationState::default();
        let evaluating = status("a", ApprovalStatus::Pending, EvaluationState::Running);
        observe(&mut state, &evaluating);
        let id = state.current.as_ref().unwrap().id.clone();
        assert!(state.dismiss(&id));
        assert!(!observe(&mut state, &evaluating));
        state.set_enabled(false);
        let approved = status("a", ApprovalStatus::Approved, EvaluationState::Completed);
        assert!(!observe(&mut state, &approved));
        state.set_enabled(true);
        assert!(!observe(&mut state, &approved));
        assert!(state.current.is_none());
        assert!(observe(
            &mut state,
            &status("new", ApprovalStatus::Approved, EvaluationState::Completed)
        ));
    }

    #[test]
    fn late_snapshots_do_not_replace_a_newer_result() {
        let mut state = NotificationState::default();
        let evaluating = status("a", ApprovalStatus::Pending, EvaluationState::Running);
        observe(
            &mut state,
            &status("a", ApprovalStatus::Approved, EvaluationState::Completed),
        );
        assert!(!observe(&mut state, &evaluating));
        assert_eq!(
            state.current.as_ref().unwrap().stage,
            NotificationStage::Approved
        );
    }

    #[test]
    fn small_notification_uses_the_monitor_work_area() {
        assert_eq!((NOTIFICATION_WIDTH, NOTIFICATION_HEIGHT), (320.0, 170.0));
        assert_eq!(
            notification_position(
                tauri::PhysicalPosition::new(-2880, 50),
                tauri::PhysicalSize::new(2880, 1750),
                2.0
            ),
            tauri::PhysicalPosition::new(-672, 82)
        );
    }
}
