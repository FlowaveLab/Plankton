mod desktop_handoff;
mod onepassword_import;
mod skill_install;

use std::{
    collections::BTreeMap,
    env,
    future::Future,
    io::{self, Write},
    path::PathBuf,
    process::ExitCode,
    time::{Duration, Instant},
};

use anyhow::{bail, Context, Result};
use clap::{ArgGroup, Args, Parser, Subcommand, ValueEnum};
use desktop_handoff::{
    trigger_local_vault_manager_handoff, trigger_password_change_handoff,
    trigger_password_draft_handoff, trigger_password_edit_handoff,
    trigger_password_migration_handoff, trigger_request_handoff,
};
use plankton_client::{ClientError, DaemonClient};
use plankton_core::{
    collect_runtime_call_chain, derive_script_path, passwords::parse_password_source_descriptor,
    prompt_call_chain_paths, CallChainNode,
};
use plankton_protocol::{
    exposure::{
        CredentialAccessMode, CredentialExposurePolicy, CredentialExposureSurface,
        ExposureBreachAction, NetworkDestinationRule,
    },
    password_changes::{
        PasswordChangeOperation, PasswordChangeState, PasswordChangeStatus,
        SubmitPasswordChangeRequest,
    },
    passwords::{
        FileFormat, PasswordDestination, PasswordDraftCreated, PasswordDraftInput,
        PasswordDraftLayoutSuggestion, PasswordDraftState, PasswordDraftStatus,
        PasswordSourceDescriptor, SelectedPasswordEntry,
    },
    resources::{
        ResourceAccessCallChainNode, ResourceAccessRequest, ResourceAccessResponse,
        ResourceAccessState, ResourceSearchItem, ResourceSearchRequest,
    },
};
use serde::Serialize;
use tokio::time::sleep;
use tracing_subscriber::{fmt, EnvFilter};

const GET_POLL_INTERVAL: Duration = Duration::from_millis(250);
const STATUS_POLL_RETRY_LIMIT: u8 = 3;

#[derive(Debug, Parser)]
#[command(
    author,
    version,
    about = "AI-facing Plankton client for provider-neutral password search and approved reads",
    arg_required_else_help = true,
    after_help = "Password values are never returned by management commands.\n\
                  `password create` opens an empty human-filled batch draft; `password add` \
                  imports selected existing values. `password edit --human` opens a local human \
                  editor without putting values in CLI arguments or output. Other edit, \
                  `rename-field`, `edit-field`, `refresh`, and `delete` operations stage \
                  value-free changes in the daemon and require desktop confirmation before commit."
)]
struct Cli {
    #[arg(
        long,
        global = true,
        value_enum,
        help = "Choose text, JSON, or JSON Lines output"
    )]
    output: Option<OutputFormat>,
    #[command(subcommand)]
    command: Commands,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
enum OutputFormat {
    Text,
    Json,
    Jsonl,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
enum CredentialAccessModeArg {
    Protected,
    Direct,
}

impl From<CredentialAccessModeArg> for CredentialAccessMode {
    fn from(value: CredentialAccessModeArg) -> Self {
        match value {
            CredentialAccessModeArg::Protected => Self::Protected,
            CredentialAccessModeArg::Direct => Self::Direct,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
enum ExposureBreachActionArg {
    HumanReview,
    Deny,
}

impl From<ExposureBreachActionArg> for ExposureBreachAction {
    fn from(value: ExposureBreachActionArg) -> Self {
        match value {
            ExposureBreachActionArg::HumanReview => Self::HumanReview,
            ExposureBreachActionArg::Deny => Self::Deny,
        }
    }
}

#[derive(Debug, Args, Default)]
struct ExposurePolicyArgs {
    #[arg(
        long,
        value_enum,
        help = "Protected fields require exposure review; direct fields bypass approval"
    )]
    access_mode: Option<CredentialAccessModeArg>,
    #[arg(
        long,
        value_enum,
        help = "Action when any actual exposure exceeds this policy"
    )]
    breach_action: Option<ExposureBreachActionArg>,
    #[arg(long, value_parser = clap::value_parser!(u8).range(0..=1), value_name = "0|1")]
    llm_context: Option<u8>,
    #[arg(long, value_parser = clap::value_parser!(u8).range(0..=2), value_name = "0|1|2")]
    network: Option<u8>,
    #[arg(long, value_parser = clap::value_parser!(u8).range(0..=1), value_name = "0|1")]
    local_persistence: Option<u8>,
    #[arg(long, value_parser = clap::value_parser!(u8).range(0..=1), value_name = "0|1")]
    terminal_log: Option<u8>,
    #[arg(long, value_parser = clap::value_parser!(u8).range(0..=1), value_name = "0|1")]
    process_propagation: Option<u8>,
    #[arg(
        long = "network-domain",
        value_name = "DOMAIN",
        help = "Allow one exact network domain; repeatable"
    )]
    network_domains: Vec<String>,
    #[arg(
        long = "network-subdomains",
        value_name = "DOMAIN",
        help = "Allow subdomains below a domain; repeatable"
    )]
    network_subdomains: Vec<String>,
    #[arg(
        long = "network-regex",
        value_name = "REGEX",
        help = "Allow destinations matching a regular expression; repeatable"
    )]
    network_regexes: Vec<String>,
    #[arg(
        long = "exposure-note",
        value_name = "SURFACE=TEXT",
        help = "LLM-visible note for an exposure surface; repeatable"
    )]
    notes: Vec<String>,
}

impl ExposurePolicyArgs {
    fn is_specified(&self) -> bool {
        self.access_mode.is_some()
            || self.breach_action.is_some()
            || self.llm_context.is_some()
            || self.network.is_some()
            || self.local_persistence.is_some()
            || self.terminal_log.is_some()
            || self.process_propagation.is_some()
            || !self.network_domains.is_empty()
            || !self.network_subdomains.is_empty()
            || !self.network_regexes.is_empty()
            || !self.notes.is_empty()
    }

    fn build(&self) -> Result<CredentialExposurePolicy> {
        let mut policy = CredentialExposurePolicy::default();
        if let Some(mode) = self.access_mode {
            policy.access_mode = mode.into();
        }
        if let Some(action) = self.breach_action {
            policy.breach_action = action.into();
        }
        for (surface, level) in [
            (CredentialExposureSurface::LlmContext, self.llm_context),
            (CredentialExposureSurface::Network, self.network),
            (
                CredentialExposureSurface::LocalPersistence,
                self.local_persistence,
            ),
            (CredentialExposureSurface::TerminalLog, self.terminal_log),
            (
                CredentialExposureSurface::ProcessPropagation,
                self.process_propagation,
            ),
        ] {
            if let Some(level) = level {
                policy
                    .surfaces
                    .iter_mut()
                    .find(|entry| entry.surface == surface)
                    .expect("default policy contains every surface")
                    .max_level = level;
            }
        }
        let mut allowlist = Vec::new();
        for domain in &self.network_domains {
            validate_domain(domain)?;
            allowlist.push(NetworkDestinationRule::ExactDomain {
                domain: domain.trim().to_ascii_lowercase(),
            });
        }
        for domain in &self.network_subdomains {
            validate_domain(domain)?;
            allowlist.push(NetworkDestinationRule::SubdomainsOf {
                domain: domain.trim().to_ascii_lowercase(),
                include_apex: false,
            });
        }
        for pattern in &self.network_regexes {
            regex::Regex::new(pattern)
                .with_context(|| format!("invalid --network-regex {pattern:?}"))?;
            allowlist.push(NetworkDestinationRule::Regex {
                pattern: pattern.clone(),
            });
        }
        if !allowlist.is_empty() {
            let network = policy
                .surfaces
                .iter_mut()
                .find(|entry| entry.surface == CredentialExposureSurface::Network)
                .expect("network surface");
            network.network_allowlist = allowlist;
            if self.network.is_none() {
                network.max_level = 1;
            }
        }
        for raw in &self.notes {
            let (surface, note) = raw
                .split_once('=')
                .ok_or_else(|| anyhow::anyhow!("--exposure-note must be SURFACE=TEXT"))?;
            let surface = parse_exposure_surface(surface)?;
            let note = note.trim();
            if note.is_empty() {
                bail!("--exposure-note text cannot be empty");
            }
            policy
                .surfaces
                .iter_mut()
                .find(|entry| entry.surface == surface)
                .expect("surface exists")
                .note = Some(note.to_string());
        }
        policy.validate().map_err(anyhow::Error::msg)?;
        Ok(policy)
    }
}

fn parse_exposure_surface(value: &str) -> Result<CredentialExposureSurface> {
    match value.trim().replace('-', "_").as_str() {
        "llm" | "llm_context" => Ok(CredentialExposureSurface::LlmContext),
        "network" => Ok(CredentialExposureSurface::Network),
        "local" | "local_persistence" => Ok(CredentialExposureSurface::LocalPersistence),
        "terminal" | "terminal_log" => Ok(CredentialExposureSurface::TerminalLog),
        "process" | "process_propagation" => Ok(CredentialExposureSurface::ProcessPropagation),
        other => bail!("unknown exposure surface {other:?}"),
    }
}

