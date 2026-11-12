//! Batch transport does not merge authorization: every request retains its own
//! evidence, enabled outcomes, exposure policy and final store transaction.
use std::collections::{BTreeMap, BTreeSet};

use super::*;

pub const MAX_REVIEW_BATCH_SIZE: usize = 8;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BatchReviewInput {
    pub request_id: String,
    pub policy_mode: PolicyMode,
    pub input: ProviderInputSnapshot,
}

#[derive(Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
struct BatchResult {
    reviews: Vec<BatchEntry>,
}

#[derive(Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
struct BatchEntry {
    request_id: String,
    decision: BatchDecision,
    #[serde(default)]
    evidence: Vec<SourceCitation>,
}

// Request-keyed batches have no legacy resource-selector decisions. Keep this
// compatibility field out of the generated schema and accept only an empty array:
// it carries no decision, authorization or evidence that could be discarded.
#[derive(Default)]
struct EmptyLegacyBatchDecisions;

impl<'de> Deserialize<'de> for EmptyLegacyBatchDecisions {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let _: [serde::de::IgnoredAny; 0] = Deserialize::deserialize(deserializer)?;
        Ok(Self)
    }
}

#[derive(Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
struct BatchDecision {
    suggested_decision: SuggestedDecision,
    rationale_summary: String,
    #[schemars(range(min = 0, max = 100))]
    risk_score: u8,
    #[serde(deserialize_with = "deserialize_batch_exposure_report")]
    #[schemars(required, with = "CredentialExposureReport")]
    exposure_report: Option<CredentialExposureReport>,
    #[serde(default, rename = "batch_decisions")]
    #[schemars(skip)]
    _legacy_batch_decisions: EmptyLegacyBatchDecisions,
}

fn deserialize_batch_exposure_report<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<CredentialExposureReport>, D::Error> {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Report {
        chain_summary: String,
        #[serde(default)]
        node_assessments: Vec<CallChainNodeAssessment>,
        surfaces: Vec<crate::ExposureSurfaceAssessment>,
        #[serde(default, rename = "batch_decisions")]
        _legacy_batch_decisions: EmptyLegacyBatchDecisions,
    }
    // Typed deserialization also retains duplicate-field rejection. Parsing into
    // a generic JSON map and deleting keys would silently accept duplicate keys.
    Ok(
        Option::<Report>::deserialize(deserializer)?.map(|report| CredentialExposureReport {
            chain_summary: report.chain_summary,
            node_assessments: report.node_assessments,
            surfaces: report.surfaces,
        }),
    )
}

#[derive(Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
struct SourceCitation {
    node_index: usize,
    source_id: String,
    #[schemars(range(min = 1))]
    start_line: usize,
    #[schemars(range(min = 1))]
    end_line: usize,
    #[schemars(length(min = 1))]
    surfaces: Vec<CredentialExposureSurface>,
    #[schemars(length(min = 1))]
    reason: String,
}

pub async fn request_llm_batch_suggestions(
    settings: &PlanktonSettings,
    inputs: &[BatchReviewInput],
) -> BTreeMap<String, LlmSuggestion> {
    let result = review_batch(settings, inputs).await;
    match result {
        Ok(suggestions) => suggestions,
        Err(error) => inputs
            .iter()
            .map(|request| {
                (
                    request.request_id.clone(),
                    llm_suggestion_from_error(&request.input, ACP_PROVIDER_KIND, &error),
                )
            })
            .collect(),
    }
}

