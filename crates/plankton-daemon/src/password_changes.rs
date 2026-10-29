use std::sync::Arc;

use chrono::Utc;
use plankton_core::{apply_password_changes, list_local_secret_catalog};
use plankton_protocol::password_changes::{
    ConfirmPasswordChangeRequest, PasswordChangeDiff, PasswordChangeState, PasswordChangeStatus,
    RejectPasswordChangeRequest, SubmitPasswordChangeRequest, SubmitPasswordChangeResponse,
};
use plankton_store::{SqliteStore, StoredPasswordChange};
use tokio::sync::Mutex;
use uuid::Uuid;

#[derive(Debug, Clone, Default)]
pub struct PasswordChangeCoordinator {
    lock: Arc<Mutex<()>>,
}

impl PasswordChangeCoordinator {
    pub async fn submit(
        &self,
        store: &SqliteStore,
        request: SubmitPasswordChangeRequest,
    ) -> anyhow::Result<SubmitPasswordChangeResponse> {
        let _guard = self.lock.lock().await;
        let requested_change_id = request.change_id.clone();
        validate_submit(&request)?;

        let (mut change, persisted, parent_to_link) = match request.change_id.as_deref() {
            Some(change_id) => {
                let existing = store.get_password_change(change_id).await?;
                if existing.status.state == PasswordChangeState::PendingConfirmation {
                    (existing, true, None)
                } else {
                    self.successor_for(store, existing).await?
                }
            }
            None => {
                let reason = request
                    .reason
                    .as_deref()
                    .map(str::trim)
                    .filter(|reason| !reason.is_empty())
                    .ok_or_else(|| anyhow::anyhow!("reason is required for password changes"))?;
                let requested_by = request.requested_by.trim().to_string();
                let pending = match request
                    .batch_id
                    .as_deref()
                    .map(str::trim)
                    .filter(|batch_id| !batch_id.is_empty())
                {
                    Some(batch_id) => {
                        store
                            .find_pending_password_change_by_batch(batch_id, &requested_by)
                            .await?
                    }
                    None => None,
                };
                match pending {
                    Some(existing) => (existing, true, None),
                    None => (
                        new_change(request.batch_id.clone(), reason.to_string(), requested_by)?,
                        false,
                        None,
                    ),
                }
            }
        };

        if change.operation_ids.contains(&request.operation_id) {
            return Ok(SubmitPasswordChangeResponse {
                requested_change_id,
                effective_change_id: change.status.change_id.clone(),
                status: change.status,
            });
        }
        let previous_diff = change.status.diff.clone();
        change.operations.push(request.operation);
        change.operation_ids.push(request.operation_id);
        let (base_revision, diff) = preview_operations(store, change.operations.clone()).await?;
        if diff.items.is_empty() || diff == previous_diff {
            anyhow::bail!("password change operation produced no changes");
        }
        if change.operations.len() == 1 {
            change.base_revision = base_revision;
        }
        change.status.version += 1;
        change.status.diff = diff;
        change.status.updated_at = Utc::now();

        if persisted {
            store.save_password_change(&change).await?;
        } else {
            store.insert_password_change(&change).await?;
        }
        if let Some(mut parent) = parent_to_link {
            parent.status.successor_change_id = Some(change.status.change_id.clone());
            parent.status.updated_at = Utc::now();
            store.save_password_change(&parent).await?;
        }
        Ok(SubmitPasswordChangeResponse {
            requested_change_id,
            effective_change_id: change.status.change_id.clone(),
            status: change.status,
        })
    }