fn validate_domain(value: &str) -> Result<()> {
    let value = value.trim();
    if value.is_empty()
        || value.contains('/')
        || value.contains(':')
        || value.split('.').any(|part| {
            part.is_empty()
                || part.starts_with('-')
                || part.ends_with('-')
                || !part
                    .chars()
                    .all(|character| character.is_ascii_alphanumeric() || character == '-')
        })
    {
        bail!("invalid network domain {value:?}; use a hostname without scheme, port, or path");
    }
    Ok(())
}

#[derive(Debug, Subcommand)]
enum Commands {
    #[command(
        visible_alias = "request",
        about = "Request one provider-neutral resource; text output prints only its approved value"
    )]
    Get(GetArgs),
    #[command(about = "List provider-neutral password fields without revealing values")]
    List(ListArgs),
    #[command(
        about = "Fuzzy-search names, aliases, notes, tags, field keys, labels, sections, and metadata"
    )]
    Search(SearchArgs),
    #[command(
        about = "Create password drafts or stage metadata-only catalog changes for human confirmation"
    )]
    Password(PasswordArgs),
    #[command(about = "Inspect or install Plankton's bundled agent skill")]
    Skill(SkillArgs),
}

#[derive(Debug, Args)]
struct SkillArgs {
    #[command(subcommand)]
    command: Option<SkillCommand>,
}

#[derive(Debug, Subcommand)]
enum SkillCommand {
    #[command(about = "Install the bundled secret-access skill for explicit agent targets")]
    Install(SkillInstallArgs),
}

#[derive(Debug, Args)]
struct SkillInstallArgs {
    #[arg(
        long = "agent",
        value_name = "AGENT",
        required = true,
        help = "Target one Vercel Skills agent ID; repeatable, or use '*' for every agent"
    )]
    agents: Vec<String>,
}

#[derive(Debug, Args)]
#[command(group(
    ArgGroup::new("resource_input")
        .required(true)
        .args(["resource", "resource_flag"]),
))]
struct GetArgs {
    #[arg(value_name = "RESOURCE", group = "resource_input")]
    resource: Option<String>,
    #[arg(long = "resource", hide = true, group = "resource_input")]
    resource_flag: Option<String>,
    #[arg(long, help = "Why access is needed")]
    reason: String,
    #[arg(long, help = "Requester identity; defaults to the OS user")]
    requested_by: Option<String>,
    #[arg(
        long = "env",
        value_name = "KEY=VALUE",
        help = "Repeat to attach explicit environment context"
    )]
    env_vars: Vec<String>,
    #[arg(
        long = "metadata",
        value_name = "KEY=VALUE",
        help = "Repeat to attach request metadata"
    )]
    metadata: Vec<String>,
}

#[derive(Debug, Args)]
struct ListArgs {}

#[derive(Debug, Args)]
struct SearchArgs {
    #[arg(value_name = "QUERY")]
    query: String,
    #[arg(
        long = "tag",
        value_name = "TAG",
        help = "Require every tag; repeatable"
    )]
    tag_all: Vec<String>,
    #[arg(
        long = "any-tag",
        value_name = "TAG",
        help = "Require at least one matching tag; repeatable"
    )]
    tag_any: Vec<String>,
    #[arg(long, value_name = "KEY", help = "Fuzzy-filter field keys")]
    field_key: Option<String>,
    #[arg(long, value_name = "TEXT", help = "Fuzzy-filter notes")]
    notes: Option<String>,
    #[arg(
        long,
        default_value_t = 50,
        value_parser = clap::value_parser!(u16).range(1..=200)
    )]
    limit: u16,
    #[arg(long, help = "Continue a prior search page")]
    cursor: Option<String>,
}

#[derive(Debug, Args)]
#[command(
    after_help = "Management commands never return password values. Reuse --change-id to aggregate multiple operations into one cumulative diff. Calls made after confirmation automatically continue under a successor change ID in the same batch."
)]
struct PasswordArgs {
    #[command(subcommand)]
    command: PasswordCommand,
}

#[derive(Debug, Subcommand)]
enum PasswordCommand {
    #[command(about = "Open one aggregated desktop popup for a title and empty password fields")]
    Create(PasswordCreateArgs),
    #[command(about = "Open a human destination and confirmation popup for selected values")]
    Add(PasswordAddArgs),
    #[command(about = "Open desktop confirmation for a verified cross-vault copy or move")]
    Migrate(PasswordMigrateArgs),
    #[command(about = "Open desktop management for creating or deleting local encrypted vaults")]
    Vault,
    #[command(about = "Edit item metadata, or open the human editor for password values")]
    Edit(PasswordEditArgs),
    #[command(about = "Stage a resource key rename without reading its password value")]
    RenameField(PasswordRenameFieldArgs),
    #[command(about = "Stage a field display-label change without reading its password value")]
    EditField(PasswordEditFieldArgs),
    #[command(about = "Stage moving one field into a new or existing logical password item")]
    MoveField(PasswordMoveFieldArgs),
    #[command(about = "Stage merging every field from one password item into another")]
    Merge(PasswordMergeArgs),
    #[command(
        about = "Stage deletion only when two existing stored fields match; no value is returned"
    )]
    DedupeField(PasswordDedupeFieldArgs),
    #[command(about = "Stage an upstream snapshot refresh; no value is returned to the CLI")]
    Refresh(PasswordRefreshArgs),
    #[command(about = "Stage deletion of one logical item for desktop confirmation")]
    Delete(PasswordDeleteArgs),
    #[command(about = "Inspect a staged password change and optionally wait for its outcome")]
    Change(PasswordChangeArgs),
}

#[derive(Debug, Args)]
struct PasswordCreateArgs {
    #[arg(long, value_name = "TITLE", help = "Pre-fill the editable item title")]
    title: String,
    #[arg(
        long = "key",
        value_name = "KEY",
        required = true,
        help = "Create an empty password field for the human to fill; repeatable"
    )]
    keys: Vec<String>,
    #[arg(
        long,
        default_value = "plankton",
        value_name = "BACKEND_ID",
        help = "Preselect Plankton or an enabled external connection"
    )]
    backend: String,
    #[arg(
        long,
        default_value = "default",
        value_name = "VAULT_ID",
        help = "Preselect the destination vault"
    )]
    vault: String,
    #[arg(
        long,
        help = "Wait for the human save and return only the created resource IDs"
    )]
    wait: bool,
    #[arg(
        long,
        default_value_t = 900,
        value_name = "SECONDS",
        help = "Maximum wait for --wait"
    )]
    timeout: u64,
    #[command(flatten)]
    exposure: ExposurePolicyArgs,
}

#[derive(Debug, Args)]
#[command(group(
    ArgGroup::new("item_edit")
        .required(true)
        .multiple(true)
        .args(["human", "next_item_id", "title", "description", "clear_description", "tags", "clear_tags", "metadata"]),
))]
struct PasswordEditArgs {
    #[arg(
        value_name = "ITEM_ID",
        help = "Current user-facing item ID or internal record ID"
    )]
    item_id: String,
    #[arg(
        long,
        conflicts_with_all = ["next_item_id", "title", "description", "clear_description", "tags", "clear_tags", "metadata"],
        help = "Open the desktop human editor; password values never enter CLI arguments or output"
    )]
    human: bool,
    #[arg(
        long = "item-id",
        value_name = "NEW_ID",
        help = "Set a new user-facing item ID"
    )]
    next_item_id: Option<String>,
    #[arg(long, value_name = "TITLE", help = "Set the display title")]
    title: Option<String>,
    #[arg(long, value_name = "TEXT", help = "Set the item description or notes")]
    description: Option<String>,
    #[arg(
        long,
        conflicts_with = "description",
        help = "Remove the item description"
    )]
    clear_description: bool,
    #[arg(
        long = "tag",
        value_name = "TAG",
        help = "Replace item tags; repeatable"
    )]
    tags: Vec<String>,
    #[arg(long, conflicts_with = "tags", help = "Remove every item tag")]
    clear_tags: bool,
    #[arg(
        long = "metadata",
        value_name = "KEY=VALUE",
        help = "Replace custom metadata; repeatable"
    )]
    metadata: Vec<String>,
    #[command(flatten)]
    change: PasswordChangeSubmitArgs,
}

#[derive(Debug, Args)]
struct PasswordRenameFieldArgs {
    #[arg(
        value_name = "RESOURCE_KEY",
        help = "Current provider-neutral resource key"
    )]
    resource_id: String,
    #[arg(
        long = "to",
        value_name = "NEW_RESOURCE_KEY",
        help = "Replacement resource key"
    )]
    next_resource_id: String,
    #[command(flatten)]
    change: PasswordChangeSubmitArgs,
}

#[derive(Debug, Args)]
struct PasswordEditFieldArgs {
    #[arg(
        value_name = "RESOURCE_KEY",
        help = "Current provider-neutral resource key"
    )]
    resource_id: String,
    #[arg(
        long,
        value_name = "LABEL",
        help = "Replacement human-readable field label"
    )]
    label: Option<String>,
    #[command(flatten)]
    exposure: ExposurePolicyArgs,
    #[command(flatten)]
    change: PasswordChangeSubmitArgs,
}

