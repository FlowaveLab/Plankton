use std::{
    future::Future,
    sync::atomic::{AtomicBool, Ordering},
    time::Duration,
};

use tauri::{Manager, Runtime, WindowEvent};
use tokio::sync::OnceCell;
use tracing::error;

static EXPLICIT_QUIT: AtomicBool = AtomicBool::new(false);
static EXIT_SCHEDULED: AtomicBool = AtomicBool::new(false);
static SHUTDOWN: OnceCell<ShutdownOutcome> = OnceCell::const_new();
const SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(5);
const MAIN_WINDOW_LABEL: &str = "main";

pub fn request_explicit_quit<R: Runtime>(app: &tauri::AppHandle<R>) {
    request_exit_after_shutdown(app, 0);
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ShutdownOutcome {
    Completed,
    Failed,
    TimedOut,
}

async fn shutdown_once<F, Fut>(
    state: &OnceCell<ShutdownOutcome>,
    deadline: Duration,
    shutdown: F,
) -> ShutdownOutcome
where
    F: FnOnce() -> Fut,
    Fut: Future<Output = Result<(), ()>>,
{
    *state
        .get_or_init(|| async {
            match tokio::time::timeout(deadline, shutdown()).await {
                Ok(Ok(())) => ShutdownOutcome::Completed,
                Ok(Err(())) => ShutdownOutcome::Failed,
                Err(_) => ShutdownOutcome::TimedOut,
            }
        })
        .await
}

async fn finish_shutdown<R: Runtime>(app: &tauri::AppHandle<R>) {
    let outcome = shutdown_once(&SHUTDOWN, SHUTDOWN_TIMEOUT, || {
        crate::shutdown_owned_daemon(app)
    })
    .await;
    match outcome {
        ShutdownOutcome::Completed => {}
        ShutdownOutcome::Failed => error!("Plankton daemon shutdown reported an error before exit"),
        ShutdownOutcome::TimedOut => {
            error!("Plankton daemon shutdown reached its deadline before exit")
        }
    }
}

fn request_exit_after_shutdown<R: Runtime>(app: &tauri::AppHandle<R>, code: i32) {
    EXPLICIT_QUIT.store(true, Ordering::SeqCst);
    if EXIT_SCHEDULED.swap(true, Ordering::SeqCst) {
        return;
    }
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        finish_shutdown(&app).await;
        app.exit(code);
    });
}

pub fn handle_window_event<R: Runtime>(window: &tauri::Window<R>, event: &WindowEvent) {
    if let WindowEvent::CloseRequested { api, .. } = event {
        if !EXPLICIT_QUIT.load(Ordering::SeqCst) {
            api.prevent_close();
            let result = if window.label() == crate::approval_window::APPROVAL_LABEL {
                crate::approval_window::dismiss_popup(window.app_handle())
            } else if window.label() == crate::approval_notification::NOTIFICATION_LABEL {
                crate::approval_notification::dismiss_current(window.app_handle())
            } else {
                window.hide().map_err(anyhow::Error::from).and_then(|()| {
                    synchronize_dock_visibility(window.app_handle()).map_err(anyhow::Error::from)
                })
            };
            if let Err(error) = result {
                error!(window = window.label(), %error, "failed to hide persistent Plankton window");
            }
        }
    }
}

pub fn show_main_window<R: Runtime>(app: &tauri::AppHandle<R>) -> tauri::Result<()> {
    if let Some(window) = app.get_webview_window(MAIN_WINDOW_LABEL) {
        window.show()?;
        window.unminimize()?;
        synchronize_dock_visibility(app)?;
        window.set_focus()?;
    }
    Ok(())
}

fn close_target<'a>(windows: &[(&'a str, bool, bool)]) -> Option<&'a str> {
    windows
        .iter()
        .find(|(_, visible, focused)| *visible && *focused)
        .or_else(|| {
            windows
                .iter()
                .find(|(label, visible, _)| *visible && *label == MAIN_WINDOW_LABEL)
        })
        .or_else(|| windows.iter().find(|(_, visible, _)| *visible))
        .map(|(label, _, _)| *label)
}

