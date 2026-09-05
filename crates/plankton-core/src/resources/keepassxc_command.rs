use std::{
    ffi::OsString,
    path::{Path, PathBuf},
    process::Stdio,
    time::Duration,
};

use sha2::{Digest, Sha256};
use tokio::{
    io::AsyncWriteExt,
    process::Command,
    time::{error::Elapsed, timeout},
};

#[derive(Debug, Clone)]
pub struct KeepassxcCommandRunner {
    executable: PathBuf,
    expected_sha256: String,
    timeout: Duration,
}

impl KeepassxcCommandRunner {
    pub fn new(
        executable: impl Into<PathBuf>,
        expected_sha256: impl Into<String>,
        timeout: Duration,
    ) -> Self {
        Self {
            executable: executable.into(),
            expected_sha256: expected_sha256.into(),
            timeout,
        }
    }

    pub fn verify_engine(&self) -> Result<(), KeepassxcCommandError> {
        let bytes = std::fs::read(&self.executable).map_err(KeepassxcCommandError::ReadEngine)?;
        let actual = format!("{:x}", Sha256::digest(bytes));
        if actual != self.expected_sha256 {
            return Err(KeepassxcCommandError::ChecksumMismatch {
                expected: self.expected_sha256.clone(),
                actual,
            });
        }
        Ok(())
    }

    pub async fn run(
        &self,
        operation: KeepassxcOperation,
        unlock_secret: &str,
    ) -> Result<String, KeepassxcCommandError> {
        self.run_with_stdin(operation, format!("{unlock_secret}\n"))
            .await
    }

