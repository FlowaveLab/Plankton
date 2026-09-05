use std::collections::BTreeMap;

use chrono::Utc;
use plankton_protocol::{
    acp::{AcpProfile, AgentKind, VersionMode},
    daemon::{RequestEnvelope, ResponseEnvelope},
    error::{ErrorCode, ErrorSeverity, ErrorSource, PlanktonError},
    resources::{
        MatchField, ResourceAccessCallChainNode, ResourceAccessRequest, ResourceSearchError,
        ResourceSearchItem, ResourceSearchRequest, ResourceSearchResponse, ResourceSearchWarning,
        ResourceWarningCode,
    },
    PROTOCOL_VERSION,
};
use serde_json::{json, Value};
use uuid::Uuid;

#[test]
fn request_envelope_rejects_unknown_fields() {
    let value = json!({
        "protocol_version": PROTOCOL_VERSION,
        "correlation_id": Uuid::new_v4(),
        "payload": {"query": "github"},
        "unexpected": true
    });

    let error = serde_json::from_value::<RequestEnvelope<Value>>(value)
        .expect_err("unknown request fields must fail closed");

    assert!(error.to_string().contains("unknown field"));
}

#[test]
fn request_envelope_rejects_incompatible_protocol_versions() {
    let request = RequestEnvelope {
        protocol_version: PROTOCOL_VERSION + 1,
        correlation_id: Uuid::new_v4(),
        payload: json!({}),
    };

    let error = request.validate_version().expect_err("version must fail");
    assert_eq!(error.expected, PROTOCOL_VERSION);
    assert_eq!(error.received, PROTOCOL_VERSION + 1);
}

#[test]
fn version_three_is_rejected_after_structured_call_chain_schema_change() {
    let request = RequestEnvelope {
        protocol_version: 3,
        correlation_id: Uuid::new_v4(),
        payload: json!({}),
    };

    let error = request
        .validate_version()
        .expect_err("v3 predates structured access call-chain fields");

    assert_eq!(error.expected, plankton_protocol::PROTOCOL_VERSION);
    assert_eq!(error.received, 3);
}

#[test]
fn resource_access_serializes_structured_runtime_context_without_preview_content() {
    let request = ResourceAccessRequest {
        resource_id: "plankton://field/example".into(),
        reason: "read one trace".into(),
        requested_by: "integration-test".into(),
        script_path: Some("/workspace/analyze.sh".into()),
        call_chain_details: vec![ResourceAccessCallChainNode {
            pid: Some(42),
            ppid: Some(1),
            process_name: Some("zsh".into()),
            executable_path: Some("/bin/zsh".into()),
            argv: vec!["/workspace/analyze.sh".into(), "--trace-id".into()],
            resolved_file_path: Some("/workspace/analyze.sh".into()),
        }],
        call_chain: vec!["/workspace/analyze.sh".into()],
        metadata: BTreeMap::from([("operation".into(), "trace.read".into())]),
    };

    let serialized = serde_json::to_value(&request).expect("resource access serializes");
    assert_eq!(serialized["script_path"], "/workspace/analyze.sh");
    assert_eq!(serialized["call_chain_details"][0]["argv"][1], "--trace-id");
    assert!(serialized["call_chain_details"][0]
        .as_object()
        .is_some_and(|node| !node.contains_key("preview_text")));
    assert_eq!(
        serde_json::from_value::<ResourceAccessRequest>(serialized)
            .expect("resource access round-trips"),
        request
    );
}

#[test]
fn resource_access_defaults_new_runtime_context_for_legacy_payloads() {
    let legacy = json!({
        "resource_id": "plankton://field/example",
        "reason": "legacy request",
        "requested_by": "integration-test",
        "call_chain": ["/bin/zsh"],
        "metadata": {"script_path": "/workspace/legacy.sh"}
    });

    let request = serde_json::from_value::<ResourceAccessRequest>(legacy)
        .expect("legacy request remains readable by the v4 daemon");
    assert_eq!(request.script_path, None);
    assert!(request.call_chain_details.is_empty());
    assert_eq!(request.call_chain, ["/bin/zsh"]);
}

