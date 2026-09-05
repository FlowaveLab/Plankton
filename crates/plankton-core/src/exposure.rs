use std::collections::BTreeMap;

use plankton_protocol::exposure::CredentialExposureSurface;
use plankton_protocol::exposure::{CredentialAccessMode, CredentialExposurePolicy};
use serde::{Deserialize, Serialize};

pub const EXPOSURE_POLICY_METADATA_KEY: &str = "credential_exposure_policy_v1";

pub const ITEM_EXPOSURE_POLICY_METADATA_KEY: &str = "credential_item_exposure_policy_v1";
pub const EXPOSURE_INHERITANCE_METADATA_KEY: &str = "credential_exposure_source_v1";

pub fn item_exposure_policy_from_metadata(
    metadata: &BTreeMap<String, String>,
) -> CredentialExposurePolicy {
    metadata
        .get(ITEM_EXPOSURE_POLICY_METADATA_KEY)
        .and_then(|encoded| serde_json::from_str::<CredentialExposurePolicy>(encoded).ok())
        .filter(|policy| policy.validate().is_ok())
        .unwrap_or_default()
}

pub fn inherits_exposure_policy(metadata: &BTreeMap<String, String>) -> bool {
    metadata
        .get(EXPOSURE_INHERITANCE_METADATA_KEY)
        .is_some_and(|mode| mode == "inherit")
        || !metadata.contains_key(EXPOSURE_POLICY_METADATA_KEY)
}

/// Materialize the effective field policy for existing access and approval consumers.
/// Collection edits update every member in the same catalog transaction.
pub fn store_item_exposure_policy(
    metadata: &mut BTreeMap<String, String>,
    default_policy: &CredentialExposurePolicy,
    custom_policy: Option<&CredentialExposurePolicy>,
) -> Result<(), plankton_protocol::exposure::ExposurePolicyError> {
    default_policy.validate()?;
    store_exposure_policy(metadata, custom_policy.unwrap_or(default_policy))?;
    metadata.insert(
        ITEM_EXPOSURE_POLICY_METADATA_KEY.into(),
        serde_json::to_string(default_policy).expect("policy serializes"),
    );
    metadata.insert(
        EXPOSURE_INHERITANCE_METADATA_KEY.into(),
        if custom_policy.is_some() {
            "custom"
        } else {
            "inherit"
        }
        .into(),
    );
    Ok(())
}

pub fn exposure_policy_from_metadata(
    metadata: &BTreeMap<String, String>,
) -> CredentialExposurePolicy {
    metadata
        .get(EXPOSURE_POLICY_METADATA_KEY)
        .and_then(|encoded| serde_json::from_str::<CredentialExposurePolicy>(encoded).ok())
        .filter(|policy| policy.validate().is_ok())
        .unwrap_or_default()
}

pub fn store_exposure_policy(
    metadata: &mut BTreeMap<String, String>,
    policy: &CredentialExposurePolicy,
) -> Result<(), plankton_protocol::exposure::ExposurePolicyError> {
    policy.validate()?;
    metadata.insert(
        EXPOSURE_POLICY_METADATA_KEY.to_string(),
        serde_json::to_string(policy).expect("validated exposure policy serializes"),
    );
    Ok(())
}

pub fn is_direct_access(metadata: &BTreeMap<String, String>) -> bool {
    exposure_policy_from_metadata(metadata).access_mode == CredentialAccessMode::Direct
}

