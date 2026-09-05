pub mod acp;
pub mod approval_batch;
pub mod automatic;
pub mod call_chain;
pub mod config;
pub mod domain;
pub mod exposure;
pub mod password_changes;
pub mod passwords;
pub mod provider;
mod provider_tools;
pub mod resources;
pub mod sanitization;
pub mod sync;
pub mod template;
pub mod value_resolver;

pub use acp::{
    shutdown_acp_supervisor, AcpChatEvent, AcpChatToolCall, AcpChatToolCallUpdate, AcpChatTurn,
    AcpProbeResult, AcpPromptResult, AcpSessionClient, AcpSessionConfig, AcpStagedReview,
    ACP_DEFAULT_ARGS, ACP_DEFAULT_PROGRAM, ACP_LEGACY_CODEX_PROVIDER_KIND, ACP_PROVIDER_KIND,
    ACP_TRANSPORT_STDIO,
};
pub use approval_batch::{
    context_matches_resource_selector, semantic_call_chain_sha256, shared_resource_metadata_sha256,
    validate_batch_resource_decisions, BatchResourceDecision, JsonRepairStrategy,
    APPROVAL_BATCH_TICKET_TTL_SECONDS, MAX_BATCH_RESOURCE_DECISIONS,
};
pub use automatic::{
    automatic_decision_from_batch, evaluate_automatic_disposition, AutomaticDecisionSource,
    AutomaticDecisionTrace, AutomaticDisposition,
};
pub use call_chain::{
    collect_runtime_call_chain, derive_script_path, preview_call_chain_for_desktop,
    prompt_call_chain_paths, read_allowlisted_call_chain_file, read_allowlisted_paths_file,
    CallChainError, CallChainNode, CallChainNodeSource, CallChainPreviewStatus,
    CallChainReadFileResult,
};
pub use config::{
    load_settings, save_user_default_policy_mode, save_user_locale, save_user_settings,
    should_auto_approve_password_change, user_settings_path, PlanktonSettings, SettingsError,
    SettingsPersistError, UserSettings, DEFAULT_LOCALE, DEFAULT_USER_PROVIDER_KIND,
    SUPPORTED_LOCALES,
};
pub use domain::{
    AccessRequest, ApprovalStatus, AuditAction, AuditRecord, DashboardData, Decision, DomainError,
    EvaluationState, LlmApprovalDecisionPolicy, LlmReviewDetailState, LlmReviewProgress,
    LlmSuggestion, LlmSuggestionUsage, PolicyMode, ProviderInputSnapshot, ProviderTrace,
    RequestContext, SanitizedArgument, SanitizedCallChainEntry, SanitizedInlineSource,
    SanitizedPromptContext, SanitizedSourceLine, SuggestedDecision,
};
pub use exposure::{
    exposure_policy_from_metadata, is_direct_access, store_exposure_policy,
    store_item_exposure_policy, CallChainNodeAssessment, CredentialExposureReport,
    ExposureArgumentAnchor, ExposureEvidenceAnnotation, ExposureEvidenceState,
    ExposureEvidenceTarget, ExposureSurfaceAssessment, EXPOSURE_POLICY_METADATA_KEY,
};
pub use password_changes::{
    apply_password_changes, password_catalog_metadata, preview_password_changes,
};
pub use provider::{
    build_provider_input_snapshot, generate_llm_suggestion, request_llm_suggestion,
    request_llm_suggestion_with_progress, AcpAdapter, ClaudeAdapter, ClaudeMessagesAdapter,
    LlmSuggestionProgress, MockProviderAdapter, OpenAiCompatibleAdapter, ProviderAdapter,
    ProviderError, ProviderRequest, ProviderResponse, CLAUDE_PROVIDER_KIND,
};
pub use sanitization::{
    build_prompt_context, sanitize_audit_payload_for_display, sanitize_request_context_for_storage,
};
pub use sync::{
    plan_sync, EncryptedVaultBlob, GitCommand, GitRemote, HttpRequest, HttpResponse, HttpSyncKind,
    HttpSyncRemote, HttpTransport, LocalFolderRemote, RemoteBlob, ReqwestHttpTransport,
    RetryPolicy, SyncConfiguration, SyncEngine, SyncError, SyncMetadata, SyncOperation, SyncPlan,
    SyncRemote, VersionToken,
};
pub use template::{
    render_llm_advice_template, render_request_template, TemplateError,
    DEFAULT_LLM_ADVICE_TEMPLATE, DEFAULT_LLM_SYSTEM_PROMPT, DEFAULT_REQUEST_TEMPLATE,
    LLM_ADVICE_TEMPLATE_ID, LLM_ADVICE_TEMPLATE_VERSION, PROMPT_CONTRACT_VERSION,
    REQUEST_TEMPLATE_ID, REQUEST_TEMPLATE_VERSION,
};
pub use value_resolver::{
    default_value_resolver, delete_imported_secret_reference, delete_local_secret_entries,
    delete_local_secret_entry, get_local_secret_resource_description, import_secret_reference,
    import_secret_references, list_imported_secret_references, list_local_secret_catalog,
    local_secret_catalog_path, refresh_imported_secret_reference, register_secret_reference,
    register_secret_references, rename_local_secret_entry, resolve_imported_secret_reference,
    update_imported_secret_reference, update_imported_secret_snapshots,
    update_local_secret_resource_description, upsert_local_secret_literal,
    ImportedSecretBatchReceipt, ImportedSecretCatalog, ImportedSecretReceipt,
    ImportedSecretReference, ImportedSecretReferenceUpdate, LocalSecretCatalog,
    LocalSecretCatalogResolver, LocalSecretLiteralEntry, LocalSecretLiteralUpsert,
    SecretImportBatchSpec, SecretImportError, SecretImportSpec, SecretSourceLocator, ValueResolver,
    ValueResolverError, BITWARDEN_CLI_PROVIDER_KIND, DOTENV_FILE_PROVIDER_KIND,
    KEEPASSXC_CLI_PROVIDER_KIND, ONEPASSWORD_CLI_PROVIDER_KIND,
};
