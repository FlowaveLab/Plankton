use std::{sync::Arc, time::Duration};

use async_trait::async_trait;
use plankton_core::sync::{
    plan_sync, EncryptedVaultBlob, GitCommand, GitRemote, HttpRequest, HttpResponse,
    HttpSyncRemote, HttpTransport, LocalFolderRemote, RemoteBlob, RetryPolicy, SyncConfiguration,
    SyncEngine, SyncError, SyncMetadata, SyncOperation, SyncPlan, SyncRemote, VersionToken,
};
use tokio::sync::Mutex;

const KDBX: [u8; 12] = [
    0x03, 0xd9, 0xa2, 0x9a, 0x67, 0xfb, 0x4b, 0xb5, 0x00, 0x01, 0x02, 0x03,
];

fn encrypted_blob() -> EncryptedVaultBlob {
    EncryptedVaultBlob::from_kdbx_bytes(KDBX.to_vec()).expect("KDBX fixture is valid")
}

fn changed_blob(marker: u8) -> EncryptedVaultBlob {
    let mut bytes = KDBX.to_vec();
    bytes.push(marker);
    EncryptedVaultBlob::from_kdbx_bytes(bytes).expect("changed KDBX fixture is valid")
}

fn remote_blob(blob: &EncryptedVaultBlob, version: u64) -> RemoteBlob {
    RemoteBlob {
        bytes: blob.as_bytes().to_vec(),
        metadata: SyncMetadata::for_blob(VersionToken(version), blob),
    }
}

#[test]
fn automatic_sync_plan_distinguishes_one_sided_changes_and_real_merges() {
    let base = encrypted_blob();
    let local = changed_blob(1);
    let remote = changed_blob(2);

    assert_eq!(
        plan_sync(Some(&local), None, None).expect("local-only vault uploads"),
        SyncPlan::Upload
    );
    assert_eq!(
        plan_sync(None, Some(&remote_blob(&remote, 2)), None).expect("remote-only vault downloads"),
        SyncPlan::Download
    );
    assert_eq!(
        plan_sync(
            Some(&local),
            Some(&remote_blob(&base, 1)),
            Some(&base.sha256()),
        )
        .expect("remote unchanged since base"),
        SyncPlan::Upload
    );
    assert_eq!(
        plan_sync(
            Some(&base),
            Some(&remote_blob(&remote, 2)),
            Some(&base.sha256()),
        )
        .expect("local unchanged since base"),
        SyncPlan::Download
    );
    assert_eq!(
        plan_sync(
            Some(&local),
            Some(&remote_blob(&remote, 2)),
            Some(&base.sha256()),
        )
        .expect("both sides changed"),
        SyncPlan::Merge
    );
}

#[test]
fn automatic_sync_plan_merges_different_first_sync_vaults_but_accepts_identical_files() {
    let local = changed_blob(1);
    let remote = changed_blob(2);

    assert_eq!(
        plan_sync(Some(&local), Some(&remote_blob(&remote, 2)), None)
            .expect("different first-sync vaults merge"),
        SyncPlan::Merge
    );
    assert_eq!(
        plan_sync(Some(&local), Some(&remote_blob(&local, 2)), None)
            .expect("identical first-sync vaults establish a baseline"),
        SyncPlan::UpToDate
    );
    assert!(matches!(
        plan_sync(None, None, None),
        Err(SyncError::NotFound)
    ));
}

#[derive(Clone)]
struct FakeHttpTransport {
    responses: Arc<Mutex<Vec<Result<HttpResponse, SyncError>>>>,
    requests: Arc<Mutex<Vec<HttpRequest>>>,
}

impl FakeHttpTransport {
    fn new(responses: Vec<Result<HttpResponse, SyncError>>) -> Self {
        Self {
            responses: Arc::new(Mutex::new(responses)),
            requests: Arc::new(Mutex::new(Vec::new())),
        }
    }
}

#[async_trait]
impl HttpTransport for FakeHttpTransport {
    async fn execute(&self, request: HttpRequest) -> Result<HttpResponse, SyncError> {
        self.requests.lock().await.push(request);
        self.responses.lock().await.remove(0)
    }
}

fn enabled_engine(attempts: u8) -> SyncEngine {
    SyncEngine::new(SyncConfiguration {
        enabled: true,
        retry: RetryPolicy {
            max_attempts: attempts,
            retry_delay: Duration::ZERO,
        },
    })
}

#[tokio::test]
async fn disabled_sync_rejects_a_push_before_contacting_remote() {
    let transport = FakeHttpTransport::new(Vec::new());
    let remote = HttpSyncRemote::custom("https://sync.example/vault", transport.clone());
    let engine = SyncEngine::default();

    let error = engine
        .push(&remote, &encrypted_blob(), None)
        .await
        .expect_err("sync must be opt-in");

    assert!(matches!(error, SyncError::Disabled));
    assert!(transport.requests.lock().await.is_empty());
}