    pub async fn confirm(
        &self,
        store: &SqliteStore,
        request: ConfirmPasswordChangeRequest,
    ) -> anyhow::Result<PasswordChangeStatus> {
        let _guard = self.lock.lock().await;
        let mut change = store.get_password_change(&request.change_id).await?;
        if change.status.state != PasswordChangeState::PendingConfirmation {
            return Ok(change.status);
        }
        if request.confirmed_version == 0 || request.confirmed_version > change.status.version {
            anyhow::bail!(
                "confirmed_version must be between 1 and {}, received {}",
                change.status.version,
                request.confirmed_version
            );
        }

        let confirmed_len = request.confirmed_version as usize;
        let remainder_operations = change.operations.split_off(confirmed_len);
        let remainder_operation_ids = change.operation_ids.split_off(confirmed_len);
        let (_, confirmed_diff) = preview_operations(store, change.operations.clone()).await?;
        let policy_operations = change.operations.clone();
        let replaced_policies = tokio::task::spawn_blocking(move || {
            plankton_core::password_changes::replaced_exposure_policy_resources(&policy_operations)
        })
        .await??;
        let now = Utc::now();
        change.status.version = request.confirmed_version;
        change.status.confirmed_version = Some(request.confirmed_version);
        change.status.state = PasswordChangeState::Committing;
        change.status.diff = confirmed_diff;
        change.status.confirmed_at = Some(now);
        change.status.updated_at = now;
        store.save_password_change(&change).await?;

        // An explicit policy edit replaces remembered authorization even when it resets to the
        // catalog's original policy. Revoking first is fail-closed if the file commit fails.
        store
            .forget_remembered_exposure_policies(&replaced_policies)
            .await?;
        let operations = change.operations.clone();
        let base_revision = change.base_revision.clone();
        let commit = tokio::task::spawn_blocking(move || {
            apply_password_changes(&operations, &base_revision)
        })
        .await?;
        match commit {
            Ok(_diff) => {
                change.status.state = PasswordChangeState::Committed;
                change.status.committed_at = Some(Utc::now());
                change.status.updated_at = Utc::now();
                let snapshots = tokio::task::spawn_blocking(|| {
                    list_local_secret_catalog().map(|catalog| {
                        plankton_core::exposure::catalog_exposure_snapshots(&catalog)
                    })
                })
                .await??;
                store.prune_remembered_exposure_policies(&snapshots).await?;
            }
            Err(plankton_core::value_resolver::SecretImportError::CatalogConflict {
                expected,
                actual,
            }) => {
                change.status.state = PasswordChangeState::Conflict;
                change.status.error = Some(format!(
                    "password catalog changed while confirming (expected {expected}, actual {actual})"
                ));
                change.status.updated_at = Utc::now();
            }
            Err(error) => {
                change.status.state = PasswordChangeState::Failed;
                change.status.error = Some(error.to_string());
                change.status.updated_at = Utc::now();
            }
        }

        if !remainder_operations.is_empty() {
            let successor = self
                .create_successor_with_operations(
                    store,
                    &change,
                    remainder_operations,
                    remainder_operation_ids,
                )
                .await?;
            change.status.successor_change_id = Some(successor.status.change_id);
        }
        store.save_password_change(&change).await?;
        Ok(change.status)
    }

    pub async fn reject(
        &self,
        store: &SqliteStore,
        request: RejectPasswordChangeRequest,
    ) -> anyhow::Result<PasswordChangeStatus> {
        let _guard = self.lock.lock().await;
        let mut change = store.get_password_change(&request.change_id).await?;
        if change.status.state == PasswordChangeState::PendingConfirmation {
            change.status.state = PasswordChangeState::Rejected;
            change.status.updated_at = Utc::now();
            change.status.error = request
                .note
                .as_deref()
                .map(str::trim)
                .filter(|note| !note.is_empty())
                .map(ToOwned::to_owned);
            store.save_password_change(&change).await?;
        }
        Ok(change.status)
    }

    async fn successor_for(
        &self,
        store: &SqliteStore,
        mut parent: StoredPasswordChange,
    ) -> anyhow::Result<(StoredPasswordChange, bool, Option<StoredPasswordChange>)> {
        if let Some(successor_id) = parent.status.successor_change_id.as_deref() {
            return Ok((store.get_password_change(successor_id).await?, true, None));
        }
        let successor = new_change(
            Some(parent.status.batch_id.clone()),
            parent.status.reason.clone(),
            parent.status.requested_by.clone(),
        )?;
        parent.status.successor_change_id = Some(successor.status.change_id.clone());
        parent.status.updated_at = Utc::now();
        Ok((successor, false, Some(parent)))
    }

    async fn create_successor_with_operations(
        &self,
        store: &SqliteStore,
        parent: &StoredPasswordChange,
        operations: Vec<plankton_protocol::password_changes::PasswordChangeOperation>,
        operation_ids: Vec<String>,
    ) -> anyhow::Result<StoredPasswordChange> {
        let mut successor = new_change(
            Some(parent.status.batch_id.clone()),
            parent.status.reason.clone(),
            parent.status.requested_by.clone(),
        )?;
        let (base_revision, diff) = preview_operations(store, operations.clone()).await?;
        successor.base_revision = base_revision;
        successor.operations = operations;
        successor.operation_ids = operation_ids;
        successor.status.version = successor.operations.len() as u64;
        successor.status.diff = diff;
        successor.status.updated_at = Utc::now();
        store.insert_password_change(&successor).await?;
        Ok(successor)
    }
}

