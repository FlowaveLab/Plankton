use std::{
    fs,
    path::{Path, PathBuf},
};

use config::{Config, ConfigError, Environment, File};
use directories::ProjectDirs;
use plankton_protocol::acp::{AcpProfile, AgentKind, VersionMode};
use plankton_protocol::password_changes::{PasswordChangeDiff, PasswordChangeImpact};
use serde::{Deserialize, Serialize};
use toml::Value as TomlValue;

use crate::acp::{profile_from_legacy_command, ACP_DEFAULT_ARGS, ACP_DEFAULT_PROGRAM};
use crate::template::{
    DEFAULT_LLM_ADVICE_TEMPLATE, DEFAULT_LLM_SYSTEM_PROMPT, DEFAULT_REQUEST_TEMPLATE,
};
use crate::{LlmApprovalDecisionPolicy, PolicyMode};

pub const DEFAULT_USER_PROVIDER_KIND: &str = "acp";
pub const DEFAULT_LOCALE: &str = "en";
pub const SUPPORTED_LOCALES: [&str; 2] = ["en", "zh-CN"];

fn default_acp_profile() -> AcpProfile {
    AcpProfile {
        session_options: Default::default(),
        agent_kind: AgentKind::Codex,
        version_mode: VersionMode::Latest,
        version: None,
        program: None,
        args: Vec::new(),
    }
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlanktonSettings {
    pub database_url: String,
    pub locale: String,
    pub default_policy_mode: PolicyMode,
    #[serde(default = "default_true")]
    pub llm_approval_allow_enabled: bool,
    #[serde(default = "default_true")]
    pub llm_approval_deny_enabled: bool,
    #[serde(default = "default_true")]
    pub llm_approval_escalate_enabled: bool,
    #[serde(default)]
    pub llm_auto_approve_password_edits: bool,
    #[serde(default)]
    pub llm_auto_approve_password_renames: bool,
    #[serde(default)]
    pub llm_auto_approve_password_refreshes: bool,
    #[serde(default)]
    pub llm_auto_approve_password_deletes: bool,
    pub provider_kind: String,
    pub request_template: String,
    pub llm_advice_template: String,
    pub llm_advice_system_prompt: String,
    pub openai_api_base: String,
    pub openai_api_key: String,
    pub openai_model: String,
    pub openai_temperature: f32,
    pub claude_api_base: String,
    pub claude_api_key: String,
    pub claude_model: String,
    pub claude_anthropic_version: String,
    pub claude_max_tokens: u32,
    pub claude_temperature: f32,
    pub claude_timeout_secs: u64,
    #[serde(default = "default_acp_profile")]
    pub acp_profile: AcpProfile,
    pub acp_codex_program: String,
    pub acp_codex_args: String,
    pub acp_timeout_secs: u64,
    pub recent_audit_limit: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct UserSettings {
    pub locale: String,
    pub default_policy_mode: PolicyMode,
    #[serde(default = "default_true")]
    pub llm_approval_allow_enabled: bool,
    #[serde(default = "default_true")]
    pub llm_approval_deny_enabled: bool,
    #[serde(default = "default_true")]
    pub llm_approval_escalate_enabled: bool,
    #[serde(default)]
    pub llm_auto_approve_password_edits: bool,
    #[serde(default)]
    pub llm_auto_approve_password_renames: bool,
    #[serde(default)]
    pub llm_auto_approve_password_refreshes: bool,
    #[serde(default)]
    pub llm_auto_approve_password_deletes: bool,
    pub provider_kind: String,
    pub request_template: String,
    pub llm_advice_template: String,
    pub openai_api_base: String,
    pub openai_api_key: String,
    pub openai_model: String,
    pub openai_temperature: f32,
    pub claude_api_base: String,
    pub claude_api_key: String,
    pub claude_model: String,
    pub claude_anthropic_version: String,
    pub claude_max_tokens: u32,
    pub claude_temperature: f32,
    pub claude_timeout_secs: u64,
    #[serde(default = "default_acp_profile")]
    pub acp_profile: AcpProfile,
    pub acp_codex_program: String,
    pub acp_codex_args: String,
    pub acp_timeout_secs: u64,
}

impl Default for PlanktonSettings {
    fn default() -> Self {
        Self {
            database_url: default_database_url(),
            locale: DEFAULT_LOCALE.to_string(),
            default_policy_mode: PolicyMode::ManualOnly,
            llm_approval_allow_enabled: true,
            llm_approval_deny_enabled: true,
            llm_approval_escalate_enabled: true,
            llm_auto_approve_password_edits: false,
            llm_auto_approve_password_renames: false,
            llm_auto_approve_password_refreshes: false,
            llm_auto_approve_password_deletes: false,
            provider_kind: DEFAULT_USER_PROVIDER_KIND.to_string(),
            request_template: DEFAULT_REQUEST_TEMPLATE.to_string(),
            llm_advice_template: DEFAULT_LLM_ADVICE_TEMPLATE.to_string(),
            llm_advice_system_prompt: DEFAULT_LLM_SYSTEM_PROMPT.to_string(),
            openai_api_base: "https://api.openai.com/v1".to_string(),
            openai_api_key: String::new(),
            openai_model: String::new(),
            openai_temperature: 0.0,
            claude_api_base: "https://api.anthropic.com".to_string(),
            claude_api_key: String::new(),
            claude_model: "claude-sonnet-4-5".to_string(),
            claude_anthropic_version: "2023-06-01".to_string(),
            claude_max_tokens: 512,
            claude_temperature: 0.0,
            claude_timeout_secs: 30,
            acp_profile: default_acp_profile(),
            acp_codex_program: ACP_DEFAULT_PROGRAM.to_string(),
            acp_codex_args: ACP_DEFAULT_ARGS.to_string(),
            acp_timeout_secs: 60,
            recent_audit_limit: 20,
        }
    }
}

impl From<&PlanktonSettings> for UserSettings {
    fn from(settings: &PlanktonSettings) -> Self {
        Self {
            locale: settings.locale.clone(),
            default_policy_mode: settings.default_policy_mode,
            llm_approval_allow_enabled: settings.llm_approval_allow_enabled,
            llm_approval_deny_enabled: settings.llm_approval_deny_enabled,
            llm_approval_escalate_enabled: settings.llm_approval_escalate_enabled,
            llm_auto_approve_password_edits: settings.llm_auto_approve_password_edits,
            llm_auto_approve_password_renames: settings.llm_auto_approve_password_renames,
            llm_auto_approve_password_refreshes: settings.llm_auto_approve_password_refreshes,
            llm_auto_approve_password_deletes: settings.llm_auto_approve_password_deletes,
            provider_kind: settings.provider_kind.clone(),
            request_template: settings.request_template.clone(),
            llm_advice_template: settings.llm_advice_template.clone(),
            openai_api_base: settings.openai_api_base.clone(),
            openai_api_key: settings.openai_api_key.clone(),
            openai_model: settings.openai_model.clone(),
            openai_temperature: settings.openai_temperature,
            claude_api_base: settings.claude_api_base.clone(),
            claude_api_key: settings.claude_api_key.clone(),
            claude_model: settings.claude_model.clone(),
            claude_anthropic_version: settings.claude_anthropic_version.clone(),
            claude_max_tokens: settings.claude_max_tokens,
            claude_temperature: settings.claude_temperature,
            claude_timeout_secs: settings.claude_timeout_secs,
            acp_profile: settings.acp_profile.clone(),
            acp_codex_program: settings.acp_codex_program.clone(),
            acp_codex_args: settings.acp_codex_args.clone(),
            acp_timeout_secs: settings.acp_timeout_secs,
        }
    }
}

impl UserSettings {
    fn normalized(&self) -> Self {
        Self {
            locale: normalize_locale_value(&self.locale).to_string(),
            default_policy_mode: self.default_policy_mode,
            llm_approval_allow_enabled: self.llm_approval_allow_enabled,
            llm_approval_deny_enabled: self.llm_approval_deny_enabled,
            llm_approval_escalate_enabled: self.llm_approval_escalate_enabled,
            llm_auto_approve_password_edits: self.llm_auto_approve_password_edits,
            llm_auto_approve_password_renames: self.llm_auto_approve_password_renames,
            llm_auto_approve_password_refreshes: self.llm_auto_approve_password_refreshes,
            llm_auto_approve_password_deletes: self.llm_auto_approve_password_deletes,
            provider_kind: canonicalize_provider_kind(&self.provider_kind),
            request_template: self.request_template.clone(),
            llm_advice_template: self.llm_advice_template.clone(),
            openai_api_base: self.openai_api_base.trim().to_string(),
            openai_api_key: self.openai_api_key.clone(),
            openai_model: self.openai_model.trim().to_string(),
            openai_temperature: self.openai_temperature,
            claude_api_base: self.claude_api_base.trim().to_string(),
            claude_api_key: self.claude_api_key.clone(),
            claude_model: self.claude_model.trim().to_string(),
            claude_anthropic_version: self.claude_anthropic_version.trim().to_string(),
            claude_max_tokens: self.claude_max_tokens,
            claude_temperature: self.claude_temperature,
            claude_timeout_secs: self.claude_timeout_secs,
            acp_profile: self.acp_profile.clone(),
            acp_codex_program: self.acp_codex_program.trim().to_string(),
            acp_codex_args: self.acp_codex_args.trim().to_string(),
            acp_timeout_secs: self.acp_timeout_secs,
        }
    }
}

impl PlanktonSettings {
    pub fn llm_approval_decision_policy(&self) -> LlmApprovalDecisionPolicy {
        LlmApprovalDecisionPolicy {
            allow: self.llm_approval_allow_enabled,
            deny: self.llm_approval_deny_enabled,
            escalate: self.llm_approval_escalate_enabled,
        }
    }
}

pub fn should_auto_approve_password_change(
    settings: &PlanktonSettings,
    diff: &PasswordChangeDiff,
) -> bool {
    if settings.default_policy_mode != PolicyMode::LlmAutomatic || diff.items.is_empty() {
        return false;
    }

    let mut impacts = diff
        .items
        .iter()
        .flat_map(|item| &item.entries)
        .map(|entry| entry.impact);
    let Some(first) = impacts.next() else {
        return false;
    };
    password_change_impact_allowed(settings, first)
        && impacts.all(|impact| password_change_impact_allowed(settings, impact))
}

fn password_change_impact_allowed(
    settings: &PlanktonSettings,
    impact: PasswordChangeImpact,
) -> bool {
    match impact {
        PasswordChangeImpact::Metadata => settings.llm_auto_approve_password_edits,
        PasswordChangeImpact::ExposurePolicy => false,
        PasswordChangeImpact::References => settings.llm_auto_approve_password_renames,
        PasswordChangeImpact::Locator | PasswordChangeImpact::Refresh => {
            settings.llm_auto_approve_password_refreshes
        }
        PasswordChangeImpact::Delete => settings.llm_auto_approve_password_deletes,
    }
}

#[derive(Debug, thiserror::Error)]
pub enum SettingsError {
    #[error("failed to build configuration: {0}")]
    Build(#[from] ConfigError),
    #[error("invalid ACP profile: {0}")]
    AcpProfile(String),
    #[error("invalid LLM approval decision policy: {0}")]
    LlmApprovalDecisionPolicy(String),
}

#[derive(Debug, thiserror::Error)]
pub enum SettingsPersistError {
    #[error("failed to create settings directory: {0}")]
    CreateDirectory(#[source] std::io::Error),
    #[error("failed to read existing settings file: {0}")]
    ReadFile(#[source] std::io::Error),
    #[error("failed to parse existing settings file: {0}")]
    ParseToml(#[source] toml::de::Error),
    #[error("failed to serialize settings file: {0}")]
    SerializeToml(#[source] toml::ser::Error),
    #[error("failed to write settings file: {0}")]
    WriteFile(#[source] std::io::Error),
    #[error("invalid setting for {field}: {reason}")]
    InvalidField {
        field: &'static str,
        reason: &'static str,
    },
    #[error("invalid setting for acp_profile: {0}")]
    InvalidAcpProfile(String),
}

pub fn load_settings() -> Result<PlanktonSettings, SettingsError> {
    load_settings_from_path(user_settings_path().as_path(), true)
}

fn load_settings_from_path(
    user_settings: &Path,
    include_project_settings: bool,
) -> Result<PlanktonSettings, SettingsError> {
    let defaults = PlanktonSettings::default();
    let mut builder = Config::builder()
        .set_default("database_url", defaults.database_url.clone())?
        .set_default("locale", defaults.locale.clone())?
        .set_default(
            "default_policy_mode",
            policy_mode_to_string(defaults.default_policy_mode),
        )?
        .set_default(
            "llm_approval_allow_enabled",
            defaults.llm_approval_allow_enabled,
        )?
        .set_default(
            "llm_approval_deny_enabled",
            defaults.llm_approval_deny_enabled,
        )?
        .set_default(
            "llm_approval_escalate_enabled",
            defaults.llm_approval_escalate_enabled,
        )?
        .set_default(
            "llm_auto_approve_password_edits",
            defaults.llm_auto_approve_password_edits,
        )?
        .set_default(
            "llm_auto_approve_password_renames",
            defaults.llm_auto_approve_password_renames,
        )?
        .set_default(
            "llm_auto_approve_password_refreshes",
            defaults.llm_auto_approve_password_refreshes,
        )?
        .set_default(
            "llm_auto_approve_password_deletes",
            defaults.llm_auto_approve_password_deletes,
        )?
        .set_default("provider_kind", defaults.provider_kind.clone())?
        .set_default("request_template", defaults.request_template.clone())?
        .set_default("llm_advice_template", defaults.llm_advice_template.clone())?
        .set_default(
            "llm_advice_system_prompt",
            defaults.llm_advice_system_prompt.clone(),
        )?
        .set_default("openai_api_base", defaults.openai_api_base.clone())?
        .set_default("openai_api_key", defaults.openai_api_key.clone())?
        .set_default("openai_model", defaults.openai_model.clone())?
        .set_default("openai_temperature", defaults.openai_temperature as f64)?
        .set_default("claude_api_base", defaults.claude_api_base.clone())?
        .set_default("claude_api_key", defaults.claude_api_key.clone())?
        .set_default("claude_model", defaults.claude_model.clone())?
        .set_default(
            "claude_anthropic_version",
            defaults.claude_anthropic_version.clone(),
        )?
        .set_default("claude_max_tokens", defaults.claude_max_tokens as i64)?
        .set_default("claude_temperature", defaults.claude_temperature as f64)?
        .set_default("claude_timeout_secs", defaults.claude_timeout_secs as i64)?
        .set_default("acp_codex_program", defaults.acp_codex_program.clone())?
        .set_default("acp_codex_args", defaults.acp_codex_args.clone())?
        .set_default("acp_timeout_secs", defaults.acp_timeout_secs as i64)?
        .set_default("recent_audit_limit", defaults.recent_audit_limit)?
        .add_source(File::from(user_settings.to_path_buf()).required(false));

    if include_project_settings {
        builder = builder.add_source(File::with_name("plankton").required(false));
    }

    let config = builder
        .add_source(Environment::with_prefix("PLANKTON").separator("__"))
        .build()?;
    let acp_program_alias = config.get_string("acp_program").ok();
    let acp_args_alias = config.get_string("acp_args").ok();
    let configured_acp_profile = config.get::<AcpProfile>("acp_profile").ok();

    let mut settings: PlanktonSettings = config.try_deserialize()?;
    apply_generic_acp_config_aliases(acp_program_alias, acp_args_alias, &mut settings);
    let legacy_acp_environment_override = apply_env_overrides(&mut settings);
    if configured_acp_profile.is_none() || legacy_acp_environment_override {
        settings.acp_profile =
            profile_from_legacy_command(&settings.acp_codex_program, &settings.acp_codex_args)
                .map_err(|error| SettingsError::AcpProfile(error.to_string()))?;
    }
    settings
        .acp_profile
        .validate()
        .map_err(|error| SettingsError::AcpProfile(error.to_string()))?;
    normalize_user_provider_kind(&mut settings);
    normalize_locale(&mut settings);
    validate_llm_approval_decision_policy(
        settings.llm_approval_allow_enabled,
        settings.llm_approval_deny_enabled,
        settings.llm_approval_escalate_enabled,
    )
    .map_err(|reason| SettingsError::LlmApprovalDecisionPolicy(reason.to_string()))?;

    Ok(settings)
}

pub fn default_database_url() -> String {
    let db_path = default_database_path();
    format!("sqlite://{}", db_path.display())
}

pub fn user_settings_path() -> PathBuf {
    if let Some(project_dirs) = ProjectDirs::from("com", "OpenAquarium", "Plankton") {
        return project_dirs.config_local_dir().join("plankton.toml");
    }

    std::env::temp_dir().join("plankton-user-settings.toml")
}

pub fn save_user_default_policy_mode(
    policy_mode: PolicyMode,
) -> Result<PathBuf, SettingsPersistError> {
    save_user_default_policy_mode_to_path(user_settings_path().as_path(), policy_mode)
}

pub fn save_user_locale(locale: &str) -> Result<PathBuf, SettingsPersistError> {
    save_user_locale_to_path(user_settings_path().as_path(), locale)
}

pub fn save_user_settings(settings: &UserSettings) -> Result<PathBuf, SettingsPersistError> {
    save_user_settings_to_path(user_settings_path().as_path(), settings)
}

fn default_database_path() -> PathBuf {
    if let Some(project_dirs) = ProjectDirs::from("com", "OpenAquarium", "Plankton") {
        return project_dirs.data_local_dir().join("plankton.db");
    }

    std::env::temp_dir().join("plankton.db")
}

fn apply_env_overrides(settings: &mut PlanktonSettings) -> bool {
    let mut legacy_acp_override = false;
    if let Ok(database_url) = std::env::var("PLANKTON_DATABASE_URL") {
        if !database_url.trim().is_empty() {
            settings.database_url = database_url;
        }
    }

    if let Ok(policy_mode) = std::env::var("PLANKTON_DEFAULT_POLICY_MODE") {
        if let Some(policy_mode) = parse_policy_mode(policy_mode.as_str()) {
            settings.default_policy_mode = policy_mode;
        }
    }

    if let Ok(locale) = std::env::var("PLANKTON_LOCALE") {
        if !locale.trim().is_empty() {
            settings.locale = locale;
        }
    }

    if let Ok(provider_kind) = std::env::var("PLANKTON_PROVIDER_KIND") {
        if !provider_kind.trim().is_empty() {
            settings.provider_kind = provider_kind;
        }
    }

    if let Ok(template) = std::env::var("PLANKTON_REQUEST_TEMPLATE") {
        if !template.trim().is_empty() {
            settings.request_template = template;
        }
    }

    if let Ok(template) = std::env::var("PLANKTON_LLM_ADVICE_TEMPLATE") {
        if !template.trim().is_empty() {
            settings.llm_advice_template = template;
        }
    }

    if let Ok(system_prompt) = std::env::var("PLANKTON_LLM_ADVICE_SYSTEM_PROMPT") {
        if !system_prompt.trim().is_empty() {
            settings.llm_advice_system_prompt = system_prompt;
        }
    }

    if let Ok(api_base) = std::env::var("PLANKTON_OPENAI_API_BASE") {
        if !api_base.trim().is_empty() {
            settings.openai_api_base = api_base;
        }
    }

    if let Ok(api_key) = std::env::var("PLANKTON_OPENAI_API_KEY") {
        settings.openai_api_key = api_key;
    }

    if let Ok(model) = std::env::var("PLANKTON_OPENAI_MODEL") {
        if !model.trim().is_empty() {
            settings.openai_model = model;
        }
    }

    if let Ok(temperature) = std::env::var("PLANKTON_OPENAI_TEMPERATURE") {
        if let Ok(temperature) = temperature.parse::<f32>() {
            settings.openai_temperature = temperature;
        }
    }

    if let Ok(api_base) = std::env::var("PLANKTON_CLAUDE_API_BASE") {
        if !api_base.trim().is_empty() {
            settings.claude_api_base = api_base;
        }
    }

    if let Ok(api_key) = std::env::var("PLANKTON_CLAUDE_API_KEY") {
        settings.claude_api_key = api_key;
    }

    if let Ok(model) = std::env::var("PLANKTON_CLAUDE_MODEL") {
        if !model.trim().is_empty() {
            settings.claude_model = model;
        }
    }

    if let Ok(version) = std::env::var("PLANKTON_CLAUDE_ANTHROPIC_VERSION") {
        if !version.trim().is_empty() {
            settings.claude_anthropic_version = version;
        }
    }

    if let Ok(version) = std::env::var("PLANKTON_CLAUDE_VERSION") {
        if !version.trim().is_empty() {
            settings.claude_anthropic_version = version;
        }
    }

    if let Ok(max_tokens) = std::env::var("PLANKTON_CLAUDE_MAX_TOKENS") {
        if let Ok(max_tokens) = max_tokens.parse::<u32>() {
            settings.claude_max_tokens = max_tokens;
        }
    }

    if let Ok(temperature) = std::env::var("PLANKTON_CLAUDE_TEMPERATURE") {
        if let Ok(temperature) = temperature.parse::<f32>() {
            settings.claude_temperature = temperature;
        }
    }

    if let Ok(timeout_secs) = std::env::var("PLANKTON_CLAUDE_TIMEOUT_SECS") {
        if let Ok(timeout_secs) = timeout_secs.parse::<u64>() {
            settings.claude_timeout_secs = timeout_secs;
        }
    }

    if let Some(program) =
        first_non_empty_env(&["PLANKTON_ACP_PROGRAM", "PLANKTON_ACP_CODEX_PROGRAM"])
    {
        settings.acp_codex_program = program;
        legacy_acp_override = true;
    }

    if let Some(args) = first_present_env(&["PLANKTON_ACP_ARGS", "PLANKTON_ACP_CODEX_ARGS"]) {
        settings.acp_codex_args = args;
        legacy_acp_override = true;
    }

    if let Ok(timeout_secs) = std::env::var("PLANKTON_ACP_TIMEOUT_SECS") {
        if let Ok(timeout_secs) = timeout_secs.parse::<u64>() {
            settings.acp_timeout_secs = timeout_secs;
        }
    }

    if let Ok(limit) = std::env::var("PLANKTON_RECENT_AUDIT_LIMIT") {
        if let Ok(limit) = limit.parse::<u32>() {
            settings.recent_audit_limit = limit;
        }
    }

    legacy_acp_override
}

fn normalize_user_provider_kind(settings: &mut PlanktonSettings) {
    let normalized = canonicalize_provider_kind(&settings.provider_kind);
    if normalized.is_empty() || normalized == "mock" {
        settings.provider_kind = DEFAULT_USER_PROVIDER_KIND.to_string();
    } else {
        settings.provider_kind = normalized;
    }
}

fn normalize_locale(settings: &mut PlanktonSettings) {
    settings.locale = normalize_locale_value(&settings.locale).to_string();
}

fn normalize_locale_value(value: &str) -> &'static str {
    match value.trim() {
        "zh-CN" | "zh-cn" | "zh_CN" | "zh" => "zh-CN",
        "en" | "en-US" | "en-us" => "en",
        _ => DEFAULT_LOCALE,
    }
}

fn apply_generic_acp_config_aliases(
    acp_program: Option<String>,
    acp_args: Option<String>,
    settings: &mut PlanktonSettings,
) {
    if let Some(program) = acp_program {
        if !program.trim().is_empty() {
            settings.acp_codex_program = program;
        }
    }

    if let Some(args) = acp_args {
        settings.acp_codex_args = args;
    }
}

fn first_non_empty_env(keys: &[&str]) -> Option<String> {
    keys.iter().find_map(|key| {
        std::env::var(key)
            .ok()
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
    })
}

fn first_present_env(keys: &[&str]) -> Option<String> {
    keys.iter().find_map(|key| std::env::var(key).ok())
}

fn canonicalize_provider_kind(value: &str) -> String {
    let normalized = value.trim().to_ascii_lowercase();
    match normalized.as_str() {
        "acp_codex" => DEFAULT_USER_PROVIDER_KIND.to_string(),
        _ => normalized,
    }
}

fn save_user_default_policy_mode_to_path(
    path: &Path,
    policy_mode: PolicyMode,
) -> Result<PathBuf, SettingsPersistError> {
    let mut document = read_user_settings_document(path)?;
    let table = root_table(&mut document);
    table.insert(
        "default_policy_mode".to_string(),
        TomlValue::String(policy_mode_to_string(policy_mode)),
    );
    write_user_settings_document(path, &document)
}

fn save_user_locale_to_path(path: &Path, locale: &str) -> Result<PathBuf, SettingsPersistError> {
    let normalized = normalize_locale_value(locale);
    let mut document = read_user_settings_document(path)?;
    let table = root_table(&mut document);
    table.insert(
        "locale".to_string(),
        TomlValue::String(normalized.to_string()),
    );
    write_user_settings_document(path, &document)
}

fn save_user_settings_to_path(
    path: &Path,
    settings: &UserSettings,
) -> Result<PathBuf, SettingsPersistError> {
    let normalized = settings.normalized();
    validate_user_settings(&normalized)?;

    let mut document = read_user_settings_document(path)?;
    let table = root_table(&mut document);
    table.insert("locale".to_string(), TomlValue::String(normalized.locale));
    table.insert(
        "default_policy_mode".to_string(),
        TomlValue::String(policy_mode_to_string(normalized.default_policy_mode)),
    );
    table.insert(
        "llm_approval_allow_enabled".to_string(),
        TomlValue::Boolean(normalized.llm_approval_allow_enabled),
    );
    table.insert(
        "llm_approval_deny_enabled".to_string(),
        TomlValue::Boolean(normalized.llm_approval_deny_enabled),
    );
    table.insert(
        "llm_approval_escalate_enabled".to_string(),
        TomlValue::Boolean(normalized.llm_approval_escalate_enabled),
    );
    table.insert(
        "llm_auto_approve_password_edits".to_string(),
        TomlValue::Boolean(normalized.llm_auto_approve_password_edits),
    );
    table.insert(
        "llm_auto_approve_password_renames".to_string(),
        TomlValue::Boolean(normalized.llm_auto_approve_password_renames),
    );
    table.insert(
        "llm_auto_approve_password_refreshes".to_string(),
        TomlValue::Boolean(normalized.llm_auto_approve_password_refreshes),
    );
    table.insert(
        "llm_auto_approve_password_deletes".to_string(),
        TomlValue::Boolean(normalized.llm_auto_approve_password_deletes),
    );
    table.insert(
        "provider_kind".to_string(),
        TomlValue::String(normalized.provider_kind),
    );
    table.insert(
        "request_template".to_string(),
        TomlValue::String(normalized.request_template),
    );
    table.insert(
        "llm_advice_template".to_string(),
        TomlValue::String(normalized.llm_advice_template),
    );
    table.remove("memory_enabled");
    table.remove("memory_template");
    table.insert(
        "openai_api_base".to_string(),
        TomlValue::String(normalized.openai_api_base),
    );
    table.insert(
        "openai_api_key".to_string(),
        TomlValue::String(normalized.openai_api_key),
    );
    table.insert(
        "openai_model".to_string(),
        TomlValue::String(normalized.openai_model),
    );
    table.insert(
        "openai_temperature".to_string(),
        TomlValue::Float(normalized.openai_temperature as f64),
    );
    table.insert(
        "claude_api_base".to_string(),
        TomlValue::String(normalized.claude_api_base),
    );
    table.insert(
        "claude_api_key".to_string(),
        TomlValue::String(normalized.claude_api_key),
    );
    table.insert(
        "claude_model".to_string(),
        TomlValue::String(normalized.claude_model),
    );
    table.insert(
        "claude_anthropic_version".to_string(),
        TomlValue::String(normalized.claude_anthropic_version),
    );
    table.insert(
        "claude_max_tokens".to_string(),
        TomlValue::Integer(normalized.claude_max_tokens as i64),
    );
    table.insert(
        "claude_temperature".to_string(),
        TomlValue::Float(normalized.claude_temperature as f64),
    );
    table.insert(
        "claude_timeout_secs".to_string(),
        TomlValue::Integer(normalized.claude_timeout_secs as i64),
    );
    let acp_profile =
        TomlValue::try_from(normalized.acp_profile).map_err(SettingsPersistError::SerializeToml)?;
    table.insert("acp_profile".to_string(), acp_profile);
    table.insert(
        "acp_timeout_secs".to_string(),
        TomlValue::Integer(normalized.acp_timeout_secs as i64),
    );
    table.remove("acp_program");
    table.remove("acp_args");
    table.remove("acp_codex_program");
    table.remove("acp_codex_args");

    write_user_settings_document(path, &document)
}

fn read_user_settings_document(path: &Path) -> Result<TomlValue, SettingsPersistError> {
    if path.exists() {
        let existing = fs::read_to_string(path).map_err(SettingsPersistError::ReadFile)?;
        toml::from_str::<TomlValue>(&existing).map_err(SettingsPersistError::ParseToml)
    } else {
        Ok(TomlValue::Table(toml::map::Map::new()))
    }
}

fn write_user_settings_document(
    path: &Path,
    document: &TomlValue,
) -> Result<PathBuf, SettingsPersistError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(SettingsPersistError::CreateDirectory)?;
    }

    let serialized =
        toml::to_string_pretty(document).map_err(SettingsPersistError::SerializeToml)?;
    fs::write(path, serialized).map_err(SettingsPersistError::WriteFile)?;
    Ok(path.to_path_buf())
}

fn root_table(document: &mut TomlValue) -> &mut toml::map::Map<String, TomlValue> {
    match document {
        TomlValue::Table(table) => table,
        _ => {
            *document = TomlValue::Table(toml::map::Map::new());
            match document {
                TomlValue::Table(table) => table,
                _ => unreachable!("table document should be created"),
            }
        }
    }
}

fn validate_user_settings(settings: &UserSettings) -> Result<(), SettingsPersistError> {
    if !SUPPORTED_LOCALES.contains(&settings.locale.as_str()) {
        return Err(SettingsPersistError::InvalidField {
            field: "locale",
            reason: "must be one of en or zh-CN",
        });
    }

    if settings.provider_kind.is_empty() {
        return Err(SettingsPersistError::InvalidField {
            field: "provider_kind",
            reason: "must not be empty",
        });
    }

    validate_llm_approval_decision_policy(
        settings.llm_approval_allow_enabled,
        settings.llm_approval_deny_enabled,
        settings.llm_approval_escalate_enabled,
    )
    .map_err(|reason| SettingsPersistError::InvalidField {
        field: "llm_approval_decisions",
        reason,
    })?;

    if settings.provider_kind.eq_ignore_ascii_case("mock") {
        return Err(SettingsPersistError::InvalidField {
            field: "provider_kind",
            reason: "mock is reserved for internal testing and is not a user-facing provider",
        });
    }

    settings
        .acp_profile
        .validate()
        .map_err(|error| SettingsPersistError::InvalidAcpProfile(error.to_string()))?;

    validate_non_negative_float("openai_temperature", settings.openai_temperature)?;
    validate_non_negative_float("claude_temperature", settings.claude_temperature)?;

    if settings.claude_max_tokens == 0 {
        return Err(SettingsPersistError::InvalidField {
            field: "claude_max_tokens",
            reason: "must be greater than zero",
        });
    }

    if settings.claude_timeout_secs == 0 {
        return Err(SettingsPersistError::InvalidField {
            field: "claude_timeout_secs",
            reason: "must be greater than zero",
        });
    }

    if settings.acp_timeout_secs == 0 {
        return Err(SettingsPersistError::InvalidField {
            field: "acp_timeout_secs",
            reason: "must be greater than zero",
        });
    }

    Ok(())
}

fn validate_llm_approval_decision_policy(
    allow: bool,
    deny: bool,
    escalate: bool,
) -> Result<(), &'static str> {
    if !allow {
        return Err("allow must remain enabled");
    }
    if !deny && !escalate {
        return Err("deny or escalate must remain enabled");
    }
    Ok(())
}

fn validate_non_negative_float(
    field: &'static str,
    value: f32,
) -> Result<(), SettingsPersistError> {
    if !value.is_finite() || value < 0.0 {
        return Err(SettingsPersistError::InvalidField {
            field,
            reason: "must be a finite value greater than or equal to zero",
        });
    }
    Ok(())
}

fn parse_policy_mode(value: &str) -> Option<PolicyMode> {
    match value.trim() {
        "manual_only" | "manual-only" => Some(PolicyMode::ManualOnly),
        "assisted" => Some(PolicyMode::Assisted),
        "llm_automatic" | "llm-automatic" | "auto" => Some(PolicyMode::LlmAutomatic),
        _ => None,
    }
}

fn policy_mode_to_string(value: PolicyMode) -> String {
    match value {
        PolicyMode::ManualOnly => "manual_only".to_string(),
        PolicyMode::Assisted => "assisted".to_string(),
        PolicyMode::LlmAutomatic => "llm_automatic".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::{
        load_settings_from_path, save_user_default_policy_mode_to_path, save_user_locale_to_path,
        save_user_settings_to_path, should_auto_approve_password_change, PlanktonSettings,
        PolicyMode, SettingsPersistError, UserSettings, DEFAULT_USER_PROVIDER_KIND,
    };
    use plankton_protocol::password_changes::{
        PasswordChangeDiff, PasswordChangeDiffEntry, PasswordChangeImpact, PasswordItemDiff,
    };

    #[test]
    fn defaults_acp_timeout_to_one_minute() {
        assert_eq!(PlanktonSettings::default().acp_timeout_secs, 60);
    }

    fn temp_settings_path(test_name: &str) -> std::path::PathBuf {
        let unique = uuid::Uuid::new_v4();
        std::env::temp_dir()
            .join("plankton-core-config-tests")
            .join(format!("{test_name}-{unique}.toml"))
    }

    fn password_diff(impact: PasswordChangeImpact) -> PasswordChangeDiff {
        PasswordChangeDiff {
            items: vec![PasswordItemDiff {
                record_id: "item-test".to_string(),
                item_id: "test".to_string(),
                title: "Test".to_string(),
                vaults: vec!["default".to_string()],
                entries: vec![PasswordChangeDiffEntry {
                    path: "/test".to_string(),
                    label: "Test".to_string(),
                    before: Some("before".to_string()),
                    after: Some("after".to_string()),
                    impact,
                }],
            }],
            changed_items: 1,
            changed_fields: 0,
            breaking_changes: 0,
        }
    }

    #[test]
    fn password_change_auto_approval_is_fail_closed_and_permission_scoped() {
        let mut settings = PlanktonSettings {
            llm_auto_approve_password_edits: true,
            ..PlanktonSettings::default()
        };
        assert!(!should_auto_approve_password_change(
            &settings,
            &password_diff(PasswordChangeImpact::Metadata)
        ));

        settings.default_policy_mode = PolicyMode::LlmAutomatic;
        assert!(should_auto_approve_password_change(
            &settings,
            &password_diff(PasswordChangeImpact::Metadata)
        ));
        assert!(!should_auto_approve_password_change(
            &settings,
            &password_diff(PasswordChangeImpact::Delete)
        ));

        let mut mixed = password_diff(PasswordChangeImpact::Metadata);
        mixed.items[0].entries.push(PasswordChangeDiffEntry {
            path: "/item".to_string(),
            label: "Delete".to_string(),
            before: Some("Test".to_string()),
            after: None,
            impact: PasswordChangeImpact::Delete,
        });
        assert!(!should_auto_approve_password_change(&settings, &mixed));

        settings.llm_auto_approve_password_deletes = true;
        assert!(should_auto_approve_password_change(
            &settings,
            &password_diff(PasswordChangeImpact::Delete)
        ));
        assert!(should_auto_approve_password_change(&settings, &mixed));
        assert!(!should_auto_approve_password_change(
            &settings,
            &PasswordChangeDiff {
                items: Vec::new(),
                changed_items: 0,
                changed_fields: 0,
                breaking_changes: 0,
            }
        ));
    }

    #[test]
    fn retired_settings_load_but_are_not_exposed_or_saved() {
        let path = temp_settings_path("retired-settings");
        std::fs::create_dir_all(path.parent().expect("temp parent")).expect("create temp parent");
        std::fs::write(
            &path,
            "memory_enabled = true\nmemory_template = \"obsolete\"\nrecent_audit_limit = 42\n",
        )
        .expect("write legacy settings");

        let loaded = load_settings_from_path(&path, false).expect("load legacy settings");
        let serialized = serde_json::to_value(&loaded).expect("serialize settings");
        assert!(serialized.get("memory_enabled").is_none());
        assert!(serialized.get("memory_template").is_none());
        save_user_settings_to_path(&path, &UserSettings::from(&loaded))
            .expect("save current settings");
        let saved = std::fs::read_to_string(&path).expect("read settings");
        assert!(!saved.contains("memory_"));
        assert!(saved.contains("recent_audit_limit = 42"));
    }

    #[test]
    fn saves_user_settings_subset_without_clobbering_unrelated_keys() {
        let path = temp_settings_path("preserve-unrelated");
        std::fs::create_dir_all(path.parent().expect("temp parent should exist"))
            .expect("temp parent should be created");
        std::fs::write(
            &path,
            "recent_audit_limit = 42\nrequest_template = \"keep-me\"\n",
        )
        .expect("seed settings file should be written");

        let settings = UserSettings {
            locale: "zh-CN".to_string(),
            default_policy_mode: PolicyMode::Assisted,
            llm_approval_allow_enabled: true,
            llm_approval_deny_enabled: false,
            llm_approval_escalate_enabled: true,
            llm_auto_approve_password_edits: true,
            llm_auto_approve_password_renames: false,
            llm_auto_approve_password_refreshes: true,
            llm_auto_approve_password_deletes: false,
            provider_kind: "claude".to_string(),
            request_template: "review {{ context.resource }}".to_string(),
            llm_advice_template: "advise {{ context.resource }}".to_string(),
            openai_api_base: "https://example.com/openai".to_string(),
            openai_api_key: "openai-key".to_string(),
            openai_model: "gpt-test".to_string(),
            openai_temperature: 0.2,
            claude_api_base: "https://example.com/claude".to_string(),
            claude_api_key: "claude-key".to_string(),
            claude_model: "claude-sonnet".to_string(),
            claude_anthropic_version: "2023-06-01".to_string(),
            claude_max_tokens: 1024,
            claude_temperature: 0.1,
            claude_timeout_secs: 45,
            acp_profile: super::default_acp_profile(),
            acp_codex_program: "npx".to_string(),
            acp_codex_args: "-y codex-acp".to_string(),
            acp_timeout_secs: 60,
        };

        save_user_settings_to_path(&path, &settings).expect("settings should persist");

        let written = std::fs::read_to_string(&path).expect("settings file should be readable");
        assert!(written.contains("recent_audit_limit = 42"));
        assert!(written.contains("locale = \"zh-CN\""));
        assert!(written.contains("llm_approval_allow_enabled = true"));
        assert!(written.contains("llm_approval_deny_enabled = false"));
        assert!(written.contains("llm_approval_escalate_enabled = true"));
        assert!(written.contains("llm_auto_approve_password_edits = true"));
        assert!(written.contains("llm_auto_approve_password_deletes = false"));
        assert!(written.contains("request_template = \"review {{ context.resource }}\""));
        assert!(written.contains("llm_advice_template = \"advise {{ context.resource }}\""));
        assert!(written.contains("provider_kind = \"claude\""));
        assert!(written.contains("claude_timeout_secs = 45"));
        assert!(written.contains("[acp_profile]"));
        assert!(written.contains("agent_kind = \"codex\""));
        assert!(written.contains("version_mode = \"latest\""));
        assert!(!written.contains("acp_program"));
        assert!(!written.contains("acp_args"));
        assert!(!written.contains("acp_codex_program"));
        assert!(!written.contains("acp_codex_args"));

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn loads_saved_user_settings_from_custom_path() {
        let path = temp_settings_path("load-saved-user-settings");
        let settings = UserSettings {
            locale: "zh-CN".to_string(),
            default_policy_mode: PolicyMode::LlmAutomatic,
            llm_approval_allow_enabled: true,
            llm_approval_deny_enabled: true,
            llm_approval_escalate_enabled: false,
            llm_auto_approve_password_edits: true,
            llm_auto_approve_password_renames: true,
            llm_auto_approve_password_refreshes: false,
            llm_auto_approve_password_deletes: false,
            provider_kind: "openai_compatible".to_string(),
            request_template: "review".to_string(),
            llm_advice_template: "advise".to_string(),
            openai_api_base: "https://openai.example/v1".to_string(),
            openai_api_key: "sk-test".to_string(),
            openai_model: "gpt-5.4-mini".to_string(),
            openai_temperature: 0.7,
            claude_api_base: "https://claude.example".to_string(),
            claude_api_key: "claude-secret".to_string(),
            claude_model: "claude-sonnet-4-5".to_string(),
            claude_anthropic_version: "2023-06-01".to_string(),
            claude_max_tokens: 2048,
            claude_temperature: 0.3,
            claude_timeout_secs: 90,
            acp_profile: plankton_protocol::acp::AcpProfile {
                session_options: std::collections::BTreeMap::from([
                    ("model".into(), "agent-advertised-model".into()),
                    ("speed".into(), "fast".into()),
                ]),
                agent_kind: plankton_protocol::acp::AgentKind::Custom,
                version_mode: plankton_protocol::acp::VersionMode::Custom,
                version: None,
                program: Some("node".to_string()),
                args: vec!["codex-acp.js".to_string()],
            },
            acp_codex_program: "node".to_string(),
            acp_codex_args: "codex-acp.js".to_string(),
            acp_timeout_secs: 75,
        };

        save_user_settings_to_path(&path, &settings).expect("settings should persist");
        let loaded =
            load_settings_from_path(&path, false).expect("settings should load from custom path");

        assert_eq!(loaded.default_policy_mode, PolicyMode::LlmAutomatic);
        assert!(loaded.llm_approval_allow_enabled);
        assert!(loaded.llm_approval_deny_enabled);
        assert!(!loaded.llm_approval_escalate_enabled);
        assert!(loaded.llm_auto_approve_password_edits);
        assert!(loaded.llm_auto_approve_password_renames);
        assert!(!loaded.llm_auto_approve_password_refreshes);
        assert!(!loaded.llm_auto_approve_password_deletes);
        assert_eq!(loaded.locale, "zh-CN");
        assert_eq!(loaded.provider_kind, "openai_compatible");
        assert_eq!(loaded.request_template, "review");
        assert_eq!(loaded.llm_advice_template, "advise");
        assert_eq!(loaded.openai_model, "gpt-5.4-mini");
        assert_eq!(loaded.openai_temperature, 0.7);
        assert_eq!(loaded.claude_timeout_secs, 90);
        assert_eq!(
            loaded.acp_profile.agent_kind,
            plankton_protocol::acp::AgentKind::Custom
        );
        assert_eq!(loaded.acp_profile.program.as_deref(), Some("node"));
        assert_eq!(loaded.acp_profile.args, vec!["codex-acp.js"]);
        assert_eq!(
            loaded.acp_profile.session_options,
            settings.acp_profile.session_options
        );

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn legacy_policy_mode_persist_keeps_existing_values() {
        let path = temp_settings_path("legacy-policy-mode");
        std::fs::create_dir_all(path.parent().expect("temp parent should exist"))
            .expect("temp parent should be created");
        std::fs::write(&path, "provider_kind = \"acp_codex\"\n")
            .expect("seed settings should exist");

        save_user_default_policy_mode_to_path(&path, PolicyMode::Assisted)
            .expect("policy mode should persist");
        let loaded =
            load_settings_from_path(&path, false).expect("settings should load from custom path");

        assert_eq!(loaded.default_policy_mode, PolicyMode::Assisted);
        assert_eq!(loaded.provider_kind, DEFAULT_USER_PROVIDER_KIND);

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn saves_locale_without_touching_other_settings() {
        let path = temp_settings_path("save-locale");
        std::fs::create_dir_all(path.parent().expect("temp parent should exist"))
            .expect("temp parent should be created");
        std::fs::write(&path, "provider_kind = \"claude\"\n").expect("seed settings should exist");

        save_user_locale_to_path(&path, "zh").expect("locale should persist");
        let written = std::fs::read_to_string(&path).expect("settings file should be readable");

        assert!(written.contains("locale = \"zh-CN\""));
        assert!(written.contains("provider_kind = \"claude\""));

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn rejects_invalid_provider_kind() {
        let path = temp_settings_path("invalid-provider-kind");
        let mut settings = UserSettings::from(&super::PlanktonSettings::default());
        settings.provider_kind = "   ".to_string();

        let error =
            save_user_settings_to_path(&path, &settings).expect_err("blank provider should fail");

        assert!(matches!(
            error,
            SettingsPersistError::InvalidField {
                field: "provider_kind",
                ..
            }
        ));
    }

    #[test]
    fn rejects_llm_approval_policy_without_allow() {
        let path = temp_settings_path("llm-policy-without-allow");
        let mut settings = UserSettings::from(&super::PlanktonSettings::default());
        settings.llm_approval_allow_enabled = false;

        let error =
            save_user_settings_to_path(&path, &settings).expect_err("allow must remain enabled");

        assert!(matches!(
            error,
            SettingsPersistError::InvalidField {
                field: "llm_approval_decisions",
                reason: "allow must remain enabled",
            }
        ));
    }

    #[test]
    fn rejects_llm_approval_policy_without_non_allow_outcome() {
        let path = temp_settings_path("llm-policy-without-fallback");
        let mut settings = UserSettings::from(&super::PlanktonSettings::default());
        settings.llm_approval_deny_enabled = false;
        settings.llm_approval_escalate_enabled = false;

        let error = save_user_settings_to_path(&path, &settings)
            .expect_err("a non-allow outcome must remain enabled");

        assert!(matches!(
            error,
            SettingsPersistError::InvalidField {
                field: "llm_approval_decisions",
                reason: "deny or escalate must remain enabled",
            }
        ));
    }

    #[test]
    fn rejects_mock_provider_kind_for_user_settings() {
        let path = temp_settings_path("reject-mock-provider-kind");
        let mut settings = UserSettings::from(&super::PlanktonSettings::default());
        settings.provider_kind = "mock".to_string();

        let error = save_user_settings_to_path(&path, &settings)
            .expect_err("mock should not be accepted as a user-facing provider");

        assert!(matches!(
            error,
            SettingsPersistError::InvalidField {
                field: "provider_kind",
                ..
            }
        ));
    }

    #[test]
    fn rejects_invalid_acp_profile_before_modifying_the_settings_file() {
        let path = temp_settings_path("invalid-acp-profile");
        std::fs::create_dir_all(path.parent().expect("temp parent should exist"))
            .expect("temp parent should be created");
        let original = "locale = \"en\"\ncustom_marker = \"unchanged\"\n";
        std::fs::write(&path, original).expect("existing settings should be written");
        let mut settings = UserSettings::from(&super::PlanktonSettings::default());
        settings.acp_profile = plankton_protocol::acp::AcpProfile {
            session_options: Default::default(),
            agent_kind: plankton_protocol::acp::AgentKind::Codex,
            version_mode: plankton_protocol::acp::VersionMode::Pinned,
            version: None,
            program: None,
            args: Vec::new(),
        };

        let error = save_user_settings_to_path(&path, &settings)
            .expect_err("invalid ACP profile must be rejected before persistence");

        assert!(error
            .to_string()
            .contains("pinned ACP profiles require an exact semantic version"));
        assert_eq!(
            std::fs::read_to_string(&path).expect("settings should remain readable"),
            original
        );

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn upgrades_legacy_mock_provider_setting_to_default_acp_on_load() {
        let path = temp_settings_path("upgrade-legacy-mock-provider");
        std::fs::create_dir_all(path.parent().expect("temp parent should exist"))
            .expect("temp parent should be created");
        std::fs::write(&path, "provider_kind = \"mock\"\n")
            .expect("legacy settings file should be written");

        let loaded =
            load_settings_from_path(&path, false).expect("settings should load from custom path");

        assert_eq!(loaded.provider_kind, DEFAULT_USER_PROVIDER_KIND);

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn upgrades_legacy_acp_codex_provider_kind_to_generic_acp_on_load() {
        let path = temp_settings_path("upgrade-legacy-acp-provider-kind");
        std::fs::create_dir_all(path.parent().expect("temp parent should exist"))
            .expect("temp parent should be created");
        std::fs::write(&path, "provider_kind = \"acp_codex\"\n")
            .expect("legacy settings file should be written");

        let loaded =
            load_settings_from_path(&path, false).expect("settings should load from custom path");

        assert_eq!(loaded.provider_kind, DEFAULT_USER_PROVIDER_KIND);

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn loads_generic_acp_program_keys_from_settings_file() {
        let path = temp_settings_path("generic-acp-program-keys");
        std::fs::create_dir_all(path.parent().expect("temp parent should exist"))
            .expect("temp parent should be created");
        std::fs::write(
            &path,
            "provider_kind = \"acp\"\nacp_program = \"custom-acp\"\nacp_args = \"\"\n",
        )
        .expect("settings file should be written");

        let loaded =
            load_settings_from_path(&path, false).expect("settings should load from custom path");

        assert_eq!(loaded.provider_kind, "acp");
        assert_eq!(loaded.acp_codex_program, "custom-acp");
        assert_eq!(loaded.acp_codex_args, "");

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn migrates_legacy_codex_command_to_latest_profile() {
        let path = temp_settings_path("legacy-codex-acp-profile");
        std::fs::create_dir_all(path.parent().expect("temp parent should exist"))
            .expect("temp parent should be created");
        std::fs::write(
            &path,
            "acp_codex_program = \"npx\"\nacp_codex_args = \"-y @zed-industries/codex-acp@latest\"\n",
        )
        .expect("legacy settings file should be written");

        let loaded =
            load_settings_from_path(&path, false).expect("legacy settings should load safely");

        assert_eq!(
            loaded.acp_profile.agent_kind,
            plankton_protocol::acp::AgentKind::Codex
        );
        assert_eq!(
            loaded.acp_profile.version_mode,
            plankton_protocol::acp::VersionMode::Latest
        );
        assert_eq!(loaded.acp_profile.version, None);

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn upgrades_legacy_zed_pinned_codex_commands_to_latest_profile() {
        for version in ["0.11.1", "0.12.3"] {
            let path = temp_settings_path(&format!("legacy-codex-pinned-profile-{version}"));
            std::fs::create_dir_all(path.parent().expect("temp parent should exist"))
                .expect("temp parent should be created");
            let package = format!("@zed-industries/codex-acp@{version}");
            std::fs::write(
                &path,
                format!("acp_codex_program = \"npx\"\nacp_codex_args = \"-y {package}\"\n"),
            )
            .expect("legacy settings file should be written");

            let loaded =
                load_settings_from_path(&path, false).expect("legacy settings should load safely");

            assert_eq!(
                loaded.acp_profile.agent_kind,
                plankton_protocol::acp::AgentKind::Codex
            );
            assert_eq!(
                loaded.acp_profile.version_mode,
                plankton_protocol::acp::VersionMode::Latest
            );
            assert_eq!(loaded.acp_profile.version, None);
            assert_eq!(loaded.acp_profile.program, None);
            assert!(loaded.acp_profile.args.is_empty());

            let _ = std::fs::remove_file(&path);
        }
    }
}