#[tokio::test]
async fn offline_push_retries_then_transmits_the_same_encrypted_blob() {
    let expected = SyncMetadata::for_blob(VersionToken(7), &encrypted_blob());
    let transport = FakeHttpTransport::new(vec![
        Err(SyncError::Offline { attempts: 1 }),
        Ok(HttpResponse::pushed(expected.clone())),
    ]);
    let remote = HttpSyncRemote::webdav("https://dav.example/vault.kdbx", transport.clone());

    let metadata = enabled_engine(2)
        .push(&remote, &encrypted_blob(), Some(VersionToken(6)))
        .await
        .expect("second attempt should succeed");

    assert_eq!(metadata, expected);
    let requests = transport.requests.lock().await;
    assert_eq!(requests.len(), 2);
    for request in requests.iter() {
        assert_eq!(request.operation, SyncOperation::Push);
        assert_eq!(request.expected_version, Some(VersionToken(6)));
        assert_eq!(
            request.blob.as_ref().expect("push has blob").as_bytes(),
            KDBX
        );
    }
}

#[tokio::test]
async fn concurrent_version_conflict_is_returned_without_overwriting_remote_data() {
    let remote_metadata = SyncMetadata::for_blob(VersionToken(9), &encrypted_blob());
    let transport = FakeHttpTransport::new(vec![Err(SyncError::Conflict {
        expected: Some(VersionToken(8)),
        actual: Some(VersionToken(9)),
    })]);
    let remote = HttpSyncRemote::custom("https://sync.example/vault", transport);

    let error = enabled_engine(1)
        .push(&remote, &encrypted_blob(), Some(VersionToken(8)))
        .await
        .expect_err("compare-and-swap conflict must not be hidden");

    assert!(matches!(
        error,
        SyncError::Conflict {
            expected: Some(VersionToken(8)),
            actual: Some(VersionToken(9)),
        }
    ));
    assert_eq!(remote_metadata.version, VersionToken(9));
}

#[tokio::test]
async fn local_folder_compare_and_swap_allows_only_one_concurrent_writer() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let remote = LocalFolderRemote::new(directory.path(), "vault.kdbx").expect("safe local remote");
    let engine = enabled_engine(1);
    let first = engine
        .push(&remote, &encrypted_blob(), None)
        .await
        .expect("initial local push");
    assert_eq!(first.version, VersionToken(1));

    let other_remote = remote.clone();
    let first_blob = encrypted_blob();
    let second_blob = encrypted_blob();
    let first_writer = engine.push(&remote, &first_blob, Some(first.version));
    let second_writer = engine.push(&other_remote, &second_blob, Some(first.version));
    let (left, right) = tokio::join!(first_writer, second_writer);

    assert!(
        left.is_ok() ^ right.is_ok(),
        "exactly one compare-and-swap may win"
    );
    assert!(matches!(
        left.err().or(right.err()),
        Some(SyncError::Conflict {
            expected: Some(VersionToken(1)),
            actual: Some(VersionToken(2)),
        })
    ));
}

#[tokio::test]
async fn pull_rejects_a_remote_blob_with_an_invalid_hash_without_replacing_local_file() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let destination = directory.path().join("vault.kdbx");
    tokio::fs::write(&destination, b"existing encrypted vault")
        .await
        .expect("existing vault");
    let metadata = SyncMetadata::for_blob(VersionToken(2), &encrypted_blob());
    let transport = FakeHttpTransport::new(vec![Ok(HttpResponse::pulled(
        KDBX.to_vec(),
        SyncMetadata {
            sha256: "00".repeat(32),
            ..metadata
        },
    ))]);
    let remote = HttpSyncRemote::custom("https://sync.example/vault", transport);

    let error = enabled_engine(1)
        .pull_to_path(&remote, &destination)
        .await
        .expect_err("bad remote hash must be rejected");

    assert!(matches!(error, SyncError::InvalidRemoteBlob { .. }));
    assert_eq!(
        tokio::fs::read(&destination).await.unwrap(),
        b"existing encrypted vault"
    );
}

#[tokio::test]
async fn pull_rejects_a_non_kdbx_remote_blob_without_replacing_local_file() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let destination = directory.path().join("vault.kdbx");
    tokio::fs::write(&destination, b"existing encrypted vault")
        .await
        .expect("existing vault");
    let metadata = SyncMetadata {
        version: VersionToken(2),
        sha256: "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855".to_owned(),
    };
    let transport = FakeHttpTransport::new(vec![Ok(HttpResponse::pulled(Vec::new(), metadata))]);
    let remote = HttpSyncRemote::custom("https://sync.example/vault", transport);

    let error = enabled_engine(1)
        .pull_to_path(&remote, &destination)
        .await
        .expect_err("non-KDBX bytes must be rejected");

    assert!(matches!(error, SyncError::InvalidRemoteBlob { .. }));
    assert_eq!(
        tokio::fs::read(&destination).await.unwrap(),
        b"existing encrypted vault"
    );
}

