//! Optional synchronization for already-encrypted KDBX vault files.
//!
//! This module deliberately has no password, entry, or field-value types.  A caller must turn a
//! vault file into [`EncryptedVaultBlob`] before it can cross this boundary.

use std::{
    fmt, io,
    path::{Component, Path, PathBuf},
    time::Duration,
};

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tokio::{io::AsyncWriteExt, process::Command};
use uuid::Uuid;

const KDBX_SIGNATURE: [u8; 4] = [0x03, 0xd9, 0xa2, 0x9a];
const KDBX_SECOND_SIGNATURE: [u8; 4] = [0x67, 0xfb, 0x4b, 0xb5];
const GIT_COMMIT_MESSAGE: &str = "plankton encrypted vault sync";

/// A KDBX file after KeePassXC has encrypted it. Its bytes are intentionally private.
#[derive(Clone, PartialEq, Eq)]
pub struct EncryptedVaultBlob {
    bytes: Vec<u8>,
}

impl fmt::Debug for EncryptedVaultBlob {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("EncryptedVaultBlob")
            .field("length", &self.bytes.len())
            .field("sha256", &self.sha256())
            .finish()
    }
}

impl EncryptedVaultBlob {
    /// Validates the public KDBX signatures. This rejects accidental JSON/CSV/plaintext input;
    /// authentication and decryption remain KeePassXC's responsibility.
    pub fn from_kdbx_bytes(bytes: Vec<u8>) -> Result<Self, SyncError> {
        let header_is_valid = bytes.starts_with(&KDBX_SIGNATURE)
            && bytes
                .get(KDBX_SIGNATURE.len()..KDBX_SIGNATURE.len() + KDBX_SECOND_SIGNATURE.len())
                .is_some_and(|signature| signature == KDBX_SECOND_SIGNATURE);
        if !header_is_valid {
            return Err(SyncError::InvalidEncryptedBlob);
        }
        Ok(Self { bytes })
    }

    pub fn as_bytes(&self) -> &[u8] {
        &self.bytes
    }

    pub fn sha256(&self) -> String {
        sha256_hex(&self.bytes)
    }
}

/// Opaque server-side version used for compare-and-swap. It is not a vault secret.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct VersionToken(pub u64);

/// The only metadata exchanged alongside a KDBX blob.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SyncMetadata {
    pub version: VersionToken,
    pub sha256: String,
}

