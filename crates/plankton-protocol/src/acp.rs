use semver::Version;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentKind {
    Codex,
    ClaudeCode,
    OpenCode,
    Custom,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VersionMode {
    Latest,
    Pinned,
    Custom,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AcpProfile {
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub session_options: std::collections::BTreeMap<String, String>,
    pub agent_kind: AgentKind,
    pub version_mode: VersionMode,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub program: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub args: Vec<String>,
}

impl AcpProfile {
    pub fn validate(&self) -> Result<(), AcpProfileError> {
        match self.version_mode {
            VersionMode::Latest => {
                if self.agent_kind == AgentKind::Custom {
                    return Err(AcpProfileError::CustomRequiresCustomMode);
                }
                if self.version.is_some() {
                    return Err(AcpProfileError::LatestCannotSpecifyVersion);
                }
                if self.program.is_some() || !self.args.is_empty() {
                    return Err(AcpProfileError::PresetCannotSpecifyCommand);
                }
            }
            VersionMode::Pinned => {
                if self.agent_kind == AgentKind::Custom {
                    return Err(AcpProfileError::CustomRequiresCustomMode);
                }
                let version = self
                    .version
                    .as_deref()
                    .ok_or(AcpProfileError::PinnedRequiresVersion)?;
                Version::parse(version)
                    .map_err(|_| AcpProfileError::InvalidPinnedVersion(version.to_string()))?;
                if self.program.is_some() || !self.args.is_empty() {
                    return Err(AcpProfileError::PresetCannotSpecifyCommand);
                }
            }
            VersionMode::Custom => {
                if self.agent_kind != AgentKind::Custom {
                    return Err(AcpProfileError::CustomModeRequiresCustomAgent);
                }
                if self.version.is_some() {
                    return Err(AcpProfileError::CustomCannotSpecifyVersion);
                }
                let program = self
                    .program
                    .as_deref()
                    .ok_or(AcpProfileError::CustomRequiresProgram)?;
                if program.trim().is_empty() {
                    return Err(AcpProfileError::CustomRequiresProgram);
                }
            }
        }

        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum AcpProfileError {
    #[error("latest ACP profiles cannot specify a version")]
    LatestCannotSpecifyVersion,
    #[error("pinned ACP profiles require an exact semantic version")]
    PinnedRequiresVersion,
    #[error("invalid pinned ACP version {0:?}")]
    InvalidPinnedVersion(String),
    #[error("built-in ACP presets cannot specify a custom program or arguments")]
    PresetCannotSpecifyCommand,
    #[error("custom ACP agents require custom version mode")]
    CustomRequiresCustomMode,
    #[error("custom ACP mode requires the custom agent kind")]
    CustomModeRequiresCustomAgent,
    #[error("custom ACP profiles cannot specify a package version")]
    CustomCannotSpecifyVersion,
    #[error("custom ACP profiles require a non-empty program")]
    CustomRequiresProgram,
}
