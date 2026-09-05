use std::{
    collections::BTreeMap,
    time::{Duration, SystemTime},
};

use plankton_protocol::passwords::{PasswordDestination, PasswordDraftState, PasswordDraftStatus};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use super::ParsedPasswordSource;

pub type DraftId = Uuid;

#[derive(Debug)]
pub struct ConfirmationLedger {
    drafts: BTreeMap<DraftId, Draft>,
    committing: BTreeMap<DraftId, PasswordDestination>,
    completed: BTreeMap<DraftId, PasswordDraftStatus>,
    grants: BTreeMap<String, HumanConfirmationGrant>,
    lifetime: Duration,
}

impl ConfirmationLedger {
    pub fn new(lifetime: Duration) -> Self {
        Self {
            drafts: BTreeMap::new(),
            committing: BTreeMap::new(),
            completed: BTreeMap::new(),
            grants: BTreeMap::new(),
            lifetime,
        }
    }

    pub fn create_draft(&mut self, source: ParsedPasswordSource) -> DraftId {
        let id = Uuid::new_v4();
        self.drafts.insert(id, Draft { source });
        id
    }

    pub fn confirm(
        &mut self,
        draft_id: DraftId,
        destination: PasswordDestination,
    ) -> Result<HumanConfirmationGrant, ConfirmationError> {
        let draft = self
            .drafts
            .get(&draft_id)
            .ok_or(ConfirmationError::DraftNotFound)?;
        let content_hash = hash_draft(&draft.source, &destination)?;
        let token = format!("{}{}", Uuid::new_v4().simple(), Uuid::new_v4().simple());
        let grant = HumanConfirmationGrant {
            token: token.clone(),
            draft_id,
            destination,
            content_hash,
            expires_at: SystemTime::now() + self.lifetime,
        };
        self.grants.insert(token, grant.clone());
        Ok(grant)
    }

    pub fn consume(
        &mut self,
        token: &str,
        draft_id: DraftId,
        destination: &PasswordDestination,
    ) -> Result<ConfirmedPasswordWrite, ConfirmationError> {
        let grant = self
            .grants
            .remove(token)
            .ok_or(ConfirmationError::InvalidOrConsumed)?;
        if grant.expires_at < SystemTime::now() {
            return Err(ConfirmationError::Expired);
        }
        if grant.draft_id != draft_id || &grant.destination != destination {
            return Err(ConfirmationError::BindingMismatch);
        }
        let draft = self
            .drafts
            .remove(&draft_id)
            .ok_or(ConfirmationError::DraftNotFound)?;
        self.committing.insert(draft_id, destination.clone());
        let current_hash = hash_draft(&draft.source, destination)?;
        if current_hash != grant.content_hash {
            return Err(ConfirmationError::ContentChanged);
        }
        Ok(ConfirmedPasswordWrite {
            source: draft.source,
            destination: destination.clone(),
            content_hash: current_hash,
        })
    }

    pub fn replace_draft(
        &mut self,
        draft_id: DraftId,
        source: ParsedPasswordSource,
    ) -> Result<(), ConfirmationError> {
        let draft = self
            .drafts
            .get_mut(&draft_id)
            .ok_or(ConfirmationError::DraftNotFound)?;
        draft.source = source;
        Ok(())
    }

    pub fn preview(&self, draft_id: DraftId) -> Result<ParsedPasswordSource, ConfirmationError> {
        self.drafts
            .get(&draft_id)
            .map(|draft| draft.source.clone())
            .ok_or(ConfirmationError::DraftNotFound)
    }

    pub fn confirm_and_consume(
        &mut self,
        draft_id: DraftId,
        destination: PasswordDestination,
    ) -> Result<ConfirmedPasswordWrite, ConfirmationError> {
        let grant = self.confirm(draft_id, destination.clone())?;
        self.consume(&grant.token, draft_id, &destination)
    }

    pub fn restore_draft(&mut self, draft_id: DraftId, source: ParsedPasswordSource) {
        self.committing.remove(&draft_id);
        self.drafts.insert(draft_id, Draft { source });
    }

    pub fn complete_draft(
        &mut self,
        draft_id: DraftId,
        destination: String,
        resource_ids: Vec<String>,
    ) -> Result<(), ConfirmationError> {
        if self.committing.remove(&draft_id).is_none() {
            return Err(ConfirmationError::DraftNotCommitting);
        }
        self.completed.insert(
            draft_id,
            PasswordDraftStatus {
                draft_id,
                state: PasswordDraftState::Committed,
                destination: Some(destination),
                resource_ids,
            },
        );
        Ok(())
    }

