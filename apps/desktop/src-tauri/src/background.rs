use std::sync::atomic::{AtomicBool, Ordering};

use tauri::{Manager, Runtime, WindowEvent};
use tracing::error;

static EXPLICIT_QUIT: AtomicBool = AtomicBool::new(false);
const MAIN_WINDOW_LABEL: &str = "main";

pub fn request_explicit_quit<R: Runtime>(app: &tauri::AppHandle<R>) {
    EXPLICIT_QUIT.store(true, Ordering::SeqCst);
    app.exit(0);
}

pub fn handle_window_event<R: Runtime>(window: &tauri::Window<R>, event: &WindowEvent) {
    if let WindowEvent::CloseRequested { api, .. } = event {
        match close_action(window.label(), EXPLICIT_QUIT.load(Ordering::SeqCst)) {
            CloseAction::Hide { hide_dock } => {
                api.prevent_close();
                match window.hide() {
                    Ok(()) if hide_dock => hide_dock_icon(window.app_handle()),
                    Ok(()) => {}
                    Err(error) => error!(
                        window = window.label(),
                        error = %error,
                        "failed to hide persistent Plankton window"
                    ),
                }
            }
            CloseAction::Exit => {}
        }
    }
}

pub fn show_main_window<R: Runtime>(app: &tauri::AppHandle<R>) -> tauri::Result<()> {
    show_dock_icon(app)?;
    if let Some(window) = app.get_webview_window(MAIN_WINDOW_LABEL) {
        window.show()?;
        window.unminimize()?;
        window.set_focus()?;
    }
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CloseAction {
    Hide { hide_dock: bool },
    Exit,
}

fn close_action(window_label: &str, explicit_quit: bool) -> CloseAction {
    if explicit_quit {
        CloseAction::Exit
    } else {
        CloseAction::Hide {
            hide_dock: window_label == MAIN_WINDOW_LABEL,
        }
    }
}

#[cfg(target_os = "macos")]
fn hide_dock_icon<R: Runtime>(app: &tauri::AppHandle<R>) {
    if let Err(error) = app.set_dock_visibility(false) {
        error!(%error, "failed to hide Plankton Dock icon");
    }
}

#[cfg(not(target_os = "macos"))]
fn hide_dock_icon<R: Runtime>(_app: &tauri::AppHandle<R>) {}

#[cfg(target_os = "macos")]
fn show_dock_icon<R: Runtime>(app: &tauri::AppHandle<R>) -> tauri::Result<()> {
    app.set_dock_visibility(true)
}

#[cfg(not(target_os = "macos"))]
fn show_dock_icon<R: Runtime>(_app: &tauri::AppHandle<R>) -> tauri::Result<()> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{close_action, CloseAction};

    #[test]
    fn ordinary_main_close_hides_window_and_dock() {
        assert_eq!(
            close_action("main", false),
            CloseAction::Hide { hide_dock: true }
        );
    }

    #[test]
    fn auxiliary_window_close_does_not_change_dock_visibility() {
        assert_eq!(
            close_action("compact-approval", false),
            CloseAction::Hide { hide_dock: false }
        );
    }

    #[test]
    fn explicit_quit_exits_from_any_window() {
        assert_eq!(close_action("main", true), CloseAction::Exit);
    }
}