#[derive(schemars::JsonSchema, Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExposureEvidenceState {
    NotObserved,
    Observed,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ExposureEvidenceTarget {
    SourceFile {
        node_index: usize,
        source_id: String,
    },
    Resource {
        resource_selector: String,
    },
    Node {
        node_index: usize,
    },
    ArgumentQuote {
        node_index: usize,
        argument_index: usize,
        quote: String,
        #[serde(default)]
        occurrence: usize,
    },
    ArgumentSpan {
        node_index: usize,
        start: ExposureArgumentAnchor,
        end: ExposureArgumentAnchor,
    },
    SourceQuote {
        node_index: usize,
        source_id: String,
        start_line: usize,
        end_line: usize,
        quote: String,
        #[serde(default)]
        occurrence: usize,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExposureArgumentAnchor {
    pub argument_index: usize,
    pub quote: String,
    #[serde(default)]
    pub occurrence: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExposureEvidenceAnnotation {
    pub reason: String,
    pub target: ExposureEvidenceTarget,
}

#[derive(schemars::JsonSchema, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExposureSurfaceAssessment {
    pub surface: CredentialExposureSurface,
    #[schemars(range(min = 0, max = 2))]
    pub actual_level: u8,
    pub evidence_state: ExposureEvidenceState,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub network_destinations: Vec<String>,
    pub summary: String,
    #[serde(default)]
    #[schemars(skip)]
    pub annotations: Vec<ExposureEvidenceAnnotation>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CallChainNodeAssessment {
    pub node_index: usize,
    pub summary: String,
    #[serde(default)]
    pub capabilities: Vec<String>,
}

#[derive(schemars::JsonSchema, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CredentialExposureReport {
    pub chain_summary: String,
    #[serde(default)]
    #[schemars(skip)]
    pub node_assessments: Vec<CallChainNodeAssessment>,
    #[schemars(length(min = 5, max = 5))]
    pub surfaces: Vec<ExposureSurfaceAssessment>,
}

impl CredentialExposureReport {
    pub fn validate(&self) -> Result<(), String> {
        if self.chain_summary.trim().is_empty() {
            return Err("exposure report chain_summary cannot be empty".into());
        }
        for surface in CredentialExposureSurface::ALL {
            let matches = self
                .surfaces
                .iter()
                .filter(|assessment| assessment.surface == surface)
                .collect::<Vec<_>>();
            if matches.len() != 1 {
                return Err(format!(
                    "exposure report must contain one {surface:?} assessment"
                ));
            }
            if matches[0].actual_level > 2 {
                return Err(format!("actual exposure level exceeds 2 for {surface:?}"));
            }
            let assessment = matches[0];
            if assessment.evidence_state == ExposureEvidenceState::Unknown
                && assessment.actual_level != 2
            {
                return Err(format!("unknown evidence requires level 2 for {surface:?}"));
            }
            if assessment.evidence_state == ExposureEvidenceState::NotObserved
                && assessment.actual_level != 0
            {
                return Err(format!(
                    "not_observed evidence requires level 0 for {surface:?}"
                ));
            }
            if surface != CredentialExposureSurface::Network
                && !assessment.network_destinations.is_empty()
            {
                return Err("network_destinations belongs only to the network surface".into());
            }
            if surface == CredentialExposureSurface::Network
                && assessment.evidence_state == ExposureEvidenceState::Observed
                && assessment.actual_level > 0
                && assessment.network_destinations.is_empty()
            {
                return Err("observed network exposure requires destination evidence".into());
            }
            if matches[0].summary.trim().is_empty() {
                return Err(format!("exposure report summary is empty for {surface:?}"));
            }
        }
        Ok(())
    }

    pub fn exceeded_surfaces(
        &self,
        policy: &CredentialExposurePolicy,
    ) -> Vec<CredentialExposureSurface> {
        self.surfaces
            .iter()
            .filter(|assessment| {
                let configured = policy.surface(assessment.surface);
                assessment.actual_level > configured.max_level
                    || (assessment.surface == CredentialExposureSurface::Network
                        && assessment.actual_level > 0
                        && configured.max_level == 1
                        && (assessment.network_destinations.is_empty()
                            || assessment.network_destinations.iter().any(|destination| {
                                !network_destination_allowed(
                                    destination,
                                    &configured.network_allowlist,
                                )
                            })))
            })
            .map(|assessment| assessment.surface)
            .collect()
    }
}

fn network_destination_allowed(
    destination: &str,
    rules: &[plankton_protocol::exposure::NetworkDestinationRule],
) -> bool {
    use plankton_protocol::exposure::NetworkDestinationRule;
    let url = if destination.contains("://") {
        destination.to_string()
    } else {
        format!("https://{destination}")
    };
    let Ok(url) = reqwest::Url::parse(&url) else {
        return false;
    };
    let Some(host) = url.host_str() else {
        return false;
    };
    let host = host.trim_end_matches('.').to_ascii_lowercase();
    rules.iter().any(|rule| match rule {
        NetworkDestinationRule::ExactDomain { domain } => {
            host == domain.trim_end_matches('.').to_ascii_lowercase()
        }
        NetworkDestinationRule::SubdomainsOf {
            domain,
            include_apex,
        } => {
            let domain = domain.trim_end_matches('.').to_ascii_lowercase();
            (*include_apex && host == domain) || host.ends_with(&format!(".{domain}"))
        }
        NetworkDestinationRule::Regex { pattern } => regex::Regex::new(&format!("^(?:{pattern})$"))
            .is_ok_and(|pattern| pattern.is_match(&host)),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn network_rules_match_parsed_hosts_not_url_text() {
        use plankton_protocol::exposure::NetworkDestinationRule::*;
        let exact = vec![ExactDomain {
            domain: "api.example.test".into(),
        }];
        for url in [
            "https://api.example.test/v1",
            "API.EXAMPLE.TEST",
            "https://api.example.test.:443/",
        ] {
            assert!(network_destination_allowed(url, &exact), "{url}");
        }
        for url in [
            "https://api.example.test.attacker.test/",
            "https://api.example.test@attacker.test/",
            "https://attacker.test/api.example.test",
            "not a hostname",
        ] {
            assert!(!network_destination_allowed(url, &exact), "{url}");
        }
        let subdomains = vec![SubdomainsOf {
            domain: "example.test".into(),
            include_apex: false,
        }];
        assert!(network_destination_allowed(
            "https://api.example.test",
            &subdomains
        ));
        assert!(!network_destination_allowed(
            "https://example.test",
            &subdomains
        ));
        let regex = vec![Regex {
            pattern: r"api\.example\.test".into(),
        }];
        assert!(network_destination_allowed(
            "https://api.example.test",
            &regex
        ));
        assert!(!network_destination_allowed(
            "https://api.example.test.attacker.test",
            &regex
        ));
    }

    #[test]
    fn controlled_network_requires_destinations_and_local_matching() {
        let mut report: CredentialExposureReport = serde_json::from_value(serde_json::json!({
            "chain_summary":"test", "surfaces": CredentialExposureSurface::ALL.iter().map(|surface|
                serde_json::json!({"surface":surface,"actual_level":0,"evidence_state":"not_observed","summary":"none"})).collect::<Vec<_>>()
        })).unwrap();
        let mut policy = CredentialExposurePolicy::default();
        let configured = policy
            .surfaces
            .iter_mut()
            .find(|p| p.surface == CredentialExposureSurface::Network)
            .unwrap();
        configured.max_level = 1;
        configured.network_allowlist = vec![
            plankton_protocol::exposure::NetworkDestinationRule::ExactDomain {
                domain: "api.example.test".into(),
            },
        ];
        let index = report
            .surfaces
            .iter()
            .position(|p| p.surface == CredentialExposureSurface::Network)
            .unwrap();
        report.surfaces[index].actual_level = 1;
        report.surfaces[index].evidence_state = ExposureEvidenceState::Observed;
        assert!(report.validate().is_err());
        report.surfaces[index].network_destinations = vec!["https://api.example.test/v1".into()];
        assert!(report.validate().is_ok());
        assert!(report.exceeded_surfaces(&policy).is_empty());
        report.surfaces[index]
            .network_destinations
            .push("https://other.test".into());
        assert_eq!(
            report.exceeded_surfaces(&policy),
            [CredentialExposureSurface::Network]
        );
    }

    #[test]
    fn missing_or_invalid_metadata_fails_closed_to_protected() {
        assert_eq!(
            exposure_policy_from_metadata(&BTreeMap::new()).access_mode,
            CredentialAccessMode::Protected
        );

        let span: ExposureEvidenceTarget = serde_json::from_str(
            r#"{"kind":"argument_span","node_index":2,"start":{"argument_index":1,"quote":"python3"},"end":{"argument_index":3,"quote":"requests.post","occurrence":1}}"#,
        )
        .expect("argument span");
        assert_eq!(
            span,
            ExposureEvidenceTarget::ArgumentSpan {
                node_index: 2,
                start: ExposureArgumentAnchor {
                    argument_index: 1,
                    quote: "python3".to_string(),
                    occurrence: 0,
                },
                end: ExposureArgumentAnchor {
                    argument_index: 3,
                    quote: "requests.post".to_string(),
                    occurrence: 1,
                },
            }
        );
        let invalid = BTreeMap::from([(EXPOSURE_POLICY_METADATA_KEY.into(), "{}".into())]);
        assert_eq!(
            exposure_policy_from_metadata(&invalid).access_mode,
            CredentialAccessMode::Protected
        );
    }

    #[test]
    fn direct_policy_round_trips_without_secret_material() {
        let mut metadata = BTreeMap::new();
        store_exposure_policy(&mut metadata, &CredentialExposurePolicy::direct())
            .expect("store direct policy");
        assert!(is_direct_access(&metadata));
        assert!(!metadata[EXPOSURE_POLICY_METADATA_KEY].contains("value"));
    }

    #[test]
    fn evidence_targets_require_exact_quote_contract() {
        let argument: ExposureEvidenceTarget =
            serde_json::from_str(
                r#"{"kind":"argument_quote","node_index":2,"argument_index":4,"quote":"python3 -c","occurrence":0}"#,
            )
            .expect("argument quote");
        assert_eq!(
            argument,
            ExposureEvidenceTarget::ArgumentQuote {
                node_index: 2,
                argument_index: 4,
                quote: "python3 -c".to_string(),
                occurrence: 0,
            }
        );

        let source: ExposureEvidenceTarget = serde_json::from_str(
            r#"{"kind":"source_quote","node_index":1,"source_id":"file:/tmp/example","start_line":7,"end_line":9,"quote":"requests.post"}"#,
        )
        .expect("source quote");
        assert_eq!(
            source,
            ExposureEvidenceTarget::SourceQuote {
                node_index: 1,
                source_id: "file:/tmp/example".to_string(),
                start_line: 7,
                end_line: 9,
                quote: "requests.post".to_string(),
                occurrence: 0,
            }
        );

        for legacy in [
            r#"{"kind":"free_text","excerpt":"x"}"#,
            r#"{"kind":"argument","node_index":0,"argument_index":0}"#,
            r#"{"kind":"source","node_index":0,"path":"/tmp/x","start_line":1,"end_line":1}"#,
        ] {
            assert!(serde_json::from_str::<ExposureEvidenceTarget>(legacy).is_err());
        }
    }
}