#[test]
fn response_envelopes_preserve_request_correlation() {
    let request = RequestEnvelope::new(ResourceSearchRequest {
        query: "production".into(),
        tag_all: vec![],
        tag_any: vec![],
        field_key: None,
        notes: None,
        limit: 50,
        cursor: None,
    });
    let response = ResponseEnvelope::Success {
        protocol_version: PROTOCOL_VERSION,
        correlation_id: request.correlation_id,
        data: ResourceSearchResponse {
            items: vec![],
            next_cursor: None,
            index_generation: 7,
            warnings: vec![ResourceSearchWarning {
                code: ResourceWarningCode::IndexRebuilding,
                message: "Search index is rebuilding".into(),
            }],
        },
    };

    let encoded = serde_json::to_value(response).expect("serialize response");
    assert_eq!(
        encoded["correlation_id"],
        request.correlation_id.to_string()
    );
    assert_eq!(encoded["data"]["index_generation"], 7);
}

#[test]
fn resource_search_rejects_invalid_limits_and_unknown_fields() {
    for invalid_limit in [0, 201, u16::MAX] {
        let request = ResourceSearchRequest {
            query: String::new(),
            tag_all: vec![],
            tag_any: vec![],
            field_key: None,
            notes: None,
            limit: invalid_limit,
            cursor: None,
        };
        assert_eq!(
            request.validate(),
            Err(ResourceSearchError::InvalidLimit(invalid_limit))
        );
    }

    let unknown = json!({
        "query": "api",
        "limit": 10,
        "provider": "1password"
    });
    assert!(serde_json::from_value::<ResourceSearchRequest>(unknown).is_err());
}

#[test]
fn ai_error_projection_cannot_serialize_backend_details() {
    let correlation_id = Uuid::new_v4();
    let error = PlanktonError {
        code: ErrorCode::BackendUnavailable,
        user_message: "部分资源暂时不可用".to_string(),
        internal_message: Some("op item list exited 1: not signed in".to_string()),
        public_context: BTreeMap::from([("operation".to_string(), "resources.search".to_string())]),
        internal_context: BTreeMap::from([("backend".to_string(), "1password".to_string())]),
        severity: ErrorSeverity::Error,
        retryable: true,
        timestamp: Utc::now(),
        correlation_id,
        source: ErrorSource::Backend {
            backend_id: "1password".to_string(),
        },
    };

    let serialized = serde_json::to_string(&error.ai_safe()).expect("AI error should serialize");

    assert!(!serialized.contains("1password"));
    assert!(!serialized.contains("op item"));
    assert!(!serialized.contains("internal_message"));
    assert_eq!(error.ai_safe().correlation_id, correlation_id);
}

#[test]
fn resource_search_item_has_no_backend_identity() {
    let item = ResourceSearchItem {
        resource_id: "plankton://field/018f".to_string(),
        display_name: "GitHub".to_string(),
        aliases: vec!["secret/github/token".to_string()],
        description: Some("production automation".to_string()),
        tags: vec!["prod".to_string(), "scm".to_string()],
        field_key: "token".to_string(),
        field_label: "API Token".to_string(),
        matched_on: vec![MatchField::FieldKey, MatchField::Tag],
        highlights: vec!["token".to_string()],
        score: 912,
    };

    let serialized = serde_json::to_value(item).expect("search item should serialize");
    let object = serialized
        .as_object()
        .expect("search item should be an object");

    assert!(!object.contains_key("backend"));
    assert!(!object.contains_key("provider_kind"));
    assert!(!object.contains_key("vault"));
    assert_eq!(object["field_key"], "token");
}

#[test]
fn acp_profiles_validate_latest_pinned_and_custom_modes() {
    let latest = AcpProfile {
        session_options: Default::default(),
        agent_kind: AgentKind::Codex,
        version_mode: VersionMode::Latest,
        version: None,
        program: None,
        args: Vec::new(),
    };
    latest.validate().expect("latest preset should be valid");

    let pinned = AcpProfile {
        session_options: Default::default(),
        agent_kind: AgentKind::ClaudeCode,
        version_mode: VersionMode::Pinned,
        version: Some("0.16.2".to_string()),
        program: None,
        args: Vec::new(),
    };
    pinned
        .validate()
        .expect("exact pinned preset should be valid");

    let missing_version = AcpProfile {
        version: None,
        ..pinned
    };
    assert!(missing_version.validate().is_err());

    let invalid_semver = AcpProfile {
        session_options: Default::default(),
        agent_kind: AgentKind::Codex,
        version_mode: VersionMode::Pinned,
        version: Some("newest".to_string()),
        program: None,
        args: Vec::new(),
    };
    assert!(invalid_semver.validate().is_err());

    let custom = AcpProfile {
        session_options: Default::default(),
        agent_kind: AgentKind::Custom,
        version_mode: VersionMode::Custom,
        version: None,
        program: Some("my-acp".to_string()),
        args: vec!["serve".to_string()],
    };
    custom
        .validate()
        .expect("custom executable should be valid");
}