pub fn hide_current_user_window<R: Runtime>(app: &tauri::AppHandle<R>) -> anyhow::Result<()> {
    let windows = app.webview_windows();
    let states = windows
        .iter()
        .map(|(label, window)| Ok((label.as_str(), window.is_visible()?, window.is_focused()?)))
        .collect::<tauri::Result<Vec<_>>>()?;
    if let Some(label) = close_target(&states) {
        if label == crate::approval_window::APPROVAL_LABEL {
            crate::approval_window::dismiss_popup(app)?;
        } else if label == crate::approval_notification::NOTIFICATION_LABEL {
            crate::approval_notification::dismiss_current(app)?;
        } else if let Some(window) = windows.get(label) {
            window.hide()?;
        }
    }
    synchronize_dock_visibility(app)?;
    Ok(())
}

fn keep_running_after_exit_request(explicit_quit: bool, code: Option<i32>) -> bool {
    !explicit_quit && code.is_none()
}

pub fn handle_run_event<R: Runtime>(app: &tauri::AppHandle<R>, event: tauri::RunEvent) {
    match event {
        tauri::RunEvent::ExitRequested { api, code, .. }
            if keep_running_after_exit_request(EXPLICIT_QUIT.load(Ordering::SeqCst), code) =>
        {
            api.prevent_exit();
            if let Err(error) = hide_current_user_window(app) {
                error!(%error, "failed to close current window while keeping Plankton running");
            }
        }
        tauri::RunEvent::ExitRequested { api, code, .. } if SHUTDOWN.get().is_none() => {
            if code == Some(tauri::RESTART_EXIT_CODE) {
                // Tauri ignores prevent_exit for restart. Its runtime keeps running
                // while this bounded wait cancels owned consumers before process exit.
                EXPLICIT_QUIT.store(true, Ordering::SeqCst);
                tauri::async_runtime::block_on(finish_shutdown(app));
            } else {
                api.prevent_exit();
                request_exit_after_shutdown(app, code.unwrap_or(0));
            }
        }
        #[cfg(target_os = "macos")]
        tauri::RunEvent::Reopen { .. } => {
            if let Err(error) = show_main_window(app) {
                error!(%error, "failed to restore Plankton after reopening the app");
            }
        }
        _ => {}
    }
}

#[cfg(target_os = "macos")]
pub fn setup_application_menu<R: Runtime>(app: &tauri::AppHandle<R>) -> tauri::Result<()> {
    use tauri::menu::{Menu, MenuItemBuilder, SubmenuBuilder};
    let close = MenuItemBuilder::with_id("plankton.close-current", "Close current window")
        .accelerator("Cmd+Q")
        .build(app)?;
    let app_menu = SubmenuBuilder::new(app, "Plankton")
        .about(None)
        .separator()
        .services()
        .separator()
        .hide()
        .hide_others()
        .separator()
        .item(&close)
        .text("plankton.explicit-quit", "Quit Plankton")
        .build()?;
    // Retain native Edit, View, Window and Help actions, but replace the app
    // menu so its predefined Cmd+Q quit cannot bypass the background workflow.
    let menu = Menu::default(app)?;
    menu.remove_at(0)?;
    menu.insert(&app_menu, 0)?;
    app.set_menu(menu)?;
    app.on_menu_event(|app, event| match event.id().as_ref() {
        "plankton.close-current" => {
            if let Err(error) = hide_current_user_window(app) {
                error!(%error, "failed to close current window from Command-Q");
            }
        }
        "plankton.explicit-quit" => request_explicit_quit(app),
        _ => {}
    });
    Ok(())
}

#[cfg(not(target_os = "macos"))]
pub fn setup_application_menu<R: Runtime>(_app: &tauri::AppHandle<R>) -> tauri::Result<()> {
    Ok(())
}

fn window_needs_dock(label: &str, visible: bool) -> bool {
    visible && label != crate::approval_notification::NOTIFICATION_LABEL
}

/// Dock presence follows all user windows, including confirmations. Passive
/// approval notifications never make a background Plankton app appear in Dock.
#[cfg(target_os = "macos")]
pub fn synchronize_dock_visibility<R: Runtime>(app: &tauri::AppHandle<R>) -> tauri::Result<()> {
    let mut visible_user_window = false;
    for (label, window) in app.webview_windows() {
        if window_needs_dock(&label, window.is_visible()?) {
            visible_user_window = true;
            break;
        }
    }
    app.set_activation_policy(if visible_user_window {
        tauri::ActivationPolicy::Regular
    } else {
        tauri::ActivationPolicy::Accessory
    })
}