async fn review_batch(
    settings: &PlanktonSettings,
    inputs: &[BatchReviewInput],
) -> Result<BTreeMap<String, LlmSuggestion>, ProviderError> {
    if inputs.is_empty() || inputs.len() > MAX_REVIEW_BATCH_SIZE {
        return Err(ProviderError::RequestBuild(
            "review batch must contain 1..8 requests".into(),
        ));
    }
    let expected = inputs
        .iter()
        .map(|request| request.request_id.clone())
        .collect::<BTreeSet<_>>();
    if expected.len() != inputs.len() {
        return Err(ProviderError::RequestBuild(
            "duplicate request ID in review batch".into(),
        ));
    }
    let prompt = batch_prompt(settings, inputs)?;
    let files = inputs
        .iter()
        .flat_map(|request| request.input.allowed_read_files.iter().cloned())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    let validation_inputs = inputs.to_vec();
    let validation_ids = expected.clone();
    let validator = std::sync::Arc::new(move |content: &str| {
        let entries = decode_batch(content, &validation_ids)?;
        let result = crate::AcpPromptResult {
            content: content.into(),
            provider_model: None,
            trace: Default::default(),
            validated_exposure_report: None,
        };
        let errors = validation_inputs
            .iter()
            .filter_map(|input| {
                response_for_entry(
                    &provider_request(input),
                    &entries[&input.request_id],
                    &result,
                    "",
                )
                .err()
                .map(|error| format!("request {}: {error}", input.request_id))
            })
            .collect::<Vec<_>>();
        if errors.is_empty() {
            Ok(JsonRepairStrategy::Strict)
        } else {
            Err(ProviderError::InvalidResponse(errors.join("; ")))
        }
    });
    let mut result = AcpSessionClient::from_settings(settings)?
        .prompt_validated_review_with_files(prompt.clone(), files, validator)
        .await?;
    result.trace.rendered_prompt = Some(prompt.clone());
    result
        .trace
        .audit_events
        .push(serde_json::json!({"type":"batch_review_result","content":result.content}));
    let entries = decode_batch(&result.content, &expected)
        .map_err(|error| batch_validation_error(error, &result.trace))?;
    let mut suggestions = BTreeMap::new();
    for input in inputs {
        let entry = entries
            .get(&input.request_id)
            .expect("exact coverage validated");
        let request = provider_request(input);
        let response = response_for_entry(&request, entry, &result, &prompt);
        let suggestion = match response {
            Ok(response) => {
                suggestion_from_provider_response(&input.input, ACP_PROVIDER_KIND, response)
            }
            Err(error) => llm_suggestion_from_error(
                &input.input,
                ACP_PROVIDER_KIND,
                &batch_validation_error(error, &result.trace),
            ),
        };
        suggestions.insert(input.request_id.clone(), suggestion);
    }
    Ok(suggestions)
}

fn batch_validation_error(error: ProviderError, trace: &ProviderTrace) -> ProviderError {
    let mut trace = trace.clone();
    trace.decision_attempt_errors.push(error.to_string());
    ProviderError::DecisionFailed {
        message: error.to_string(),
        trace: Box::new(trace),
    }
}

fn provider_request(input: &BatchReviewInput) -> ProviderRequest {
    ProviderRequest {
        template_id: input.input.template_id.clone(),
        template_version: input.input.template_version.clone(),
        prompt_contract_version: input.input.prompt_contract_version.clone(),
        prompt_sha256: input.input.prompt_sha256.clone(),
        policy_mode: input.policy_mode,
        prompt: input.input.prompt.clone(),
        decision_policy: input.input.decision_policy,
        allowed_read_files: input.input.allowed_read_files.clone(),
        sanitized_context: input.input.sanitized_context.clone(),
    }
}

