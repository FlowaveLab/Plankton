use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::{exposure_policy_from_metadata, EXPOSURE_POLICY_METADATA_KEY};
use crate::{
    LlmApprovalDecisionPolicy, LlmSuggestion, ProviderInputSnapshot, SanitizedPromptContext,
    SuggestedDecision, LLM_ADVICE_TEMPLATE_ID, LLM_ADVICE_TEMPLATE_VERSION,
    PROMPT_CONTRACT_VERSION,
};
use plankton_protocol::exposure::ExposureBreachAction;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AutomaticDisposition {
    Allow,
    Deny,
    Escalate,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AutomaticDecisionSource {
    LocalRule,
    LlmSuggestion,
    CombinedGuardrail,
    BatchTicket,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AutomaticDecisionTrace {
    pub auto_disposition: AutomaticDisposition,
    pub decision_source: AutomaticDecisionSource,
    pub matched_rule_ids: Vec<String>,
    pub secret_exposure_risk: bool,
    pub provider_called: bool,
    pub suggested_decision: Option<SuggestedDecision>,
    pub risk_score: Option<u8>,
    pub template_id: Option<String>,
    pub template_version: Option<String>,
    pub prompt_contract_version: Option<String>,
    pub provider_kind: Option<String>,
    pub provider_model: Option<String>,
    pub x_request_id: Option<String>,
    pub provider_response_id: Option<String>,
    pub auto_rationale_summary: String,
    pub fail_closed: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub batch_source_request_id: Option<String>,
    pub evaluated_at: DateTime<Utc>,
}

pub fn evaluate_automatic_disposition(
    request_provider_kind: Option<&str>,
    provider_input: Option<&ProviderInputSnapshot>,
    suggestion: Option<&LlmSuggestion>,
    sanitized_context: &SanitizedPromptContext,
) -> AutomaticDecisionTrace {
    let decision_policy = provider_input
        .map(|input| input.decision_policy)
        .unwrap_or_default();
    let mut base_trace = AutomaticDecisionTrace {
        auto_disposition: safe_non_allow_disposition(decision_policy),
        decision_source: AutomaticDecisionSource::CombinedGuardrail,
        matched_rule_ids: Vec::new(),
        secret_exposure_risk: false,
        provider_called: provider_input.is_some() || suggestion.is_some(),
        suggested_decision: suggestion.map(|suggestion| suggestion.suggested_decision),
        risk_score: suggestion.map(|suggestion| suggestion.risk_score),
        template_id: provider_input.map(|input| input.template_id.clone()),
        template_version: provider_input.map(|input| input.template_version.clone()),
        prompt_contract_version: provider_input.map(|input| input.prompt_contract_version.clone()),
        provider_kind: suggestion
            .map(|suggestion| suggestion.provider_kind.clone())
            .or_else(|| request_provider_kind.map(ToOwned::to_owned)),
        provider_model: suggestion.and_then(|suggestion| suggestion.provider_model.clone()),
        x_request_id: suggestion.and_then(|suggestion| suggestion.x_request_id.clone()),
        provider_response_id: suggestion
            .and_then(|suggestion| suggestion.provider_response_id.clone()),
        auto_rationale_summary: guard_rationale(
            decision_policy,
            "no final automatic disposition could be proven",
        ),
        fail_closed: true,
        batch_source_request_id: None,
        evaluated_at: Utc::now(),
    };

    let Some(provider_input) = provider_input else {
        base_trace.matched_rule_ids = vec!["guard_provider_input_missing".to_string()];
        base_trace.auto_rationale_summary =
            guard_rationale(decision_policy, "provider_input was missing");
        return base_trace;
    };

    if !decision_policy.allow || (!decision_policy.deny && !decision_policy.escalate) {
        base_trace.auto_disposition = AutomaticDisposition::Escalate;
        base_trace.matched_rule_ids = vec!["guard_decision_policy_invalid".to_string()];
        base_trace.auto_rationale_summary =
            "Automatic mode escalated because the saved LLM decision policy was invalid"
                .to_string();
        return base_trace;
    }

    let Some(suggestion) = suggestion else {
        base_trace.matched_rule_ids = vec!["guard_llm_suggestion_missing".to_string()];
        base_trace.auto_rationale_summary =
            guard_rationale(decision_policy, "no LLM suggestion was returned");
        return base_trace;
    };

    if suggestion.error.is_some() {
        base_trace.matched_rule_ids = vec!["guard_provider_error".to_string()];
        base_trace.auto_rationale_summary = guard_rationale(
            decision_policy,
            &format!(
                "the provider failed: {}",
                suggestion
                    .error
                    .as_deref()
                    .unwrap_or("unknown provider error")
            ),
        );
        return base_trace;
    }

    if provider_input.template_id != LLM_ADVICE_TEMPLATE_ID
        || provider_input.template_version != LLM_ADVICE_TEMPLATE_VERSION
        || provider_input.prompt_contract_version != PROMPT_CONTRACT_VERSION
    {
        base_trace.matched_rule_ids = vec!["guard_template_not_allowlisted".to_string()];
        base_trace.auto_rationale_summary = guard_rationale(
            decision_policy,
            "template/provider contract was not allow-listed",
        );
        return base_trace;
    }

    if provider_input.prompt_contract_version != suggestion.prompt_contract_version {
        base_trace.matched_rule_ids = vec!["guard_prompt_contract_mismatch".to_string()];
        base_trace.auto_rationale_summary = guard_rationale(
            decision_policy,
            "provider_input and suggestion used different prompt contracts",
        );
        return base_trace;
    }

    if provider_input.template_id != suggestion.template_id
        || provider_input.template_version != suggestion.template_version
    {
        base_trace.matched_rule_ids = vec!["guard_template_trace_mismatch".to_string()];
        base_trace.auto_rationale_summary = guard_rationale(
            decision_policy,
            "template trace mismatched between provider_input and suggestion",
        );
        return base_trace;
    }

    if provider_input.prompt_sha256 != suggestion.prompt_sha256 {
        base_trace.matched_rule_ids = vec!["guard_prompt_sha_mismatch".to_string()];
        base_trace.auto_rationale_summary = guard_rationale(
            decision_policy,
            "provider_input and suggestion referenced different prompt digests",
        );
        return base_trace;
    }

    if suggestion.rationale_summary.trim().is_empty() {
        base_trace.matched_rule_ids = vec!["guard_missing_rationale_summary".to_string()];
        base_trace.auto_rationale_summary =
            guard_rationale(decision_policy, "the provider returned an empty rationale");
        return base_trace;
    }

    if let Some(request_provider_kind) = request_provider_kind {
        let request_provider_kind = request_provider_kind.trim();
        if !request_provider_kind.is_empty() && request_provider_kind != suggestion.provider_kind {
            base_trace.matched_rule_ids = vec!["guard_provider_kind_mismatch".to_string()];
            base_trace.auto_rationale_summary =
                guard_rationale(decision_policy, "request/provider kinds did not match");
            return base_trace;
        }
    }

    if let Some(report) = suggestion.exposure_report.as_ref() {
        if let Err(error) = report.validate() {
            base_trace.matched_rule_ids = vec!["guard_exposure_report_invalid".into()];
            base_trace.auto_rationale_summary = guard_rationale(decision_policy, &error);
            return base_trace;
        }
        if report
            .surfaces
            .iter()
            .any(|surface| surface.evidence_state == crate::ExposureEvidenceState::Unknown)
        {
            base_trace.matched_rule_ids = vec!["guard_exposure_evidence_unknown".into()];
            base_trace.auto_rationale_summary = guard_rationale(
                decision_policy,
                "decision-relevant exposure evidence is unknown",
            );
            return base_trace;
        }
    }

    if sanitized_context
        .metadata
        .contains_key(EXPOSURE_POLICY_METADATA_KEY)
    {
        let policy = exposure_policy_from_metadata(&sanitized_context.metadata);
        let Some(report) = suggestion.exposure_report.as_ref() else {
            return exposure_policy_trace(
                &policy,
                "guard_exposure_report_missing",
                "the configured credential exposure policy requires a validated exposure report",
                provider_input,
                suggestion,
                sanitized_context,
            );
        };
        let exceeded = report.exceeded_surfaces(&policy);
        if !exceeded.is_empty() {
            return exposure_policy_trace(
                &policy,
                "guard_exposure_policy_exceeded",
                &format!("credential exposure exceeded configured surfaces: {exceeded:?}"),
                provider_input,
                suggestion,
                sanitized_context,
            );
        }
    }

    match suggestion.suggested_decision {
        SuggestedDecision::Allow => build_model_trace(
            AutomaticDisposition::Allow,
            "llm_suggested_allow",
            provider_input,
            suggestion,
            sanitized_context,
        ),
        SuggestedDecision::Deny if decision_policy.deny => build_model_trace(
            AutomaticDisposition::Deny,
            "llm_suggested_deny",
            provider_input,
            suggestion,
            sanitized_context,
        ),
        SuggestedDecision::Deny => build_policy_routed_model_trace(
            AutomaticDisposition::Escalate,
            "guard_llm_deny_disabled",
            "The model suggested deny, but settings route disabled deny decisions to human review",
            provider_input,
            suggestion,
            sanitized_context,
        ),
        SuggestedDecision::Escalate if decision_policy.escalate => build_model_trace(
            AutomaticDisposition::Escalate,
            "llm_suggested_escalate",
            provider_input,
            suggestion,
            sanitized_context,
        ),
        SuggestedDecision::Escalate => build_policy_routed_model_trace(
            AutomaticDisposition::Deny,
            "guard_llm_escalate_disabled",
            "The model suggested human review, but settings route disabled escalation decisions to deny",
            provider_input,
            suggestion,
            sanitized_context,
        ),
    }
}

fn exposure_policy_trace(
    policy: &plankton_protocol::exposure::CredentialExposurePolicy,
    rule: &str,
    reason: &str,
    provider_input: &ProviderInputSnapshot,
    suggestion: &LlmSuggestion,
    sanitized_context: &SanitizedPromptContext,
) -> AutomaticDecisionTrace {
    let disposition = match policy.breach_action {
        ExposureBreachAction::HumanReview => AutomaticDisposition::Escalate,
        ExposureBreachAction::Deny => AutomaticDisposition::Deny,
    };
    build_policy_routed_model_trace(
        disposition,
        rule,
        reason,
        provider_input,
        suggestion,
        sanitized_context,
    )
}

fn safe_non_allow_disposition(decision_policy: LlmApprovalDecisionPolicy) -> AutomaticDisposition {
    if decision_policy.escalate {
        AutomaticDisposition::Escalate
    } else if decision_policy.deny {
        AutomaticDisposition::Deny
    } else {
        AutomaticDisposition::Escalate
    }
}

fn guard_rationale(decision_policy: LlmApprovalDecisionPolicy, reason: &str) -> String {
    match safe_non_allow_disposition(decision_policy) {
        AutomaticDisposition::Deny => format!(
            "Automatic mode denied the request because {reason}; human escalation is disabled by settings"
        ),
        _ => format!("Automatic mode escalated because {reason}"),
    }
}

fn build_model_trace(
    auto_disposition: AutomaticDisposition,
    matched_rule_id: &str,
    provider_input: &ProviderInputSnapshot,
    suggestion: &LlmSuggestion,
    _sanitized_context: &SanitizedPromptContext,
) -> AutomaticDecisionTrace {
    AutomaticDecisionTrace {
        auto_disposition,
        decision_source: AutomaticDecisionSource::LlmSuggestion,
        matched_rule_ids: vec![matched_rule_id.to_string()],
        secret_exposure_risk: false,
        provider_called: true,
        suggested_decision: Some(suggestion.suggested_decision),
        risk_score: Some(suggestion.risk_score),
        template_id: Some(provider_input.template_id.clone()),
        template_version: Some(provider_input.template_version.clone()),
        prompt_contract_version: Some(provider_input.prompt_contract_version.clone()),
        provider_kind: Some(suggestion.provider_kind.clone()),
        provider_model: suggestion.provider_model.clone(),
        x_request_id: suggestion.x_request_id.clone(),
        provider_response_id: suggestion.provider_response_id.clone(),
        auto_rationale_summary: suggestion.rationale_summary.clone(),
        fail_closed: false,
        batch_source_request_id: None,
        evaluated_at: Utc::now(),
    }
}

fn build_policy_routed_model_trace(
    auto_disposition: AutomaticDisposition,
    matched_rule_id: &str,
    rationale_summary: &str,
    provider_input: &ProviderInputSnapshot,
    suggestion: &LlmSuggestion,
    sanitized_context: &SanitizedPromptContext,
) -> AutomaticDecisionTrace {
    let mut trace = build_model_trace(
        auto_disposition,
        matched_rule_id,
        provider_input,
        suggestion,
        sanitized_context,
    );
    trace.decision_source = AutomaticDecisionSource::CombinedGuardrail;
    trace.auto_rationale_summary = rationale_summary.to_string();
    trace
}

pub fn automatic_decision_from_batch(
    source_request_id: String,
    decision: &crate::BatchResourceDecision,
    provider_kind: Option<String>,
    provider_model: Option<String>,
    decision_policy: LlmApprovalDecisionPolicy,
    sanitized_context: &SanitizedPromptContext,
    shared_report: Option<&crate::CredentialExposureReport>,
) -> AutomaticDecisionTrace {
    let report_error = shared_report.and_then(|report| report.validate().err());
    let unknown = shared_report.is_some_and(|report| {
        report
            .surfaces
            .iter()
            .any(|surface| surface.evidence_state == crate::ExposureEvidenceState::Unknown)
    });
    let policy = exposure_policy_from_metadata(&sanitized_context.metadata);
    let policy_required = sanitized_context
        .metadata
        .contains_key(EXPOSURE_POLICY_METADATA_KEY);
    let exceeded =
        shared_report.is_some_and(|report| !report.exceeded_surfaces(&policy).is_empty());
    let guard_disposition =
        if report_error.is_some() || unknown || (policy_required && shared_report.is_none()) {
            Some(safe_non_allow_disposition(decision_policy))
        } else if policy_required && exceeded {
            Some(match policy.breach_action {
                ExposureBreachAction::HumanReview => AutomaticDisposition::Escalate,
                ExposureBreachAction::Deny => AutomaticDisposition::Deny,
            })
        } else {
            None
        };
    let auto_disposition = guard_disposition.unwrap_or(match decision.suggested_decision {
        SuggestedDecision::Allow => AutomaticDisposition::Allow,
        SuggestedDecision::Deny if decision_policy.deny => AutomaticDisposition::Deny,
        SuggestedDecision::Deny => AutomaticDisposition::Escalate,
        SuggestedDecision::Escalate if decision_policy.escalate => AutomaticDisposition::Escalate,
        SuggestedDecision::Escalate => AutomaticDisposition::Deny,
    });
    let decision_was_routed = !decision_policy.allows(decision.suggested_decision);
    AutomaticDecisionTrace {
        auto_disposition,
        decision_source: AutomaticDecisionSource::BatchTicket,
        matched_rule_ids: vec![if guard_disposition.is_some() {
            "guard_shared_batch_exposure".to_string()
        } else if decision_was_routed {
            "semantic_call_chain_batch_ticket_policy_routed".to_string()
        } else {
            "semantic_call_chain_batch_ticket".to_string()
        }],
        secret_exposure_risk: false,
        provider_called: false,
        suggested_decision: Some(decision.suggested_decision),
        risk_score: Some(decision.risk_score),
        template_id: Some(LLM_ADVICE_TEMPLATE_ID.to_string()),
        template_version: Some(LLM_ADVICE_TEMPLATE_VERSION.to_string()),
        prompt_contract_version: Some(PROMPT_CONTRACT_VERSION.to_string()),
        provider_kind,
        provider_model,
        x_request_id: None,
        provider_response_id: None,
        auto_rationale_summary: if guard_disposition.is_some() {
            "Shared batch exposure evidence did not satisfy the current request policy".into()
        } else if decision_was_routed {
            format!(
                "{} The saved decision was routed to an enabled outcome by current request settings.",
                decision.rationale_summary
            )
        } else {
            decision.rationale_summary.clone()
        },
        fail_closed: guard_disposition.is_some(),
        batch_source_request_id: Some(source_request_id),
        evaluated_at: Utc::now(),
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        build_prompt_context, store_exposure_policy, CredentialExposureReport,
        ExposureEvidenceState, ExposureSurfaceAssessment, LlmSuggestion, ProviderInputSnapshot,
        RequestContext, SuggestedDecision, LLM_ADVICE_TEMPLATE_ID, LLM_ADVICE_TEMPLATE_VERSION,
        PROMPT_CONTRACT_VERSION,
    };

    use super::*;

    fn sample_request_context() -> RequestContext {
        let mut context = RequestContext::new(
            "secret/dev-token".to_string(),
            "Need smoke test access".to_string(),
            "alice".to_string(),
        );
        context
            .metadata
            .insert("environment".to_string(), "dev".to_string());
        context
    }

    fn sample_provider_input() -> ProviderInputSnapshot {
        let sanitized = build_prompt_context(&sample_request_context());

        ProviderInputSnapshot {
            template_id: LLM_ADVICE_TEMPLATE_ID.to_string(),
            template_version: LLM_ADVICE_TEMPLATE_VERSION.to_string(),
            prompt_contract_version: PROMPT_CONTRACT_VERSION.to_string(),
            prompt_sha256: "digest-1".to_string(),
            prompt: "safe prompt".to_string(),
            decision_policy: LlmApprovalDecisionPolicy::default(),
            allowed_read_files: Vec::new(),
            sanitized_context: sanitized,
        }
    }

    fn sample_suggestion(decision: SuggestedDecision, risk_score: u8) -> LlmSuggestion {
        LlmSuggestion {
            template_id: LLM_ADVICE_TEMPLATE_ID.to_string(),
            template_version: LLM_ADVICE_TEMPLATE_VERSION.to_string(),
            prompt_contract_version: PROMPT_CONTRACT_VERSION.to_string(),
            prompt_sha256: "digest-1".to_string(),
            suggested_decision: decision,
            rationale_summary: "model rationale".to_string(),
            risk_score,
            batch_decisions: Vec::new(),
            exposure_report: None,
            json_repair_strategy: None,
            provider_kind: "mock".to_string(),
            provider_model: Some("mock-suggestion-v1".to_string()),
            provider_response_id: None,
            x_request_id: None,
            provider_trace: None,
            usage: None,
            error: None,
            generated_at: Utc::now(),
        }
    }

    fn exposure_report(level: u8) -> CredentialExposureReport {
        CredentialExposureReport {
            chain_summary: "bounded test chain".into(),
            node_assessments: Vec::new(),
            surfaces: plankton_protocol::exposure::CredentialExposureSurface::ALL
                .into_iter()
                .map(|surface| ExposureSurfaceAssessment {
                    surface,
                    actual_level: level,
                    evidence_state: ExposureEvidenceState::Observed,
                    network_destinations: if surface
                        == plankton_protocol::exposure::CredentialExposureSurface::Network
                        && level > 0
                    {
                        vec!["https://example.test".into()]
                    } else {
                        Vec::new()
                    },
                    summary: "test assessment".into(),
                    annotations: Vec::new(),
                })
                .collect(),
        }
    }

    #[test]
    fn unknown_evidence_cannot_allow_even_when_network_level_two_is_permitted() {
        let mut context = sample_request_context();
        let mut policy = exposure_policy_from_metadata(&context.resource_metadata);
        policy
            .surfaces
            .iter_mut()
            .find(|p| p.surface == plankton_protocol::exposure::CredentialExposureSurface::Network)
            .unwrap()
            .max_level = 2;
        store_exposure_policy(&mut context.resource_metadata, &policy).unwrap();
        let sanitized = build_prompt_context(&context);
        let input = sample_provider_input();
        let mut suggestion = sample_suggestion(SuggestedDecision::Allow, 0);
        let mut report = exposure_report(0);
        let network = report
            .surfaces
            .iter_mut()
            .find(|p| p.surface == plankton_protocol::exposure::CredentialExposureSurface::Network)
            .unwrap();
        network.actual_level = 2;
        network.evidence_state = ExposureEvidenceState::Unknown;
        suggestion.exposure_report = Some(report.clone());
        let result = evaluate_automatic_disposition(
            Some("mock"),
            Some(&input),
            Some(&suggestion),
            &sanitized,
        );
        assert_eq!(result.auto_disposition, AutomaticDisposition::Escalate);
        assert_eq!(result.matched_rule_ids, ["guard_exposure_evidence_unknown"]);
        let batch = crate::BatchResourceDecision {
            resource_selector: "other".into(),
            suggested_decision: SuggestedDecision::Allow,
            rationale_summary: "shared".into(),
            risk_score: 0,
        };
        let result = automatic_decision_from_batch(
            "source".into(),
            &batch,
            None,
            None,
            input.decision_policy,
            &sanitized,
            Some(&report),
        );
        assert_eq!(result.auto_disposition, AutomaticDisposition::Escalate);
        assert!(result.fail_closed);
        suggestion
            .exposure_report
            .as_mut()
            .unwrap()
            .surfaces
            .iter_mut()
            .find(|p| p.evidence_state == ExposureEvidenceState::Unknown)
            .unwrap()
            .actual_level = 0;
        let result = evaluate_automatic_disposition(
            Some("mock"),
            Some(&input),
            Some(&suggestion),
            &sanitized,
        );
        assert_eq!(result.matched_rule_ids, ["guard_exposure_report_invalid"]);
    }

    #[test]
    fn configured_exposure_breach_routes_allow_to_human_review() {
        let mut context = sample_request_context();
        store_exposure_policy(
            &mut context.resource_metadata,
            &plankton_protocol::exposure::CredentialExposurePolicy::default(),
        )
        .expect("store policy");
        let sanitized = build_prompt_context(&context);
        let mut provider_input = sample_provider_input();
        provider_input.sanitized_context = sanitized.clone();
        let mut suggestion = sample_suggestion(SuggestedDecision::Allow, 10);
        suggestion.exposure_report = Some(exposure_report(2));

        let trace = evaluate_automatic_disposition(
            Some("mock"),
            Some(&provider_input),
            Some(&suggestion),
            &sanitized,
        );
        assert_eq!(trace.auto_disposition, AutomaticDisposition::Escalate);
        assert_eq!(
            trace.matched_rule_ids,
            vec!["guard_exposure_policy_exceeded"]
        );
    }

    #[test]
    fn provider_context_does_not_report_secret_exposure_risk() {
        let mut context = sample_request_context();
        context.metadata.insert(
            "api_token".to_string(),
            "sk-live-super-secret-value".to_string(),
        );
        let sanitized = build_prompt_context(&context);

        assert!(!evaluate_automatic_disposition(None, None, None, &sanitized).secret_exposure_risk);
    }

    #[test]
    fn automatic_path_allows_when_model_says_allow() {
        let context = sample_request_context();
        let sanitized = build_prompt_context(&context);
        let provider_input = sample_provider_input();
        let suggestion = sample_suggestion(SuggestedDecision::Allow, 55);

        let trace = evaluate_automatic_disposition(
            Some("mock"),
            Some(&provider_input),
            Some(&suggestion),
            &sanitized,
        );

        assert_eq!(trace.auto_disposition, AutomaticDisposition::Allow);
        assert!(trace.provider_called);
        assert_eq!(
            trace.decision_source,
            AutomaticDecisionSource::LlmSuggestion
        );
        assert!(trace
            .matched_rule_ids
            .contains(&"llm_suggested_allow".to_string()));
        assert_eq!(trace.auto_rationale_summary, "model rationale");
    }

    #[test]
    fn automatic_path_denies_when_model_says_deny() {
        let context = sample_request_context();
        let sanitized = build_prompt_context(&context);
        let provider_input = sample_provider_input();
        let suggestion = sample_suggestion(SuggestedDecision::Deny, 12);

        let trace = evaluate_automatic_disposition(
            Some("mock"),
            Some(&provider_input),
            Some(&suggestion),
            &sanitized,
        );

        assert_eq!(trace.auto_disposition, AutomaticDisposition::Deny);
        assert!(trace
            .matched_rule_ids
            .contains(&"llm_suggested_deny".to_string()));
    }

    #[test]
    fn automatic_path_escalates_with_model_rationale() {
        let context = sample_request_context();
        let sanitized = build_prompt_context(&context);
        let provider_input = sample_provider_input();
        let suggestion = sample_suggestion(SuggestedDecision::Escalate, 55);

        let trace = evaluate_automatic_disposition(
            Some("mock"),
            Some(&provider_input),
            Some(&suggestion),
            &sanitized,
        );

        assert_eq!(trace.auto_disposition, AutomaticDisposition::Escalate);
        assert!(!trace.fail_closed);
        assert!(trace
            .matched_rule_ids
            .contains(&"llm_suggested_escalate".to_string()));
        assert_eq!(trace.auto_rationale_summary, "model rationale");
    }

    #[test]
    fn disabled_deny_is_routed_to_human_review() {
        let context = sample_request_context();
        let sanitized = build_prompt_context(&context);
        let mut provider_input = sample_provider_input();
        provider_input.decision_policy.deny = false;
        let suggestion = sample_suggestion(SuggestedDecision::Deny, 70);

        let trace = evaluate_automatic_disposition(
            Some("mock"),
            Some(&provider_input),
            Some(&suggestion),
            &sanitized,
        );

        assert_eq!(trace.auto_disposition, AutomaticDisposition::Escalate);
        assert_eq!(
            trace.decision_source,
            AutomaticDecisionSource::CombinedGuardrail
        );
        assert_eq!(trace.matched_rule_ids, ["guard_llm_deny_disabled"]);
    }

    #[test]
    fn disabled_escalation_routes_provider_failure_to_deny() {
        let context = sample_request_context();
        let sanitized = build_prompt_context(&context);
        let mut provider_input = sample_provider_input();
        provider_input.decision_policy.escalate = false;
        let mut suggestion = sample_suggestion(SuggestedDecision::Escalate, 100);
        suggestion.error = Some("provider unavailable".to_string());

        let trace = evaluate_automatic_disposition(
            Some("mock"),
            Some(&provider_input),
            Some(&suggestion),
            &sanitized,
        );

        assert_eq!(trace.auto_disposition, AutomaticDisposition::Deny);
        assert!(trace.fail_closed);
        assert!(trace
            .auto_rationale_summary
            .contains("escalation is disabled"));
    }
}