#[tokio::test]
async fn authentication_failure_is_typed_and_never_retried() {
    let transport = FakeHttpTransport::new(vec![Err(SyncError::Authentication)]);
    let remote = HttpSyncRemote::webdav("https://dav.example/vault.kdbx", transport.clone());

    let error = enabled_engine(3)
        .push(&remote, &encrypted_blob(), None)
        .await
        .expect_err("authentication failure must reach caller");

    assert!(matches!(error, SyncError::Authentication));
    assert_eq!(transport.requests.lock().await.len(), 1);
}

#[tokio::test]
async fn transport_api_carries_only_an_encrypted_blob_and_non_secret_metadata() {
    let response = SyncMetadata::for_blob(VersionToken(1), &encrypted_blob());
    let transport = FakeHttpTransport::new(vec![Ok(HttpResponse::pushed(response))]);
    let remote = HttpSyncRemote::custom("https://sync.example/vault", transport.clone());

    enabled_engine(1)
        .push(&remote, &encrypted_blob(), None)
        .await
        .expect("push succeeds");

    let request = transport.requests.lock().await.remove(0);
    assert_eq!(
        request.blob.as_ref().expect("blob is present").as_bytes(),
        KDBX
    );
    assert_eq!(
        request
            .metadata
            .as_ref()
            .expect("metadata is present")
            .version,
        VersionToken(0)
    );
    assert!(request.plaintext_fields().is_empty());
}

#[test]
fn encrypted_vault_type_rejects_plaintext_before_it_can_reach_a_transport() {
    let error =
        EncryptedVaultBlob::from_kdbx_bytes(br#"{\"title\":\"plain field value\"}"#.to_vec())
            .expect_err("plaintext JSON is not a KDBX blob");

    assert!(matches!(error, SyncError::InvalidEncryptedBlob));
}

#[test]
fn git_command_uses_a_fixed_argv_allowlist_without_shell_syntax() {
    let allowed = vec![
        "-C".to_owned(),
        "/safe/repository".to_owned(),
        "commit".to_owned(),
        "--no-verify".to_owned(),
        "-m".to_owned(),
        "plankton encrypted vault sync".to_owned(),
        "--".to_owned(),
        "vault.kdbx".to_owned(),
    ];
    GitCommand::validate_argv(&allowed).expect("fixed binary commit command is allowed");

    let shell_escape = vec!["commit".to_owned(), "-m".to_owned(), "$(leak)".to_owned()];
    assert!(matches!(
        GitCommand::validate_argv(&shell_escape),
        Err(SyncError::GitCommandNotAllowed)
    ));
}

#[tokio::test]
async fn git_push_initializes_an_empty_remote_branch() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let bare_remote = directory.path().join("remote.git");
    let repository = directory.path().join("repository");
    let init = std::process::Command::new("git")
        .args(["init", "--bare", "--quiet"])
        .arg(&bare_remote)
        .status()
        .expect("initialize bare remote");
    assert!(init.success());
    let clone = std::process::Command::new("git")
        .arg("clone")
        .arg("--quiet")
        .arg(&bare_remote)
        .arg(&repository)
        .status()
        .expect("clone empty remote");
    assert!(clone.success());
    for arguments in [
        vec!["checkout", "-b", "main"],
        vec!["config", "user.name", "Plankton Sync Test"],
        vec!["config", "user.email", "plankton-sync@example.test"],
    ] {
        let status = std::process::Command::new("git")
            .arg("-C")
            .arg(&repository)
            .args(arguments)
            .status()
            .expect("prepare local checkout");
        assert!(status.success());
    }
    let remote = GitRemote::new(&repository, "default.kdbx", "origin", "main")
        .expect("valid Git sync remote");
    assert!(matches!(remote.pull().await, Err(SyncError::NotFound)));

    enabled_engine(1)
        .push(&remote, &encrypted_blob(), None)
        .await
        .expect("first push creates the remote branch");

    let stored = std::process::Command::new("git")
        .arg("--git-dir")
        .arg(&bare_remote)
        .args(["show", "main:default.kdbx"])
        .output()
        .expect("read pushed encrypted vault");
    assert!(stored.status.success());
    assert_eq!(stored.stdout, KDBX);

    assert!(matches!(
        enabled_engine(1)
            .push(&remote, &encrypted_blob(), None)
            .await,
        Err(SyncError::Conflict {
            expected: None,
            actual: Some(_),
        })
    ));
}
