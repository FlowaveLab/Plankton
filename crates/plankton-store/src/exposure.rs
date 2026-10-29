use std::collections::BTreeMap;

use plankton_core::{
    exposure::{catalog_exposure_snapshots, CatalogExposureSnapshot},
    store_exposure_policy, AccessRequest, Decision, LocalSecretCatalog,
};
use plankton_protocol::exposure::{CredentialAccessMode, CredentialExposurePolicy};
use sqlx::Row;

use crate::{SqliteStore, StoreError};

pub(crate) struct RememberedExposureUpdate<'a> {
    pub snapshot: &'a CatalogExposureSnapshot,
    pub expected_policy: &'a CredentialExposurePolicy,
    pub policy: &'a CredentialExposurePolicy,
}

impl SqliteStore {
    /// Commits the human approval and its remembered policy in a single database transaction.
    pub async fn approve_and_remember_exposure(
        &self,
        request_id: &str,
        snapshot: &CatalogExposureSnapshot,
        expected_policy: &CredentialExposurePolicy,
        policy: &CredentialExposurePolicy,
        actor: &str,
        note: Option<String>,
    ) -> Result<AccessRequest, StoreError> {
        policy
            .validate()
            .map_err(|error| StoreError::InvalidExposurePolicy(error.to_string()))?;
        expected_policy
            .validate()
            .map_err(|error| StoreError::InvalidExposurePolicy(error.to_string()))?;
        if policy.access_mode != CredentialAccessMode::Protected {
            return Err(StoreError::InvalidExposurePolicy(
                "remembered approvals must keep the credential protected".into(),
            ));
        }
        self.record_decision_internal(
            request_id,
            Decision::Allow,
            actor,
            note,
            Some(RememberedExposureUpdate {
                snapshot,
                expected_policy,
                policy,
            }),
        )
        .await
    }

    pub async fn effective_exposure_policies(
        &self,
        snapshots: &[CatalogExposureSnapshot],
    ) -> Result<BTreeMap<String, CredentialExposurePolicy>, StoreError> {
        let rows = sqlx::query(
            "SELECT resource_id, catalog_fingerprint, policy_json FROM remembered_exposure_policies",
        ).fetch_all(&self.pool).await?;
        let mut policies = BTreeMap::new();
        for row in rows {
            let resource: String = row.try_get("resource_id")?;
            let fingerprint: String = row.try_get("catalog_fingerprint")?;
            if !snapshots.iter().any(|snapshot| {
                snapshot.resource_id == resource && snapshot.fingerprint == fingerprint
            }) {
                continue;
            }
            let policy: CredentialExposurePolicy =
                serde_json::from_str(row.try_get("policy_json")?)?;
            policy
                .validate()
                .map_err(|error| StoreError::InvalidExposurePolicy(error.to_string()))?;
            if policy.access_mode != CredentialAccessMode::Protected {
                return Err(StoreError::InvalidExposurePolicy(
                    "stored remembered policy must keep the credential protected".into(),
                ));
            }
            policies.insert(resource, policy);
        }
        Ok(policies)
    }

    pub async fn apply_remembered_exposure_policies(
        &self,
        catalog: &mut LocalSecretCatalog,
    ) -> Result<(), StoreError> {
        let policies = self
            .effective_exposure_policies(&catalog_exposure_snapshots(catalog))
            .await?;
        for entry in &mut catalog.literals {
            if let Some(policy) = policies.get(&entry.resource) {
                store_exposure_policy(&mut entry.metadata, policy)
                    .map_err(|error| StoreError::InvalidExposurePolicy(error.to_string()))?;
            }
        }
        for entry in &mut catalog.imports {
            if let Some(policy) = policies.get(&entry.resource) {
                store_exposure_policy(&mut entry.metadata, policy)
                    .map_err(|error| StoreError::InvalidExposurePolicy(error.to_string()))?;
            }
        }
        Ok(())
    }