impl SyncMetadata {
    pub fn for_blob(version: VersionToken, blob: &EncryptedVaultBlob) -> Self {
        Self {
            version,
            sha256: blob.sha256(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RetryPolicy {
    pub max_attempts: u8,
    pub retry_delay: Duration,
}

impl Default for RetryPolicy {
    fn default() -> Self {
        Self {
            max_attempts: 3,
            retry_delay: Duration::from_millis(250),
        }
    }
}

/// Remote synchronization is opt-in. A default-constructed engine cannot contact any remote.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SyncConfiguration {
    pub enabled: bool,
    pub retry: RetryPolicy,
}

#[derive(Debug, thiserror::Error)]
pub enum SyncError {
    #[error("vault synchronization is disabled")]
    Disabled,
    #[error("the remote is offline after {attempts} attempt(s)")]
    Offline { attempts: u8 },
    #[error("remote authentication failed")]
    Authentication,
    #[error("sync credential resolution failed: {0}")]
    Credential(String),
    #[error("remote version conflict (expected {expected:?}, actual {actual:?})")]
    Conflict {
        expected: Option<VersionToken>,
        actual: Option<VersionToken>,
    },
    #[error(
        "local vault changed since the last successful sync (expected hash {expected_hash:?}, actual hash {actual_hash})"
    )]
    LocalDivergence {
        expected_hash: Option<String>,
        actual_hash: String,
    },
    #[error("remote returned an invalid encrypted vault blob: {reason}")]
    InvalidRemoteBlob { reason: String },
    #[error("value is not a KDBX encrypted vault blob")]
    InvalidEncryptedBlob,
    #[error("remote vault was not found")]
    NotFound,
    #[error("the matching unlock file is required before this vault can synchronize")]
    UnlockRequired,
    #[error("the selected unlock file does not open both synchronized vault copies")]
    UnlockMismatch,
    #[error("encrypted vault merge failed: {0}")]
    Merge(String),
    #[error("sync transport failed: {0}")]
    Transport(String),
    #[error("filesystem operation failed: {0}")]
    Io(#[from] io::Error),
    #[error("remote metadata could not be decoded: {0}")]
    InvalidMetadata(String),
    #[error("git invocation is not allowlisted")]
    GitCommandNotAllowed,
    #[error("git command failed: {0}")]
    Git(String),
}

/// A transport-independent remote contract. Pull responses are raw untrusted bytes so the core
/// validates the KDBX signature and SHA-256 before atomically replacing a local vault.
#[async_trait]
pub trait SyncRemote: Send + Sync {
    async fn pull(&self) -> Result<RemoteBlob, SyncError>;
    async fn push(
        &self,
        blob: &EncryptedVaultBlob,
        metadata: &SyncMetadata,
        expected_version: Option<VersionToken>,
    ) -> Result<SyncMetadata, SyncError>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteBlob {
    pub bytes: Vec<u8>,
    pub metadata: SyncMetadata,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SyncPlan {
    Upload,
    Download,
    Merge,
    UpToDate,
}

pub fn plan_sync(
    local: Option<&EncryptedVaultBlob>,
    remote: Option<&RemoteBlob>,
    base_hash: Option<&str>,
) -> Result<SyncPlan, SyncError> {
    match (local, remote) {
        (None, None) => Err(SyncError::NotFound),
        (Some(_), None) => Ok(SyncPlan::Upload),
        (None, Some(_)) => Ok(SyncPlan::Download),
        (Some(local), Some(remote)) => {
            let remote_hash = remote.metadata.sha256.as_str();
            let local_hash = local.sha256();
            if local_hash == remote_hash {
                return Ok(SyncPlan::UpToDate);
            }
            match base_hash {
                Some(base) if local_hash == base => Ok(SyncPlan::Download),
                Some(base) if remote_hash == base => Ok(SyncPlan::Upload),
                _ => Ok(SyncPlan::Merge),
            }
        }
    }
}

/// Coordinates typed errors, bounded offline retry, blob validation, and atomic local writes.
#[derive(Debug, Clone)]
pub struct SyncEngine {
    configuration: SyncConfiguration,
}

impl Default for SyncEngine {
    fn default() -> Self {
        Self::new(SyncConfiguration::default())
    }
}

impl SyncEngine {
    pub fn new(configuration: SyncConfiguration) -> Self {
        Self { configuration }
    }

    pub async fn push<R: SyncRemote>(
        &self,
        remote: &R,
        blob: &EncryptedVaultBlob,
        expected_version: Option<VersionToken>,
    ) -> Result<SyncMetadata, SyncError> {
        self.ensure_enabled()?;
        let metadata = SyncMetadata::for_blob(expected_version.unwrap_or(VersionToken(0)), blob);
        let returned = self
            .retry(|| remote.push(blob, &metadata, expected_version))
            .await?;
        if returned.sha256 != blob.sha256() {
            return Err(SyncError::InvalidRemoteBlob {
                reason: "push response SHA-256 does not match the uploaded KDBX blob".to_owned(),
            });
        }
        Ok(returned)
    }

    pub async fn pull_to_path<R: SyncRemote>(
        &self,
        remote: &R,
        destination: &Path,
    ) -> Result<SyncMetadata, SyncError> {
        let remote_blob = self.fetch(remote).await?.ok_or(SyncError::NotFound)?;
        let blob = EncryptedVaultBlob::from_kdbx_bytes(remote_blob.bytes)?;
        write_blob_atomically(destination, &blob).await?;
        Ok(remote_blob.metadata)
    }

    pub async fn fetch<R: SyncRemote>(&self, remote: &R) -> Result<Option<RemoteBlob>, SyncError> {
        self.ensure_enabled()?;
        let remote_blob = match self.retry(|| remote.pull()).await {
            Ok(remote_blob) => remote_blob,
            Err(SyncError::NotFound) => return Ok(None),
            Err(error) => return Err(error),
        };
        let blob = EncryptedVaultBlob::from_kdbx_bytes(remote_blob.bytes).map_err(|_| {
            SyncError::InvalidRemoteBlob {
                reason: "KDBX signature is missing or malformed".to_owned(),
            }
        })?;
        if blob.sha256() != remote_blob.metadata.sha256 {
            return Err(SyncError::InvalidRemoteBlob {
                reason: "SHA-256 does not match the supplied metadata".to_owned(),
            });
        }
        Ok(Some(RemoteBlob {
            bytes: blob.bytes,
            metadata: remote_blob.metadata,
        }))
    }

    fn ensure_enabled(&self) -> Result<(), SyncError> {
        if self.configuration.enabled {
            Ok(())
        } else {
            Err(SyncError::Disabled)
        }
    }

    async fn retry<T, F, Future>(&self, mut operation: F) -> Result<T, SyncError>
    where
        F: FnMut() -> Future,
        Future: std::future::Future<Output = Result<T, SyncError>>,
    {
        let attempts = self.configuration.retry.max_attempts.max(1);
        for attempt in 1..=attempts {
            match operation().await {
                Ok(value) => return Ok(value),
                Err(SyncError::Offline { .. }) if attempt < attempts => {
                    tokio::time::sleep(self.configuration.retry.retry_delay).await;
                }
                Err(SyncError::Offline { .. }) => {
                    return Err(SyncError::Offline { attempts: attempt })
                }
                Err(error) => return Err(error),
            }
        }
        unreachable!("a nonzero retry count always returns")
    }
}

async fn write_blob_atomically(
    destination: &Path,
    blob: &EncryptedVaultBlob,
) -> Result<(), SyncError> {
    let parent = destination.parent().ok_or_else(|| {
        SyncError::Io(io::Error::new(
            io::ErrorKind::InvalidInput,
            "vault destination has no parent directory",
        ))
    })?;
    tokio::fs::create_dir_all(parent).await?;
    let file_name = destination.file_name().ok_or_else(|| {
        SyncError::Io(io::Error::new(
            io::ErrorKind::InvalidInput,
            "vault destination has no filename",
        ))
    })?;
    let temporary = parent.join(format!(
        ".{}.plankton-sync-{}.tmp",
        file_name.to_string_lossy(),
        Uuid::new_v4()
    ));
    let mut file = tokio::fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&temporary)
        .await?;
    file.write_all(blob.as_bytes()).await?;
    file.sync_all().await?;
    drop(file);
    tokio::fs::rename(&temporary, destination).await?;
    Ok(())
}

fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SyncOperation {
    Pull,
    Push,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HttpSyncKind {
    WebDav,
    Custom,
}

/// The request body is deliberately typed: only an [`EncryptedVaultBlob`] can be sent.
#[derive(Debug, Clone)]
pub struct HttpRequest {
    pub endpoint: String,
    pub kind: HttpSyncKind,
    pub operation: SyncOperation,
    pub expected_version: Option<VersionToken>,
    pub blob: Option<EncryptedVaultBlob>,
    pub metadata: Option<SyncMetadata>,
}

impl HttpRequest {
    /// Kept for auditing/tests: this transport contract has no plaintext field channel.
    pub fn plaintext_fields(&self) -> &[String] {
        &[]
    }
}

#[derive(Debug, Clone)]
pub struct HttpResponse {
    pub status: u16,
    pub blob: Option<Vec<u8>>,
    pub metadata: Option<SyncMetadata>,
}

impl HttpResponse {
    pub fn pulled(blob: Vec<u8>, metadata: SyncMetadata) -> Self {
        Self {
            status: 200,
            blob: Some(blob),
            metadata: Some(metadata),
        }
    }

    pub fn pushed(metadata: SyncMetadata) -> Self {
        Self {
            status: 201,
            blob: None,
            metadata: Some(metadata),
        }
    }
}

/// Adapter boundary for WebDAV and application-specific HTTP clients. Production applications can
/// implement it with reqwest; tests use a fake transport and never open a network connection.
#[async_trait]
pub trait HttpTransport: Send + Sync {
    async fn execute(&self, request: HttpRequest) -> Result<HttpResponse, SyncError>;
}

/// Cross-platform HTTPS transport for WebDAV-compatible endpoints and custom blob services.
///
/// The endpoint exchanges only `application/x-keepass2` bytes. Version and digest metadata travel
/// in `x-plankton-version` and `x-plankton-sha256` headers.
#[derive(Clone)]
pub struct ReqwestHttpTransport {
    client: reqwest::Client,
    bearer_token: Option<String>,
}

impl ReqwestHttpTransport {
    pub fn new(bearer_token: Option<String>) -> Result<Self, SyncError> {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(30))
            .build()
            .map_err(|error| SyncError::Transport(error.to_string()))?;
        Ok(Self {
            client,
            bearer_token,
        })
    }
}

#[async_trait]
impl HttpTransport for ReqwestHttpTransport {
    async fn execute(&self, request: HttpRequest) -> Result<HttpResponse, SyncError> {
        let mut builder = match request.operation {
            SyncOperation::Pull => self.client.get(&request.endpoint),
            SyncOperation::Push if request.kind == HttpSyncKind::WebDav => {
                self.client.put(&request.endpoint)
            }
            SyncOperation::Push => self.client.post(&request.endpoint),
        };
        if let Some(token) = self.bearer_token.as_deref() {
            builder = builder.bearer_auth(token);
        }
        if let Some(expected) = request.expected_version {
            builder = builder.header("if-match", expected.0.to_string());
        }
        if let Some(metadata) = request.metadata.as_ref() {
            builder = builder
                .header("x-plankton-version", metadata.version.0.to_string())
                .header("x-plankton-sha256", &metadata.sha256);
        }
        if let Some(blob) = request.blob.as_ref() {
            builder = builder
                .header("content-type", "application/x-keepass2")
                .body(blob.as_bytes().to_vec());
        }
        let response = builder
            .send()
            .await
            .map_err(|error| SyncError::Transport(error.to_string()))?;
        let status = response.status().as_u16();
        let response_metadata = parse_http_metadata(response.headers())?;
        let body = response
            .bytes()
            .await
            .map_err(|error| SyncError::Transport(error.to_string()))?
            .to_vec();
        let metadata = match response_metadata {
            Some(metadata) => Some(metadata),
            None if request.operation == SyncOperation::Push => {
                request.metadata.map(|mut metadata| {
                    metadata.version = VersionToken(
                        request
                            .expected_version
                            .map_or(1, |value| value.0.saturating_add(1)),
                    );
                    metadata
                })
            }
            _ => None,
        };
        Ok(HttpResponse {
            status,
            blob: (request.operation == SyncOperation::Pull).then_some(body),
            metadata,
        })
    }
}

fn parse_http_metadata(
    headers: &reqwest::header::HeaderMap,
) -> Result<Option<SyncMetadata>, SyncError> {
    let version = headers.get("x-plankton-version");
    let sha256 = headers.get("x-plankton-sha256");
    match (version, sha256) {
        (None, None) => Ok(None),
        (Some(version), Some(sha256)) => {
            let version = version
                .to_str()
                .map_err(|error| SyncError::InvalidMetadata(error.to_string()))?
                .parse::<u64>()
                .map_err(|error| SyncError::InvalidMetadata(error.to_string()))?;
            let sha256 = sha256
                .to_str()
                .map_err(|error| SyncError::InvalidMetadata(error.to_string()))?
                .to_owned();
            if sha256.len() != 64 || !sha256.bytes().all(|byte| byte.is_ascii_hexdigit()) {
                return Err(SyncError::InvalidMetadata(
                    "x-plankton-sha256 must contain 64 hexadecimal characters".to_owned(),
                ));
            }
            Ok(Some(SyncMetadata {
                version: VersionToken(version),
                sha256,
            }))
        }
        _ => Err(SyncError::InvalidMetadata(
            "x-plankton-version and x-plankton-sha256 must be returned together".to_owned(),
        )),
    }
}

pub struct HttpSyncRemote<T> {
    endpoint: String,
    kind: HttpSyncKind,
    transport: T,
}

impl<T> HttpSyncRemote<T> {
    pub fn webdav(endpoint: impl Into<String>, transport: T) -> Self {
        Self {
            endpoint: endpoint.into(),
            kind: HttpSyncKind::WebDav,
            transport,
        }
    }

    pub fn custom(endpoint: impl Into<String>, transport: T) -> Self {
        Self {
            endpoint: endpoint.into(),
            kind: HttpSyncKind::Custom,
            transport,
        }
    }
}

#[async_trait]
impl<T: HttpTransport> SyncRemote for HttpSyncRemote<T> {
    async fn pull(&self) -> Result<RemoteBlob, SyncError> {
        let response = self
            .transport
            .execute(HttpRequest {
                endpoint: self.endpoint.clone(),
                kind: self.kind,
                operation: SyncOperation::Pull,
                expected_version: None,
                blob: None,
                metadata: None,
            })
            .await?;
        map_http_status(&response)?;
        Ok(RemoteBlob {
            bytes: response.blob.ok_or_else(|| SyncError::InvalidRemoteBlob {
                reason: "HTTP response did not include a KDBX blob".to_owned(),
            })?,
            metadata: response.metadata.ok_or_else(|| {
                SyncError::InvalidMetadata(
                    "HTTP response did not include synchronization metadata".to_owned(),
                )
            })?,
        })
    }

    async fn push(
        &self,
        blob: &EncryptedVaultBlob,
        metadata: &SyncMetadata,
        expected_version: Option<VersionToken>,
    ) -> Result<SyncMetadata, SyncError> {
        let response = self
            .transport
            .execute(HttpRequest {
                endpoint: self.endpoint.clone(),
                kind: self.kind,
                operation: SyncOperation::Push,
                expected_version,
                blob: Some(blob.clone()),
                metadata: Some(metadata.clone()),
            })
            .await?;
        map_http_status(&response)?;
        response.metadata.ok_or_else(|| {
            SyncError::InvalidMetadata(
                "HTTP push response did not include synchronization metadata".to_owned(),
            )
        })
    }
}

fn map_http_status(response: &HttpResponse) -> Result<(), SyncError> {
    match response.status {
        200 | 201 | 204 => Ok(()),
        401 | 403 => Err(SyncError::Authentication),
        409 | 412 => Err(SyncError::Conflict {
            expected: None,
            actual: response.metadata.as_ref().map(|metadata| metadata.version),
        }),
        500..=599 => Err(SyncError::Offline { attempts: 1 }),
        status => Err(SyncError::Transport(format!(
            "unexpected HTTP status {status}"
        ))),
    }
}

/// Folder adapter for removable drives and cloud-synced folders. The folder holds exactly one
/// binary KDBX blob plus a JSON metadata sidecar; neither contains entry fields or passwords.
#[derive(Debug, Clone)]
pub struct LocalFolderRemote {
    directory: PathBuf,
    file_name: String,
}

impl LocalFolderRemote {
    pub fn new(
        directory: impl Into<PathBuf>,
        file_name: impl Into<String>,
    ) -> Result<Self, SyncError> {
        let file_name = file_name.into();
        if !is_safe_relative_file_name(&file_name) {
            return Err(SyncError::Transport(
                "local folder filename must be a single relative path".to_owned(),
            ));
        }
        Ok(Self {
            directory: directory.into(),
            file_name,
        })
    }

    fn blob_path(&self) -> PathBuf {
        self.directory.join(&self.file_name)
    }

    fn metadata_path(&self) -> PathBuf {
        self.directory
            .join(format!("{}.plankton-sync.json", self.file_name))
    }
}

#[async_trait]
impl SyncRemote for LocalFolderRemote {
    async fn pull(&self) -> Result<RemoteBlob, SyncError> {
        let bytes = tokio::fs::read(self.blob_path())
            .await
            .map_err(map_local_read_error)?;
        let metadata = read_metadata(&self.metadata_path()).await?;
        Ok(RemoteBlob { bytes, metadata })
    }

    async fn push(
        &self,
        blob: &EncryptedVaultBlob,
        metadata: &SyncMetadata,
        expected_version: Option<VersionToken>,
    ) -> Result<SyncMetadata, SyncError> {
        tokio::fs::create_dir_all(&self.directory).await?;
        let lock_path = self
            .directory
            .join(format!("{}.plankton-sync.lock", self.file_name));
        acquire_local_lock(&lock_path).await?;
        let result = self.push_locked(blob, metadata, expected_version).await;
        let cleanup = tokio::fs::remove_file(&lock_path).await;
        match (result, cleanup) {
            (Err(error), _) => Err(error),
            (Ok(_), Err(error)) => Err(SyncError::Io(error)),
            (Ok(metadata), Ok(())) => Ok(metadata),
        }
    }
}

impl LocalFolderRemote {
    async fn push_locked(
        &self,
        blob: &EncryptedVaultBlob,
        metadata: &SyncMetadata,
        expected_version: Option<VersionToken>,
    ) -> Result<SyncMetadata, SyncError> {
        let metadata_path = self.metadata_path();
        match read_metadata(&metadata_path).await {
            Ok(current) if Some(current.version) != expected_version => {
                return Err(SyncError::Conflict {
                    expected: expected_version,
                    actual: Some(current.version),
                });
            }
            Err(SyncError::NotFound) if expected_version.is_none() => {}
            Err(SyncError::NotFound) => {
                return Err(SyncError::Conflict {
                    expected: expected_version,
                    actual: None,
                });
            }
            Err(error) => return Err(error),
            Ok(_) => {}
        }
        let next = SyncMetadata::for_blob(VersionToken(metadata.version.0.saturating_add(1)), blob);
        write_blob_atomically(&self.blob_path(), blob).await?;
        write_metadata_atomically(&metadata_path, &next).await?;
        Ok(next)
    }
}

async fn acquire_local_lock(path: &Path) -> Result<(), SyncError> {
    const MAX_LOCK_ATTEMPTS: u8 = 100;
    for attempt in 1..=MAX_LOCK_ATTEMPTS {
        match tokio::fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(path)
            .await
        {
            Ok(file) => {
                drop(file);
                return Ok(());
            }
            Err(error)
                if error.kind() == io::ErrorKind::AlreadyExists && attempt < MAX_LOCK_ATTEMPTS =>
            {
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
                return Err(SyncError::Offline {
                    attempts: MAX_LOCK_ATTEMPTS,
                });
            }
            Err(error) => return Err(SyncError::Io(error)),
        }
    }
    unreachable!("the bounded lock loop always returns")
}

async fn read_metadata(path: &Path) -> Result<SyncMetadata, SyncError> {
    let bytes = tokio::fs::read(path).await.map_err(map_local_read_error)?;
    serde_json::from_slice(&bytes).map_err(|error| SyncError::InvalidMetadata(error.to_string()))
}

async fn write_metadata_atomically(path: &Path, metadata: &SyncMetadata) -> Result<(), SyncError> {
    let bytes = serde_json::to_vec(metadata)
        .map_err(|error| SyncError::InvalidMetadata(error.to_string()))?;
    write_bytes_atomically(path, &bytes).await
}

async fn write_bytes_atomically(destination: &Path, bytes: &[u8]) -> Result<(), SyncError> {
    let parent = destination.parent().ok_or_else(|| {
        SyncError::Io(io::Error::new(
            io::ErrorKind::InvalidInput,
            "metadata destination has no parent",
        ))
    })?;
    tokio::fs::create_dir_all(parent).await?;
    let temporary = parent.join(format!(".plankton-sync-{}.tmp", Uuid::new_v4()));
    let mut file = tokio::fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&temporary)
        .await?;
    file.write_all(bytes).await?;
    file.sync_all().await?;
    drop(file);
    tokio::fs::rename(temporary, destination).await?;
    Ok(())
}

fn map_local_read_error(error: io::Error) -> SyncError {
    if error.kind() == io::ErrorKind::NotFound {
        SyncError::NotFound
    } else {
        SyncError::Io(error)
    }
}

/// Runs a deliberately tiny, argv-only allowlist of git operations. It never invokes a shell and
/// never asks git to merge KDBX content; a non-fast-forward push is surfaced as [`SyncError::Conflict`]
/// for KeePassXC's merge flow.
#[derive(Debug, Clone)]
pub struct GitCommand {
    program: PathBuf,
}

impl Default for GitCommand {
    fn default() -> Self {
        Self {
            program: PathBuf::from("git"),
        }
    }
}

impl GitCommand {
    pub fn new(program: impl Into<PathBuf>) -> Self {
        Self {
            program: program.into(),
        }
    }

    pub fn validate_argv(arguments: &[String]) -> Result<(), SyncError> {
        let command_index = if arguments.first().is_some_and(|argument| argument == "-C") {
            2
        } else {
            0
        };
        let Some(command) = arguments.get(command_index).map(String::as_str) else {
            return Err(SyncError::GitCommandNotAllowed);
        };
        if command_index == 2 && (arguments.len() < 3 || arguments[1].contains('\0')) {
            return Err(SyncError::GitCommandNotAllowed);
        }
        let tail = &arguments[command_index + 1..];
        let allowed = match command {
            "fetch" | "push" => {
                matches!(tail, [remote, branch] if is_git_identifier(remote) && is_git_identifier(branch))
            }
            "show" => {
                matches!(tail, [reference] if !reference.starts_with('-') && !reference.contains('\0'))
            }
            "add" => {
                matches!(tail, [separator, path] if separator == "--" && is_safe_git_path(Path::new(path)))
            }
            "commit" => matches!(tail, [no_verify, message_flag, message, separator, path]
                if no_verify == "--no-verify"
                    && message_flag == "-m"
                    && message == GIT_COMMIT_MESSAGE
                    && separator == "--"
                    && is_safe_git_path(Path::new(path))),
            _ => false,
        };
        allowed.then_some(()).ok_or(SyncError::GitCommandNotAllowed)
    }

    async fn run(&self, arguments: &[String]) -> Result<std::process::Output, SyncError> {
        Self::validate_argv(arguments)?;
        let output = Command::new(&self.program)
            .args(arguments)
            .output()
            .await
            .map_err(SyncError::Io)?;
        if output.status.success() {
            Ok(output)
        } else {
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
            let stdout = String::from_utf8_lossy(&output.stdout).trim().to_owned();
            Err(SyncError::Git(if stderr.is_empty() {
                stdout
            } else if stdout.is_empty() {
                stderr
            } else {
                format!("{stderr}\n{stdout}")
            }))
        }
    }
}

#[derive(Debug, Clone)]
pub struct GitRemote {
    repository: PathBuf,
    blob_path: PathBuf,
    remote_name: String,
    branch: String,
    git: GitCommand,
}

impl GitRemote {
    pub fn new(
        repository: impl Into<PathBuf>,
        blob_path: impl Into<PathBuf>,
        remote_name: impl Into<String>,
        branch: impl Into<String>,
    ) -> Result<Self, SyncError> {
        let blob_path = blob_path.into();
        let remote_name = remote_name.into();
        let branch = branch.into();
        if !is_safe_git_path(&blob_path)
            || blob_path
                .extension()
                .is_none_or(|extension| extension != "kdbx")
            || !is_git_identifier(&remote_name)
            || !is_git_identifier(&branch)
        {
            return Err(SyncError::GitCommandNotAllowed);
        }
        Ok(Self {
            repository: repository.into(),
            blob_path,
            remote_name,
            branch,
            git: GitCommand::default(),
        })
    }

    pub fn with_git_command(mut self, git: GitCommand) -> Self {
        self.git = git;
        self
    }

    fn base_arguments(&self, command: &str) -> Vec<String> {
        vec![
            "-C".to_owned(),
            self.repository.to_string_lossy().into_owned(),
            command.to_owned(),
        ]
    }

    fn version_for(blob: &EncryptedVaultBlob) -> VersionToken {
        let hash = Sha256::digest(blob.as_bytes());
        VersionToken(u64::from_be_bytes(
            hash[..8].try_into().expect("SHA-256 prefix has 8 bytes"),
        ))
    }

    async fn fetch_remote_blob(&self) -> Result<Option<EncryptedVaultBlob>, SyncError> {
        let mut fetch = self.base_arguments("fetch");
        fetch.extend([self.remote_name.clone(), self.branch.clone()]);
        match self.git.run(&fetch).await {
            Ok(_) => {}
            Err(SyncError::Git(message)) if git_remote_branch_is_missing(&message) => {
                return Ok(None)
            }
            Err(error) => return Err(error),
        }
        let mut show = self.base_arguments("show");
        show.push(format!("FETCH_HEAD:{}", self.blob_path.display()));
        let output = match self.git.run(&show).await {
            Ok(output) => output,
            Err(SyncError::Git(message)) if git_remote_blob_is_missing(&message) => {
                return Ok(None)
            }
            Err(error) => return Err(error),
        };
        EncryptedVaultBlob::from_kdbx_bytes(output.stdout)
            .map(Some)
            .map_err(|_| SyncError::InvalidRemoteBlob {
                reason: "git remote did not contain a KDBX blob".to_owned(),
            })
    }
}

fn git_remote_branch_is_missing(message: &str) -> bool {
    message.contains("couldn't find remote ref") || message.contains("could not find remote branch")
}

fn git_remote_blob_is_missing(message: &str) -> bool {
    message.contains("does not exist in") || message.contains("exists on disk, but not in")
}

fn git_commit_has_no_changes(message: &str) -> bool {
    message.contains("nothing to commit") || message.contains("no changes added to commit")
}

#[async_trait]
impl SyncRemote for GitRemote {
    async fn pull(&self) -> Result<RemoteBlob, SyncError> {
        let blob = self.fetch_remote_blob().await?.ok_or(SyncError::NotFound)?;
        Ok(RemoteBlob {
            metadata: SyncMetadata::for_blob(Self::version_for(&blob), &blob),
            bytes: blob.bytes,
        })
    }

    async fn push(
        &self,
        blob: &EncryptedVaultBlob,
        _metadata: &SyncMetadata,
        expected_version: Option<VersionToken>,
    ) -> Result<SyncMetadata, SyncError> {
        let remote_blob = self.fetch_remote_blob().await?;
        let actual = remote_blob.as_ref().map(Self::version_for);
        if actual != expected_version {
            return Err(SyncError::Conflict {
                expected: expected_version,
                actual,
            });
        }
        let destination = self.repository.join(&self.blob_path);
        write_blob_atomically(&destination, blob).await?;
        let path = self.blob_path.to_string_lossy().into_owned();
        let mut add = self.base_arguments("add");
        add.extend(["--".to_owned(), path.clone()]);
        self.git.run(&add).await?;
        let mut commit = self.base_arguments("commit");
        commit.extend([
            "--no-verify".to_owned(),
            "-m".to_owned(),
            GIT_COMMIT_MESSAGE.to_owned(),
            "--".to_owned(),
            path,
        ]);
        match self.git.run(&commit).await {
            Ok(_) => {}
            Err(SyncError::Git(message)) if git_commit_has_no_changes(&message) => {}
            Err(error) => return Err(error),
        }
        let mut push = self.base_arguments("push");
        push.extend([self.remote_name.clone(), self.branch.clone()]);
        match self.git.run(&push).await {
            Ok(_) => Ok(SyncMetadata::for_blob(Self::version_for(blob), blob)),
            Err(SyncError::Git(_)) => Err(SyncError::Conflict {
                expected: expected_version,
                actual,
            }),
            Err(error) => Err(error),
        }
    }
}

fn is_safe_relative_file_name(value: &str) -> bool {
    let path = Path::new(value);
    path.components()
        .all(|component| matches!(component, Component::Normal(_)))
}

fn is_safe_git_path(path: &Path) -> bool {
    !path.as_os_str().is_empty()
        && path
            .components()
            .all(|component| matches!(component, Component::Normal(_)))
}

fn is_git_identifier(value: &str) -> bool {
    !value.is_empty()
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'/'))
}