    pub fn draft_status(
        &self,
        draft_id: DraftId,
    ) -> Result<PasswordDraftStatus, ConfirmationError> {
        if self.drafts.contains_key(&draft_id) {
            return Ok(PasswordDraftStatus {
                draft_id,
                state: PasswordDraftState::PendingHumanInput,
                destination: None,
                resource_ids: Vec::new(),
            });
        }
        if let Some(destination) = self.committing.get(&draft_id) {
            return Ok(PasswordDraftStatus {
                draft_id,
                state: PasswordDraftState::Committing,
                destination: Some(destination_label(destination)),
                resource_ids: Vec::new(),
            });
        }
        self.completed
            .get(&draft_id)
            .cloned()
            .ok_or(ConfirmationError::DraftNotFound)
    }
}

fn destination_label(destination: &PasswordDestination) -> String {
    match destination {
        PasswordDestination::Plankton { vault_id } => format!("plankton:{vault_id}"),
        PasswordDestination::External {
            binding_id,
            vault_id,
        } => format!("external:{binding_id}:{vault_id}"),
    }
}

#[derive(Debug)]
struct Draft {
    source: ParsedPasswordSource,
}

#[derive(Debug, Clone)]
pub struct HumanConfirmationGrant {
    pub token: String,
    pub draft_id: DraftId,
    pub destination: PasswordDestination,
    pub content_hash: String,
    pub expires_at: SystemTime,
}

#[derive(Debug)]
pub struct ConfirmedPasswordWrite {
    pub source: ParsedPasswordSource,
    pub destination: PasswordDestination,
    pub content_hash: String,
}

fn hash_draft(
    source: &ParsedPasswordSource,
    destination: &PasswordDestination,
) -> Result<String, ConfirmationError> {
    let canonical =
        serde_json::to_vec(&(source, destination)).map_err(ConfirmationError::Serialize)?;
    Ok(format!("{:x}", Sha256::digest(canonical)))
}

#[derive(Debug, thiserror::Error)]
pub enum ConfirmationError {
    #[error("password draft was not found")]
    DraftNotFound,
    #[error("confirmation is invalid or was already consumed")]
    InvalidOrConsumed,
    #[error("confirmation expired")]
    Expired,
    #[error("confirmation does not match the draft or destination")]
    BindingMismatch,
    #[error("password draft changed after confirmation")]
    ContentChanged,
    #[error("password draft is not being committed")]
    DraftNotCommitting,
    #[error("failed to serialize password draft: {0}")]
    Serialize(#[source] serde_json::Error),
}

#[cfg(test)]
mod tests {
    use super::ConfirmationLedger;
    use crate::passwords::{ParsedPasswordEntry, ParsedPasswordSource};
    use plankton_protocol::passwords::{
        PasswordDestination, PasswordDraftState, PasswordSourceDescriptor,
    };
    use std::time::Duration;

    fn manual_source() -> ParsedPasswordSource {
        ParsedPasswordSource {
            descriptor: PasswordSourceDescriptor::Manual {
                keys: vec!["TOKEN".into()],
            },
            entries: vec![ParsedPasswordEntry {
                key: "TOKEN".into(),
                value: "human-entered".into(),
            }],
            suggested_item_title: Some("Service token".into()),
            suggested_destination: None,
            suggested_layout: None,
        }
    }

    #[test]
    fn draft_status_moves_from_human_input_through_commit_without_values() {
        let mut ledger = ConfirmationLedger::new(Duration::from_secs(60));
        let draft_id = ledger.create_draft(manual_source());
        assert_eq!(
            ledger.draft_status(draft_id).expect("pending").state,
            PasswordDraftState::PendingHumanInput
        );

        ledger
            .confirm_and_consume(
                draft_id,
                PasswordDestination::Plankton {
                    vault_id: "default".into(),
                },
            )
            .expect("confirmation");
        assert_eq!(
            ledger.draft_status(draft_id).expect("committing").state,
            PasswordDraftState::Committing
        );

        ledger
            .complete_draft(
                draft_id,
                "plankton:default".into(),
                vec!["plankton://field/example/token".into()],
            )
            .expect("completion");
        let status = ledger.draft_status(draft_id).expect("committed");
        assert_eq!(status.state, PasswordDraftState::Committed);
        assert_eq!(status.resource_ids, vec!["plankton://field/example/token"]);
        assert!(!serde_json::to_string(&status)
            .expect("status serializes")
            .contains("human-entered"));
    }
}