#[derive(Debug, Args)]
struct PasswordMoveFieldArgs {
    #[arg(value_name = "RESOURCE_KEY", help = "Field resource key to move")]
    resource_id: String,
    #[arg(
        long = "to-item",
        value_name = "ITEM_ID",
        help = "Existing target item, or the ID of a new item"
    )]
    target_item_id: String,
    #[arg(
        long = "title",
        value_name = "TITLE",
        help = "Required display title when --to-item creates a new item"
    )]
    target_title: Option<String>,
    #[command(flatten)]
    change: PasswordChangeSubmitArgs,
}

#[derive(Debug, Args)]
struct PasswordMergeArgs {
    #[arg(value_name = "SOURCE_ITEM", help = "Logical item to merge")]
    source_item_id: String,
    #[arg(
        long = "into",
        value_name = "TARGET_ITEM",
        help = "Existing logical item that receives every source field"
    )]
    target_item_id: String,
    #[command(flatten)]
    change: PasswordChangeSubmitArgs,
}

#[derive(Debug, Args)]
struct PasswordDedupeFieldArgs {
    #[arg(
        value_name = "DUPLICATE_RESOURCE",
        help = "Duplicate field resource to remove after a trusted local equality check"
    )]
    resource_id: String,
    #[arg(
        long = "keep",
        value_name = "CANONICAL_RESOURCE",
        help = "Existing canonical field resource that must have the same stored value"
    )]
    canonical_resource_id: String,
    #[command(flatten)]
    change: PasswordChangeSubmitArgs,
}

#[derive(Debug, Args)]
struct PasswordRefreshArgs {
    #[arg(
        value_name = "ITEM_ID",
        help = "Item whose retained locator should be refreshed"
    )]
    item_id: String,
    #[command(flatten)]
    change: PasswordChangeSubmitArgs,
}

#[derive(Debug, Args)]
struct PasswordDeleteArgs {
    #[arg(value_name = "ITEM_ID", help = "Logical catalog item to delete")]
    item_id: String,
    #[command(flatten)]
    change: PasswordChangeSubmitArgs,
}

#[derive(Debug, Args)]
struct PasswordChangeSubmitArgs {
    #[arg(
        long,
        value_name = "CHANGE_ID",
        help = "Append to an existing change; confirmed IDs roll over automatically"
    )]
    change_id: Option<String>,
    #[arg(
        long,
        value_name = "BATCH_ID",
        help = "Append to the pending change for the same batch and requester, or start that batch"
    )]
    batch_id: Option<String>,
    #[arg(
        long,
        value_name = "TEXT",
        help = "Required for the first operation in a change batch"
    )]
    reason: Option<String>,
    #[arg(
        long,
        value_name = "ACTOR",
        help = "Requester identity; defaults to the OS user"
    )]
    requested_by: Option<String>,
    #[arg(
        long,
        value_enum,
        default_value = "async",
        help = "Return after staging, or wait for confirmation and commit"
    )]
    commit: PasswordChangeCommitMode,
    #[arg(
        long,
        default_value_t = 300,
        value_name = "SECONDS",
        help = "Maximum wait for --commit sync"
    )]
    timeout: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
enum PasswordChangeCommitMode {
    Async,
    Sync,
}

#[derive(Debug, Args)]
struct PasswordChangeArgs {
    #[arg(
        value_name = "CHANGE_ID",
        help = "Change ID returned by a management command"
    )]
    change_id: String,
    #[arg(long, help = "Wait until the change reaches a terminal state")]
    wait: bool,
    #[arg(
        long,
        default_value_t = 300,
        value_name = "SECONDS",
        help = "Maximum wait when --wait is used"
    )]
    timeout: u64,
}

#[derive(Debug, Args)]
#[command(group(
    ArgGroup::new("password_source")
        .required(true)
        .multiple(false)
        .args(["env_vars", "file", "onepassword"]),
))]
struct PasswordAddArgs {
    #[arg(long, visible_alias = "1password", value_name = "[KEY=]op://VAULT/ITEM/FIELD", value_parser = onepassword_import::parse_selection, help = "Import a selected 1Password field into a human-confirmed draft; repeatable")]
    onepassword: Vec<plankton_protocol::passwords::OnePasswordFieldReference>,
    #[arg(
        long,
        requires = "onepassword",
        conflicts_with_all = ["env_vars", "file"],
        value_name = "ACCOUNT",
        help = "1Password source account; separate from the destination --backend and --vault"
    )]
    onepassword_account: Option<String>,

    #[arg(
        long = "env",
        value_name = "NAME",
        help = "Explicit environment variable name; repeatable"
    )]
    env_vars: Vec<String>,
    #[arg(long, value_name = "PATH", help = "A .env, JSON, YAML, or YML file")]
    file: Option<PathBuf>,
    #[arg(
        long,
        value_enum,
        default_value = "auto",
        requires = "file",
        help = "Override file format detection"
    )]
    format: PasswordFileFormat,
    #[arg(
        long = "key",
        value_name = "PATH",
        requires = "file",
        help = "Select a dotted field path; repeatable"
    )]
    keys: Vec<String>,
    #[arg(
        long,
        value_name = "TITLE",
        help = "Suggest an item title for desktop confirmation; it remains editable before saving"
    )]
    title: Option<String>,
    #[arg(
        long,
        default_value = "plankton",
        value_name = "BACKEND_ID",
        help = "Preselect Plankton or an enabled external connection for desktop confirmation"
    )]
    backend: String,
    #[arg(
        long,
        default_value = "default",
        value_name = "VAULT_ID",
        help = "Preselect the destination vault for desktop confirmation"
    )]
    vault: String,
    #[command(flatten)]
    exposure: ExposurePolicyArgs,
}

#[derive(Debug, Args)]
struct PasswordMigrateArgs {
    #[arg(
        value_name = "ITEM_ID",
        help = "User-facing item ID or internal record ID"
    )]
    item_id: String,
    #[arg(
        long,
        default_value = "plankton",
        value_name = "BACKEND_ID",
        help = "Plankton or an enabled external connection"
    )]
    backend: String,
    #[arg(long, value_name = "VAULT_ID", help = "Destination vault ID")]
    vault: String,
    #[arg(
        long,
        help = "Remove the source only after the destination is written and verified"
    )]
    move_source: bool,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum PasswordFileFormat {
    Auto,
    Dotenv,
    Json,
    Yaml,
}

impl From<PasswordFileFormat> for FileFormat {
    fn from(value: PasswordFileFormat) -> Self {
        match value {
            PasswordFileFormat::Auto => Self::Auto,
            PasswordFileFormat::Dotenv => Self::Dotenv,
            PasswordFileFormat::Json => Self::Json,
            PasswordFileFormat::Yaml => Self::Yaml,
        }
    }
}

#[derive(Debug, Serialize)]
struct PasswordDraftOutput {
    draft_id: String,
    keys: Vec<String>,
    status: &'static str,
    expires_at: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    resource_ids: Vec<String>,
}

#[derive(Debug, Serialize)]
struct PasswordMigrationHandoffOutput {
    item_id: String,
    backend: String,
    vault: String,
    mode: &'static str,
    status: &'static str,
}

impl From<PasswordDraftCreated> for PasswordDraftOutput {
    fn from(draft: PasswordDraftCreated) -> Self {
        Self {
            draft_id: draft.draft_id.to_string(),
            keys: draft.keys,
            status: "awaiting_human_confirmation",
            expires_at: draft.expires_at.to_rfc3339(),
            resource_ids: Vec::new(),
        }
    }
}

#[tokio::main]
async fn main() -> ExitCode {
    init_tracing();
    match run().await {
        Ok(code) => code,
        Err(error) => {
            eprintln!("error: {error}");
            for cause in error.chain().skip(1) {
                eprintln!("caused by: {cause}");
            }
            ExitCode::FAILURE
        }
    }
}

async fn run() -> Result<ExitCode> {
    let Cli { output, command } = Cli::parse();
    match command {
        Commands::Get(args) => run_get(output.unwrap_or(OutputFormat::Text), args).await,
        Commands::List(_) => {
            let client = connect().await?;
            let items = search_all_resources(&client).await?;
            print_search_items(output.unwrap_or(OutputFormat::Jsonl), &items)?;
            Ok(ExitCode::SUCCESS)
        }
        Commands::Search(args) => {
            let client = connect().await?;
            let result = client
                .search_resources(ResourceSearchRequest {
                    query: args.query,
                    tag_all: args.tag_all,
                    tag_any: args.tag_any,
                    field_key: args.field_key,
                    notes: args.notes,
                    limit: args.limit,
                    cursor: args.cursor,
                })
                .await
                .context("resource search failed")?;
            print_search_items(output.unwrap_or(OutputFormat::Jsonl), &result.items)?;
            if let Some(cursor) = result.next_cursor {
                eprintln!("next_cursor: {cursor}");
            }
            Ok(ExitCode::SUCCESS)
        }
        Commands::Password(args) => run_password(output.unwrap_or(OutputFormat::Text), args).await,
        Commands::Skill(args) => match args.command {
            None => {
                print_bundled_skill(output.unwrap_or(OutputFormat::Text))?;
                Ok(ExitCode::SUCCESS)
            }
            Some(SkillCommand::Install(args)) => {
                if output.is_some_and(|format| format != OutputFormat::Text) {
                    bail!("skill install supports only text output");
                }
                skill_install::install_bundled_skill(&args.agents)?;
                Ok(ExitCode::SUCCESS)
            }
        },
    }
}