#[cfg(not(target_os = "macos"))]
pub fn synchronize_dock_visibility<R: Runtime>(_app: &tauri::AppHandle<R>) -> tauri::Result<()> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{
        close_target, keep_running_after_exit_request, shutdown_once, window_needs_dock,
        ShutdownOutcome,
    };
    use std::{
        sync::atomic::{AtomicBool, AtomicUsize, Ordering},
        time::Duration,
    };
    use tokio::sync::OnceCell;

    #[tokio::test]
    async fn duplicate_exit_requests_wait_for_the_same_completed_shutdown() {
        let state = OnceCell::new();
        let calls = AtomicUsize::new(0);
        let shutdown = || async {
            calls.fetch_add(1, Ordering::SeqCst);
            tokio::task::yield_now().await;
            Ok(())
        };
        let (first, second) = tokio::join!(
            shutdown_once(&state, Duration::from_secs(1), shutdown),
            shutdown_once(&state, Duration::from_secs(1), shutdown)
        );
        assert_eq!(first, ShutdownOutcome::Completed);
        assert_eq!(second, ShutdownOutcome::Completed);
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn shutdown_deadline_drops_cleanup_future_and_does_not_retry() {
        struct CleanupGuard<'a>(&'a AtomicBool);
        impl Drop for CleanupGuard<'_> {
            fn drop(&mut self) {
                self.0.store(true, Ordering::SeqCst);
            }
        }
        let state = OnceCell::new();
        let dropped = AtomicBool::new(false);
        let outcome = shutdown_once(&state, Duration::from_millis(10), || async {
            let _guard = CleanupGuard(&dropped);
            std::future::pending::<Result<(), ()>>().await
        })
        .await;
        assert_eq!(outcome, ShutdownOutcome::TimedOut);
        assert!(dropped.load(Ordering::SeqCst));
        assert_eq!(
            shutdown_once(&state, Duration::from_secs(1), || async {
                panic!("a timed out shutdown must not start again")
            })
            .await,
            ShutdownOutcome::TimedOut
        );
    }

    #[tokio::test]
    async fn shutdown_failure_is_recorded_without_backend_error_contents() {
        let state = OnceCell::new();
        assert_eq!(
            shutdown_once(&state, Duration::from_secs(1), || async { Err(()) }).await,
            ShutdownOutcome::Failed
        );
    }

    #[test]
    fn command_q_closes_only_the_focused_window_including_notifications() {
        assert_eq!(
            close_target(&[("main", true, false), ("approval", true, true)]),
            Some("approval")
        );
        assert_eq!(
            close_target(&[("main", true, false), ("password-change", true, true)]),
            Some("password-change")
        );
        assert_eq!(
            close_target(&[("main", true, false), ("approval", true, false)]),
            Some("main")
        );
        assert_eq!(close_target(&[("main", false, false)]), None);
        assert_eq!(
            close_target(&[("main", true, false), ("approval-notification", true, true)]),
            Some("approval-notification")
        );
    }

    #[test]
    fn default_quit_keeps_running_but_explicit_quit_and_restart_exit() {
        assert!(keep_running_after_exit_request(false, None));
        assert!(!keep_running_after_exit_request(true, None));
        assert!(!keep_running_after_exit_request(true, Some(0)));
        assert!(!keep_running_after_exit_request(
            false,
            Some(tauri::RESTART_EXIT_CODE)
        ));
    }

    #[test]
    fn passive_notifications_do_not_need_a_dock_icon() {
        assert!(!window_needs_dock("approval-notification", true));
        assert!(!window_needs_dock("approval-notification", false));
    }

    #[test]
    fn any_visible_user_window_keeps_the_dock_icon() {
        assert!(window_needs_dock("main", true));
        assert!(window_needs_dock("approval", true));
        assert!(window_needs_dock("password-change", true));
        assert!(!window_needs_dock("main", false));
        assert!(!window_needs_dock("password-change", false));
    }
}