fn batch_prompt(
    settings: &PlanktonSettings,
    inputs: &[BatchReviewInput],
) -> Result<String, ProviderError> {
    let mut shared_sources = BTreeMap::<String, String>::new();
    let mut shared_guidance = BTreeMap::<String, String>::new();
    let default_system = settings.llm_advice_system_prompt.trim()
        == crate::template::DEFAULT_LLM_SYSTEM_PROMPT.trim();
    let mut requests = Vec::new();
    for input in inputs {
        let mut context = serde_json::to_value(&input.input.sanitized_context)
            .map_err(|error| ProviderError::RequestBuild(error.to_string()))?;
        // Preserve every captured byte but transmit identical source bodies once.
        // The original typed snapshot remains the authority for validation.
        if let Some(metadata) = context
            .get_mut("request_metadata")
            .and_then(serde_json::Value::as_object_mut)
        {
            if let Some(encoded) = metadata
                .get("execution_sources")
                .and_then(serde_json::Value::as_str)
            {
                let mut sources: Vec<serde_json::Value> =
                    serde_json::from_str(encoded).map_err(|error| {
                        ProviderError::RequestBuild(format!("invalid captured sources: {error}"))
                    })?;
                for source in &mut sources {
                    if let Some(object) = source.as_object_mut() {
                        if let Some(serde_json::Value::String(content)) = object.remove("content") {
                            let id = format!("sha256:{:x}", Sha256::digest(content.as_bytes()));
                            shared_sources.insert(id.clone(), content);
                            object.insert("content_ref".into(), serde_json::Value::String(id));
                        }
                    }
                }
                metadata.insert("execution_sources".into(), serde_json::json!(sources));
            }
            if let Some(serde_json::Value::String(content)) = metadata.remove("execution_source") {
                let id = format!("sha256:{:x}", Sha256::digest(content.as_bytes()));
                shared_sources.insert(id.clone(), content);
                metadata.insert("execution_source_ref".into(), serde_json::Value::String(id));
            }
        }
        // The built-in batch instructions contain the default template and execution
        // guidance once. Preserve custom templates verbatim after rendering; identical
        // guidance can share bytes, never request identity or authorization.
        let mut guidance = if settings.llm_advice_template.trim()
            == crate::template::DEFAULT_LLM_ADVICE_TEMPLATE.trim()
        {
            String::new()
        } else {
            render_llm_advice_template(
                &settings.llm_advice_template,
                &input.input.sanitized_context,
                input.policy_mode,
                &settings.locale,
            )?
        };
        if !default_system {
            append_context_guidance(
                &mut guidance,
                &input.input.sanitized_context,
                &settings.locale,
            );
        }
        let guidance_ref = (!guidance.is_empty()).then(|| {
            let id = format!("sha256:{:x}", Sha256::digest(guidance.as_bytes()));
            shared_guidance.insert(id.clone(), guidance);
            id
        });
        requests.push(serde_json::json!({
            "request_id": input.request_id,
            "enabled_outcomes": input.input.decision_policy.allowed_values(),
            "policy_mode": input.policy_mode,
            "guidance_ref": guidance_ref,
            "context": context,
        }));
    }
    let mut schema =
        serde_json::to_value(schemars::schema_for!(BatchResult)).expect("schema serializes");
    let outcomes = inputs
        .iter()
        .flat_map(|input| input.input.decision_policy.allowed_values())
        .collect::<BTreeSet<_>>();
    // The schema must describe the enabled outcomes too, rather than contradicting
    // the per-request guidance with the enum of every possible product outcome.
    schema["$defs"]["BatchDecision"]["properties"]["suggested_decision"]["enum"] =
        serde_json::json!(outcomes);
    schema["$defs"]["BatchEntry"]["properties"]["request_id"]["enum"] = serde_json::json!(inputs
        .iter()
        .map(|input| &input.request_id)
        .collect::<Vec<_>>());
    schema["$defs"]["BatchEntry"]["allOf"] = serde_json::json!(inputs.iter().map(|input| serde_json::json!({
        "if":{"properties":{"request_id":{"const":input.request_id}}},
        "then":{"properties":{"decision":{"properties":{"suggested_decision":{"enum":input.input.decision_policy.allowed_values()}}}}}
    })).collect::<Vec<_>>());
    Ok(format!(
        "{}\n{}\nBATCH REVIEW CONTRACT (replaces the single-request output and audit-turn format):\n\
         Review each supplied request independently, including every credential's own data flow and policy. \
         Shared source bytes are deduplicated, not shared authorization. Resolve every content_ref and \
         execution_source_ref through shared_sources, and each non-null guidance_ref through shared_guidance \
         before reviewing. A result for one request never covers another. \
         Return exactly one reviews entry for EVERY supplied request_id, no missing, repeated or extra IDs. \
         Each decision has exactly five exposure surfaces. Use only that request's enabled outcomes. \
         If deny is disabled, use escalate for a policy violation; the host independently enforces \
         the credential's breach_action. A policy saying deny does not enable a disabled model outcome. \
         Each decision contains only suggested_decision, rationale_summary, risk_score and exposure_report; \
         all results are explicitly keyed by request_id. \
         Include concise source evidence in the same result, referencing that request's captured source_id, \
         node_index and one-based inclusive line range. source_id is the original identity (for example \
         file:/absolute/path), not the sha256 content_ref used to store shared bytes. Do not recopy quotes or calculate offsets; the host \
         resolves those from the captured bytes. Identify every affected surface. Do not include node_assessments \
         or annotations inside decision; evidence belongs in its sibling evidence array. For argv-only evidence, \
         explain it in the surface summary. There is no mandatory second audit turn. Reasons use locale {}.\n\
         Request evidence:\n{}\n\nResult schema:\n{}",
        if default_system { "" } else { &settings.llm_advice_system_prompt }, include_str!("review_batch_prompt.md"), settings.locale,
        serde_json::json!({"requests":requests,"shared_sources":shared_sources,"shared_guidance":shared_guidance}),
        serde_json::to_string(&schema).expect("schema serializes")
    ))
}

fn decode_batch(
    content: &str,
    expected: &BTreeSet<String>,
) -> Result<BTreeMap<String, BatchEntry>, ProviderError> {
    let result: BatchResult =
        serde_json::from_str(normalize_json_payload(content)).map_err(|error| {
            ProviderError::InvalidResponse(format!("invalid batch result: {error}"))
        })?;
    let mut entries = BTreeMap::new();
    for entry in result.reviews {
        if !expected.contains(&entry.request_id) || entries.contains_key(&entry.request_id) {
            return Err(ProviderError::InvalidResponse(
                "batch contains an unexpected or duplicate request ID".into(),
            ));
        }
        entries.insert(entry.request_id.clone(), entry);
    }
    if entries.keys().cloned().collect::<BTreeSet<_>>() != *expected {
        return Err(ProviderError::InvalidResponse(
            "batch omitted one or more requested credential reviews".into(),
        ));
    }
    Ok(entries)
}