fn print_bundled_skill(output: OutputFormat) -> Result<()> {
    let name = skill_install::bundled_skill_name();
    let content = skill_install::bundled_skill_markdown();
    match output {
        OutputFormat::Text => {
            let mut stdout = io::stdout().lock();
            stdout
                .write_all(content.as_bytes())
                .context("failed to write bundled skill to stdout")?;
            stdout
                .flush()
                .context("failed to flush bundled skill output")
        }
        OutputFormat::Json => {
            println!(
                "{}",
                serde_json::to_string_pretty(&serde_json::json!({
                    "name": name,
                    "content": content,
                }))
                .context("failed to serialize bundled skill")?
            );
            Ok(())
        }
        OutputFormat::Jsonl => {
            println!(
                "{}",
                serde_json::to_string(&serde_json::json!({
                    "name": name,
                    "content": content,
                }))
                .context("failed to serialize bundled skill")?
            );
            Ok(())
        }
    }
}

async fn run_get(output: OutputFormat, args: GetArgs) -> Result<ExitCode> {
    let resource_id = args
        .resource
        .or(args.resource_flag)
        .expect("clap requires a resource");
    let call_chain =
        collect_runtime_call_chain().context("failed to collect runtime call chain")?;
    let mut metadata = parse_key_values("metadata", args.metadata)?;
    for (key, value) in parse_key_values("env", args.env_vars)? {
        metadata.insert(format!("env.{key}"), value);
    }
    let script_path = derive_script_path(&call_chain);
    let call_chain_details = resource_access_call_chain(&call_chain);
    let call_chain = prompt_call_chain_paths(&call_chain);

    let client = connect().await?;
    let result = client
        .request_resource_access(ResourceAccessRequest {
            resource_id,
            reason: args.reason,
            requested_by: args.requested_by.unwrap_or_else(default_actor),
            script_path,
            call_chain_details,
            call_chain,
            metadata,
        })
        .await
        .context("failed to submit access request")?;
    let result = poll_access_until_resolved(
        result,
        |request_id| client.resource_access_status(request_id),
        sleep,
        trigger_request_handoff,
    )
    .await?;
    handle_get_result(output, result)
}

fn resource_access_call_chain(call_chain: &[CallChainNode]) -> Vec<ResourceAccessCallChainNode> {
    call_chain
        .iter()
        .map(|node| ResourceAccessCallChainNode {
            pid: node.pid,
            ppid: node.ppid,
            process_name: node.process_name.clone(),
            executable_path: node.executable_path.clone(),
            argv: node.argv.clone(),
            resolved_file_path: node.resolved_file_path.clone(),
        })
        .collect()
}

async fn poll_access_until_resolved<Poll, PollFuture, Wait, WaitFuture, Handoff>(
    mut result: ResourceAccessResponse,
    mut poll: Poll,
    mut wait: Wait,
    mut handoff: Handoff,
) -> Result<ResourceAccessResponse>
where
    Poll: FnMut(String) -> PollFuture,
    PollFuture: Future<Output = Result<ResourceAccessResponse, ClientError>>,
    Wait: FnMut(Duration) -> WaitFuture,
    WaitFuture: Future<Output = ()>,
    Handoff: FnMut(String) -> Result<()>,
{
    let mut previous_human_review_required = false;
    let mut handoff_dispatched = false;
    loop {
        if !handoff_dispatched && !previous_human_review_required && result.human_review_required {
            handoff(result.request_id.clone())?;
            handoff_dispatched = true;
        }
        previous_human_review_required = result.human_review_required;

        if result.state != ResourceAccessState::Pending {
            return Ok(result);
        }

        wait(GET_POLL_INTERVAL).await;
        let request_id = result.request_id.clone();
        result = retry_transient_status(|| poll(request_id.clone()), &mut wait)
            .await
            .with_context(|| format!("failed to query request {}", result.request_id))?;
    }
}

async fn retry_transient_status<T, Poll, PollFuture, Wait, WaitFuture>(
    mut poll: Poll,
    mut wait: Wait,
) -> Result<T, ClientError>
where
    Poll: FnMut() -> PollFuture,
    PollFuture: Future<Output = Result<T, ClientError>>,
    Wait: FnMut(Duration) -> WaitFuture,
    WaitFuture: Future<Output = ()>,
{
    let mut retries = 0_u8;
    loop {
        match poll().await {
            Ok(value) => return Ok(value),
            Err(ClientError::Unavailable(_)) if retries < STATUS_POLL_RETRY_LIMIT => {
                let delay = Duration::from_millis(100_u64 << retries);
                retries += 1;
                wait(delay).await;
            }
            Err(error) => return Err(error),
        }
    }
}

async fn run_password(output: OutputFormat, args: PasswordArgs) -> Result<ExitCode> {
    match args.command {
        PasswordCommand::Create(args) => {
            let input = manual_password_draft_input(&args)?;
            let client = connect().await?;
            let draft = client
                .create_password_draft(input)
                .await
                .context("failed to create empty password draft")?;
            trigger_password_draft_handoff(draft.draft_id.to_string())?;
            let mut view = PasswordDraftOutput::from(draft.clone());
            view.status = "awaiting_human_input";
            if args.wait {
                let status = wait_for_password_draft(&client, draft.draft_id, args.timeout).await?;
                view.status = "saved";
                view.resource_ids = status.resource_ids;
            }
            println!("{}", format_password_draft_output(output, &view)?);
            Ok(ExitCode::SUCCESS)
        }
        PasswordCommand::Add(args) => {
            let mut input = password_draft_input(args)?;
            if let PasswordSourceDescriptor::OnePassword { account, fields } = &input.descriptor {
                input.entries = onepassword_import::read_fields(fields, account.as_deref()).await?;
            }
            let draft = connect()
                .await?
                .create_password_draft(input)
                .await
                .context("failed to create password draft")?;
            trigger_password_draft_handoff(draft.draft_id.to_string())?;
            let view = PasswordDraftOutput::from(draft);
            println!("{}", format_password_draft_output(output, &view)?);
            Ok(ExitCode::SUCCESS)
        }
        PasswordCommand::Migrate(args) => {
            let item_id = args.item_id.trim().to_string();
            let backend = args.backend.trim().to_string();
            let vault = args.vault.trim().to_string();
            if item_id.is_empty() || backend.is_empty() || vault.is_empty() {
                bail!("item ID, --backend, and --vault cannot be empty");
            }
            let mode = if args.move_source { "move" } else { "copy" };
            trigger_password_migration_handoff(&item_id, &backend, &vault, mode)?;
            let receipt = PasswordMigrationHandoffOutput {
                item_id,
                backend,
                vault,
                mode,
                status: "awaiting_human_confirmation",
            };
            println!(
                "{}",
                match output {
                    OutputFormat::Text => format!(
                        "item_id: {}\ndestination: {}:{}\nmode: {}\nstatus: {}",
                        receipt.item_id,
                        receipt.backend,
                        receipt.vault,
                        receipt.mode,
                        receipt.status
                    ),
                    OutputFormat::Json => serde_json::to_string_pretty(&receipt)
                        .context("failed to serialize migration handoff")?,
                    OutputFormat::Jsonl => serde_json::to_string(&receipt)
                        .context("failed to serialize migration handoff")?,
                }
            );
            Ok(ExitCode::SUCCESS)
        }
        PasswordCommand::Vault => {
            trigger_local_vault_manager_handoff()?;
            println!(
                "{}",
                match output {
                    OutputFormat::Text => "status: awaiting_human_confirmation".to_string(),
                    OutputFormat::Json => serde_json::to_string_pretty(&serde_json::json!({
                        "status": "awaiting_human_confirmation",
                        "surface": "local_vault_manager"
                    }))
                    .context("failed to serialize vault handoff")?,
                    OutputFormat::Jsonl => serde_json::to_string(&serde_json::json!({
                        "status": "awaiting_human_confirmation",
                        "surface": "local_vault_manager"
                    }))
                    .context("failed to serialize vault handoff")?,
                }
            );
            Ok(ExitCode::SUCCESS)
        }
        PasswordCommand::Edit(args) => {
            if args.human {
                let item_id = args.item_id.trim();
                if item_id.is_empty() {
                    bail!("item ID cannot be empty");
                }
                trigger_password_edit_handoff(item_id)?;
                let receipt = serde_json::json!({
                    "item_id": item_id,
                    "status": "awaiting_human_input",
                    "surface": "password_editor"
                });
                println!(
                    "{}",
                    match output {
                        OutputFormat::Text => format!(
                            "item_id: {item_id}\nstatus: awaiting_human_input\nsurface: password_editor"
                        ),
                        OutputFormat::Json => serde_json::to_string_pretty(&receipt)
                            .context("failed to serialize password edit handoff")?,
                        OutputFormat::Jsonl => serde_json::to_string(&receipt)
                            .context("failed to serialize password edit handoff")?,
                    }
                );
                return Ok(ExitCode::SUCCESS);
            }
            let tags = if args.clear_tags {
                Some(Vec::new())
            } else if args.tags.is_empty() {
                None
            } else {
                Some(args.tags)
            };
            let metadata = if args.metadata.is_empty() {
                None
            } else {
                Some(parse_key_values("metadata", args.metadata)?)
            };
            submit_password_change(
                output,
                args.change,
                PasswordChangeOperation::UpdateItem {
                    item_id: args.item_id,
                    next_item_id: args.next_item_id,
                    title: args.title,
                    description: args.description,
                    clear_description: args.clear_description,
                    tags,
                    metadata,
                },
            )
            .await
        }
        PasswordCommand::RenameField(args) => {
            submit_password_change(
                output,
                args.change,
                PasswordChangeOperation::RenameResource {
                    resource_id: args.resource_id,
                    next_resource_id: args.next_resource_id,
                },
            )
            .await
        }
        PasswordCommand::EditField(args) => {
            if args.label.is_none() && !args.exposure.is_specified() {
                bail!("edit-field requires --label or at least one exposure option");
            }
            let exposure_policy = args
                .exposure
                .is_specified()
                .then(|| args.exposure.build())
                .transpose()?;
            submit_password_change(
                output,
                args.change,
                PasswordChangeOperation::UpdateField {
                    resource_id: args.resource_id,
                    label: args.label,
                    exposure_policy,
                },
            )
            .await
        }
        PasswordCommand::MoveField(args) => {
            submit_password_change(
                output,
                args.change,
                PasswordChangeOperation::MoveField {
                    resource_id: args.resource_id,
                    target_item_id: args.target_item_id,
                    target_title: args.target_title,
                },
            )
            .await
        }
        PasswordCommand::Merge(args) => {
            submit_password_change(
                output,
                args.change,
                PasswordChangeOperation::MergeItems {
                    source_item_id: args.source_item_id,
                    target_item_id: args.target_item_id,
                },
            )
            .await
        }
        PasswordCommand::DedupeField(args) => {
            submit_password_change(
                output,
                args.change,
                PasswordChangeOperation::DeleteDuplicateField {
                    resource_id: args.resource_id,
                    canonical_resource_id: args.canonical_resource_id,
                },
            )
            .await
        }
        PasswordCommand::Refresh(args) => {
            submit_password_change(
                output,
                args.change,
                PasswordChangeOperation::RefreshItem {
                    item_id: args.item_id,
                },
            )
            .await
        }
        PasswordCommand::Delete(args) => {
            submit_password_change(
                output,
                args.change,
                PasswordChangeOperation::DeleteItem {
                    item_id: args.item_id,
                },
            )
            .await
        }
        PasswordCommand::Change(args) => {
            let client = connect().await?;
            let status = if args.wait {
                wait_for_password_change(&client, args.change_id, Duration::from_secs(args.timeout))
                    .await?
            } else {
                client
                    .password_change_status(args.change_id)
                    .await
                    .context("failed to read password change status")?
            };
            print_password_change(output, &status)?;
            Ok(exit_for_password_change(status.state))
        }
    }
}