fn validate_submit(request: &SubmitPasswordChangeRequest) -> anyhow::Result<()> {
    if request.requested_by.trim().is_empty() {
        anyhow::bail!("requested_by is required for password changes");
    }
    if request.operation_id.trim().is_empty() {
        anyhow::bail!("operation_id is required for password changes");
    }
    if request.change_id.is_none()
        && request
            .reason
            .as_deref()
            .map(str::trim)
            .is_none_or(str::is_empty)
    {
        anyhow::bail!("reason is required for password changes");
    }
    Ok(())
}

fn new_change(
    batch_id: Option<String>,
    reason: String,
    requested_by: String,
) -> anyhow::Result<StoredPasswordChange> {
    let now = Utc::now();
    let change_id = format!("chg_{}", Uuid::new_v4().simple());
    let batch_id = batch_id
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| format!("batch_{}", Uuid::new_v4().simple()));
    let (base_revision, diff) = plankton_core::preview_password_changes(&[])?;
    Ok(StoredPasswordChange {
        status: PasswordChangeStatus {
            batch_id,
            change_id,
            version: 0,
            confirmed_version: None,
            state: PasswordChangeState::PendingConfirmation,
            reason,
            requested_by,
            diff,
            successor_change_id: None,
            created_at: now,
            updated_at: now,
            confirmed_at: None,
            committed_at: None,
            error: None,
        },
        base_revision,
        operations: Vec::new(),
        operation_ids: Vec::new(),
    })
}

async fn preview_operations(
    store: &SqliteStore,
    operations: Vec<plankton_protocol::password_changes::PasswordChangeOperation>,
) -> anyhow::Result<(String, PasswordChangeDiff)> {
    let snapshots = tokio::task::spawn_blocking(|| {
        list_local_secret_catalog()
            .map(|catalog| plankton_core::exposure::catalog_exposure_snapshots(&catalog))
    })
    .await??;
    let policies = store.effective_exposure_policies(&snapshots).await?;
    Ok(tokio::task::spawn_blocking(move || {
        plankton_core::password_changes::preview_password_changes_with_exposure_policies(
            &operations,
            &policies,
        )
    })
    .await??)
}

#[cfg(test)]
mod tests {
    use std::ffi::OsString;

    use plankton_core::PlanktonSettings;
    use plankton_protocol::password_changes::{
        ConfirmPasswordChangeRequest, PasswordChangeOperation, SubmitPasswordChangeRequest,
    };
    use plankton_store::SqliteStore;

    use super::PasswordChangeCoordinator;

    static SECRET_FILE_TEST_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

    struct SecretFileOverride(Option<OsString>);

    impl SecretFileOverride {
        fn set(path: &std::path::Path) -> Self {
            let previous = std::env::var_os("PLANKTON_SECRET_FILE");
            std::env::set_var("PLANKTON_SECRET_FILE", path);
            Self(previous)
        }
    }

    impl Drop for SecretFileOverride {
        fn drop(&mut self) {
            match self.0.take() {
                Some(previous) => std::env::set_var("PLANKTON_SECRET_FILE", previous),
                None => std::env::remove_var("PLANKTON_SECRET_FILE"),
            }
        }
    }

    fn update(operation_id: &str, title: &str) -> SubmitPasswordChangeRequest {
        SubmitPasswordChangeRequest {
            change_id: None,
            batch_id: None,
            reason: Some("test password changes".to_string()),
            requested_by: "test-agent".to_string(),
            operation_id: operation_id.to_string(),
            operation: PasswordChangeOperation::UpdateItem {
                item_id: "secret/test".to_string(),
                next_item_id: None,
                title: Some(title.to_string()),
                description: None,
                clear_description: false,
                tags: None,
                metadata: None,
            },
        }
    }

    #[tokio::test(flavor = "current_thread")]
    async fn catalog_edit_can_reset_a_remembered_policy_to_the_original_default() {
        assert_catalog_edit_resets_remembered_policy("field").await;
    }