fn response_for_entry(
    request: &ProviderRequest,
    entry: &BatchEntry,
    result: &crate::AcpPromptResult,
    prompt: &str,
) -> Result<ProviderResponse, ProviderError> {
    let decision = &entry.decision;
    if !request.decision_policy.allows(decision.suggested_decision)
        || decision.rationale_summary.trim().is_empty()
        || decision.risk_score > 100
    {
        return Err(ProviderError::InvalidResponse(
            "batch entry violated its independent decision contract".into(),
        ));
    }
    let mut report = decision.exposure_report.clone().ok_or_else(|| {
        ProviderError::InvalidResponse("batch entry omitted exposure_report".into())
    })?;
    validate_staged_decision_report(&report)?;
    // Delivery to the declared consumer is a daemon-known exposure, even when
    // there is no further subprocess. Never let a model's zero erase that fact.
    // Higher exposure and uncertainty remain untouched.
    let metadata = &request.sanitized_context.request_metadata;
    let admitted = matches!(
        metadata.get("execution_admission").map(String::as_str),
        Some("daemon_command_v1" | "daemon_python_file_v1")
    );
    let delivered = matches!(
        metadata
            .get("execution_credential_transport")
            .map(String::as_str),
        Some("env" | "stdin")
    );
    let mut delivery_floor_applied = false;
    if admitted && delivered {
        for surface in &mut report.surfaces {
            if surface.surface == CredentialExposureSurface::ProcessPropagation
                && surface.actual_level == 0
            {
                surface.actual_level = 1;
                surface.evidence_state = crate::ExposureEvidenceState::Observed;
                surface.summary.push_str(" [Daemon evidence: the admitted plan delivers this credential to the declared consumer; process exposure is at least level 1.]");
                delivery_floor_applied = true;
            }
        }
    }
    for citation in &entry.evidence {
        add_citation(request, &mut report, citation)?;
    }
    let mut trace = result.trace.clone();
    if delivery_floor_applied {
        trace.audit_events.push(serde_json::json!({"type":"daemon_exposure_floor","surface":"process_propagation","actual_level":1,"evidence_state":"observed"}));
    }
    trace.rendered_prompt = Some(prompt.into());
    trace.output_format = Some("request_keyed_batch_json".into());
    let complete_nodes =
        report.node_assessments.len() == request.sanitized_context.call_chain.len();
    trace.review_progress = Some(LlmReviewProgress {
        state: if complete_nodes { LlmReviewDetailState::Complete } else { LlmReviewDetailState::Partial },
        completed_units: u16::try_from(report.node_assessments.len() + 6).unwrap_or(u16::MAX),
        total_units: review_detail_units(request),
        error: (!complete_nodes).then(|| "Decision recorded with the supplied evidence; some nodes have no detailed annotation".into()),
        updated_at: chrono::Utc::now(),
    });
    let response = ProviderResponse {
        suggested_decision: decision.suggested_decision,
        rationale_summary: decision.rationale_summary.clone(),
        risk_score: decision.risk_score,
        batch_decisions: Vec::new(),
        exposure_report: Some(report),
        json_repair_strategy: Some(JsonRepairStrategy::Strict),
        provider_response_id: None,
        x_request_id: result.trace.client_request_id.clone(),
        provider_trace: Some(trace),
        usage: None,
        model: result.provider_model.clone(),
    };
    validate_provider_response_evidence(request, &response)?;
    Ok(response)
}