#[cfg(test)]
mod tests {
    use reqwest::header::{HeaderMap, HeaderValue};

    use super::{parse_http_metadata, SyncError, VersionToken};

    #[test]
    fn http_metadata_rejects_a_malformed_version_instead_of_treating_it_as_absent() {
        let mut headers = HeaderMap::new();
        headers.insert(
            "x-plankton-version",
            HeaderValue::from_static("not-a-number"),
        );
        headers.insert(
            "x-plankton-sha256",
            HeaderValue::from_static(
                "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
            ),
        );

        assert!(matches!(
            parse_http_metadata(&headers),
            Err(SyncError::InvalidMetadata(_))
        ));
    }

    #[test]
    fn http_metadata_requires_the_version_and_hash_as_a_pair() {
        let mut headers = HeaderMap::new();
        headers.insert("x-plankton-version", HeaderValue::from_static("7"));

        assert!(matches!(
            parse_http_metadata(&headers),
            Err(SyncError::InvalidMetadata(_))
        ));
    }

    #[test]
    fn http_metadata_decodes_a_complete_valid_pair() {
        let mut headers = HeaderMap::new();
        headers.insert("x-plankton-version", HeaderValue::from_static("7"));
        headers.insert(
            "x-plankton-sha256",
            HeaderValue::from_static(
                "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
            ),
        );

        let metadata = parse_http_metadata(&headers)
            .expect("valid metadata")
            .expect("metadata is present");
        assert_eq!(metadata.version, VersionToken(7));
    }
}
