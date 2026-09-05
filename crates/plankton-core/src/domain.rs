use std::collections::BTreeMap;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use uuid::Uuid;

use crate::{AutomaticDecisionTrace, AutomaticDisposition, CallChainNode};

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PolicyMode {
    #[default]
    ManualOnly,
    LlmAutomatic,
    Assisted,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Decision {
    Allow,
    Deny,
}

#[derive(schemars::JsonSchema, Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SuggestedDecision {
    Allow,
    Deny,
    Escalate,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub struct LlmApprovalDecisionPolicy {
    #[serde(default = "default_true")]
    pub allow: bool,
    #[serde(default = "default_true")]
    pub deny: bool,
    #[serde(default = "default_true")]
    pub escalate: bool,
}

impl Default for LlmApprovalDecisionPolicy {
    fn default() -> Self {
        Self {
            allow: true,
            deny: true,
            escalate: true,
        }
    }
}

impl LlmApprovalDecisionPolicy {
    pub fn allows(self, decision: SuggestedDecision) -> bool {
        match decision {
            SuggestedDecision::Allow => self.allow,
            SuggestedDecision::Deny => self.deny,
            SuggestedDecision::Escalate => self.escalate,
        }
    }

    pub fn allowed_values(self) -> Vec<&'static str> {
        let mut values = Vec::with_capacity(3);
        if self.allow {
            values.push("allow");
        }
        if self.deny {
            values.push("deny");
        }
        if self.escalate {
            values.push("escalate");
        }
        values
    }
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ApprovalStatus {
    Pending,
    Approved,
    Rejected,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum EvaluationState {
    #[default]
    NotRequired,
    Queued,
    Running,
    Completed,
    Failed,
    Interrupted,
    Superseded,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AuditAction {
    RequestSubmitted,
    LlmSuggestionGenerated,
    LlmSuggestionFailed,
    LlmReviewDetailsUpdated,
    AutomaticDecisionRecorded,
    AutomaticEscalatedToHuman,
    ApprovalRecorded,
    HumanDecisionOverrodeLlm,
    // Read-only compatibility with audit records created before this feature was retired.
    #[serde(rename = "memory_evaluated")]
    LegacyNoteEvaluated,
    StatusViewed,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RequestContext {
    pub resource: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub resource_tags: Vec<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub resource_metadata: BTreeMap<String, String>,
    pub reason: String,
    pub requested_by: String,
    pub script_path: Option<String>,
    #[serde(default)]
    pub call_chain: Vec<CallChainNode>,
    pub env_vars: BTreeMap<String, String>,
    pub metadata: BTreeMap<String, String>,
    pub created_at: DateTime<Utc>,
}

impl RequestContext {
    pub fn new(resource: String, reason: String, requested_by: String) -> Self {
        Self {
            resource,
            resource_tags: Vec::new(),
            resource_metadata: BTreeMap::new(),
            reason,
            requested_by,
            script_path: None,
            call_chain: Vec::new(),
            env_vars: BTreeMap::new(),
            metadata: BTreeMap::new(),
            created_at: Utc::now(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SanitizedPromptContext {
    pub resource: String,
    pub resource_tags: Vec<String>,
    pub metadata: BTreeMap<String, String>,
    pub request_metadata: BTreeMap<String, String>,
    pub reason: String,
    pub requested_by: String,
    pub script_path: Option<String>,
    pub call_chain: Vec<String>,
    pub call_chain_details: Vec<SanitizedCallChainEntry>,
    pub inline_sources: Vec<SanitizedInlineSource>,
    pub env_vars: BTreeMap<String, String>,
    pub env_var_names: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SanitizedCallChainEntry {
    #[serde(default)]
    pub pid: Option<u32>,
    #[serde(default)]
    pub ppid: Option<u32>,
    #[serde(default = "default_call_chain_source")]
    pub source: crate::CallChainNodeSource,
    pub process_name: Option<String>,
    pub executable_path: Option<String>,
    pub arguments: Vec<SanitizedArgument>,
    pub resolved_file_path: Option<String>,
}

fn default_call_chain_source() -> crate::CallChainNodeSource {
    crate::CallChainNodeSource::BestEffort
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SanitizedArgument {
    pub argument_index: usize,
    pub text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SanitizedInlineSource {
    pub source_id: String,
    pub node_index: usize,
    pub argument_index: usize,
    pub language: String,
    pub lines: Vec<SanitizedSourceLine>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SanitizedSourceLine {
    pub line: usize,
    pub text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProviderInputSnapshot {
    pub template_id: String,
    pub template_version: String,
    pub prompt_contract_version: String,
    pub prompt_sha256: String,
    pub prompt: String,
    pub decision_policy: LlmApprovalDecisionPolicy,
    pub allowed_read_files: Vec<String>,
    pub sanitized_context: SanitizedPromptContext,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DecisionAttempt {
    pub prompt: String,
    pub raw_response: String,
    pub started_at: DateTime<Utc>,
    pub finished_at: DateTime<Utc>,
    pub tool_events: Vec<serde_json::Value>,
    pub validation_error: Option<String>,
    pub normalization: Option<crate::JsonRepairStrategy>,
    pub evidence_path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct ProviderTrace {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub audit_events: Vec<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub decision_attempts: Vec<DecisionAttempt>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_configuration: Option<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rendered_prompt: Option<String>,
    pub transport: Option<String>,
    pub protocol: Option<String>,
    pub api_version: Option<String>,
    pub output_format: Option<String>,
    pub stop_reason: Option<String>,
    pub package_name: Option<String>,
    pub package_version: Option<String>,
    pub session_id: Option<String>,
    pub client_request_id: Option<String>,
    pub agent_name: Option<String>,
    pub agent_version: Option<String>,
    #[serde(default)]
    pub beta_headers: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub review_progress: Option<LlmReviewProgress>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum LlmReviewDetailState {
    Running,
    Complete,
    Partial,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LlmReviewProgress {
    pub state: LlmReviewDetailState,
    pub completed_units: u16,
    pub total_units: u16,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LlmSuggestionUsage {
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
    pub total_tokens: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LlmSuggestion {
    pub template_id: String,
    pub template_version: String,
    pub prompt_contract_version: String,
    pub prompt_sha256: String,
    pub suggested_decision: SuggestedDecision,
    pub rationale_summary: String,
    pub risk_score: u8,
    #[serde(default)]
    pub batch_decisions: Vec<crate::BatchResourceDecision>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exposure_report: Option<crate::CredentialExposureReport>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub json_repair_strategy: Option<crate::JsonRepairStrategy>,
    pub provider_kind: String,
    pub provider_model: Option<String>,
    pub provider_response_id: Option<String>,
    pub x_request_id: Option<String>,
    pub provider_trace: Option<ProviderTrace>,
    pub usage: Option<LlmSuggestionUsage>,
    pub error: Option<String>,
    pub generated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AccessRequest {
    pub id: String,
    pub context: RequestContext,
    pub policy_mode: PolicyMode,
    pub approval_status: ApprovalStatus,
    pub evaluation_state: EvaluationState,
    pub final_decision: Option<Decision>,
    pub provider_kind: Option<String>,
    pub rendered_prompt: String,
    pub provider_input: Option<ProviderInputSnapshot>,
    pub llm_suggestion: Option<LlmSuggestion>,
    pub automatic_decision: Option<AutomaticDecisionTrace>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub resolved_at: Option<DateTime<Utc>>,
}

impl AccessRequest {
    pub fn human_review_required(&self) -> bool {
        self.approval_status == ApprovalStatus::Pending
            && match self.policy_mode {
                PolicyMode::ManualOnly => true,
                PolicyMode::Assisted | PolicyMode::LlmAutomatic => !matches!(
                    self.evaluation_state,
                    EvaluationState::Queued | EvaluationState::Running
                ),
            }
    }

    pub fn new_pending(
        context: RequestContext,
        policy_mode: PolicyMode,
        provider_kind: Option<String>,
        rendered_prompt: String,
        provider_input: Option<ProviderInputSnapshot>,
        llm_suggestion: Option<LlmSuggestion>,
    ) -> Self {
        let now = Utc::now();
        Self {
            id: Uuid::new_v4().to_string(),
            context,
            policy_mode,
            approval_status: ApprovalStatus::Pending,
            evaluation_state: match policy_mode {
                PolicyMode::ManualOnly => EvaluationState::NotRequired,
                PolicyMode::Assisted | PolicyMode::LlmAutomatic => EvaluationState::Queued,
            },
            final_decision: None,
            provider_kind,
            rendered_prompt,
            provider_input,
            llm_suggestion,
            automatic_decision: None,
            created_at: now,
            updated_at: now,
            resolved_at: None,
        }
    }

    pub fn record_submission_audit(&self) -> AuditRecord {
        AuditRecord::new(
            self.id.clone(),
            AuditAction::RequestSubmitted,
            self.context.requested_by.clone(),
            Some(self.context.reason.clone()),
            json!({
                "policy_mode": self.policy_mode,
                "resource": self.context.resource,
                "resource_tags": self.context.resource_tags,
                "resource_metadata": self.context.resource_metadata,
            }),
        )
    }

    pub fn record_llm_suggestion_audit(&self) -> Option<AuditRecord> {
        let suggestion = self.llm_suggestion.as_ref()?;
        let action = if suggestion.error.is_some() {
            AuditAction::LlmSuggestionFailed
        } else {
            AuditAction::LlmSuggestionGenerated
        };
        let note = suggestion
            .error
            .clone()
            .or_else(|| Some(suggestion.rationale_summary.clone()));

        Some(AuditRecord::new(
            self.id.clone(),
            action,
            suggestion.provider_kind.clone(),
            note,
            json!({
                "template_id": suggestion.template_id,
                "template_version": suggestion.template_version,
                "prompt_contract_version": suggestion.prompt_contract_version,
                "prompt_sha256": suggestion.prompt_sha256,
                "suggested_decision": suggestion.suggested_decision,
                "rationale_summary": suggestion.rationale_summary,
                "risk_score": suggestion.risk_score,
                "batch_decisions": suggestion.batch_decisions,
                "exposure_report": suggestion.exposure_report,
                "json_repair_strategy": suggestion.json_repair_strategy,
                "provider_response_id": suggestion.provider_response_id,
                "x_request_id": suggestion.x_request_id,
                "provider_model": suggestion.provider_model,
                "provider_trace": suggestion.provider_trace,
                "approval_status": self.approval_status,
            }),
        ))
    }

    pub fn record_llm_review_details_audit(&self) -> Option<AuditRecord> {
        let suggestion = self.llm_suggestion.as_ref()?;
        let progress = suggestion
            .provider_trace
            .as_ref()?
            .review_progress
            .as_ref()?;
        Some(AuditRecord::new(
            self.id.clone(),
            AuditAction::LlmReviewDetailsUpdated,
            suggestion.provider_kind.clone(),
            Some(format!(
                "call-chain details {}/{} ({:?})",
                progress.completed_units, progress.total_units, progress.state
            )),
            json!({
                "suggested_decision": suggestion.suggested_decision,
                "risk_score": suggestion.risk_score,
                "exposure_report": suggestion.exposure_report,
                "review_progress": progress,
                "provider_model": suggestion.provider_model,
                "provider_trace": suggestion.provider_trace,
                "approval_status": self.approval_status,
            }),
        ))
    }

    pub fn apply_manual_decision(
        &mut self,
        decision: Decision,
        actor: impl Into<String>,
        note: Option<String>,
    ) -> Result<Vec<AuditRecord>, DomainError> {
        if self.approval_status != ApprovalStatus::Pending {
            return Err(DomainError::AlreadyResolved {
                request_id: self.id.clone(),
                status: self.approval_status,
            });
        }
        let previous_evaluation_state = self.evaluation_state;
        let preempted_evaluation = matches!(
            self.evaluation_state,
            EvaluationState::Queued | EvaluationState::Running
        );
        if preempted_evaluation {
            self.evaluation_state = EvaluationState::Superseded;
        }

        self.final_decision = Some(decision);
        self.approval_status = match decision {
            Decision::Allow => ApprovalStatus::Approved,
            Decision::Deny => ApprovalStatus::Rejected,
        };
        let now = Utc::now();
        self.updated_at = now;
        self.resolved_at = Some(now);

        let actor = actor.into();
        let mut audits = vec![AuditRecord::new(
            self.id.clone(),
            AuditAction::ApprovalRecorded,
            actor.clone(),
            note.clone(),
            json!({
                "approval_status": self.approval_status,
                "decision": decision,
            }),
        )];

        if preempted_evaluation || self.should_record_human_override(decision) {
            audits.push(AuditRecord::new(
                self.id.clone(),
                AuditAction::HumanDecisionOverrodeLlm,
                actor,
                note,
                json!({
                    "approval_status": self.approval_status,
                    "decision": decision,
                    "previous_evaluation_state": previous_evaluation_state,
                    "evaluation_state": self.evaluation_state,
                    "suggested_decision": self.llm_suggestion.as_ref().map(|suggestion| suggestion.suggested_decision),
                    "risk_score": self.llm_suggestion.as_ref().map(|suggestion| suggestion.risk_score),
                }),
            ));
        }

        Ok(audits)
    }

    pub fn apply_automatic_decision(
        &mut self,
        automatic_decision: AutomaticDecisionTrace,
    ) -> Result<Vec<AuditRecord>, DomainError> {
        if self.approval_status != ApprovalStatus::Pending {
            return Err(DomainError::AlreadyResolved {
                request_id: self.id.clone(),
                status: self.approval_status,
            });
        }

        let evaluated_at = automatic_decision.evaluated_at;
        let auto_disposition = automatic_decision.auto_disposition;
        self.updated_at = evaluated_at;
        self.automatic_decision = Some(automatic_decision.clone());

        match auto_disposition {
            AutomaticDisposition::Allow => {
                self.final_decision = Some(Decision::Allow);
                self.approval_status = ApprovalStatus::Approved;
                self.resolved_at = Some(evaluated_at);
            }
            AutomaticDisposition::Deny => {
                self.final_decision = Some(Decision::Deny);
                self.approval_status = ApprovalStatus::Rejected;
                self.resolved_at = Some(evaluated_at);
            }
            AutomaticDisposition::Escalate => {
                self.final_decision = None;
                self.approval_status = ApprovalStatus::Pending;
                self.resolved_at = None;
            }
        }

        let mut audits = vec![AuditRecord::new(
            self.id.clone(),
            AuditAction::AutomaticDecisionRecorded,
            "system_auto".to_string(),
            Some(automatic_decision.auto_rationale_summary.clone()),
            json!({
                "auto_disposition": auto_disposition,
                "decision": auto_disposition,
                "decision_source": automatic_decision.decision_source,
                "approval_status": self.approval_status,
                "final_decision": self.final_decision,
                "matched_rule_ids": automatic_decision.matched_rule_ids,
                "secret_exposure_risk": automatic_decision.secret_exposure_risk,
                "provider_called": automatic_decision.provider_called,
                "suggested_decision": automatic_decision.suggested_decision,
                "risk_score": automatic_decision.risk_score,
                "template_id": automatic_decision.template_id,
                "template_version": automatic_decision.template_version,
                "prompt_contract_version": automatic_decision.prompt_contract_version,
                "provider_kind": automatic_decision.provider_kind,
                "provider_model": automatic_decision.provider_model,
                "x_request_id": automatic_decision.x_request_id,
                "provider_response_id": automatic_decision.provider_response_id,
                "auto_rationale_summary": automatic_decision.auto_rationale_summary,
                "fail_closed": automatic_decision.fail_closed,
                "batch_source_request_id": automatic_decision.batch_source_request_id,
                "evaluated_at": automatic_decision.evaluated_at,
            }),
        )];

        match auto_disposition {
            AutomaticDisposition::Allow | AutomaticDisposition::Deny => {
                audits.push(AuditRecord::new(
                    self.id.clone(),
                    AuditAction::ApprovalRecorded,
                    "system_auto".to_string(),
                    Some(automatic_decision.auto_rationale_summary.clone()),
                    json!({
                        "approval_status": self.approval_status,
                        "decision": self.final_decision,
                        "auto_disposition": auto_disposition,
                        "decision_source": automatic_decision.decision_source,
                    }),
                ))
            }
            AutomaticDisposition::Escalate => audits.push(AuditRecord::new(
                self.id.clone(),
                AuditAction::AutomaticEscalatedToHuman,
                "system_auto".to_string(),
                Some(automatic_decision.auto_rationale_summary.clone()),
                json!({
                    "auto_disposition": auto_disposition,
                    "decision": auto_disposition,
                    "decision_source": automatic_decision.decision_source,
                    "matched_rule_ids": automatic_decision.matched_rule_ids,
                    "secret_exposure_risk": automatic_decision.secret_exposure_risk,
                    "provider_called": automatic_decision.provider_called,
                    "suggested_decision": automatic_decision.suggested_decision,
                    "risk_score": automatic_decision.risk_score,
                    "template_id": automatic_decision.template_id,
                    "template_version": automatic_decision.template_version,
                    "prompt_contract_version": automatic_decision.prompt_contract_version,
                    "provider_kind": automatic_decision.provider_kind,
                    "provider_model": automatic_decision.provider_model,
                    "x_request_id": automatic_decision.x_request_id,
                    "provider_response_id": automatic_decision.provider_response_id,
                    "auto_rationale_summary": automatic_decision.auto_rationale_summary,
                    "fail_closed": automatic_decision.fail_closed,
                    "batch_source_request_id": automatic_decision.batch_source_request_id,
                }),
            )),
        }

        Ok(audits)
    }

    fn should_record_human_override(&self, decision: Decision) -> bool {
        let Some(suggestion) = self.llm_suggestion.as_ref() else {
            return false;
        };
        if suggestion.error.is_some() {
            return false;
        }

        match suggestion.suggested_decision {
            SuggestedDecision::Allow => decision != Decision::Allow,
            SuggestedDecision::Deny => decision != Decision::Deny,
            SuggestedDecision::Escalate => true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AuditRecord {
    pub id: String,
    pub request_id: String,
    pub action: AuditAction,
    pub actor: String,
    pub note: Option<String>,
    pub payload: Value,
    pub created_at: DateTime<Utc>,
}

impl AuditRecord {
    pub fn new(
        request_id: String,
        action: AuditAction,
        actor: String,
        note: Option<String>,
        payload: Value,
    ) -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            request_id,
            action,
            actor,
            note,
            payload,
            created_at: Utc::now(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DashboardData {
    pub pending_requests: Vec<AccessRequest>,
    pub recent_audit_records: Vec<AuditRecord>,
}

#[derive(Debug, thiserror::Error)]
pub enum DomainError {
    #[error("request {request_id} has already been resolved with status {status:?}")]
    AlreadyResolved {
        request_id: String,
        status: ApprovalStatus,
    },
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{LLM_ADVICE_TEMPLATE_VERSION, PROMPT_CONTRACT_VERSION};

    #[test]
    fn manual_decision_updates_request_state() {
        let context = RequestContext::new(
            "secret/api-token".to_string(),
            "Need smoke test access".to_string(),
            "alice".to_string(),
        );
        let mut request = AccessRequest::new_pending(
            context,
            PolicyMode::ManualOnly,
            None,
            "rendered prompt".to_string(),
            None,
            None,
        );

        let audits = request
            .apply_manual_decision(Decision::Allow, "reviewer", Some("looks safe".to_string()))
            .expect("manual decision should succeed");

        assert_eq!(request.approval_status, ApprovalStatus::Approved);
        assert_eq!(request.final_decision, Some(Decision::Allow));
        assert_eq!(audits.len(), 1);
        assert_eq!(audits[0].action, AuditAction::ApprovalRecorded);
    }

    #[test]
    fn manual_decision_preempts_queued_and_running_ai_evaluations() {
        for mode in [PolicyMode::Assisted, PolicyMode::LlmAutomatic] {
            for state in [EvaluationState::Queued, EvaluationState::Running] {
                for decision in [Decision::Allow, Decision::Deny] {
                    let mut request = AccessRequest::new_pending(
                        RequestContext::new(
                            "secret/api-token".into(),
                            "test".into(),
                            "alice".into(),
                        ),
                        mode,
                        Some("acp".into()),
                        "rendered prompt".into(),
                        None,
                        None,
                    );
                    request.evaluation_state = state;
                    let audits = request
                        .apply_manual_decision(decision, "reviewer", None)
                        .expect("human decision must not wait for AI");
                    assert_eq!(request.final_decision, Some(decision));
                    assert_eq!(request.evaluation_state, EvaluationState::Superseded);
                    assert!(request.resolved_at.is_some());
                    assert_eq!(audits[1].action, AuditAction::HumanDecisionOverrodeLlm);
                    assert_eq!(audits[1].payload["previous_evaluation_state"], json!(state));
                    assert!(matches!(
                        request.apply_manual_decision(decision, "reviewer", None),
                        Err(DomainError::AlreadyResolved { .. })
                    ));
                }
            }
        }
    }

    #[test]
    fn automatic_decision_updates_request_state() {
        let context = RequestContext::new(
            "secret/api-token".to_string(),
            "Need smoke test access".to_string(),
            "alice".to_string(),
        );
        let mut request = AccessRequest::new_pending(
            context,
            PolicyMode::LlmAutomatic,
            Some("mock".to_string()),
            "rendered prompt".to_string(),
            None,
            None,
        );
        let trace = AutomaticDecisionTrace {
            auto_disposition: AutomaticDisposition::Allow,
            decision_source: crate::AutomaticDecisionSource::LlmSuggestion,
            matched_rule_ids: vec!["llm_suggested_allow".to_string()],
            secret_exposure_risk: false,
            provider_called: true,
            suggested_decision: Some(SuggestedDecision::Allow),
            risk_score: Some(20),
            template_id: Some("llm_advice_request".to_string()),
            template_version: Some(LLM_ADVICE_TEMPLATE_VERSION.to_string()),
            prompt_contract_version: Some(PROMPT_CONTRACT_VERSION.to_string()),
            provider_kind: Some("mock".to_string()),
            provider_model: Some("mock-suggestion-v1".to_string()),
            x_request_id: None,
            provider_response_id: None,
            auto_rationale_summary: "model rationale".to_string(),
            fail_closed: false,
            batch_source_request_id: None,
            evaluated_at: Utc::now(),
        };

        let audits = request
            .apply_automatic_decision(trace)
            .expect("automatic decision should succeed");

        assert_eq!(request.approval_status, ApprovalStatus::Approved);
        assert_eq!(request.final_decision, Some(Decision::Allow));
        assert_eq!(audits.len(), 2);
        assert_eq!(audits[0].action, AuditAction::AutomaticDecisionRecorded);
        assert_eq!(audits[1].action, AuditAction::ApprovalRecorded);
    }

    #[test]
    fn human_review_required_matches_policy_evaluation_and_approval_matrix() {
        struct Case {
            name: &'static str,
            policy_mode: PolicyMode,
            approval_status: ApprovalStatus,
            evaluation_state: EvaluationState,
            expected: bool,
        }

        let cases = [
            Case {
                name: "manual_not_required",
                policy_mode: PolicyMode::ManualOnly,
                approval_status: ApprovalStatus::Pending,
                evaluation_state: EvaluationState::NotRequired,
                expected: true,
            },
            Case {
                name: "manual_queued",
                policy_mode: PolicyMode::ManualOnly,
                approval_status: ApprovalStatus::Pending,
                evaluation_state: EvaluationState::Queued,
                expected: true,
            },
            Case {
                name: "manual_running",
                policy_mode: PolicyMode::ManualOnly,
                approval_status: ApprovalStatus::Pending,
                evaluation_state: EvaluationState::Running,
                expected: true,
            },
            Case {
                name: "manual_completed",
                policy_mode: PolicyMode::ManualOnly,
                approval_status: ApprovalStatus::Pending,
                evaluation_state: EvaluationState::Completed,
                expected: true,
            },
            Case {
                name: "manual_failed",
                policy_mode: PolicyMode::ManualOnly,
                approval_status: ApprovalStatus::Pending,
                evaluation_state: EvaluationState::Failed,
                expected: true,
            },
            Case {
                name: "manual_interrupted",
                policy_mode: PolicyMode::ManualOnly,
                approval_status: ApprovalStatus::Pending,
                evaluation_state: EvaluationState::Interrupted,
                expected: true,
            },
            Case {
                name: "manual_superseded",
                policy_mode: PolicyMode::ManualOnly,
                approval_status: ApprovalStatus::Pending,
                evaluation_state: EvaluationState::Superseded,
                expected: true,
            },
            Case {
                name: "assisted_not_required",
                policy_mode: PolicyMode::Assisted,
                approval_status: ApprovalStatus::Pending,
                evaluation_state: EvaluationState::NotRequired,
                expected: true,
            },
            Case {
                name: "assisted_queued",
                policy_mode: PolicyMode::Assisted,
                approval_status: ApprovalStatus::Pending,
                evaluation_state: EvaluationState::Queued,
                expected: false,
            },
            Case {
                name: "assisted_running",
                policy_mode: PolicyMode::Assisted,
                approval_status: ApprovalStatus::Pending,
                evaluation_state: EvaluationState::Running,
                expected: false,
            },
            Case {
                name: "assisted_completed",
                policy_mode: PolicyMode::Assisted,
                approval_status: ApprovalStatus::Pending,
                evaluation_state: EvaluationState::Completed,
                expected: true,
            },
            Case {
                name: "assisted_failed",
                policy_mode: PolicyMode::Assisted,
                approval_status: ApprovalStatus::Pending,
                evaluation_state: EvaluationState::Failed,
                expected: true,
            },
            Case {
                name: "assisted_interrupted",
                policy_mode: PolicyMode::Assisted,
                approval_status: ApprovalStatus::Pending,
                evaluation_state: EvaluationState::Interrupted,
                expected: true,
            },
            Case {
                name: "assisted_superseded",
                policy_mode: PolicyMode::Assisted,
                approval_status: ApprovalStatus::Pending,
                evaluation_state: EvaluationState::Superseded,
                expected: true,
            },
            Case {
                name: "automatic_not_required",
                policy_mode: PolicyMode::LlmAutomatic,
                approval_status: ApprovalStatus::Pending,
                evaluation_state: EvaluationState::NotRequired,
                expected: true,
            },
            Case {
                name: "automatic_queued",
                policy_mode: PolicyMode::LlmAutomatic,
                approval_status: ApprovalStatus::Pending,
                evaluation_state: EvaluationState::Queued,
                expected: false,
            },
            Case {
                name: "automatic_running",
                policy_mode: PolicyMode::LlmAutomatic,
                approval_status: ApprovalStatus::Pending,
                evaluation_state: EvaluationState::Running,
                expected: false,
            },
            Case {
                name: "automatic_completed",
                policy_mode: PolicyMode::LlmAutomatic,
                approval_status: ApprovalStatus::Pending,
                evaluation_state: EvaluationState::Completed,
                expected: true,
            },
            Case {
                name: "automatic_failed",
                policy_mode: PolicyMode::LlmAutomatic,
                approval_status: ApprovalStatus::Pending,
                evaluation_state: EvaluationState::Failed,
                expected: true,
            },
            Case {
                name: "automatic_interrupted",
                policy_mode: PolicyMode::LlmAutomatic,
                approval_status: ApprovalStatus::Pending,
                evaluation_state: EvaluationState::Interrupted,
                expected: true,
            },
            Case {
                name: "automatic_superseded",
                policy_mode: PolicyMode::LlmAutomatic,
                approval_status: ApprovalStatus::Pending,
                evaluation_state: EvaluationState::Superseded,
                expected: true,
            },
            Case {
                name: "manual_approved",
                policy_mode: PolicyMode::ManualOnly,
                approval_status: ApprovalStatus::Approved,
                evaluation_state: EvaluationState::NotRequired,
                expected: false,
            },
            Case {
                name: "assisted_approved",
                policy_mode: PolicyMode::Assisted,
                approval_status: ApprovalStatus::Approved,
                evaluation_state: EvaluationState::Completed,
                expected: false,
            },
            Case {
                name: "automatic_approved",
                policy_mode: PolicyMode::LlmAutomatic,
                approval_status: ApprovalStatus::Approved,
                evaluation_state: EvaluationState::Completed,
                expected: false,
            },
            Case {
                name: "manual_rejected",
                policy_mode: PolicyMode::ManualOnly,
                approval_status: ApprovalStatus::Rejected,
                evaluation_state: EvaluationState::NotRequired,
                expected: false,
            },
            Case {
                name: "assisted_rejected",
                policy_mode: PolicyMode::Assisted,
                approval_status: ApprovalStatus::Rejected,
                evaluation_state: EvaluationState::Failed,
                expected: false,
            },
            Case {
                name: "automatic_rejected",
                policy_mode: PolicyMode::LlmAutomatic,
                approval_status: ApprovalStatus::Rejected,
                evaluation_state: EvaluationState::Failed,
                expected: false,
            },
        ];

        for case in cases {
            let mut request = AccessRequest::new_pending(
                RequestContext::new(
                    format!("secret/{}", case.name),
                    "exercise human review matrix".into(),
                    "test".into(),
                ),
                case.policy_mode,
                None,
                String::new(),
                None,
                None,
            );
            request.approval_status = case.approval_status;
            request.evaluation_state = case.evaluation_state;

            assert_eq!(
                request.human_review_required(),
                case.expected,
                "{}",
                case.name
            );
        }
    }
}
