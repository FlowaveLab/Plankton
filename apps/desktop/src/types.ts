export type StructuredCallChainNode = {
  pid?: number | null;
  ppid?: number | null;
  process_name?: string | null;
  executable_path?: string | null;
  argv?: string[] | null;
  resolved_file_path?: string | null;
  source?: string | null;
  previewable?: boolean | null;
  preview_status?: string | null;
  preview_text?: string | null;
  preview_error?: string | null;
};

export type CallChainEntry = StructuredCallChainNode;

export type RequestContext = {
  resource: string;
  resource_tags?: string[];
  resource_metadata?: Record<string, string>;
  reason: string;
  requested_by: string;
  script_path: string | null;
  call_chain: CallChainEntry[];
  env_vars: Record<string, string>;
  metadata: Record<string, string>;
  created_at: string;
};

export type LlmSuggestionUsage = {
  prompt_tokens: number;
  completion_tokens: number;
  total_tokens: number;
};

export type BatchResourceDecision = {
  resource_selector: string;
  suggested_decision: string;
  rationale_summary: string;
  risk_score: number;
};

export type ExposureEvidenceTarget =
  | { kind: "source_file"; node_index: number; source_id: string }
  | { kind: "resource"; resource_selector: string }
  | { kind: "node"; node_index: number }
  | {
      kind: "argument_quote";
      node_index: number;
      argument_index: number;
      quote: string;
      occurrence: number;
    }
  | {
      kind: "argument_span";
      node_index: number;
      start: {
        argument_index: number;
        quote: string;
        occurrence: number;
      };
      end: {
        argument_index: number;
        quote: string;
        occurrence: number;
      };
    }
  | {
      kind: "source_quote";
      node_index: number;
      source_id: string;
      start_line: number;
      end_line: number;
      quote: string;
      occurrence: number;
    };

export type ExposureEvidenceAnnotation = {
  reason: string;
  target: ExposureEvidenceTarget;
};

export type CredentialExposureReport = {
  chain_summary: string;
  node_assessments: Array<{
    node_index: number;
    summary: string;
    capabilities: string[];
  }>;
  surfaces: Array<{
    surface:
      | "llm_context"
      | "network"
      | "local_persistence"
      | "terminal_log"
      | "process_propagation";
    actual_level: number;
    evidence_state: "not_observed" | "observed" | "unknown";
    summary: string;
    annotations: ExposureEvidenceAnnotation[];
  }>;
};

export type SanitizedPromptContext = {
  resource: string;
  resource_tags: string[];
  metadata: Record<string, string>;
  request_metadata: Record<string, string>;
  reason: string;
  requested_by: string;
  script_path: string | null;
  call_chain: string[];
  call_chain_details: Array<{
    process_name: string | null;
    executable_path: string | null;
    arguments: Array<{ argument_index: number; text: string }>;
    resolved_file_path: string | null;
  }>;
  inline_sources: Array<{
    source_id: string;
    node_index: number;
    argument_index: number;
    language: string;
    lines: Array<{ line: number; text: string }>;
  }>;
  env_vars: Record<string, string>;
  env_var_names: string[];
};

export type ProviderInputSnapshot = {
  template_id: string;
  template_version: string;
  prompt_contract_version: string;
  prompt_sha256: string;
  prompt: string;
  decision_policy: {
    allow: boolean;
    deny: boolean;
    escalate: boolean;
  };
  allowed_read_files: string[];
  sanitized_context: SanitizedPromptContext;
};

export type ProviderTrace = {
  audit_events?: unknown[];
  decision_attempts?: Array<{
    prompt: string;
    raw_response: string;
    started_at: string;
    finished_at: string;
    tool_events: unknown[];
    validation_error: string | null;
    normalization: string | null;
    evidence_path: string | null;
  }>;
  session_configuration?: Record<string, unknown> | null;
  rendered_prompt: string | null;
  transport: string | null;
  protocol: string | null;
  api_version: string | null;
  output_format: string | null;
  stop_reason: string | null;
  package_name: string | null;
  package_version: string | null;
  session_id: string | null;
  client_request_id: string | null;
  agent_name: string | null;
  agent_version: string | null;
  beta_headers: string[];
  review_progress?: {
    state: "running" | "complete" | "partial" | "failed";
    completed_units: number;
    total_units: number;
    error?: string | null;
    updated_at: string;
  } | null;
};

