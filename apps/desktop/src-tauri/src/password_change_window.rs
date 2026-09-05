use anyhow::{Context, Result};
use tauri::{AppHandle, Manager, Runtime, WebviewUrl, WebviewWindowBuilder};

pub const PASSWORD_CHANGE_LABEL: &str = "password-change";

pub fn show<R: Runtime>(app: &AppHandle<R>) -> Result<()> {
    let window = match app.get_webview_window(PASSWORD_CHANGE_LABEL) {
        Some(window) => window,
        None => WebviewWindowBuilder::new(
            app,
            PASSWORD_CHANGE_LABEL,
            WebviewUrl::App("index.html".into()),
        )
        .title("Plankton password change confirmation")
        .inner_size(720.0, 760.0)
        .min_inner_size(560.0, 520.0)
        .resizable(true)
        .minimizable(true)
        .center()
        .visible(false)
        .build()
        .context("failed to create password change confirmation window")?,
    };
    window
        .show()
        .context("failed to show password change window")?;
    window
        .unminimize()
        .context("failed to restore password change window")?;
    window
        .set_focus()
        .context("failed to focus password change window")?;
    Ok(())
}