async fn submit_password_change(
    output: OutputFormat,
    args: PasswordChangeSubmitArgs,
    operation: PasswordChangeOperation,
) -> Result<ExitCode> {
    if args.change_id.is_none()
        && args
            .reason
            .as_deref()
            .map(str::trim)
            .is_none_or(str::is_empty)
    {
        bail!("--reason is required when starting a password change");
    }
    let client = connect().await?;
    let response = client
        .submit_password_change(SubmitPasswordChangeRequest {
            change_id: args.change_id,
            batch_id: args.batch_id,
            reason: args.reason,
            requested_by: args.requested_by.unwrap_or_else(default_actor),
            operation_id: uuid::Uuid::new_v4().to_string(),
            operation,
        })
        .await
        .context("failed to stage password change")?;
    trigger_password_change_handoff(response.effective_change_id.clone())?;
    let status = match args.commit {
        PasswordChangeCommitMode::Async => response.status,
        PasswordChangeCommitMode::Sync => {
            wait_for_password_change(
                &client,
                response.effective_change_id,
                Duration::from_secs(args.timeout),
            )
            .await?
        }
    };
    print_password_change(output, &status)?;
    Ok(exit_for_password_change(status.state))
}

async fn wait_for_password_change(
    client: &DaemonClient,
    change_id: String,
    timeout: Duration,
) -> Result<PasswordChangeStatus> {
    let started = Instant::now();
    loop {
        let status = client
            .password_change_status(change_id.clone())
            .await
            .context("failed to read password change status")?;
        if status.state.is_terminal() {
            return Ok(status);
        }
        if started.elapsed() >= timeout {
            bail!("timed out waiting for password change {change_id}");
        }
        sleep(GET_POLL_INTERVAL).await;
    }
}

fn print_password_change(output: OutputFormat, status: &PasswordChangeStatus) -> Result<()> {
    print_value(output, status, || {
        let successor = status
            .successor_change_id
            .as_deref()
            .map(|id| format!("\nsuccessor_change_id: {id}"))
            .unwrap_or_default();
        format!(
            "change_id: {}\nbatch_id: {}\nstatus: {:?}\nversion: {}\nchanged_items: {}\nchanged_fields: {}{}",
            status.change_id,
            status.batch_id,
            status.state,
            status.version,
            status.diff.changed_items,
            status.diff.changed_fields,
            successor
        )
    })
}

fn exit_for_password_change(state: PasswordChangeState) -> ExitCode {
    match state {
        PasswordChangeState::Rejected
        | PasswordChangeState::Conflict
        | PasswordChangeState::Failed => ExitCode::FAILURE,
        PasswordChangeState::PendingConfirmation
        | PasswordChangeState::Confirmed
        | PasswordChangeState::Committing
        | PasswordChangeState::Committed => ExitCode::SUCCESS,
    }
}

fn password_draft_input(args: PasswordAddArgs) -> Result<PasswordDraftInput> {
    let suggested_layout = if args.exposure.is_specified() {
        Some(PasswordDraftLayoutSuggestion {
            default_exposure_policy: Some(args.exposure.build()?),
            ..Default::default()
        })
    } else {
        None
    };
    let suggested_item_title = args.title;
    let backend = args.backend.trim();
    let vault = args.vault.trim();
    if backend.is_empty() || vault.is_empty() {
        bail!("--backend and --vault cannot be empty");
    }
    let suggested_destination = Some(if backend == "plankton" {
        PasswordDestination::Plankton {
            vault_id: vault.to_string(),
        }
    } else {
        PasswordDestination::External {
            binding_id: backend.to_string(),
            vault_id: vault.to_string(),
        }
    });
    if !args.onepassword.is_empty() {
        onepassword_import::validate_selection(
            &args.onepassword,
            args.onepassword_account.as_deref(),
        )?;
        return Ok(PasswordDraftInput {
            suggested_item_title: suggested_item_title
                .or_else(|| Some(onepassword_import::suggested_title(&args.onepassword))),
            descriptor: PasswordSourceDescriptor::OnePassword {
                account: args.onepassword_account,
                fields: args.onepassword,
            },
            entries: Vec::new(),
            suggested_destination,
            suggested_layout,
        });
    }
    match args.file {
        Some(path) => {
            let descriptor = PasswordSourceDescriptor::File {
                path,
                format: args.format.into(),
                keys: args.keys,
            };
            let parsed = parse_password_source_descriptor(descriptor.clone())
                .context("failed to read selected password file")?;
            Ok(PasswordDraftInput {
                descriptor,
                entries: parsed
                    .entries
                    .into_iter()
                    .map(|entry| SelectedPasswordEntry {
                        key: entry.key,
                        value: entry.value,
                    })
                    .collect(),
                suggested_item_title,
                suggested_destination,
                suggested_layout,
            })
        }
        None => {
            let entries = args
                .env_vars
                .into_iter()
                .map(|name| {
                    let value = match env::var(&name) {
                        Ok(value) => value,
                        Err(env::VarError::NotPresent) => {
                            bail!("environment variable {name} is not set")
                        }
                        Err(env::VarError::NotUnicode(_)) => {
                            bail!("environment variable {name} is not valid Unicode")
                        }
                    };
                    Ok(SelectedPasswordEntry { key: name, value })
                })
                .collect::<Result<Vec<_>>>()?;
            let descriptor = PasswordSourceDescriptor::Environment {
                names: entries.iter().map(|entry| entry.key.clone()).collect(),
            };
            Ok(PasswordDraftInput {
                descriptor,
                entries,
                suggested_item_title,
                suggested_destination,
                suggested_layout,
            })
        }
    }
}

