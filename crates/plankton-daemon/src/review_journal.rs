use std::{
    fs::{self, File, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result};
use chrono::Utc;
use plankton_core::{LlmSuggestion, PlanktonSettings};
use serde::Serialize;
use tokio::task;
use uuid::Uuid;

#[derive(Debug)]
pub(crate) struct ReviewJournal {
    directory: PathBuf,
    detail_sequence: u32,
}

#[derive(Serialize)]
struct ReviewManifest<'a> {
    request_id: &'a str,
    state: &'static str,
    created_at: chrono::DateTime<Utc>,
}

impl ReviewJournal {
    pub(crate) async fn create(settings: &PlanktonSettings, request_id: &str) -> Result<Self> {
        let directory = review_journal_root(settings).join(request_id);
        create_private_directory(directory.clone()).await?;
        write_json_atomic(
            directory.join("manifest.json"),
            &ReviewManifest {
                request_id,
                state: "running",
                created_at: Utc::now(),
            },
        )
        .await?;
        Ok(Self {
            directory,
            detail_sequence: 0,
        })
    }

    pub(crate) async fn record_decision(&self, suggestion: &LlmSuggestion) -> Result<()> {
        write_json_atomic(self.directory.join("decision.json"), suggestion).await
    }

    pub(crate) async fn record_details(&mut self, suggestion: &LlmSuggestion) -> Result<()> {
        self.detail_sequence = self.detail_sequence.saturating_add(1);
        write_json_atomic(
            self.directory
                .join("details")
                .join(format!("{:06}.json", self.detail_sequence)),
            suggestion,
        )
        .await
    }

    pub(crate) async fn complete(&self, suggestion: &LlmSuggestion) -> Result<()> {
        write_json_atomic(self.directory.join("complete.json"), suggestion).await
    }
}

fn review_journal_root(settings: &PlanktonSettings) -> PathBuf {
    let database = settings.database_url.strip_prefix("sqlite://");
    if let Some(database) = database {
        let path = Path::new(database);
        if database != ":memory:" {
            if let Some(parent) = path.parent() {
                return parent.join("approval-runs");
            }
        }
    }
    directories::ProjectDirs::from("com", "OpenAquarium", "Plankton")
        .map(|directories| directories.data_local_dir().join("approval-runs"))
        .unwrap_or_else(|| std::env::temp_dir().join("plankton-approval-runs"))
}

async fn create_private_directory(directory: PathBuf) -> Result<()> {
    task::spawn_blocking(move || {
        fs::create_dir_all(&directory)
            .with_context(|| format!("create review journal directory {}", directory.display()))?;
        set_private_directory_permissions(&directory)?;
        Ok(())
    })
    .await
    .context("join review journal directory task")?
}

async fn write_json_atomic<T: Serialize>(path: PathBuf, value: &T) -> Result<()> {
    let bytes = serde_json::to_vec_pretty(value).context("serialize review journal entry")?;
    task::spawn_blocking(move || write_bytes_atomic(&path, &bytes))
        .await
        .context("join review journal write task")?
}

fn write_bytes_atomic(path: &Path, bytes: &[u8]) -> Result<()> {
    let parent = path
        .parent()
        .with_context(|| format!("review journal path has no parent: {}", path.display()))?;
    fs::create_dir_all(parent)
        .with_context(|| format!("create review journal directory {}", parent.display()))?;
    set_private_directory_permissions(parent)?;
    let temporary = parent.join(format!(
        ".{}.{}.tmp",
        path.file_name().unwrap_or_default().to_string_lossy(),
        Uuid::new_v4()
    ));
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    set_private_file_mode(&mut options);
    let mut file = options
        .open(&temporary)
        .with_context(|| format!("create review journal file {}", temporary.display()))?;
    file.write_all(bytes)
        .with_context(|| format!("write review journal file {}", temporary.display()))?;
    file.write_all(b"\n")
        .with_context(|| format!("finish review journal file {}", temporary.display()))?;
    file.sync_all()
        .with_context(|| format!("sync review journal file {}", temporary.display()))?;
    fs::rename(&temporary, path).with_context(|| {
        format!(
            "replace review journal file {} with {}",
            temporary.display(),
            path.display()
        )
    })?;
    File::open(parent)
        .and_then(|directory| directory.sync_all())
        .with_context(|| format!("sync review journal directory {}", parent.display()))?;
    Ok(())
}

#[cfg(unix)]
fn set_private_file_mode(options: &mut OpenOptions) {
    use std::os::unix::fs::OpenOptionsExt;
    options.mode(0o600);
}

#[cfg(not(unix))]
fn set_private_file_mode(_options: &mut OpenOptions) {}

#[cfg(unix)]
fn set_private_directory_permissions(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o700))
        .with_context(|| format!("secure review journal directory {}", path.display()))
}

#[cfg(not(unix))]
fn set_private_directory_permissions(_path: &Path) -> Result<()> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use plankton_core::load_settings;
    use tempfile::tempdir;

    use super::review_journal_root;

    #[test]
    fn journal_is_stored_beside_the_configured_database() {
        let directory = tempdir().expect("temporary directory");
        let mut settings = load_settings().expect("settings");
        settings.database_url = format!("sqlite://{}", directory.path().join("store.db").display());

        assert_eq!(
            review_journal_root(&settings),
            directory.path().join("approval-runs")
        );
    }
}
