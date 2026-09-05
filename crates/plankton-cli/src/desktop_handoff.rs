use std::{
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

use anyhow::{Context, Result};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DesktopHandoff {
    route: &'static str,
    query: Vec<(&'static str, String)>,
}

impl DesktopHandoff {
    pub fn for_password_draft(draft_id: impl Into<String>) -> Self {
        Self {
            route: "password/add",
            query: vec![("draft_id", draft_id.into())],
        }
    }

    pub fn for_request_id(request_id: impl Into<String>) -> Self {
        Self {
            route: "review",
            query: vec![("request_id", request_id.into())],
        }
    }

    pub fn for_password_change(change_id: impl Into<String>) -> Self {
        Self {
            route: "password/change",
            query: vec![("change_id", change_id.into())],
        }
    }

    pub fn for_password_edit(item_id: impl Into<String>) -> Self {
        Self {
            route: "password/edit",
            query: vec![("item_id", item_id.into())],
        }
    }

    pub fn for_password_migration(
        item_id: impl Into<String>,
        backend: impl Into<String>,
        vault: impl Into<String>,
        mode: impl Into<String>,
    ) -> Self {
        Self {
            route: "password/migrate",
            query: vec![
                ("item_id", item_id.into()),
                ("backend", backend.into()),
                ("vault", vault.into()),
                ("mode", mode.into()),
            ],
        }
    }

    pub fn for_local_vault_manager() -> Self {
        Self {
            route: "password/vault",
            query: Vec::new(),
        }
    }

    pub fn deep_link_url(&self) -> String {
        let query = self
            .query
            .iter()
            .map(|(key, value)| format!("{key}={}", percent_encode(value)))
            .collect::<Vec<_>>()
            .join("&");
        if query.is_empty() {
            format!("plankton://{}", self.route)
        } else {
            format!("plankton://{}?{query}", self.route)
        }
    }
}

pub trait DesktopHandoffLauncher {
    fn launch(&self, handoff: &DesktopHandoff) -> Result<()>;
}

#[derive(Debug, Default)]
pub struct SystemDesktopHandoffLauncher;

impl DesktopHandoffLauncher for SystemDesktopHandoffLauncher {
    fn launch(&self, handoff: &DesktopHandoff) -> Result<()> {
        launch_handoff_url(&handoff.deep_link_url())
    }
}

pub fn trigger_password_draft_handoff(draft_id: impl Into<String>) -> Result<()> {
    SystemDesktopHandoffLauncher
        .launch(&DesktopHandoff::for_password_draft(draft_id))
        .context("password draft was created, but desktop confirmation handoff failed")
}

pub fn trigger_request_handoff(request_id: impl Into<String>) -> Result<()> {
    let request_id = request_id.into();
    SystemDesktopHandoffLauncher
        .launch(&DesktopHandoff::for_request_id(request_id.clone()))
        .with_context(|| format!("request {request_id} was submitted, but desktop handoff failed"))
}

pub fn trigger_password_change_handoff(change_id: impl Into<String>) -> Result<()> {
    let change_id = change_id.into();
    SystemDesktopHandoffLauncher
        .launch(&DesktopHandoff::for_password_change(change_id.clone()))
        .with_context(|| {
            format!(
                "password change {change_id} was staged, but desktop confirmation handoff failed"
            )
        })
}

pub fn trigger_password_edit_handoff(item_id: impl Into<String>) -> Result<()> {
    SystemDesktopHandoffLauncher
        .launch(&DesktopHandoff::for_password_edit(item_id))
        .context("password edit desktop handoff failed")
}

pub fn trigger_password_migration_handoff(
    item_id: impl Into<String>,
    backend: impl Into<String>,
    vault: impl Into<String>,
    mode: impl Into<String>,
) -> Result<()> {
    SystemDesktopHandoffLauncher
        .launch(&DesktopHandoff::for_password_migration(
            item_id, backend, vault, mode,
        ))
        .context("password migration desktop handoff failed")
}

pub fn trigger_local_vault_manager_handoff() -> Result<()> {
    SystemDesktopHandoffLauncher
        .launch(&DesktopHandoff::for_local_vault_manager())
        .context("local vault manager desktop handoff failed")
}

fn percent_encode(value: &str) -> String {
    value
        .bytes()
        .flat_map(|byte| match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                vec![char::from(byte)]
            }
            _ => format!("%{byte:02X}").chars().collect(),
        })
        .collect()
}