fn manual_password_draft_input(args: &PasswordCreateArgs) -> Result<PasswordDraftInput> {
    let title = args.title.trim();
    let backend = args.backend.trim();
    let vault = args.vault.trim();
    if title.is_empty() {
        bail!("--title cannot be empty");
    }
    if backend.is_empty() || vault.is_empty() {
        bail!("--backend and --vault cannot be empty");
    }
    let destination = if backend == "plankton" {
        PasswordDestination::Plankton {
            vault_id: vault.to_string(),
        }
    } else {
        PasswordDestination::External {
            binding_id: backend.to_string(),
            vault_id: vault.to_string(),
        }
    };
    Ok(PasswordDraftInput {
        descriptor: PasswordSourceDescriptor::Manual {
            keys: args.keys.clone(),
        },
        entries: Vec::new(),
        suggested_item_title: Some(title.to_string()),
        suggested_destination: Some(destination),
        suggested_layout: Some(PasswordDraftLayoutSuggestion {
            default_exposure_policy: Some(args.exposure.build()?),
            ..PasswordDraftLayoutSuggestion::default()
        }),
    })
}

async fn wait_for_password_draft(
    client: &DaemonClient,
    draft_id: uuid::Uuid,
    timeout_seconds: u64,
) -> Result<PasswordDraftStatus> {
    let deadline = Instant::now() + Duration::from_secs(timeout_seconds);
    loop {
        let status = retry_transient_status(
            || client.password_draft_status(draft_id),
            |duration| sleep(duration),
        )
        .await
        .with_context(|| format!("failed to query password draft {draft_id}"))?;
        match status.state {
            PasswordDraftState::Committed => return Ok(status),
            PasswordDraftState::PendingHumanInput | PasswordDraftState::Committing => {}
        }
        if Instant::now() >= deadline {
            bail!("timed out waiting for password draft {draft_id} to be saved");
        }
        sleep(GET_POLL_INTERVAL).await;
    }
}

fn format_password_draft_output(
    output: OutputFormat,
    view: &PasswordDraftOutput,
) -> Result<String> {
    match output {
        OutputFormat::Text => Ok(format!(
            "draft_id: {}\nfields: {}\nstatus: {}{}",
            view.draft_id,
            view.keys.join(", "),
            view.status,
            if view.resource_ids.is_empty() {
                String::new()
            } else {
                format!("\nresources: {}", view.resource_ids.join(", "))
            }
        )),
        OutputFormat::Json => {
            serde_json::to_string_pretty(view).context("failed to serialize CLI output")
        }
        OutputFormat::Jsonl => {
            serde_json::to_string(view).context("failed to serialize CLI output")
        }
    }
}

async fn connect() -> Result<DaemonClient> {
    DaemonClient::connect_default()
        .await
        .context("failed to connect to planktond; start the Plankton app")
}

async fn search_all_resources(client: &DaemonClient) -> Result<Vec<ResourceSearchItem>> {
    let mut items = Vec::new();
    let mut cursor = None;
    loop {
        let result = client
            .search_resources(ResourceSearchRequest {
                query: String::new(),
                tag_all: Vec::new(),
                tag_any: Vec::new(),
                field_key: None,
                notes: None,
                limit: 200,
                cursor,
            })
            .await
            .context("failed to list resources")?;
        items.extend(result.items);
        match result.next_cursor {
            Some(next) => cursor = Some(next),
            None => return Ok(items),
        }
    }
}

fn handle_get_result(output: OutputFormat, result: ResourceAccessResponse) -> Result<ExitCode> {
    match result.state {
        ResourceAccessState::Approved => {
            let value = result
                .value
                .as_deref()
                .context("daemon approved access without returning a value")?;
            match output {
                OutputFormat::Text => print_raw_value(value)?,
                OutputFormat::Json | OutputFormat::Jsonl => {
                    print_value(output, &result, String::new)?
                }
            }
            Ok(ExitCode::SUCCESS)
        }
        ResourceAccessState::Denied => {
            let note = result.decision_note.as_deref().unwrap_or("request denied");
            bail!("access denied for {}: {note}", result.resource_id)
        }
        ResourceAccessState::Pending => bail!("access request is still pending"),
    }
}

fn print_search_items(output: OutputFormat, items: &[ResourceSearchItem]) -> Result<()> {
    match output {
        OutputFormat::Text => {
            let rendered = items
                .iter()
                .map(|item| {
                    let mut lines = vec![
                        item.resource_id.clone(),
                        format!("  name: {}", item.display_name),
                        format!("  field: {} ({})", item.field_label, item.field_key),
                    ];
                    if !item.tags.is_empty() {
                        lines.push(format!("  tags: {}", item.tags.join(", ")));
                    }
                    if !item.matched_on.is_empty() {
                        lines.push(format!("  matched_on: {:?}", item.matched_on));
                    }
                    lines.join("\n")
                })
                .collect::<Vec<_>>()
                .join("\n\n");
            println!("{rendered}");
        }
        OutputFormat::Json => println!(
            "{}",
            serde_json::to_string_pretty(items).context("failed to serialize CLI output")?
        ),
        OutputFormat::Jsonl => {
            for item in items {
                println!(
                    "{}",
                    serde_json::to_string(item).context("failed to serialize CLI output")?
                );
            }
        }
    }
    Ok(())
}

fn print_value<T>(
    output: OutputFormat,
    value: &T,
    render_text: impl FnOnce() -> String,
) -> Result<()>
where
    T: Serialize,
{
    match output {
        OutputFormat::Text => println!("{}", render_text()),
        OutputFormat::Json => println!(
            "{}",
            serde_json::to_string_pretty(value).context("failed to serialize CLI output")?
        ),
        OutputFormat::Jsonl => println!(
            "{}",
            serde_json::to_string(value).context("failed to serialize CLI output")?
        ),
    }
    Ok(())
}

fn print_raw_value(value: &str) -> Result<()> {
    let mut stdout = io::stdout().lock();
    stdout
        .write_all(value.as_bytes())
        .context("failed to write secret value to stdout")?;
    stdout
        .write_all(b"\n")
        .context("failed to terminate secret value output")?;
    stdout.flush().context("failed to flush stdout")
}

fn parse_key_values(kind: &str, values: Vec<String>) -> Result<BTreeMap<String, String>> {
    values
        .into_iter()
        .map(|value| {
            let (key, value) = value
                .split_once('=')
                .with_context(|| format!("invalid --{kind} value; expected KEY=VALUE"))?;
            if key.trim().is_empty() {
                bail!("invalid --{kind} value; key cannot be empty");
            }
            Ok((key.to_string(), value.to_string()))
        })
        .collect()
}

fn default_actor() -> String {
    env::var("USER")
        .or_else(|_| env::var("USERNAME"))
        .unwrap_or_else(|_| "unknown".to_string())
}