    pub async fn run_with_stdin(
        &self,
        operation: KeepassxcOperation,
        stdin_content: String,
    ) -> Result<String, KeepassxcCommandError> {
        self.verify_engine()?;
        let mut args = Vec::new();
        if self
            .executable
            .extension()
            .and_then(|extension| extension.to_str())
            .is_some_and(|extension| extension.eq_ignore_ascii_case("appimage"))
        {
            args.push(OsString::from("cli"));
        }
        args.extend(operation.args());
        let mut child = Command::new(&self.executable)
            .args(&args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .map_err(KeepassxcCommandError::Spawn)?;
        let mut stdin = child
            .stdin
            .take()
            .ok_or(KeepassxcCommandError::MissingStdin)?;
        stdin
            .write_all(stdin_content.as_bytes())
            .await
            .map_err(KeepassxcCommandError::WriteStdin)?;
        drop(stdin);

        let output = timeout(self.timeout, child.wait_with_output())
            .await
            .map_err(KeepassxcCommandError::Timeout)?
            .map_err(KeepassxcCommandError::Wait)?;
        if !output.status.success() {
            return Err(KeepassxcCommandError::Exit {
                code: output.status.code(),
                stderr: String::from_utf8_lossy(&output.stderr).trim().to_string(),
            });
        }
        String::from_utf8(output.stdout).map_err(KeepassxcCommandError::Utf8)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KeepassxcOperation {
    Version,
    CreateDatabase {
        database: PathBuf,
    },
    Add {
        database: PathBuf,
        entry: String,
        username: String,
        notes: String,
    },
    Edit {
        database: PathBuf,
        entry: String,
        username: String,
        notes: String,
    },
    List {
        database: PathBuf,
        group: Option<String>,
    },
    Show {
        database: PathBuf,
        entry: String,
        attributes: bool,
    },
    Remove {
        database: PathBuf,
        entry: String,
    },
    Merge {
        destination: PathBuf,
        source: PathBuf,
    },
}

impl KeepassxcOperation {
    pub fn args(&self) -> Vec<OsString> {
        match self {
            Self::Version => vec!["--version".into()],
            Self::CreateDatabase { database } => vec![
                "db-create".into(),
                "--set-password".into(),
                database.as_os_str().to_owned(),
            ],
            Self::Add {
                database,
                entry,
                username,
                notes,
            } => vec![
                "add".into(),
                "--password-prompt".into(),
                "--username".into(),
                username.into(),
                "--notes".into(),
                notes.into(),
                database.as_os_str().to_owned(),
                entry.into(),
            ],
            Self::Edit {
                database,
                entry,
                username,
                notes,
            } => vec![
                "edit".into(),
                "--password-prompt".into(),
                "--username".into(),
                username.into(),
                "--notes".into(),
                notes.into(),
                database.as_os_str().to_owned(),
                entry.into(),
            ],
            Self::List { database, group } => {
                let mut args = vec!["ls".into(), database.as_os_str().to_owned()];
                if let Some(group) = group {
                    args.push(group.into());
                }
                args
            }
            Self::Show {
                database,
                entry,
                attributes,
            } => {
                let mut args = vec!["show".into()];
                if *attributes {
                    args.push("--show-attributes".into());
                }
                args.extend([database.as_os_str().to_owned(), entry.into()]);
                args
            }
            Self::Remove { database, entry } => vec![
                "rm".into(),
                "--quiet".into(),
                database.as_os_str().to_owned(),
                entry.into(),
            ],
            Self::Merge {
                destination,
                source,
            } => vec![
                "merge".into(),
                "--same-credentials".into(),
                destination.as_os_str().to_owned(),
                source.as_os_str().to_owned(),
            ],
        }
    }

    pub fn touches_only(&self, expected_database: &Path) -> bool {
        match self {
            Self::Version => true,
            Self::CreateDatabase { database }
            | Self::Add { database, .. }
            | Self::Edit { database, .. } => database == expected_database,
            Self::List { database, .. }
            | Self::Show { database, .. }
            | Self::Remove { database, .. } => database == expected_database,
            Self::Merge { destination, .. } => destination == expected_database,
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum KeepassxcCommandError {
    #[error("failed to read bundled KeePassXC engine: {0}")]
    ReadEngine(#[source] std::io::Error),
    #[error("KeePassXC engine checksum mismatch: expected {expected}, received {actual}")]
    ChecksumMismatch { expected: String, actual: String },
    #[error("failed to start KeePassXC engine: {0}")]
    Spawn(#[source] std::io::Error),
    #[error("KeePassXC engine stdin was unavailable")]
    MissingStdin,
    #[error("failed to write KeePassXC unlock input: {0}")]
    WriteStdin(#[source] std::io::Error),
    #[error("KeePassXC operation timed out: {0}")]
    Timeout(#[source] Elapsed),
    #[error("failed to wait for KeePassXC engine: {0}")]
    Wait(#[source] std::io::Error),
    #[error("KeePassXC engine exited with {code:?}: {stderr}")]
    Exit { code: Option<i32>, stderr: String },
    #[error("KeePassXC engine emitted invalid UTF-8: {0}")]
    Utf8(#[source] std::string::FromUtf8Error),
}

#[cfg(test)]
mod tests {
    use std::{ffi::OsString, path::PathBuf};

    use super::KeepassxcOperation;

    #[test]
    fn remove_operation_targets_only_the_selected_database() {
        let database = PathBuf::from("/tmp/demo.kdbx");
        let operation = KeepassxcOperation::Remove {
            database: database.clone(),
            entry: "API token".to_string(),
        };

        assert_eq!(
            operation.args(),
            vec![
                OsString::from("rm"),
                OsString::from("--quiet"),
                database.as_os_str().to_owned(),
                OsString::from("API token"),
            ]
        );
        assert!(operation.touches_only(database.as_path()));
        assert!(!operation.touches_only(PathBuf::from("/tmp/other.kdbx").as_path()));
    }

    #[test]
    fn edit_prompts_for_the_new_password_without_putting_it_in_argv() {
        let database = PathBuf::from("/tmp/demo.kdbx");
        let operation = KeepassxcOperation::Edit {
            database: database.clone(),
            entry: "API token".to_string(),
            username: "TOKEN".to_string(),
            notes: "Managed by Plankton".to_string(),
        };
        let args = operation.args();
        assert_eq!(args[0], OsString::from("edit"));
        assert!(args.contains(&OsString::from("--password-prompt")));
        assert!(!args.contains(&OsString::from("new-secret")));
        assert!(operation.touches_only(&database));
    }

    #[test]
    fn merge_uses_same_credentials_and_writes_only_the_destination_database() {
        let destination = PathBuf::from("/tmp/local.kdbx");
        let source = PathBuf::from("/tmp/remote.kdbx");
        let operation = KeepassxcOperation::Merge {
            destination: destination.clone(),
            source: source.clone(),
        };

        assert_eq!(
            operation.args(),
            vec![
                OsString::from("merge"),
                OsString::from("--same-credentials"),
                destination.as_os_str().to_owned(),
                source.as_os_str().to_owned(),
            ]
        );
        assert!(operation.touches_only(&destination));
        assert!(!operation.touches_only(&source));
    }
}