fn launch_handoff_url(url: &str) -> Result<()> {
    // A Cargo CLI should hand off to its matching development app, not an older installed bundle.
    if cfg!(debug_assertions) {
        if let Some(desktop) = std::env::current_exe()
            .ok()
            .and_then(|cli| development_desktop_executable(&cli))
        {
            Command::new(desktop)
                .arg(url)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .context("failed to open the development desktop confirmation window")?;
            return Ok(());
        }
    }

    let mut command = platform_handoff_command(url);
    let output = command
        .output()
        .with_context(|| format!("failed to launch desktop handoff URL {url}"))?;
    if !output.status.success() {
        anyhow::bail!(
            "desktop handoff launcher exited with {:?}: {}",
            output.status.code(),
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    Ok(())
}

fn development_desktop_executable(cli: &Path) -> Option<PathBuf> {
    let directory = cli.parent()?;
    if directory.file_name()? != "debug" {
        return None;
    }
    let candidate = directory.join(if cfg!(windows) {
        "plankton-desktop.exe"
    } else {
        "plankton-desktop"
    });
    candidate.is_file().then_some(candidate)
}

#[cfg(target_os = "macos")]
fn platform_handoff_command(url: &str) -> Command {
    let mut command = Command::new("open");
    command.arg(url);
    command
}

#[cfg(target_os = "windows")]
fn platform_handoff_command(url: &str) -> Command {
    let mut command = Command::new("cmd");
    command.args(["/C", "start", "", url]);
    command
}

#[cfg(all(unix, not(target_os = "macos")))]
fn platform_handoff_command(url: &str) -> Command {
    let mut command = Command::new("xdg-open");
    command.arg(url);
    command
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn development_handoff_requires_a_matching_debug_desktop_binary() {
        let directory = tempfile::tempdir().unwrap();
        let debug = directory.path().join("debug");
        std::fs::create_dir(&debug).unwrap();
        let cli = debug.join("plankton");
        assert_eq!(development_desktop_executable(&cli), None);
        let desktop = debug.join(if cfg!(windows) {
            "plankton-desktop.exe"
        } else {
            "plankton-desktop"
        });
        std::fs::write(&desktop, "fixture").unwrap();
        assert_eq!(development_desktop_executable(&cli), Some(desktop));
        assert_eq!(
            development_desktop_executable(&directory.path().join("plankton")),
            None
        );
    }

    #[test]
    fn handoff_uses_request_id_only_deep_link_payload() {
        let handoff = DesktopHandoff::for_request_id("request-123");
        assert_eq!(
            handoff.deep_link_url(),
            "plankton://review?request_id=request-123"
        );
    }

    #[test]
    fn password_draft_handoff_targets_add_popup() {
        assert_eq!(
            DesktopHandoff::for_password_draft("draft-123").deep_link_url(),
            "plankton://password/add?draft_id=draft-123"
        );
    }

    #[test]
    fn password_change_handoff_targets_confirmation_window() {
        assert_eq!(
            DesktopHandoff::for_password_change("chg-123").deep_link_url(),
            "plankton://password/change?change_id=chg-123"
        );
    }

    #[test]
    fn password_edit_handoff_contains_only_the_item_selector() {
        assert_eq!(
            DesktopHandoff::for_password_edit("Production API").deep_link_url(),
            "plankton://password/edit?item_id=Production%20API"
        );
    }

    #[test]
    fn password_migration_handoff_encodes_destination_without_values() {
        assert_eq!(
            DesktopHandoff::for_password_migration(
                "Production API",
                "one-password-main",
                "Team Vault",
                "move",
            )
            .deep_link_url(),
            "plankton://password/migrate?item_id=Production%20API&backend=one-password-main&vault=Team%20Vault&mode=move"
        );
    }

    #[test]
    fn local_vault_manager_handoff_has_no_secret_parameters() {
        assert_eq!(
            DesktopHandoff::for_local_vault_manager().deep_link_url(),
            "plankton://password/vault"
        );
    }
}