export type LlmSuggestion = {
  template_id: string;
  template_version: string;
  prompt_contract_version: string;
  prompt_sha256: string;
  suggested_decision: string;
  rationale_summary: string;
  risk_score: number;
  batch_decisions?: BatchResourceDecision[];
  exposure_report?: CredentialExposureReport | null;
  json_repair_strategy?: "strict" | "conservative" | null;
  provider_kind: string;
  provider_model: string | null;
  provider_response_id: string | null;
  x_request_id: string | null;
  provider_trace: ProviderTrace | null;
  usage: LlmSuggestionUsage | null;
  error: string | null;
  generated_at: string;
};

export type AutomaticDecisionTrace = {
  auto_disposition: string;
  decision_source: string;
  matched_rule_ids: string[];
  secret_exposure_risk: boolean;
  provider_called: boolean;
  suggested_decision: string | null;
  risk_score: number | null;
  template_id: string | null;
  template_version: string | null;
  prompt_contract_version: string | null;
  provider_kind: string | null;
  provider_model: string | null;
  x_request_id: string | null;
  provider_response_id: string | null;
  auto_rationale_summary: string;
  fail_closed: boolean;
  batch_source_request_id?: string | null;
  evaluated_at: string;
};

export type AccessRequest = {
  id: string;
  context: RequestContext;
  policy_mode: string;
  approval_status: string;
  evaluation_state:
    | "not_required"
    | "queued"
    | "running"
    | "completed"
    | "failed"
    | "interrupted"
    | "superseded";
  final_decision: string | null;
  provider_kind: string | null;
  rendered_prompt: string;
  provider_input?: ProviderInputSnapshot | null;
  llm_suggestion: LlmSuggestion | null;
  automatic_decision: AutomaticDecisionTrace | null;
  created_at: string;
  updated_at: string;
  resolved_at: string | null;
};

export type AuditRecord = {
  id: string;
  request_id: string;
  action: string;
  actor: string;
  note: string | null;
  payload: Record<string, unknown>;
  created_at: string;
};

export type DashboardData = {
  pending_requests: AccessRequest[];
  recent_audit_records: AuditRecord[];
};

export type DecisionCommand = "approve_request" | "reject_request";

export type AcpProfile = {
  session_options?: Record<string, string>;
  agent_kind: "codex" | "claude_code" | "open_code" | "custom";
  version_mode: "latest" | "pinned" | "custom";
  version?: string | null;
  program?: string | null;
  args?: string[];
};

export type AcpProbeStatus = "passed" | "failed" | "not_run";

export type AcpProbeError = {
  kind: "timeout" | "protocol" | "transport";
  message: string;
  code?: number | null;
  data?: AcpProbeErrorData;
};

export type AcpProbeErrorData =
  | boolean
  | number
  | string
  | null
  | AcpProbeErrorData[]
  | { [key: string]: AcpProbeErrorData };

export type AcpProbeCheck = {
  status: AcpProbeStatus;
  error?: AcpProbeError | null;
};

export type AcpConfigOption = {
  id: string;
  name: string;
  description: string | null;
  category: string | null;
  current_value: string;
  options: {
    value: string;
    name: string;
    description: string | null;
    group: string | null;
  }[];
};

export type AcpProbeResult = {
  config_options?: AcpConfigOption[];
  rejected_options?: string[];
  configured_selector: string;
  program: string;
  args: string[];
  package_name?: string | null;
  package_selector: string;
  agent_name?: string | null;
  agent_version?: string | null;
  protocol_version?: string | null;
  basic: AcpProbeCheck;
  readiness: AcpProbeCheck;
};

export type DesktopSettings = {
  locale: string;
  default_policy_mode: string;
  llm_approval_allow_enabled: boolean;
  llm_approval_deny_enabled: boolean;
  llm_approval_escalate_enabled: boolean;
  llm_auto_approve_password_edits: boolean;
  llm_auto_approve_password_renames: boolean;
  llm_auto_approve_password_refreshes: boolean;
  llm_auto_approve_password_deletes: boolean;
  provider_kind: string;
  request_template: string;
  llm_advice_template: string;
  openai_api_base: string;
  openai_api_key: string;
  openai_model: string;
  openai_temperature: number;
  claude_api_base: string;
  claude_api_key: string;
  claude_model: string;
  claude_anthropic_version: string;
  claude_max_tokens: number;
  claude_temperature: number;
  claude_timeout_secs: number;
  acp_profile: AcpProfile;
  acp_codex_program: string;
  acp_codex_args: string;
  acp_timeout_secs: number;
};