    /// An explicit catalog policy edit supersedes remembered approvals, including a reset to
    /// the original policy. Removing the override first fails closed if a later file write fails.
    pub async fn forget_remembered_exposure_policies(
        &self,
        resources: &[String],
    ) -> Result<(), StoreError> {
        let mut tx = self.pool.begin().await?;
        for resource in resources {
            sqlx::query("DELETE FROM remembered_exposure_policies WHERE resource_id = ?")
                .bind(resource)
                .execute(&mut *tx)
                .await?;
        }
        tx.commit().await?;
        Ok(())
    }

    pub async fn prune_remembered_exposure_policies(
        &self,
        snapshots: &[CatalogExposureSnapshot],
    ) -> Result<(), StoreError> {
        let mut tx = self.pool.begin().await?;
        // Acquire the writer before reading so another approval cannot be lost to pruning.
        sqlx::query("UPDATE remembered_exposure_policies SET resource_id = resource_id WHERE 0")
            .execute(&mut *tx)
            .await?;
        let rows = sqlx::query(
            "SELECT resource_id, catalog_fingerprint FROM remembered_exposure_policies",
        )
        .fetch_all(&mut *tx)
        .await?;
        for row in rows {
            let resource: String = row.try_get("resource_id")?;
            let fingerprint: String = row.try_get("catalog_fingerprint")?;
            if !snapshots.iter().any(|snapshot| {
                snapshot.resource_id == resource && snapshot.fingerprint == fingerprint
            }) {
                sqlx::query("DELETE FROM remembered_exposure_policies WHERE resource_id = ? AND catalog_fingerprint = ?")
                    .bind(resource).bind(fingerprint).execute(&mut *tx).await?;
            }
        }
        tx.commit().await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use plankton_core::{
        ApprovalStatus, AuditAction, PlanktonSettings, PolicyMode, RequestContext,
    };
    use plankton_protocol::exposure::{CredentialExposureSurface, NetworkDestinationRule};
    use tempfile::TempDir;

    use super::*;

    fn allowed_policy() -> CredentialExposurePolicy {
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
        policy
    }

    async fn fixture() -> (TempDir, SqliteStore, AccessRequest, CatalogExposureSnapshot) {
        let directory = tempfile::tempdir().unwrap();
        let settings = PlanktonSettings {
            database_url: format!("sqlite://{}", directory.path().join("test.db").display()),
            ..PlanktonSettings::default()
        };
        let store = SqliteStore::new(&settings).await.unwrap();
        let snapshot = CatalogExposureSnapshot {
            resource_id: "secret/test-token".into(),
            fingerprint: "source-and-base-policy-1".into(),
            policy: CredentialExposurePolicy::default(),
        };
        let request = store
            .submit_request(
                &settings,
                RequestContext::new(
                    snapshot.resource_id.clone(),
                    "test consumer".into(),
                    "test".into(),
                ),
                PolicyMode::ManualOnly,
            )
            .await
            .unwrap();
        (directory, store, request, snapshot)
    }

    #[tokio::test]
    async fn remembers_policy_with_approval_and_audits_before_after() {
        let (_directory, store, request, snapshot) = fixture().await;
        let policy = allowed_policy();
        let approved = store
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
        assert_eq!(approved.approval_status, ApprovalStatus::Approved);
        let policies = store
            .effective_exposure_policies(std::slice::from_ref(&snapshot))
            .await
            .unwrap();
        assert_eq!(policies[&snapshot.resource_id], policy);
        let audit = store
            .get_request(&request.id)
            .await
            .unwrap()
            .audit_records
            .into_iter()
            .find(|record| record.action == AuditAction::ApprovalRecorded)
            .unwrap();
        assert_eq!(
            audit.payload["exposure_policy_change"]["before"],
            serde_json::to_value(&snapshot.policy).unwrap()
        );
        assert_eq!(
            audit.payload["exposure_policy_change"]["after"],
            serde_json::to_value(&policy).unwrap()
        );
        assert!(store
            .approve_and_remember_exposure(
                &request.id,
                &snapshot,
                &policy,
                &snapshot.policy,
                "reviewer",
                None
            )
            .await
            .is_err());
        assert_eq!(
            store
                .effective_exposure_policies(std::slice::from_ref(&snapshot))
                .await
                .unwrap()[&snapshot.resource_id],
            policy
        );
    }

    #[tokio::test]
    async fn stale_policy_wrong_resource_and_invalid_policy_leave_request_pending() {
        let (_directory, store, request, snapshot) = fixture().await;
        let policy = allowed_policy();
        let mut other = snapshot.clone();
        other.resource_id = "secret/other".into();
        assert!(matches!(
            store
                .approve_and_remember_exposure(
                    &request.id,
                    &other,
                    &other.policy,
                    &policy,
                    "reviewer",
                    None
                )
                .await,
            Err(StoreError::InvalidExposurePolicy(_))
        ));
        assert!(matches!(
            store
                .approve_and_remember_exposure(
                    &request.id,
                    &snapshot,
                    &policy,
                    &policy,
                    "reviewer",
                    None
                )
                .await,
            Err(StoreError::ExposurePolicyConflict)
        ));
        assert!(matches!(
            store
                .approve_and_remember_exposure(
                    &request.id,
                    &snapshot,
                    &snapshot.policy,
                    &CredentialExposurePolicy::direct(),
                    "reviewer",
                    None
                )
                .await,
            Err(StoreError::InvalidExposurePolicy(_))
        ));
        let mut invalid = policy.clone();
        invalid
            .surfaces
            .iter_mut()
            .find(|surface| surface.surface == CredentialExposureSurface::Network)
            .unwrap()
            .network_allowlist
            .clear();
        assert!(store
            .approve_and_remember_exposure(
                &request.id,
                &snapshot,
                &snapshot.policy,
                &invalid,
                "reviewer",
                None
            )
            .await
            .is_err());
        assert_eq!(
            store
                .get_request(&request.id)
                .await
                .unwrap()
                .request
                .approval_status,
            ApprovalStatus::Pending
        );
        assert!(store
            .effective_exposure_policies(&[snapshot])
            .await
            .unwrap()
            .is_empty());
    }

    #[tokio::test]
    async fn failed_policy_write_rolls_back_approval_and_audits() {
        let (_directory, store, request, snapshot) = fixture().await;
        sqlx::query("CREATE TRIGGER refuse_remembered_policy BEFORE INSERT ON remembered_exposure_policies BEGIN SELECT RAISE(ABORT, 'simulated write failure'); END;").execute(&store.pool).await.unwrap();
        assert!(store
            .approve_and_remember_exposure(
                &request.id,
                &snapshot,
                &snapshot.policy,
                &allowed_policy(),
                "reviewer",
                None
            )
            .await
            .is_err());
        let result = store.get_request(&request.id).await.unwrap();
        assert_eq!(result.request.approval_status, ApprovalStatus::Pending);
        assert!(!result
            .audit_records
            .iter()
            .any(|record| record.action == AuditAction::ApprovalRecorded));
        assert!(store
            .effective_exposure_policies(&[snapshot])
            .await
            .unwrap()
            .is_empty());
    }

    #[tokio::test]
    async fn source_change_invalidates_memory_and_explicit_reset_cannot_revive_it() {
        let (_directory, store, request, snapshot) = fixture().await;
        store
            .approve_and_remember_exposure(
                &request.id,
                &snapshot,
                &snapshot.policy,
                &allowed_policy(),
                "reviewer",
                None,
            )
            .await
            .unwrap();
        let mut changed = snapshot.clone();
        changed.fingerprint = "new-source".into();
        assert!(store
            .effective_exposure_policies(&[changed])
            .await
            .unwrap()
            .is_empty());
        store
            .forget_remembered_exposure_policies(std::slice::from_ref(&snapshot.resource_id))
            .await
            .unwrap();
        assert!(store
            .effective_exposure_policies(&[snapshot])
            .await
            .unwrap()
            .is_empty());
    }
}