fn add_citation(
    request: &ProviderRequest,
    report: &mut CredentialExposureReport,
    citation: &SourceCitation,
) -> Result<(), ProviderError> {
    require_call_chain_node(request, citation.node_index)?;
    let source_id = canonical_citation_source_id(request, citation)?;
    let source =
        captured_execution_source(request, citation.node_index, &source_id)?.ok_or_else(|| {
            ProviderError::InvalidResponse(
                "citation does not identify this request's captured source".into(),
            )
        })?;
    let lines = source.lines().collect::<Vec<_>>();
    if citation.start_line == 0
        || citation.end_line < citation.start_line
        || citation.end_line > lines.len()
        || citation.reason.trim().is_empty()
        || citation.surfaces.is_empty()
    {
        return Err(ProviderError::InvalidResponse(
            "invalid source citation range or explanation".into(),
        ));
    }
    let annotation = ExposureEvidenceAnnotation {
        reason: citation.reason.clone(),
        target: ExposureEvidenceTarget::SourceQuote {
            node_index: citation.node_index,
            source_id,
            start_line: citation.start_line,
            end_line: citation.end_line,
            quote: lines[citation.start_line - 1..citation.end_line].join("\n"),
            occurrence: 0,
        },
    };
    for surface in &citation.surfaces {
        let assessment = report
            .surfaces
            .iter_mut()
            .find(|entry| entry.surface == *surface)
            .ok_or_else(|| {
                ProviderError::InvalidResponse("citation names an unknown surface".into())
            })?;
        if !assessment.annotations.contains(&annotation) {
            assessment.annotations.push(annotation.clone());
        }
    }
    if let Some(node) = report
        .node_assessments
        .iter_mut()
        .find(|node| node.node_index == citation.node_index)
    {
        if !node.summary.contains(&citation.reason) {
            node.summary.push_str(&format!("; {}", citation.reason));
        }
    } else {
        report.node_assessments.push(CallChainNodeAssessment {
            node_index: citation.node_index,
            summary: citation.reason.clone(),
            capabilities: Vec::new(),
        });
    }
    Ok(())
}

