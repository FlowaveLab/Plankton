use serde::{Deserialize, Serialize};

#[derive(
    schemars::JsonSchema, Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize,
)]
#[serde(rename_all = "snake_case")]
pub enum CredentialExposureSurface {
    LlmContext,
    Network,
    LocalPersistence,
    TerminalLog,
    ProcessPropagation,
}

impl CredentialExposureSurface {
    pub const ALL: [Self; 5] = [
        Self::LlmContext,
        Self::Network,
        Self::LocalPersistence,
        Self::TerminalLog,
        Self::ProcessPropagation,
    ];

    pub const fn maximum_configured_level(self) -> u8 {
        match self {
            Self::Network => 2,
            Self::LlmContext
            | Self::LocalPersistence
            | Self::TerminalLog
            | Self::ProcessPropagation => 1,
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CredentialAccessMode {
    Direct,
    #[default]
    Protected,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExposureBreachAction {
    #[default]
    HumanReview,
    Deny,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum NetworkDestinationRule {
    ExactDomain {
        domain: String,
    },
    SubdomainsOf {
        domain: String,
        #[serde(default)]
        include_apex: bool,
    },
    Regex {
        pattern: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExposureSurfacePolicy {
    pub surface: CredentialExposureSurface,
    pub max_level: u8,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub network_allowlist: Vec<NetworkDestinationRule>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

impl ExposureSurfacePolicy {
    pub fn validate(&self) -> Result<(), ExposurePolicyError> {
        let maximum = self.surface.maximum_configured_level();
        if self.max_level > maximum {
            return Err(ExposurePolicyError::LevelOutOfRange {
                surface: self.surface,
                level: self.max_level,
                maximum,
            });
        }
        if self.surface != CredentialExposureSurface::Network && !self.network_allowlist.is_empty()
        {
            return Err(ExposurePolicyError::AllowlistOnNonNetworkSurface(
                self.surface,
            ));
        }
        if self.surface == CredentialExposureSurface::Network {
            if self.max_level == 1 && self.network_allowlist.is_empty() {
                return Err(ExposurePolicyError::NetworkAllowlistRequired);
            }
            for rule in &self.network_allowlist {
                let value = match rule {
                    NetworkDestinationRule::ExactDomain { domain }
                    | NetworkDestinationRule::SubdomainsOf { domain, .. } => domain,
                    NetworkDestinationRule::Regex { pattern } => pattern,
                };
                if value.trim().is_empty() {
                    return Err(ExposurePolicyError::EmptyNetworkRule);
                }
                match rule {
                    NetworkDestinationRule::ExactDomain { domain }
                    | NetworkDestinationRule::SubdomainsOf { domain, .. } => {
                        if !valid_domain(domain) {
                            return Err(ExposurePolicyError::InvalidNetworkDomain(domain.clone()));
                        }
                    }
                    NetworkDestinationRule::Regex { pattern } => {
                        regex::Regex::new(pattern).map_err(|error| {
                            ExposurePolicyError::InvalidNetworkRegex(error.to_string())
                        })?;
                    }
                }
            }
        }
        Ok(())
    }
}

fn valid_domain(value: &str) -> bool {
    let value = value.trim();
    !value.is_empty()
        && !value.contains('/')
        && !value.contains(':')
        && value.split('.').all(|part| {
            !part.is_empty()
                && !part.starts_with('-')
                && !part.ends_with('-')
                && part
                    .chars()
                    .all(|character| character.is_ascii_alphanumeric() || character == '-')
        })
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CredentialExposurePolicy {
    #[serde(default)]
    pub access_mode: CredentialAccessMode,
    #[serde(default)]
    pub breach_action: ExposureBreachAction,
    pub surfaces: Vec<ExposureSurfacePolicy>,
}

impl Default for CredentialExposurePolicy {
    fn default() -> Self {
        Self {
            access_mode: CredentialAccessMode::Protected,
            breach_action: ExposureBreachAction::HumanReview,
            surfaces: vec![
                ExposureSurfacePolicy {
                    surface: CredentialExposureSurface::LlmContext,
                    max_level: 0,
                    network_allowlist: Vec::new(),
                    note: None,
                },
                ExposureSurfacePolicy {
                    surface: CredentialExposureSurface::Network,
                    max_level: 0,
                    network_allowlist: Vec::new(),
                    note: None,
                },
                ExposureSurfacePolicy {
                    surface: CredentialExposureSurface::LocalPersistence,
                    max_level: 0,
                    network_allowlist: Vec::new(),
                    note: None,
                },
                ExposureSurfacePolicy {
                    surface: CredentialExposureSurface::TerminalLog,
                    max_level: 0,
                    network_allowlist: Vec::new(),
                    note: None,
                },
                ExposureSurfacePolicy {
                    surface: CredentialExposureSurface::ProcessPropagation,
                    max_level: 1,
                    network_allowlist: Vec::new(),
                    note: Some("Only pass to the declared local consumer process.".into()),
                },
            ],
        }
    }
}

impl CredentialExposurePolicy {
    pub fn direct() -> Self {
        Self {
            access_mode: CredentialAccessMode::Direct,
            ..Self::default()
        }
    }

    pub fn validate(&self) -> Result<(), ExposurePolicyError> {
        for surface in CredentialExposureSurface::ALL {
            let matches = self
                .surfaces
                .iter()
                .filter(|candidate| candidate.surface == surface)
                .collect::<Vec<_>>();
            if matches.is_empty() {
                return Err(ExposurePolicyError::MissingSurface(surface));
            }
            if matches.len() > 1 {
                return Err(ExposurePolicyError::DuplicateSurface(surface));
            }
            matches[0].validate()?;
        }
        Ok(())
    }

    pub fn surface(&self, surface: CredentialExposureSurface) -> &ExposureSurfacePolicy {
        self.surfaces
            .iter()
            .find(|candidate| candidate.surface == surface)
            .expect("validated/default policies contain every exposure surface")
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ExposurePolicyError {
    #[error("missing exposure surface {0:?}")]
    MissingSurface(CredentialExposureSurface),
    #[error("duplicate exposure surface {0:?}")]
    DuplicateSurface(CredentialExposureSurface),
    #[error("exposure level {level} exceeds maximum {maximum} for {surface:?}")]
    LevelOutOfRange {
        surface: CredentialExposureSurface,
        level: u8,
        maximum: u8,
    },
    #[error("network allowlist is not supported for {0:?}")]
    AllowlistOnNonNetworkSurface(CredentialExposureSurface),
    #[error("network allowlist entries cannot be empty")]
    EmptyNetworkRule,
    #[error("network exposure level 1 requires at least one allowlist rule")]
    NetworkAllowlistRequired,
    #[error("invalid network domain {0:?}; use a hostname without scheme, port, or path")]
    InvalidNetworkDomain(String),
    #[error("invalid network regular expression: {0}")]
    InvalidNetworkRegex(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_policy_is_protected_and_strict() {
        let policy = CredentialExposurePolicy::default();
        policy.validate().expect("valid default policy");
        assert_eq!(policy.access_mode, CredentialAccessMode::Protected);
        assert_eq!(
            policy
                .surface(CredentialExposureSurface::LlmContext)
                .max_level,
            0
        );
    }

    #[test]
    fn only_network_accepts_level_two_and_allowlists() {
        let mut policy = CredentialExposurePolicy::default();
        let network = policy
            .surfaces
            .iter_mut()
            .find(|entry| entry.surface == CredentialExposureSurface::Network)
            .expect("network surface");
        network.max_level = 2;
        network.network_allowlist = vec![NetworkDestinationRule::SubdomainsOf {
            domain: "example.com".into(),
            include_apex: true,
        }];
        policy.validate().expect("network policy");

        policy
            .surfaces
            .iter_mut()
            .find(|entry| entry.surface == CredentialExposureSurface::TerminalLog)
            .expect("terminal surface")
            .max_level = 2;
        assert!(matches!(
            policy.validate(),
            Err(ExposurePolicyError::LevelOutOfRange { .. })
        ));
    }
}