export type OnePasswordCliLocator = {
  provider_kind: "1password_cli";
  account: string;
  account_id?: string | null;
  vault: string;
  item: string;
  field: string;
  vault_id?: string | null;
  item_id?: string | null;
  field_id?: string | null;
};

export type KeepassxcCliLocator = {
  provider_kind: "keepassxc_cli";
  database: string;
  entry: string;
  field: string;
  unlock_secret_file: string;
  executable: string;
  executable_sha256: string;
};

export type BitwardenCliLocator = {
  provider_kind: "bitwarden_cli";
  account: string;
  organization?: string | null;
  collection?: string | null;
  folder?: string | null;
  item: string;
  field: string;
  item_id?: string | null;
};

export type DotenvFileLocator = {
  provider_kind: "dotenv_file";
  file_path: string;
  namespace?: string | null;
  prefix?: string | null;
  key: string;
};

export type SecretSourceLocator =
  | KeepassxcCliLocator
  | OnePasswordCliLocator
  | BitwardenCliLocator
  | DotenvFileLocator;

export type SecretImportSpec = {
  resource: string;
  display_name: string | null;
  description: string | null;
  tags: string[];
  metadata?: Record<string, string>;
  source_locator: SecretSourceLocator;
};

export type SecretImportBatchSpec = {
  resource_template?: string | null;
  imports: SecretImportSpec[];
};

type ImportedSecretReferenceBase = {
  resource: string;
  display_name: string;
  description?: string | null;
  tags: string[];
  metadata?: Record<string, string>;
  value?: string | null;
  imported_at: string;
  last_verified_at?: string | null;
};

export type ImportedSecretReference =
  | (ImportedSecretReferenceBase & KeepassxcCliLocator)
  | (ImportedSecretReferenceBase & OnePasswordCliLocator)
  | (ImportedSecretReferenceBase & BitwardenCliLocator)
  | (ImportedSecretReferenceBase & DotenvFileLocator);

export type ImportedSecretReceipt = {
  catalog_path: string;
  reference: ImportedSecretReference;
};

export type ImportedSecretBatchReceipt = {
  catalog_path: string;
  receipts: ImportedSecretReceipt[];
};

export type ImportedSecretCatalog = {
  catalog_path: string;
  imports: ImportedSecretReference[];
};

export type LocalSecretLiteralEntry = {
  resource: string;
  value: string;
  display_name?: string | null;
  description?: string | null;
  tags?: string[];
  metadata?: Record<string, string>;
};

export type LocalSecretCatalog = {
  catalog_path: string;
  literals: LocalSecretLiteralEntry[];
  imports: ImportedSecretReference[];
};

export type ImportedSecretReferenceUpdate = {
  resource: string;
  display_name: string | null;
  description: string | null;
  tags: string[];
  metadata: Record<string, string>;
};

export type LocalSecretLiteralUpsert = {
  resource: string;
  value: string;
  display_name?: string | null;
  description?: string | null;
  tags?: string[];
  metadata?: Record<string, string>;
};

export type ImportPickerOption = {
  id: string;
  label: string;
  subtitle?: string | null;
};

export type ImportFieldOption = {
  selector: string;
  label: string;
  subtitle?: string | null;
  field_id?: string | null;
};

export type BitwardenContainerKind =
  | "all"
  | "organization"
  | "collection"
  | "folder";

export type BitwardenContainerOption = {
  id: string;
  kind: BitwardenContainerKind;
  label: string;
  subtitle?: string | null;
  organization_id?: string | null;
  organization_label?: string | null;
};

export type DotenvGroupOption = {
  id: string;
  label: string;
  namespace?: string | null;
  prefix?: string | null;
  key_count: number;
};

export type DotenvKeyOption = {
  group_id: string;
  label: string;
  full_key: string;
};

export type DotenvInspection = {
  file_path: string;
  groups: DotenvGroupOption[];
  keys: DotenvKeyOption[];
};