    #[tokio::test(flavor = "current_thread")]
    async fn item_default_edit_can_reset_a_remembered_inherited_policy() {
        assert_catalog_edit_resets_remembered_policy("item").await;
    }

    #[tokio::test(flavor = "current_thread")]
    async fn inherit_field_edit_can_reset_a_remembered_policy() {
        assert_catalog_edit_resets_remembered_policy("inherit").await;
    }

    async fn assert_catalog_edit_resets_remembered_policy(kind: &str) {
        use plankton_core::{exposure::catalog_exposure_snapshots, PolicyMode, RequestContext};
        use plankton_protocol::exposure::{
            CredentialExposurePolicy, CredentialExposureSurface, NetworkDestinationRule,
        };

        let _test_lock = SECRET_FILE_TEST_LOCK.lock().await;
        let directory = tempfile::tempdir().unwrap();
        let catalog_path = directory.path().join("secrets.toml");
        std::fs::write(
            &catalog_path,
            "[secrets]\n\"secret/test\" = \"test-value\"\n",
        )
        .unwrap();
        let _secret_file = SecretFileOverride::set(&catalog_path);
        let settings = PlanktonSettings {
            database_url: format!("sqlite://{}", directory.path().join("store.db").display()),
            ..PlanktonSettings::default()
        };
        let store = SqliteStore::new(&settings).await.unwrap();
        let catalog = plankton_core::list_local_secret_catalog().unwrap();
        let snapshot = catalog_exposure_snapshots(&catalog).remove(0);
        let request = store
            .submit_request(
                &settings,
                RequestContext::new("secret/test".into(), "test".into(), "tester".into()),
                PolicyMode::ManualOnly,
            )
            .await
            .unwrap();
        let mut policy = CredentialExposurePolicy::default();
        let network = policy
            .surfaces
            .iter_mut()
            .find(|surface| surface.surface == CredentialExposureSurface::Network)
            .unwrap();
        network.max_level = 1;
        network
            .network_allowlist
            .push(NetworkDestinationRule::ExactDomain {
                domain: "api.example.test".into(),
            });
        store
            .approve_and_remember_exposure(
                &request.id,
                &snapshot,
                &snapshot.policy,
                &policy,
                "reviewer",
                None,
            )
            .await
            .unwrap();
        let coordinator = PasswordChangeCoordinator::default();
        let mut reset = update("reset-policy", "unused");
        reset.operation = match kind {
            "field" => PasswordChangeOperation::SetFieldExposurePolicy {
                resource_id: "secret/test".into(),
                policy: CredentialExposurePolicy::default(),
            },
            "inherit" => PasswordChangeOperation::InheritFieldExposurePolicy {
                resource_id: "secret/test".into(),
            },
            "item" => PasswordChangeOperation::SetItemExposurePolicy {
                item_id: plankton_core::password_catalog_metadata().unwrap().items[0]
                    .item_id
                    .clone(),
                policy: CredentialExposurePolicy::default(),
            },
            _ => unreachable!(),
        };
        let staged = coordinator.submit(&store, reset).await.unwrap();
        let diff = staged
            .status
            .diff
            .items
            .iter()
            .flat_map(|item| &item.entries)
            .find(|entry| entry.path.ends_with("/exposure_policy"))
            .unwrap();
        assert_eq!(
            diff.before.as_deref(),
            Some(serde_json::to_string(&policy).unwrap().as_str())
        );
        let committed = coordinator
            .confirm(
                &store,
                ConfirmPasswordChangeRequest {
                    change_id: staged.effective_change_id,
                    confirmed_version: 1,
                },
            )
            .await
            .unwrap();
        assert_eq!(
            committed.state,
            plankton_protocol::password_changes::PasswordChangeState::Committed
        );
        assert!(!committed.diff.items.is_empty());
        assert!(store
            .effective_exposure_policies(&[snapshot])
            .await
            .unwrap()
            .is_empty());
        let mut effective = plankton_core::list_local_secret_catalog().unwrap();
        store
            .apply_remembered_exposure_policies(&mut effective)
            .await
            .unwrap();
        assert_eq!(
            plankton_core::exposure_policy_from_metadata(&effective.literals[0].metadata),
            CredentialExposurePolicy::default()
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn failed_successor_preview_does_not_persist_empty_change() {
        let _test_lock = SECRET_FILE_TEST_LOCK.lock().await;
        let directory = tempfile::tempdir().expect("tempdir");
        let catalog_path = directory.path().join("secrets.toml");
        std::fs::write(
            &catalog_path,
            "[secrets]\n\"secret/test\" = \"test-value\"\n",
        )
        .expect("test catalog");
        let _secret_file = SecretFileOverride::set(&catalog_path);
        let settings = PlanktonSettings {
            database_url: format!("sqlite://{}", directory.path().join("store.db").display()),
            ..PlanktonSettings::default()
        };
        let store = SqliteStore::new(&settings).await.expect("store");
        let coordinator = PasswordChangeCoordinator::default();

        let first = coordinator
            .submit(&store, update("op-1", "First title"))
            .await
            .expect("first change");
        coordinator
            .confirm(
                &store,
                ConfirmPasswordChangeRequest {
                    change_id: first.effective_change_id.clone(),
                    confirmed_version: 1,
                },
            )
            .await
            .expect("confirm first change");

        let mut invalid = update("op-invalid", "Invalid");
        invalid.change_id = Some(first.effective_change_id.clone());
        if let PasswordChangeOperation::UpdateItem { item_id, .. } = &mut invalid.operation {
            *item_id = "missing".to_string();
        }
        coordinator
            .submit(&store, invalid)
            .await
            .expect_err("invalid successor preview must fail");
        assert!(store
            .list_pending_password_changes()
            .await
            .expect("pending changes")
            .is_empty());
        assert!(store
            .get_password_change(&first.effective_change_id)
            .await
            .expect("parent")
            .status
            .successor_change_id
            .is_none());

        let mut successor = update("op-2", "Second title");
        successor.change_id = Some(first.effective_change_id.clone());
        let successor = coordinator
            .submit(&store, successor)
            .await
            .expect("valid successor");
        assert_eq!(successor.status.version, 1);

        let mut appended = update("op-3", "Third title");
        appended.change_id = Some(first.effective_change_id);
        let appended = coordinator
            .submit(&store, appended)
            .await
            .expect("append through parent id");
        assert_eq!(appended.effective_change_id, successor.effective_change_id);
        assert_eq!(appended.status.version, 2);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn shared_batch_id_accumulates_into_one_pending_diff() {
        let _test_lock = SECRET_FILE_TEST_LOCK.lock().await;
        let directory = tempfile::tempdir().expect("tempdir");
        let catalog_path = directory.path().join("secrets.toml");
        std::fs::write(
            &catalog_path,
            "[secrets]\n\"secret/test\" = \"test-value\"\n",
        )
        .expect("test catalog");
        let _secret_file = SecretFileOverride::set(&catalog_path);
        let settings = PlanktonSettings {
            database_url: format!("sqlite://{}", directory.path().join("store.db").display()),
            ..PlanktonSettings::default()
        };
        let store = SqliteStore::new(&settings).await.expect("store");
        let coordinator = PasswordChangeCoordinator::default();

        coordinator
            .submit(&store, update("noop", "secret/test"))
            .await
            .expect_err("no-op changes must not create approvals");
        assert!(store
            .list_pending_password_changes()
            .await
            .expect("no-op pending changes")
            .is_empty());

        let mut first = update("batch-op-1", "Batch title");
        first.batch_id = Some("batch-shared".to_string());
        let first = coordinator
            .submit(&store, first)
            .await
            .expect("first batch op");

        let mut second = update("batch-op-2", "Final batch title");
        second.batch_id = Some("batch-shared".to_string());
        let second = coordinator
            .submit(&store, second)
            .await
            .expect("second batch op");

        assert_eq!(second.effective_change_id, first.effective_change_id);
        assert_eq!(second.status.version, 2);

        let mut duplicate = update("batch-op-3", "Final batch title");
        duplicate.batch_id = Some("batch-shared".to_string());
        coordinator
            .submit(&store, duplicate)
            .await
            .expect_err("no-op append must not create another version");
        assert_eq!(
            store
                .get_password_change(&second.effective_change_id)
                .await
                .expect("batched change")
                .status
                .version,
            2
        );
        assert_eq!(
            store
                .list_pending_password_changes()
                .await
                .expect("pending changes")
                .len(),
            1
        );
    }
}