fn canonical_citation_source_id(
    request: &ProviderRequest,
    citation: &SourceCitation,
) -> Result<String, ProviderError> {
    if !citation.source_id.starts_with("sha256:") {
        return Ok(citation.source_id.clone());
    }
    // Some models cite the shared byte key. Normalize only an unambiguous
    // match in THIS request and node, never a source elsewhere in the batch.
    let sources: Vec<CapturedExecutionSource> = request
        .sanitized_context
        .request_metadata
        .get("execution_sources")
        .map(|encoded| serde_json::from_str(encoded))
        .transpose()
        .map_err(|error| {
            ProviderError::InvalidResponse(format!("invalid captured sources: {error}"))
        })?
        .unwrap_or_default();
    let matches = sources
        .iter()
        .filter(|source| {
            source.node_index == citation.node_index
                && format!("sha256:{:x}", Sha256::digest(source.content.as_bytes()))
                    == citation.source_id
        })
        .collect::<Vec<_>>();
    if matches.len() != 1 {
        return Err(ProviderError::InvalidResponse(
            "citation content reference does not uniquely identify this request's captured source"
                .into(),
        ));
    }
    Ok(matches[0].source_id.clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input(id: &str, process_level: u8) -> BatchReviewInput {
        let settings = PlanktonSettings::default();
        let mut context = RequestContext::new(id.into(), format!("Use {id}"), "test".into());
        let mut policy = plankton_protocol::exposure::CredentialExposurePolicy::default();
        policy
            .surfaces
            .iter_mut()
            .find(|surface| surface.surface == CredentialExposureSurface::ProcessPropagation)
            .unwrap()
            .max_level = process_level;
        crate::store_exposure_policy(&mut context.resource_metadata, &policy).unwrap();
        let mut node = crate::CallChainNode::best_effort_path("/test/consumer.py");
        node.source = crate::CallChainNodeSource::ControlledExecution;
        context.call_chain = vec![node];
        context
            .metadata
            .insert("execution_admission".into(), "daemon_command_v1".into());
        context
            .metadata
            .insert("execution_credential_env".into(), format!("TOKEN_{id}"));
        context
            .metadata
            .insert("execution_credential_transport".into(), "env".into());
        context.metadata.insert("execution_sources".into(), serde_json::json!([{
            "node_index":0,"source_id":format!("source:{id}"),"content":"consume(token)\n","binding":"frozen"
        }]).to_string());
        let input = build_provider_input_snapshot(
            &settings,
            PolicyMode::LlmAutomatic,
            &context,
            &crate::build_prompt_context(&context),
        )
        .unwrap();
        BatchReviewInput {
            request_id: id.into(),
            policy_mode: PolicyMode::LlmAutomatic,
            input,
        }
    }

    fn complete_entry(id: &str) -> BatchEntry {
        let mut value = entry(id, "allow");
        value["decision"]["exposure_report"] = serde_json::json!({
            "chain_summary":"bound consumer", "surfaces":CredentialExposureSurface::ALL.iter().map(|surface| {
                let propagation = *surface == CredentialExposureSurface::ProcessPropagation;
                serde_json::json!({"surface":surface,"actual_level":if propagation {1} else {0},
                    "evidence_state":if propagation {"observed"} else {"not_observed"},"summary":"verified test behavior"})
            }).collect::<Vec<_>>()
        });
        value["evidence"] = serde_json::json!([{"node_index":0,"source_id":format!("source:{id}"),
            "start_line":1,"end_line":1,"surfaces":["process_propagation"],"reason":"Bound consumer reads this credential"}]);
        serde_json::from_value(value).unwrap()
    }

    fn result() -> crate::AcpPromptResult {
        crate::AcpPromptResult {
            content: String::new(),
            provider_model: Some("test".into()),
            trace: Default::default(),
            validated_exposure_report: None,
        }
    }

    #[test]
    fn shared_source_bytes_preserve_each_policy_binding_and_request() {
        let mut inputs = [input("a", 1), input("b", 0)];
        for input in &mut inputs {
            input.input.decision_policy.deny = false;
        }
        let prompt = batch_prompt(&PlanktonSettings::default(), &inputs).unwrap();
        let payload = prompt
            .split("Request evidence:\n")
            .nth(1)
            .unwrap()
            .split("\n\nResult schema:")
            .next()
            .unwrap();
        let payload: serde_json::Value = serde_json::from_str(payload).unwrap();
        assert_eq!(payload["shared_sources"].as_object().unwrap().len(), 1);
        assert!(prompt.contains("requires actual_level=0"));
        assert!(prompt.contains("process_propagation actual_level=1, evidence_state=observed"));
        let schema: serde_json::Value =
            serde_json::from_str(prompt.split("\n\nResult schema:\n").nth(1).unwrap()).unwrap();
        assert_eq!(
            schema["$defs"]["BatchDecision"]["properties"]["suggested_decision"]["enum"],
            serde_json::json!(["allow", "escalate"])
        );
        assert!(schema["$defs"]["BatchDecision"]["required"]
            .as_array()
            .unwrap()
            .contains(&serde_json::json!("exposure_report")));
        assert!(!serde_json::to_string(&schema)
            .unwrap()
            .contains("batch_decisions"));
        assert_eq!(
            schema["$defs"]["BatchEntry"]["allOf"]
                .as_array()
                .unwrap()
                .len(),
            2
        );
        for (index, original) in inputs.iter().enumerate() {
            let entry = &payload["requests"][index];
            assert_eq!(entry["request_id"], original.request_id);
            assert_eq!(
                entry["context"]["metadata"],
                serde_json::to_value(&original.input.sanitized_context.metadata).unwrap()
            );
            assert_eq!(
                entry["context"]["request_metadata"]["execution_credential_env"],
                format!("TOKEN_{}", original.request_id)
            );
            let source = &entry["context"]["request_metadata"]["execution_sources"][0];
            assert_eq!(
                payload["shared_sources"][source["content_ref"].as_str().unwrap()],
                "consume(token)\n"
            );
            assert!(entry["guidance_ref"].is_null());
        }
        assert!(prompt.contains("same-account mutation race"));
        assert!(!prompt.contains("Audit phase (the next turn"));
    }

    #[test]
    fn custom_instructions_are_preserved_and_only_identical_guidance_is_shared() {
        let inputs = [input("a", 1), input("b", 0)];
        let settings = PlanktonSettings {
            llm_advice_system_prompt: "Custom policy: inspect deployment boundaries.".into(),
            llm_advice_template: "Custom template: resource={{ context.resource }}".into(),
            ..Default::default()
        };
        let prompt = batch_prompt(&settings, &inputs).unwrap();
        assert!(prompt.starts_with(&settings.llm_advice_system_prompt));
        let payload: serde_json::Value = serde_json::from_str(
            prompt
                .split("Request evidence:\n")
                .nth(1)
                .unwrap()
                .split("\n\nResult schema:")
                .next()
                .unwrap(),
        )
        .unwrap();
        for (i, input) in inputs.iter().enumerate() {
            let reference = payload["requests"][i]["guidance_ref"].as_str().unwrap();
            let guidance = payload["shared_guidance"][reference].as_str().unwrap();
            assert!(guidance.contains(&format!(
                "resource={}",
                input.input.sanitized_context.resource
            )));
            assert!(guidance.contains("remaining same-account mutation race"));
        }
    }

    #[test]
    fn each_entry_retains_its_own_policy_and_unknown_evidence_guard() {
        for (id, maximum, expected) in [
            ("a", 1, crate::AutomaticDisposition::Allow),
            ("b", 0, crate::AutomaticDisposition::Escalate),
        ] {
            let input = input(id, maximum);
            let request = provider_request(&input);
            let entry = complete_entry(id);
            let response = response_for_entry(&request, &entry, &result(), "batch prompt").unwrap();
            let mut suggestion =
                suggestion_from_provider_response(&input.input, ACP_PROVIDER_KIND, response);
            let outcome = crate::evaluate_automatic_disposition(
                Some(ACP_PROVIDER_KIND),
                Some(&input.input),
                Some(&suggestion),
                &input.input.sanitized_context,
            );
            assert_eq!(outcome.auto_disposition, expected);
            let surface = &mut suggestion.exposure_report.as_mut().unwrap().surfaces[0];
            surface.actual_level = 2;
            surface.evidence_state = crate::ExposureEvidenceState::Unknown;
            let outcome = crate::evaluate_automatic_disposition(
                Some(ACP_PROVIDER_KIND),
                Some(&input.input),
                Some(&suggestion),
                &input.input.sanitized_context,
            );
            assert_ne!(outcome.auto_disposition, crate::AutomaticDisposition::Allow);
        }
    }

    #[test]
    fn admitted_delivery_floor_does_not_erase_higher_or_unknown_exposure() {
        let input = input("a", 0);
        for (level, state) in [
            (0, crate::ExposureEvidenceState::NotObserved),
            (2, crate::ExposureEvidenceState::Observed),
            (2, crate::ExposureEvidenceState::Unknown),
        ] {
            let mut entry = complete_entry("a");
            let surface = entry
                .decision
                .exposure_report
                .as_mut()
                .unwrap()
                .surfaces
                .iter_mut()
                .find(|s| s.surface == CredentialExposureSurface::ProcessPropagation)
                .unwrap();
            surface.actual_level = level;
            surface.evidence_state = state;
            let response =
                response_for_entry(&provider_request(&input), &entry, &result(), "prompt").unwrap();
            let exposure = response
                .exposure_report
                .as_ref()
                .unwrap()
                .surfaces
                .iter()
                .find(|s| s.surface == CredentialExposureSurface::ProcessPropagation)
                .unwrap();
            assert_eq!(exposure.actual_level, level.max(1));
            assert_eq!(
                exposure.evidence_state,
                if level == 0 {
                    crate::ExposureEvidenceState::Observed
                } else {
                    state
                }
            );
            let suggestion =
                suggestion_from_provider_response(&input.input, ACP_PROVIDER_KIND, response);
            assert_ne!(
                crate::evaluate_automatic_disposition(
                    Some(ACP_PROVIDER_KIND),
                    Some(&input.input),
                    Some(&suggestion),
                    &input.input.sanitized_context
                )
                .auto_disposition,
                crate::AutomaticDisposition::Allow
            );
        }
    }

    #[test]
    fn foreign_evidence_invalid_ranges_and_disabled_outcomes_are_rejected() {
        let input = input("a", 1);
        let mut request = provider_request(&input);
        let mut entry = complete_entry("a");
        entry.evidence[0].source_id = "source:b".into();
        assert!(response_for_entry(&request, &entry, &result(), "prompt").is_err());
        entry.evidence[0].source_id = "source:a".into();
        entry.evidence[0].end_line = 2;
        assert!(response_for_entry(&request, &entry, &result(), "prompt").is_err());
        entry.evidence[0].end_line = 1;
        request.decision_policy.allow = false;
        assert!(response_for_entry(&request, &entry, &result(), "prompt").is_err());
    }

    #[test]
    fn content_reference_citations_resolve_only_within_the_request_and_node() {
        let mut request = provider_request(&input("a", 1));
        let mut entry = complete_entry("a");
        entry.evidence[0].source_id = format!("sha256:{:x}", Sha256::digest(b"consume(token)\n"));
        let response = response_for_entry(&request, &entry, &result(), "prompt").unwrap();
        let report = response.exposure_report.unwrap();
        assert!(serde_json::to_string(&report).unwrap().contains("source:a"));
        entry.evidence[0].source_id = format!("sha256:{:x}", Sha256::digest(b"foreign source"));
        assert!(response_for_entry(&request, &entry, &result(), "prompt").is_err());
        entry.evidence[0].source_id = format!("sha256:{:x}", Sha256::digest(b"consume(token)\n"));
        request.sanitized_context.request_metadata.insert(
            "execution_sources".into(),
            serde_json::json!([
                {"node_index":0,"source_id":"source:a","content":"consume(token)\n"},
                {"node_index":0,"source_id":"source:ambiguous","content":"consume(token)\n"}
            ])
            .to_string(),
        );
        assert!(response_for_entry(&request, &entry, &result(), "prompt").is_err());
        request.sanitized_context.request_metadata.insert(
            "execution_sources".into(),
            serde_json::json!([
                {"node_index":1,"source_id":"source:other-node","content":"consume(token)\n"}
            ])
            .to_string(),
        );
        assert!(response_for_entry(&request, &entry, &result(), "prompt").is_err());
    }

    fn entry(id: &str, decision: &str) -> serde_json::Value {
        serde_json::json!({"request_id":id,"decision":{"suggested_decision":decision,"rationale_summary":"independent decision","risk_score":10,"exposure_report":null}})
    }

    #[test]
    fn empty_legacy_batch_field_is_lossless_but_meaningful_or_ambiguous_fields_fail() {
        let mut value = entry("a", "allow");
        value["decision"]["exposure_report"] =
            serde_json::to_value(complete_entry("a").decision.exposure_report.unwrap()).unwrap();
        let expected = BTreeSet::from(["a".to_string()]);
        let request = provider_request(&input("a", 1));
        let decode = |entry| {
            decode_batch(
                &serde_json::json!({"reviews":[entry]}).to_string(),
                &expected,
            )
        };
        let reference = response_for_entry(
            &request,
            &decode(value.clone()).unwrap()["a"],
            &result(),
            "prompt",
        )
        .unwrap();
        for path in ["/decision", "/decision/exposure_report"] {
            let mut compatible = value.clone();
            compatible.pointer_mut(path).unwrap()["batch_decisions"] = serde_json::json!([]);
            let response = response_for_entry(
                &request,
                &decode(compatible.clone()).unwrap()["a"],
                &result(),
                "prompt",
            )
            .unwrap();
            assert_eq!(response.exposure_report, reference.exposure_report);
            assert_eq!(response.suggested_decision, reference.suggested_decision);
            for invalid in [
                serde_json::Value::Null,
                serde_json::json!({}),
                serde_json::json!("[]"),
                serde_json::json!([{"suggested_decision":"deny"}]),
            ] {
                compatible.pointer_mut(path).unwrap()["batch_decisions"] = invalid;
                assert!(decode(compatible.clone()).is_err());
            }
        }
        let mut unknown = value.clone();
        unknown["decision"]["exposure_report"]["unexpected"] = serde_json::json!([]);
        assert!(decode(unknown).is_err());
        let duplicate = serde_json::json!({"reviews":[value]}).to_string().replacen(
            "\"chain_summary\":",
            "\"batch_decisions\":[],\"batch_decisions\":[],\"chain_summary\":",
            1,
        );
        assert!(decode_batch(&duplicate, &expected).is_err());
    }

    #[test]
    #[ignore = "requires private captured inputs.json and raw.json via PLANKTON_REVIEW_FIXTURE_DIR"]
    fn captured_batch_result_passes_without_another_model_call() {
        let root = std::path::PathBuf::from(std::env::var("PLANKTON_REVIEW_FIXTURE_DIR").unwrap());
        let batches: Vec<Vec<BatchReviewInput>> =
            serde_json::from_slice(&std::fs::read(root.join("inputs.json")).unwrap()).unwrap();
        assert_eq!(batches.len(), 1);
        let inputs = &batches[0];
        let expected = inputs
            .iter()
            .map(|input| input.request_id.clone())
            .collect();
        let raw = std::fs::read_to_string(root.join("raw.json")).unwrap();
        let entries = decode_batch(&raw, &expected).unwrap();
        for input in inputs {
            let response = response_for_entry(
                &provider_request(input),
                &entries[&input.request_id],
                &result(),
                "captured result",
            )
            .unwrap();
            assert_eq!(response.exposure_report.unwrap().surfaces.len(), 5);
        }
    }

    #[test]
    fn exact_request_coverage_rejects_missing_duplicate_and_unknown_ids() {
        let expected = BTreeSet::from(["a".into(), "b".into()]);
        for reviews in [
            vec![entry("a", "allow")],
            vec![entry("a", "allow"), entry("a", "deny")],
            vec![entry("a", "allow"), entry("c", "allow")],
        ] {
            assert!(decode_batch(
                &serde_json::json!({"reviews":reviews}).to_string(),
                &expected
            )
            .is_err());
        }
        let reviews = decode_batch(
            &serde_json::json!({"reviews":[entry("b","deny"),entry("a","allow")]}).to_string(),
            &expected,
        )
        .unwrap();
        assert_eq!(
            reviews["a"].decision.suggested_decision,
            SuggestedDecision::Allow
        );
        assert_eq!(
            reviews["b"].decision.suggested_decision,
            SuggestedDecision::Deny
        );
    }
}
