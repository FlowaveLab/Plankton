use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::{CallChainNode, RequestContext, SuggestedDecision};

pub const APPROVAL_BATCH_TICKET_TTL_SECONDS: i64 = 300;
pub const MAX_BATCH_RESOURCE_DECISIONS: usize = 32;

const FIELD_SPECIFIC_METADATA_KEYS: [&str; 3] = ["field_key", "field_label", "record_id"];

#[derive(schemars::JsonSchema, Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct BatchResourceDecision {
    pub resource_selector: String,
    pub suggested_decision: SuggestedDecision,
    pub rationale_summary: String,
    pub risk_score: u8,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum JsonRepairStrategy {
    Strict,
    Conservative,
    SurfaceMap,
}

#[derive(Serialize)]
struct SemanticCallChainNode<'a> {
    process_name: &'a Option<String>,
    executable_path: &'a Option<String>,
    argv: &'a [String],
    resolved_file_path: &'a Option<String>,
    source: &'a crate::CallChainNodeSource,
}

pub fn semantic_call_chain_sha256(call_chain: &[CallChainNode]) -> String {
    let semantic_nodes = call_chain
        .iter()
        .map(|node| SemanticCallChainNode {
            process_name: &node.process_name,
            executable_path: &node.executable_path,
            argv: &node.argv,
            resolved_file_path: &node.resolved_file_path,
            source: &node.source,
        })
        .collect::<Vec<_>>();
    let serialized = serde_json::to_vec(&semantic_nodes)
        .expect("semantic call-chain nodes must always serialize");
    format!("{:x}", Sha256::digest(serialized))
}

pub fn shared_resource_metadata_sha256(metadata: &BTreeMap<String, String>) -> Option<String> {
    let item_id = metadata
        .get("item_id")
        .map(String::as_str)
        .unwrap_or_default();
    if item_id.trim().is_empty() {
        return None;
    }

    let shared_metadata = metadata
        .iter()
        .filter(|(key, _)| !FIELD_SPECIFIC_METADATA_KEYS.contains(&key.as_str()))
        .collect::<BTreeMap<_, _>>();
    let serialized = serde_json::to_vec(&shared_metadata)
        .expect("shared resource metadata must always serialize");
    Some(format!("{:x}", Sha256::digest(serialized)))
}

pub fn context_matches_resource_selector(context: &RequestContext, selector: &str) -> bool {
    let selector = selector.trim();
    if selector.is_empty() {
        return false;
    }

    context.resource == selector
        || ["field_key", "field_label"]
            .into_iter()
            .filter_map(|key| context.resource_metadata.get(key))
            .any(|candidate| candidate == selector)
}

pub fn validate_batch_resource_decisions(
    decisions: Vec<BatchResourceDecision>,
) -> Result<Vec<BatchResourceDecision>, String> {
    if decisions.len() > MAX_BATCH_RESOURCE_DECISIONS {
        return Err(format!(
            "batch_decisions exceeded the {MAX_BATCH_RESOURCE_DECISIONS}-item limit"
        ));
    }

    let mut normalized = Vec::with_capacity(decisions.len());
    let mut seen = BTreeMap::<String, SuggestedDecision>::new();
    for mut decision in decisions {
        decision.resource_selector = decision.resource_selector.trim().to_string();
        decision.rationale_summary = decision.rationale_summary.trim().to_string();
        if decision.resource_selector.is_empty() {
            return Err("batch_decisions contained an empty resource_selector".to_string());
        }
        if decision.rationale_summary.is_empty() {
            return Err(format!(
                "batch_decisions entry {} had an empty rationale_summary",
                decision.resource_selector
            ));
        }
        if decision.risk_score > 100 {
            return Err(format!(
                "batch_decisions entry {} had risk_score above 100",
                decision.resource_selector
            ));
        }
        if let Some(existing) = seen.insert(
            decision.resource_selector.clone(),
            decision.suggested_decision,
        ) {
            if existing != decision.suggested_decision {
                return Err(format!(
                    "batch_decisions contained conflicting decisions for {}",
                    decision.resource_selector
                ));
            }
            continue;
        }
        normalized.push(decision);
    }

    Ok(normalized)
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::{
        context_matches_resource_selector, semantic_call_chain_sha256,
        shared_resource_metadata_sha256, validate_batch_resource_decisions, BatchResourceDecision,
    };
    use crate::{
        CallChainNode, CallChainNodeSource, CallChainPreviewStatus, RequestContext,
        SuggestedDecision,
    };

    fn node(pid: u32) -> CallChainNode {
        CallChainNode {
            pid: Some(pid),
            ppid: Some(pid.saturating_sub(1)),
            process_name: Some("bash".into()),
            executable_path: Some("/bin/bash".into()),
            argv: vec!["bash".into(), "-lc".into(), "plankton get A".into()],
            resolved_file_path: None,
            source: CallChainNodeSource::OsProbe,
            previewable: false,
            preview_status: CallChainPreviewStatus::NotPreviewable,
            preview_text: None,
            preview_error: None,
        }
    }

    #[test]
    fn semantic_hash_ignores_process_and_preview_volatility() {
        let mut left = node(10);
        let mut right = node(20);
        right.preview_status = CallChainPreviewStatus::IoError;
        right.preview_error = Some("temporary".into());

        assert_eq!(
            semantic_call_chain_sha256(&[left.clone()]),
            semantic_call_chain_sha256(&[right])
        );

        left.argv.push("changed".into());
        assert_ne!(
            semantic_call_chain_sha256(&[left]),
            semantic_call_chain_sha256(&[node(10)])
        );
    }

    #[test]
    fn shared_metadata_ignores_only_field_identity() {
        let mut left = BTreeMap::from([
            ("item_id".into(), "item-1".into()),
            ("field_key".into(), "PUBLIC_KEY".into()),
            ("field_label".into(), "Public".into()),
            ("record_id".into(), "record-a".into()),
            ("resource_note".into(), "dev only".into()),
        ]);
        let mut right = left.clone();
        right.insert("field_key".into(), "SECRET_KEY".into());
        right.insert("field_label".into(), "Secret".into());
        right.insert("record_id".into(), "record-b".into());
        assert_eq!(
            shared_resource_metadata_sha256(&left),
            shared_resource_metadata_sha256(&right)
        );

        left.insert("resource_note".into(), "production".into());
        assert_ne!(
            shared_resource_metadata_sha256(&left),
            shared_resource_metadata_sha256(&right)
        );
    }

    #[test]
    fn selector_matches_canonical_resource_or_field_identity() {
        let mut context =
            RequestContext::new("plankton://field/item/key".into(), "r".into(), "u".into());
        context
            .resource_metadata
            .insert("field_key".into(), "SECRET_KEY".into());
        assert!(context_matches_resource_selector(&context, "SECRET_KEY"));
        assert!(context_matches_resource_selector(
            &context,
            "plankton://field/item/key"
        ));
        assert!(!context_matches_resource_selector(&context, "OTHER"));
    }

    #[test]
    fn duplicate_conflicting_batch_decisions_are_rejected() {
        let result = validate_batch_resource_decisions(vec![
            BatchResourceDecision {
                resource_selector: "KEY".into(),
                suggested_decision: SuggestedDecision::Allow,
                rationale_summary: "allowed".into(),
                risk_score: 10,
            },
            BatchResourceDecision {
                resource_selector: "KEY".into(),
                suggested_decision: SuggestedDecision::Deny,
                rationale_summary: "denied".into(),
                risk_score: 90,
            },
        ]);
        assert!(result.is_err());
    }
}