fn init_tracing() {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    if let Err(error) = fmt().with_env_filter(filter).without_time().try_init() {
        eprintln!("warning: failed to initialize tracing: {error}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resource_access_call_chain_preserves_structured_process_context() {
        let call_chain = vec![CallChainNode {
            pid: Some(42),
            ppid: Some(1),
            process_name: Some("zsh".into()),
            executable_path: Some("/bin/zsh".into()),
            argv: vec!["/workspace/analyze.sh".into(), "--trace-id".into()],
            resolved_file_path: Some("/workspace/analyze.sh".into()),
            source: plankton_core::CallChainNodeSource::OsProbe,
            previewable: true,
            preview_status: plankton_core::CallChainPreviewStatus::PreviewReady,
            preview_text: Some("must not cross the daemon boundary".into()),
            preview_error: None,
        }];

        let transferred = resource_access_call_chain(&call_chain);

        assert_eq!(transferred.len(), 1);
        assert_eq!(transferred[0].pid, Some(42));
        assert_eq!(transferred[0].process_name.as_deref(), Some("zsh"));
        assert_eq!(transferred[0].argv, ["/workspace/analyze.sh", "--trace-id"]);
        assert_eq!(
            transferred[0].resolved_file_path.as_deref(),
            Some("/workspace/analyze.sh")
        );
    }

    #[test]
    fn parses_enhanced_search_filters() {
        let cli = Cli::try_parse_from([
            "plankton",
            "search",
            "prod api",
            "--tag",
            "production",
            "--any-tag",
            "platform",
            "--field-key",
            "api_key",
            "--notes",
            "rotate",
            "--limit",
            "25",
        ])
        .expect("enhanced search should parse");
        assert!(matches!(
            cli.command,
            Commands::Search(SearchArgs {
                query,
                tag_all,
                tag_any,
                field_key: Some(field_key),
                notes: Some(notes),
                limit: 25,
                ..
            }) if query == "prod api"
                && tag_all == ["production"]
                && tag_any == ["platform"]
                && field_key == "api_key"
                && notes == "rotate"
        ));
    }

    #[test]
    fn onepassword_drafts_parse_selected_fields_and_keep_source_account_separate_from_destination()
    {
        let cli = Cli::try_parse_from([
            "plankton",
            "password",
            "add",
            "--onepassword",
            "TOKEN=op://Work/Service/password",
            "--1password",
            "op://Work/Service/username",
            "--onepassword-account",
            "team",
            "--backend",
            "plankton",
            "--vault",
            "work",
            "--network",
            "1",
            "--network-domain",
            "api.example.com",
        ])
        .unwrap();
        let Commands::Password(PasswordArgs {
            command: PasswordCommand::Add(args),
        }) = cli.command
        else {
            panic!("expected add");
        };
        let input = password_draft_input(args).unwrap();
        assert!(input.entries.is_empty());
        assert_eq!(input.suggested_item_title.as_deref(), Some("Service"));
        assert!(
            matches!(input.suggested_destination, Some(PasswordDestination::Plankton { vault_id }) if vault_id == "work")
        );
        assert!(input.suggested_layout.is_some());
        assert!(
            matches!(input.descriptor, PasswordSourceDescriptor::OnePassword { account: Some(account), fields } if account == "team" && fields.len() == 2 && fields[0].key == "TOKEN")
        );
        for source in ["--env", "--file"] {
            assert!(Cli::try_parse_from([
                "plankton",
                "password",
                "add",
                "--onepassword",
                "op://v/i/f",
                source,
                "TOKEN"
            ])
            .is_err());
        }
        assert!(Cli::try_parse_from([
            "plankton",
            "password",
            "add",
            "--env",
            "TOKEN",
            "--onepassword-account",
            "team"
        ])
        .is_err());
    }

    #[test]
    fn password_add_requires_an_explicit_source() {
        assert!(Cli::try_parse_from(["plankton", "password", "add"]).is_err());
        assert!(Cli::try_parse_from([
            "plankton",
            "password",
            "add",
            "--file",
            "/tmp/.env",
            "--env",
            "TOKEN",
        ])
        .is_err());
    }

    #[test]
    fn parses_file_and_environment_password_drafts() {
        let file = Cli::try_parse_from([
            "plankton",
            "password",
            "add",
            "--file",
            "/tmp/secrets.yml",
            "--key",
            "service.token",
            "--title",
            "Production services",
            "--backend",
            "one-password-main",
            "--vault",
            "engineering",
        ])
        .expect("file draft");
        assert!(matches!(
            file.command,
            Commands::Password(PasswordArgs {
                command: PasswordCommand::Add(PasswordAddArgs {
                    file: Some(path),
                    keys,
                    title: Some(title),
                    backend,
                    vault,
                    ..
                })
            }) if path.as_path() == std::path::Path::new("/tmp/secrets.yml")
                && keys == ["service.token"]
                && title == "Production services"
                && backend == "one-password-main"
                && vault == "engineering"
        ));

        let environment = Cli::try_parse_from([
            "plankton",
            "password",
            "add",
            "--env",
            "API_TOKEN",
            "--env",
            "DB_PASSWORD",
        ])
        .expect("environment draft");
        assert!(matches!(
            environment.command,
            Commands::Password(PasswordArgs {
                command: PasswordCommand::Add(PasswordAddArgs {
                    env_vars,
                    file: None,
                    ..
                })
            }) if env_vars == ["API_TOKEN", "DB_PASSWORD"]
        ));
    }

    #[test]
    fn parses_aggregated_manual_password_draft_without_values() {
        let cli = Cli::try_parse_from([
            "plankton",
            "password",
            "create",
            "--title",
            "Example credentials",
            "--key",
            "CLIENT_ID",
            "--key",
            "CLIENT_SECRET",
            "--wait",
        ])
        .expect("manual batch draft");
        let Commands::Password(PasswordArgs {
            command: PasswordCommand::Create(args),
        }) = cli.command
        else {
            panic!("expected password create command");
        };
        assert_eq!(args.keys, ["CLIENT_ID", "CLIENT_SECRET"]);
        assert!(args.wait);
        let input = manual_password_draft_input(&args).expect("manual input");
        assert!(input.entries.is_empty());
        assert!(matches!(
            &input.descriptor,
            PasswordSourceDescriptor::Manual { keys }
                if keys.as_slice() == ["CLIENT_ID", "CLIENT_SECRET"]
        ));
        let encoded = serde_json::to_string(&input).expect("input serializes");
        assert!(!encoded.contains("value"));
    }

    #[test]
    fn parses_cross_vault_migration_handoff_without_password_values() {
        let cli = Cli::try_parse_from([
            "plankton",
            "password",
            "migrate",
            "production-api",
            "--backend",
            "one-password-main",
            "--vault",
            "engineering",
            "--move-source",
        ])
        .expect("migration handoff should parse");

        assert!(matches!(
            cli.command,
            Commands::Password(PasswordArgs {
                command: PasswordCommand::Migrate(PasswordMigrateArgs {
                    item_id,
                    backend,
                    vault,
                    move_source: true,
                })
            }) if item_id == "production-api"
                && backend == "one-password-main"
                && vault == "engineering"
        ));
    }

    #[test]
    fn password_add_accepts_an_editable_exposure_profile_for_file_and_env_sources() {
        let temp = tempfile::tempdir().expect("temp dir");
        let file = temp.path().join(".env");
        std::fs::write(&file, "TOKEN=test-only-value\n").expect("fixture");
        let name = "PLANKTON_EXPOSURE_DRAFT_TEST_TOKEN";
        std::env::set_var(name, "test-only-value");
        for source in [vec!["--file", file.to_str().unwrap()], vec!["--env", name]] {
            let mut argv = vec!["plankton", "password", "add"];
            argv.extend(source);
            argv.extend([
                "--access-mode",
                "direct",
                "--llm-context",
                "1",
                "--network",
                "1",
                "--network-domain",
                "api.example.com",
                "--exposure-note",
                "llm_context=Review this suggestion",
            ]);
            let cli = Cli::try_parse_from(argv).expect("exposure flags accepted");
            let Commands::Password(PasswordArgs {
                command: PasswordCommand::Add(args),
            }) = cli.command
            else {
                panic!("password add")
            };
            let input = password_draft_input(args).expect("draft");
            let policy = input
                .suggested_layout
                .unwrap()
                .default_exposure_policy
                .unwrap();
            assert_eq!(policy.access_mode, CredentialAccessMode::Direct);
            assert_eq!(
                policy
                    .surfaces
                    .iter()
                    .find(|entry| entry.surface == CredentialExposureSurface::LlmContext)
                    .unwrap()
                    .max_level,
                1
            );
            assert_eq!(
                policy
                    .surfaces
                    .iter()
                    .find(|entry| entry.surface == CredentialExposureSurface::Network)
                    .unwrap()
                    .network_allowlist
                    .len(),
                1
            );
        }
        std::env::remove_var(name);
    }

    #[test]
    fn password_file_draft_preserves_the_editable_cli_title_suggestion() {
        let temp = tempfile::tempdir().expect("temp dir");
        let file = temp.path().join(".env");
        std::fs::write(&file, "TOKEN=test-only-value\n").expect("dotenv fixture");

        let input = password_draft_input(PasswordAddArgs {
            onepassword: Vec::new(),
            onepassword_account: None,
            exposure: ExposurePolicyArgs::default(),
            env_vars: Vec::new(),
            file: Some(file),
            format: PasswordFileFormat::Dotenv,
            keys: Vec::new(),
            title: Some("  Production environment  ".into()),
            backend: "plankton".into(),
            vault: "work".into(),
        })
        .expect("password draft input");

        assert_eq!(
            input.suggested_item_title.as_deref(),
            Some("  Production environment  ")
        );
        assert_eq!(
            input.suggested_destination,
            Some(PasswordDestination::Plankton {
                vault_id: "work".into()
            })
        );
    }

    #[test]
    fn legacy_direct_import_and_top_level_write_commands_are_not_exposed() {
        assert!(Cli::try_parse_from(["plankton", "import", "dotenv-file"]).is_err());
        assert!(Cli::try_parse_from(["plankton", "delete", "secret/demo"]).is_err());
        assert!(Cli::try_parse_from(["plankton", "set", "secret/demo", "value"]).is_err());
    }

    #[test]
    fn parses_bundled_skill_view_and_explicit_install_targets() {
        let view =
            Cli::try_parse_from(["plankton", "skill"]).expect("bundled skill view should parse");
        assert!(matches!(
            view.command,
            Commands::Skill(SkillArgs { command: None })
        ));

        let cli = Cli::try_parse_from([
            "plankton",
            "skill",
            "install",
            "--agent",
            "codex",
            "--agent",
            "claude-code",
        ])
        .expect("bundled skill install should parse");

        assert!(matches!(
            cli.command,
            Commands::Skill(SkillArgs {
                command: Some(SkillCommand::Install(SkillInstallArgs { agents }))
            }) if agents == ["codex", "claude-code"]
        ));
        assert!(Cli::try_parse_from(["plankton", "skill", "install"]).is_err());
        assert!(Cli::try_parse_from([
            "plankton",
            "skill",
            "install",
            "https://example.com/skill",
            "--agent",
            "codex"
        ])
        .is_err());
    }

    #[test]
    fn password_management_accepts_metadata_but_never_a_value() {
        assert!(Cli::try_parse_from([
            "plankton",
            "password",
            "edit",
            "production-api",
            "--title",
            "Production API",
            "--reason",
            "normalize title",
        ])
        .is_ok());
        assert!(Cli::try_parse_from([
            "plankton",
            "password",
            "rename-field",
            "secret/old-key",
            "--to",
            "secret/new-key",
            "--reason",
            "normalize key",
        ])
        .is_ok());
        assert!(Cli::try_parse_from([
            "plankton",
            "password",
            "move-field",
            "secret/source/token",
            "--to-item",
            "production-api",
            "--title",
            "Production API",
            "--reason",
            "split environment fields",
        ])
        .is_ok());
        assert!(Cli::try_parse_from([
            "plankton",
            "password",
            "merge",
            "legacy-api",
            "--into",
            "production-api",
            "--reason",
            "merge duplicate entries",
        ])
        .is_ok());
        assert!(Cli::try_parse_from([
            "plankton",
            "password",
            "dedupe-field",
            "secret/duplicate/token",
            "--keep",
            "secret/production/token",
            "--reason",
            "remove verified duplicate",
        ])
        .is_ok());
        assert!(Cli::try_parse_from([
            "plankton",
            "password",
            "edit",
            "production-api",
            "--title",
            "Production API",
            "--value",
            "must-not-parse",
            "--reason",
            "attempted value write",
        ])
        .is_err());
        assert!(Cli::try_parse_from([
            "plankton",
            "password",
            "dedupe-field",
            "secret/duplicate/token",
            "--keep",
            "secret/production/token",
            "--value",
            "must-not-parse",
            "--reason",
            "attempted value comparison",
        ])
        .is_err());
    }

    #[test]
    fn parses_key_values_without_muting_errors() {
        assert_eq!(
            parse_key_values("metadata", vec!["owner=platform".into()])
                .expect("key value")
                .get("owner")
                .map(String::as_str),
            Some("platform")
        );
        assert!(parse_key_values("metadata", vec!["broken".into()])
            .expect_err("malformed value")
            .to_string()
            .contains("KEY=VALUE"));
    }

    #[tokio::test]
    async fn status_poll_retries_only_transient_daemon_unavailability() {
        use std::{cell::Cell, future::ready, rc::Rc};

        let attempts = Rc::new(Cell::new(0_u8));
        let attempts_for_poll = Rc::clone(&attempts);
        let result = retry_transient_status(
            move || {
                let attempt = attempts_for_poll.get() + 1;
                attempts_for_poll.set(attempt);
                async move {
                    if attempt < 3 {
                        Err(plankton_client::ClientError::Unavailable(
                            "temporary timeout".into(),
                        ))
                    } else {
                        Ok("approved")
                    }
                }
            },
            |_| ready(()),
        )
        .await
        .expect("transient polling failures should recover");

        assert_eq!(result, "approved");
        assert_eq!(attempts.get(), 3);
    }

    #[tokio::test]
    async fn status_poll_does_not_retry_daemon_rejections() {
        use std::{cell::Cell, future::ready, rc::Rc};

        let attempts = Rc::new(Cell::new(0_u8));
        let attempts_for_poll = Rc::clone(&attempts);
        let error = retry_transient_status(
            move || {
                attempts_for_poll.set(attempts_for_poll.get() + 1);
                async {
                    Err::<(), _>(plankton_client::ClientError::InvalidEndpoint(
                        "invalid endpoint".into(),
                    ))
                }
            },
            |_| ready(()),
        )
        .await
        .expect_err("non-transient client errors must fail immediately");

        assert!(matches!(
            error,
            plankton_client::ClientError::InvalidEndpoint(_)
        ));
        assert_eq!(attempts.get(), 1);
    }

    #[tokio::test]
    async fn automatic_handoff_waits_for_human_review_transition_and_fires_once() {
        use std::{
            cell::{Cell, RefCell},
            future::ready,
            rc::Rc,
        };

        fn response(
            state: ResourceAccessState,
            human_review_required: bool,
        ) -> ResourceAccessResponse {
            ResourceAccessResponse {
                request_id: "request-automatic".into(),
                resource_id: "secret/automatic".into(),
                state,
                value: None,
                decision_note: None,
                human_review_required,
            }
        }

        let responses = Rc::new(RefCell::new(
            [
                response(ResourceAccessState::Pending, false),
                response(ResourceAccessState::Pending, true),
                response(ResourceAccessState::Pending, true),
                response(ResourceAccessState::Pending, false),
                response(ResourceAccessState::Pending, true),
                response(ResourceAccessState::Approved, false),
            ]
            .into_iter(),
        ));
        let polls = Rc::new(Cell::new(0_u8));
        let handoffs = Rc::new(RefCell::new(Vec::new()));
        let responses_for_poll = Rc::clone(&responses);
        let polls_for_poll = Rc::clone(&polls);
        let handoffs_for_dispatch = Rc::clone(&handoffs);

        let result = poll_access_until_resolved(
            response(ResourceAccessState::Pending, false),
            move |_request_id| {
                polls_for_poll.set(polls_for_poll.get() + 1);
                ready(Ok(responses_for_poll
                    .borrow_mut()
                    .next()
                    .expect("poll response")))
            },
            |_| ready(()),
            move |request_id| {
                handoffs_for_dispatch.borrow_mut().push(request_id);
                Ok(())
            },
        )
        .await
        .expect("automatic request should reach terminal status");

        assert_eq!(result.state, ResourceAccessState::Approved);
        assert_eq!(polls.get(), 6);
        assert_eq!(
            handoffs.borrow().as_slice(),
            &["request-automatic".to_string()]
        );
    }

    #[tokio::test]
    async fn missing_environment_name_fails_before_daemon_transport() {
        let missing = "PLANKTON_MISSING_ENVIRONMENT_FOR_DRAFT_TEST".to_string();
        assert!(
            std::env::var_os(&missing).is_none(),
            "fixture name must be unset"
        );

        let error = run_password(
            OutputFormat::Json,
            PasswordArgs {
                command: PasswordCommand::Add(PasswordAddArgs {
                    onepassword: Vec::new(),
                    onepassword_account: None,
                    exposure: ExposurePolicyArgs::default(),
                    env_vars: vec![missing.clone()],
                    file: None,
                    format: PasswordFileFormat::Auto,
                    keys: Vec::new(),
                    title: None,
                    backend: "plankton".into(),
                    vault: "default".into(),
                }),
            },
        )
        .await
        .expect_err("missing variable must prevent daemon connection");

        assert!(error.to_string().contains(&missing));
        assert!(error.to_string().contains("not set"));
        assert!(!error.to_string().contains("failed to connect"));
    }

    #[cfg(unix)]
    #[test]
    fn non_unicode_environment_error_omits_the_value_from_the_full_cause_chain() {
        use std::{ffi::OsString, os::unix::ffi::OsStringExt};

        let name = "PLANKTON_NON_UNICODE_ENVIRONMENT_FOR_DRAFT_TEST";
        let sentinel = "NON_UNICODE_SECRET_SENTINEL";
        let mut bytes = sentinel.as_bytes().to_vec();
        bytes.push(0xff);
        std::env::set_var(name, OsString::from_vec(bytes));
        let result = password_draft_input(PasswordAddArgs {
            onepassword: Vec::new(),
            onepassword_account: None,
            exposure: ExposurePolicyArgs::default(),
            env_vars: vec![name.into()],
            file: None,
            format: PasswordFileFormat::Auto,
            keys: Vec::new(),
            title: None,
            backend: "plankton".into(),
            vault: "default".into(),
        });
        std::env::remove_var(name);

        let error = result.expect_err("non-Unicode environment value must fail");
        let rendered_chain = error
            .chain()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join("\n");
        assert!(rendered_chain.contains(name));
        assert!(!rendered_chain.contains(sentinel));
    }

    #[test]
    fn password_add_receipt_boundary_drops_values_before_all_output_formats() {
        let sentinel = "PASSWORD_ADD_OUTPUT_SECRET_SENTINEL";
        let parsed = plankton_core::passwords::parse_password_draft_input(PasswordDraftInput {
            descriptor: PasswordSourceDescriptor::OnePassword {
                account: None,
                fields: vec![plankton_protocol::passwords::OnePasswordFieldReference {
                    key: "API_TOKEN".into(),
                    reference: "op://v/i/password".into(),
                }],
            },
            entries: vec![SelectedPasswordEntry {
                key: "API_TOKEN".into(),
                value: sentinel.into(),
            }],
            suggested_item_title: None,
            suggested_destination: None,
            suggested_layout: None,
        })
        .expect("password add input parses");
        let draft: PasswordDraftCreated = serde_json::from_value(serde_json::json!({
            "draft_id": "00000000-0000-0000-0000-000000000001",
            "keys": parsed.entries.iter().map(|entry| &entry.key).collect::<Vec<_>>(),
            "expires_at": "2026-07-30T00:00:00Z",
        }))
        .expect("daemon receipt");
        let receipt: PasswordDraftOutput = draft.into();

        for output in [OutputFormat::Text, OutputFormat::Json, OutputFormat::Jsonl] {
            let rendered =
                format_password_draft_output(output, &receipt).expect("receipt output serializes");
            assert!(rendered.contains("API_TOKEN"));
            assert!(!rendered.contains(sentinel));
        }
    }
}
