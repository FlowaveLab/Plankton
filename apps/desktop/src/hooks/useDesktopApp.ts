import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import {
  startTransition,
  useEffect,
  useEffectEvent,
  useRef,
  useState,
} from "react";

import {
  getSelectedRequest,
  getResolvedAutoDecisionEntries,
  getResolvedReviewRequestEntries,
} from "../dashboardModel";
import {
  ACP_DEFAULT_ARGS,
  ACP_DEFAULT_PROGRAM,
  normalizeRustSemanticVersion,
} from "../acpSettings";
import {
  normalizeHandoffRequestId,
  resolvePendingHandoffRequestId,
} from "../handoff";
import { getBrowserStorage } from "../browserStorage";
import {
  DEFAULT_LOCALE,
  LOCALE_STORAGE_KEY,
  isLocale,
  type Locale,
  t,
  type TranslationKey,
} from "../i18n";
import type {
  AcpProfile,
  AuditRecord,
  DashboardData,
  DecisionCommand,
  DesktopSettings,
} from "../types";

const AUTO_REFRESH_MS = 5_000;
const ACTIVE_EVALUATION_REFRESH_MS = 500;
const HANDOFF_EVENT = "plankton://handoff-request";

const NUMERIC_SETTINGS_FIELDS = new Set<keyof DesktopSettings>([
  "openai_temperature",
  "claude_max_tokens",
  "claude_temperature",
  "claude_timeout_secs",
  "acp_timeout_secs",
]);
const BOOLEAN_SETTINGS_FIELDS = new Set<keyof DesktopSettings>([
  "llm_approval_allow_enabled",
  "llm_approval_deny_enabled",
  "llm_approval_escalate_enabled",
  "llm_auto_approve_password_edits",
  "llm_auto_approve_password_renames",
  "llm_auto_approve_password_refreshes",
  "llm_auto_approve_password_deletes",
]);

type HandoffPayload = {
  request_id: string;
};

export type DetailSelection =
  | {
      kind: "pending_request";
      id: string;
    }
  | {
      kind: "resolved_request";
      id: string;
    }
  | {
      kind: "resolved_auto";
      id: string;
    }
  | null;

export type DesktopAppState = {
  dashboard: DashboardData | null;
  settings: DesktopSettings | null;
  settingsDraft: DesktopSettings | null;
  locale: Locale;
  pendingHandoffRequestId: string | null;
  lastHandoffRequestId: string | null;
  selectedDetail: DetailSelection;
  noteDraft: string;
  errorMessage: string | null;
  settingsErrorMessage: string | null;
  settingsNoticeMessage: string | null;
  lastUpdatedAt: string | null;
  isLoading: boolean;
  isRefreshing: boolean;
  isSubmitting: boolean;
  isSettingsLoading: boolean;
  isSettingsSaving: boolean;
  pendingDecision: DecisionCommand | null;
};

type UseDesktopAppResult = {
  state: DesktopAppState;
  hasUnsavedSettings: boolean;
  canSaveSettings: boolean;
  settingsValidationMessage: string | null;
  setLocale: (locale: Locale) => void;
  dismissError: () => void;
  refreshDashboard: (options?: { silent?: boolean }) => Promise<void>;
  reloadSettings: () => Promise<void>;
  saveSettings: () => Promise<void>;
  setPolicyMode: (value: string) => void;
  setProviderKind: (value: string) => void;
  setAcpProfile: (profile: AcpProfile) => void;
  updateSettingsField: (field: keyof DesktopSettings, value: string) => void;
  selectPendingRequest: (requestId: string) => void;
  selectResolvedRequest: (requestId: string) => void;
  selectResolvedAuto: (requestId: string) => void;
  setNoteDraft: (value: string) => void;
  decide: (requestId: string, decision: DecisionCommand) => Promise<void>;
};

const INITIAL_STATE: DesktopAppState = {
  dashboard: null,
  settings: null,
  settingsDraft: null,
  locale: getInitialLocale(),
  pendingHandoffRequestId: null,
  lastHandoffRequestId: null,
  selectedDetail: null,
  noteDraft: "",
  errorMessage: null,
  settingsErrorMessage: null,
  settingsNoticeMessage: null,
  lastUpdatedAt: null,
  isLoading: true,
  isRefreshing: false,
  isSubmitting: false,
  isSettingsLoading: false,
  isSettingsSaving: false,
  pendingDecision: null,
};

function getInitialLocale(): Locale {
  const savedLocale = getBrowserStorage().getItem(LOCALE_STORAGE_KEY);
  return isLocale(savedLocale) ? savedLocale : DEFAULT_LOCALE;
}

function getErrorMessage(error: unknown): string {
  if (error instanceof Error) {
    return error.message;
  }

  return String(error);
}

function hasDesktopRuntime(): boolean {
  return (
    import.meta.env.MODE === "test" ||
    Boolean(
      (window as Window & { __TAURI_INTERNALS__?: object }).__TAURI_INTERNALS__,
    )
  );
}

function hasRunningReviewDetails(dashboard: DashboardData | null): boolean {
  if (!dashboard) return false;
  if (
    dashboard.pending_requests.some(
      (request) =>
        request.evaluation_state === "queued" ||
        request.evaluation_state === "running" ||
        request.llm_suggestion?.provider_trace?.review_progress?.state ===
          "running",
    )
  ) {
    return true;
  }

  // Audit entries are historical snapshots. Only the newest progress for each
  // request can keep polling fast; completed reviews retain older running rows.
  const latestProgress = new Map<
    string,
    { record: AuditRecord; state: string }
  >();
  for (const record of dashboard.recent_audit_records) {
    const providerTrace = record.payload.provider_trace;
    if (
      !providerTrace ||
      typeof providerTrace !== "object" ||
      !("review_progress" in providerTrace)
    )
      continue;
    const progress = providerTrace.review_progress;
    if (
      !progress ||
      typeof progress !== "object" ||
      !("state" in progress) ||
      typeof progress.state !== "string"
    )
      continue;
    const previous = latestProgress.get(record.request_id)?.record;
    if (
      !previous ||
      Date.parse(record.created_at) > Date.parse(previous.created_at) ||
      (record.created_at === previous.created_at && record.id > previous.id)
    ) {
      latestProgress.set(record.request_id, { record, state: progress.state });
    }
  }
  return [...latestProgress.values()].some(
    (progress) => progress.state === "running",
  );
}

function desktopRuntimeMessage(locale: Locale): string {
  return locale === "zh-CN"
    ? "桌面运行时不可用。请通过 Plankton 桌面应用启动此界面。"
    : "Desktop runtime unavailable. Open this interface from the Plankton desktop app.";
}

function cloneSettings(
  settings: DesktopSettings | null,
): DesktopSettings | null {
  return settings ? { ...settings } : null;
}

function normalizeProviderKind(value: string): string {
  return value === "acp_codex" ? "acp" : value;
}

function normalizeAcpProfile(profile: AcpProfile): AcpProfile {
  const { session_options, ...base } = profile;
  const entries = Object.entries(session_options ?? {}).sort(([a], [b]) =>
    a.localeCompare(b),
  );
  profile = entries.length
    ? { ...base, session_options: Object.fromEntries(entries) }
    : base;
  if (profile.version_mode === "latest") {
    return {
      ...profile,
      version: null,
      program: null,
      args: [],
    };
  }

  if (profile.version_mode === "pinned") {
    const version = normalizeRustSemanticVersion(profile.version);
    return {
      ...profile,
      version,
      program: null,
      args: [],
    };
  }

  return {
    ...profile,
    agent_kind: "custom",
    version: null,
  };
}

function normalizeSettings(settings: DesktopSettings): DesktopSettings {
  return {
    ...settings,
    provider_kind: normalizeProviderKind(settings.provider_kind),
    acp_profile: normalizeAcpProfile(settings.acp_profile),
  };
}

function getSettingsValidationMessage(
  locale: Locale,
  settings: DesktopSettings | null,
): string | null {
  const profile = settings?.acp_profile;
  if (!profile) {
    return null;
  }

  if (!settings.llm_approval_allow_enabled) {
    return t(locale, "settingsLlmDecisionAllowRequiredError");
  }

  if (
    !settings.llm_approval_deny_enabled &&
    !settings.llm_approval_escalate_enabled
  ) {
    return t(locale, "settingsLlmDecisionFallbackRequiredError");
  }

  if (profile.version_mode === "pinned") {
    if (normalizeRustSemanticVersion(profile.version) === null) {
      return t(locale, "settingsAcpPinnedVersionError");
    }
  }

  if (profile.version_mode === "custom" && !profile.program?.trim()) {
    return t(locale, "settingsAcpCustomProgramError");
  }

  return null;
}

function areSettingsEqual(
  left: DesktopSettings | null,
  right: DesktopSettings | null,
): boolean {
  if (!left || !right) {
    return left === right;
  }

  return (
    left.locale === right.locale &&
    left.default_policy_mode === right.default_policy_mode &&
    left.llm_approval_allow_enabled === right.llm_approval_allow_enabled &&
    left.llm_approval_deny_enabled === right.llm_approval_deny_enabled &&
    left.llm_approval_escalate_enabled ===
      right.llm_approval_escalate_enabled &&
    left.llm_auto_approve_password_edits ===
      right.llm_auto_approve_password_edits &&
    left.llm_auto_approve_password_renames ===
      right.llm_auto_approve_password_renames &&
    left.llm_auto_approve_password_refreshes ===
      right.llm_auto_approve_password_refreshes &&
    left.llm_auto_approve_password_deletes ===
      right.llm_auto_approve_password_deletes &&
    normalizeProviderKind(left.provider_kind) ===
      normalizeProviderKind(right.provider_kind) &&
    left.request_template === right.request_template &&
    left.llm_advice_template === right.llm_advice_template &&
    left.openai_api_base === right.openai_api_base &&
    left.openai_api_key === right.openai_api_key &&
    left.openai_model === right.openai_model &&
    left.openai_temperature === right.openai_temperature &&
    left.claude_api_base === right.claude_api_base &&
    left.claude_api_key === right.claude_api_key &&
    left.claude_model === right.claude_model &&
    left.claude_anthropic_version === right.claude_anthropic_version &&
    left.claude_max_tokens === right.claude_max_tokens &&
    left.claude_temperature === right.claude_temperature &&
    left.claude_timeout_secs === right.claude_timeout_secs &&
    JSON.stringify(left.acp_profile) === JSON.stringify(right.acp_profile) &&
    left.acp_codex_program === right.acp_codex_program &&
    left.acp_codex_args === right.acp_codex_args &&
    left.acp_timeout_secs === right.acp_timeout_secs
  );
}

function getSettingsFieldLabel(
  locale: Locale,
  field: keyof DesktopSettings,
): string {
  const labelMap: Record<keyof DesktopSettings, TranslationKey> = {
    locale: "settingsInterfaceLocale",
    default_policy_mode: "settingsCurrentPolicy",
    llm_approval_allow_enabled: "settingsLlmDecisionAllow",
    llm_approval_deny_enabled: "settingsLlmDecisionDeny",
    llm_approval_escalate_enabled: "settingsLlmDecisionEscalate",
    llm_auto_approve_password_edits: "settingsPasswordAutoEdit",
    llm_auto_approve_password_renames: "settingsPasswordAutoRename",
    llm_auto_approve_password_refreshes: "settingsPasswordAutoRefresh",
    llm_auto_approve_password_deletes: "settingsPasswordAutoDelete",
    provider_kind: "provider",
    request_template: "settingsRequestTemplate",
    llm_advice_template: "settingsLlmAdviceTemplate",
    openai_api_base: "openAiBase",
    openai_api_key: "openAiApiKey",
    openai_model: "openAiModel",
    openai_temperature: "openAiTemperature",
    claude_api_base: "claudeBase",
    claude_api_key: "claudeApiKey",
    claude_model: "claudeModel",
    claude_anthropic_version: "claudeApiVersion",
    claude_max_tokens: "claudeMaxTokens",
    claude_temperature: "claudeTemperature",
    claude_timeout_secs: "claudeTimeout",
    acp_profile: "settingsAcpTitle",
    acp_codex_program: "acpProgram",
    acp_codex_args: "acpArgs",
    acp_timeout_secs: "acpTimeout",
  };

  return t(locale, labelMap[field]);
}

function getOverriddenSettingsFields(
  submitted: DesktopSettings,
  effective: DesktopSettings,
): Array<keyof DesktopSettings> {
  const fields: Array<keyof DesktopSettings> = [
    "locale",
    "default_policy_mode",
    "llm_approval_allow_enabled",
    "llm_approval_deny_enabled",
    "llm_approval_escalate_enabled",
    "llm_auto_approve_password_edits",
    "llm_auto_approve_password_renames",
    "llm_auto_approve_password_refreshes",
    "llm_auto_approve_password_deletes",
    "provider_kind",
    "request_template",
    "llm_advice_template",
    "openai_api_base",
    "openai_api_key",
    "openai_model",
    "openai_temperature",
    "claude_api_base",
    "claude_api_key",
    "claude_model",
    "claude_anthropic_version",
    "claude_max_tokens",
    "claude_temperature",
    "claude_timeout_secs",
    "acp_profile",
    "acp_codex_program",
    "acp_codex_args",
    "acp_timeout_secs",
  ];

  return fields.filter((field) => {
    if (field === "provider_kind") {
      return (
        normalizeProviderKind(submitted[field]) !==
        normalizeProviderKind(effective[field])
      );
    }

    if (field === "acp_profile") {
      return (
        JSON.stringify(submitted[field]) !== JSON.stringify(effective[field])
      );
    }

    return submitted[field] !== effective[field];
  });
}

function syncSelection(
  dashboard: DashboardData,
  selectedDetail: DetailSelection,
  pendingHandoffRequestId: string | null,
): Pick<DesktopAppState, "pendingHandoffRequestId" | "selectedDetail"> {
  const resolvedReviewEntries = getResolvedReviewRequestEntries(
    dashboard.recent_audit_records,
  );
  const resolvedAutoEntries = getResolvedAutoDecisionEntries(
    dashboard.recent_audit_records,
  );
  const handoffRequestId = resolvePendingHandoffRequestId(
    dashboard,
    pendingHandoffRequestId,
  );

  if (handoffRequestId) {
    return {
      pendingHandoffRequestId: null,
      selectedDetail: {
        kind: "pending_request",
        id: handoffRequestId,
      },
    };
  }

  const selectedPendingRequest =
    selectedDetail?.kind === "pending_request"
      ? getSelectedRequest(dashboard, selectedDetail.id)
      : null;
  const selectedResolvedAuto =
    selectedDetail?.kind === "resolved_auto"
      ? (resolvedAutoEntries.find(
          (entry) => entry.request_id === selectedDetail.id,
        ) ?? null)
      : null;
  const selectedResolvedReview =
    selectedDetail?.kind === "resolved_request"
      ? (resolvedReviewEntries.find(
          (entry) => entry.request_id === selectedDetail.id,
        ) ?? null)
      : null;

  if (selectedPendingRequest) {
    return {
      pendingHandoffRequestId,
      selectedDetail: {
        kind: "pending_request",
        id: selectedPendingRequest.id,
      },
    };
  }

  if (selectedResolvedReview) {
    return {
      pendingHandoffRequestId,
      selectedDetail: {
        kind: "resolved_request",
        id: selectedResolvedReview.request_id,
      },
    };
  }

  if (selectedResolvedAuto) {
    return {
      pendingHandoffRequestId,
      selectedDetail: {
        kind: "resolved_auto",
        id: selectedResolvedAuto.request_id,
      },
    };
  }

  const firstPendingRequest = dashboard.pending_requests[0];
  if (firstPendingRequest) {
    return {
      pendingHandoffRequestId,
      selectedDetail: {
        kind: "pending_request",
        id: firstPendingRequest.id,
      },
    };
  }

  const firstResolvedReview = resolvedReviewEntries[0];
  if (firstResolvedReview) {
    return {
      pendingHandoffRequestId,
      selectedDetail: {
        kind: "resolved_request",
        id: firstResolvedReview.request_id,
      },
    };
  }

  const firstResolvedAuto = resolvedAutoEntries[0];
  return {
    pendingHandoffRequestId,
    selectedDetail: firstResolvedAuto
      ? {
          kind: "resolved_auto",
          id: firstResolvedAuto.request_id,
        }
      : null,
  };
}

export function useDesktopApp(): UseDesktopAppResult {
  const [state, setState] = useState<DesktopAppState>(INITIAL_STATE);
  const stateRef = useRef(state);
  const dashboardRefreshRef = useRef<Promise<void> | null>(null);
  const dashboardRefreshMs = hasRunningReviewDetails(state.dashboard)
    ? ACTIVE_EVALUATION_REFRESH_MS
    : AUTO_REFRESH_MS;

  useEffect(() => {
    stateRef.current = state;
  }, [state]);

  useEffect(() => {
    document.documentElement.lang = state.locale;
    document.title = t(state.locale, "appTitle");
  }, [state.locale]);

  const loadDesktopSettings = useEffectEvent(async () => {
    if (!hasDesktopRuntime()) {
      setState((current) => ({
        ...current,
        isSettingsLoading: false,
        settingsErrorMessage: desktopRuntimeMessage(current.locale),
      }));
      return;
    }
    setState((current) => ({
      ...current,
      isSettingsLoading: true,
      settingsErrorMessage: null,
      settingsNoticeMessage: null,
    }));

    try {
      const loaded = normalizeSettings(
        await invoke<DesktopSettings>("desktop_settings"),
      );
      startTransition(() => {
        setState((current) => ({
          ...current,
          locale: isLocale(loaded.locale) ? loaded.locale : current.locale,
          settings: loaded,
          settingsDraft: current.settingsDraft ?? cloneSettings(loaded),
          isSettingsLoading: false,
        }));
      });
      if (isLocale(loaded.locale)) {
        getBrowserStorage().setItem(LOCALE_STORAGE_KEY, loaded.locale);
      }
    } catch (error) {
      setState((current) => ({
        ...current,
        isSettingsLoading: false,
        settingsErrorMessage: getErrorMessage(error),
      }));
    }
  });

  const loadDashboard = useEffectEvent(
    async (options?: { silent?: boolean }) => {
      if (!hasDesktopRuntime()) {
        setState((current) => ({
          ...current,
          isLoading: false,
          isRefreshing: false,
          errorMessage: desktopRuntimeMessage(current.locale),
        }));
        return;
      }
      // Timer ticks join the active refresh. Explicit refreshes wait and then
      // fetch again so a decision made during an older fetch is not missed.
      while (dashboardRefreshRef.current) {
        if (options?.silent) return dashboardRefreshRef.current;
        await dashboardRefreshRef.current;
      }
      const refresh = async (): Promise<void> => {
        const shouldShowLoading = stateRef.current.dashboard === null;

        setState((current) => ({
          ...current,
          isLoading: shouldShowLoading,
          isRefreshing: true,
          errorMessage: null,
        }));

        try {
          const dashboard = await invoke<DashboardData>("dashboard");
          startTransition(() => {
            setState((current) => {
              const synced = syncSelection(
                dashboard,
                current.selectedDetail,
                current.pendingHandoffRequestId,
              );

              return {
                ...current,
                dashboard,
                isLoading: false,
                isRefreshing: false,
                lastUpdatedAt: new Date().toISOString(),
                pendingHandoffRequestId: synced.pendingHandoffRequestId,
                selectedDetail: synced.selectedDetail,
              };
            });
          });
        } catch (error) {
          setState((current) => ({
            ...current,
            isLoading: false,
            isRefreshing: false,
            errorMessage: getErrorMessage(error),
          }));
        }
      };
      dashboardRefreshRef.current = refresh().finally(() => {
        dashboardRefreshRef.current = null;
      });
      return dashboardRefreshRef.current;
    },
  );

  const queueHandoffRequest = useEffectEvent(
    (requestId: string | null | undefined) => {
      const normalizedRequestId = normalizeHandoffRequestId(requestId);
      if (!normalizedRequestId) {
        return;
      }

      setState((current) => ({
        ...current,
        pendingHandoffRequestId: normalizedRequestId,
        lastHandoffRequestId: normalizedRequestId,
        noteDraft: "",
        errorMessage: null,
      }));
      void loadDashboard();
    },
  );

  const saveSettings = useEffectEvent(async () => {
    const current = stateRef.current;
    if (
      !current.settingsDraft ||
      current.isSettingsLoading ||
      current.isSettingsSaving ||
      getSettingsValidationMessage(current.locale, current.settingsDraft)
    ) {
      return;
    }
    if (!hasDesktopRuntime()) {
      setState((previous) => ({
        ...previous,
        settingsErrorMessage: desktopRuntimeMessage(previous.locale),
      }));
      return;
    }

    const submitted = normalizeSettings({
      ...current.settingsDraft,
      acp_codex_program: ACP_DEFAULT_PROGRAM,
      acp_codex_args: ACP_DEFAULT_ARGS,
    });

    setState((previous) => ({
      ...previous,
      isSettingsSaving: true,
      settingsErrorMessage: null,
      settingsNoticeMessage: null,
      settingsDraft: submitted,
    }));

    try {
      const saved = normalizeSettings(
        await invoke<DesktopSettings>("save_desktop_settings", {
          settings: submitted,
        }),
      );
      const overriddenFields = getOverriddenSettingsFields(submitted, saved);

      startTransition(() => {
        setState((previous) => ({
          ...previous,
          settings: saved,
          settingsDraft: cloneSettings(saved),
          isSettingsSaving: false,
          settingsNoticeMessage:
            overriddenFields.length > 0
              ? t(previous.locale, "settingsEnvOverrideDetected", {
                  fields: overriddenFields
                    .map((field) =>
                      getSettingsFieldLabel(previous.locale, field),
                    )
                    .join(", "),
                })
              : t(previous.locale, "settingsSavedSuccess"),
        }));
      });
    } catch (error) {
      setState((previous) => ({
        ...previous,
        isSettingsSaving: false,
        settingsErrorMessage: getErrorMessage(error),
      }));
    }
  });

  const decide = useEffectEvent(
    async (requestId: string, decision: DecisionCommand) => {
      if (!hasDesktopRuntime()) {
        setState((current) => ({
          ...current,
          errorMessage: desktopRuntimeMessage(current.locale),
        }));
        return;
      }
      setState((current) => ({
        ...current,
        isSubmitting: true,
        pendingDecision: decision,
        errorMessage: null,
      }));

      try {
        await invoke(decision, {
          requestId,
          note: stateRef.current.noteDraft.trim() || null,
        });
        setState((current) => ({
          ...current,
          noteDraft: "",
        }));
        await loadDashboard();
      } catch (error) {
        setState((current) => ({
          ...current,
          errorMessage: getErrorMessage(error),
        }));
      } finally {
        setState((current) => ({
          ...current,
          isSubmitting: false,
          pendingDecision: null,
        }));
      }
    },
  );

  useEffect(() => {
    void loadDesktopSettings();
    void loadDashboard();
  }, []);

  useEffect(() => {
    if (!hasDesktopRuntime()) {
      return;
    }
    let unlisten: null | (() => void) = null;
    let disposed = false;

    void listen<HandoffPayload>(HANDOFF_EVENT, (event) => {
      queueHandoffRequest(event.payload.request_id);
    })
      .then((handle) => {
        if (disposed) {
          handle();
          return;
        }

        unlisten = handle;
      })
      .catch((error) => {
        setState((current) => ({
          ...current,
          errorMessage: getErrorMessage(error),
        }));
      });

    void invoke<string | null>("consume_handoff_request")
      .then((requestId) => {
        queueHandoffRequest(requestId);
      })
      .catch((error) => {
        setState((current) => ({
          ...current,
          errorMessage: getErrorMessage(error),
        }));
      });

    return () => {
      disposed = true;
      unlisten?.();
    };
  }, []);

  useEffect(() => {
    if (!hasDesktopRuntime()) {
      return;
    }
    const intervalId = window.setInterval(() => {
      void loadDashboard({ silent: true });
    }, dashboardRefreshMs);

    return () => {
      window.clearInterval(intervalId);
    };
  }, [dashboardRefreshMs]);

  const hasUnsavedSettings =
    state.settings !== null &&
    state.settingsDraft !== null &&
    !areSettingsEqual(state.settingsDraft, state.settings);
  const settingsValidationMessage = getSettingsValidationMessage(
    state.locale,
    state.settingsDraft,
  );

  return {
    state,
    hasUnsavedSettings,
    canSaveSettings:
      hasUnsavedSettings &&
      settingsValidationMessage === null &&
      !state.isSettingsLoading &&
      !state.isSettingsSaving,
    settingsValidationMessage,
    setLocale: (locale) => {
      getBrowserStorage().setItem(LOCALE_STORAGE_KEY, locale);
      setState((current) => ({
        ...current,
        locale,
        settings: current.settings
          ? {
              ...current.settings,
              locale,
            }
          : current.settings,
        settingsDraft: current.settingsDraft
          ? {
              ...current.settingsDraft,
              locale,
            }
          : current.settingsDraft,
      }));
      void invoke<DesktopSettings>("save_desktop_locale", { locale })
        .then((saved) => {
          const normalized = normalizeSettings(saved);
          startTransition(() => {
            setState((current) => ({
              ...current,
              locale: isLocale(normalized.locale) ? normalized.locale : locale,
              settings: normalized,
              settingsDraft: current.settingsDraft
                ? {
                    ...current.settingsDraft,
                    locale: normalized.locale,
                  }
                : cloneSettings(normalized),
            }));
          });
        })
        .catch((error) => {
          setState((current) => ({
            ...current,
            errorMessage: getErrorMessage(error),
          }));
        });
    },
    dismissError: () => {
      setState((current) => ({
        ...current,
        errorMessage: null,
      }));
    },
    refreshDashboard: loadDashboard,
    reloadSettings: loadDesktopSettings,
    saveSettings,
    setPolicyMode: (value) => {
      setState((current) => {
        if (!current.settingsDraft) {
          return current;
        }

        return {
          ...current,
          settingsErrorMessage: null,
          settingsNoticeMessage: null,
          settingsDraft: {
            ...current.settingsDraft,
            default_policy_mode: value,
          },
        };
      });
    },
    setProviderKind: (value) => {
      setState((current) => {
        if (!current.settingsDraft) {
          return current;
        }

        return {
          ...current,
          settingsErrorMessage: null,
          settingsNoticeMessage: null,
          settingsDraft: {
            ...current.settingsDraft,
            provider_kind: normalizeProviderKind(value),
          },
        };
      });
    },
    setAcpProfile: (profile) => {
      setState((current) => {
        if (!current.settingsDraft) {
          return current;
        }

        return {
          ...current,
          settingsErrorMessage: null,
          settingsNoticeMessage: null,
          settingsDraft: {
            ...current.settingsDraft,
            acp_profile: normalizeAcpProfile(profile),
          },
        };
      });
    },
    updateSettingsField: (field, value) => {
      setState((current) => {
        if (!current.settingsDraft) {
          return current;
        }

        if (NUMERIC_SETTINGS_FIELDS.has(field)) {
          const parsedValue = Number(value);
          if (Number.isNaN(parsedValue)) {
            return current;
          }

          return {
            ...current,
            settingsErrorMessage: null,
            settingsNoticeMessage: null,
            settingsDraft: {
              ...current.settingsDraft,
              [field]: parsedValue,
            },
          };
        }

        if (BOOLEAN_SETTINGS_FIELDS.has(field)) {
          return {
            ...current,
            settingsErrorMessage: null,
            settingsNoticeMessage: null,
            settingsDraft: {
              ...current.settingsDraft,
              [field]: value === "true",
            },
          };
        }

        return {
          ...current,
          settingsErrorMessage: null,
          settingsNoticeMessage: null,
          settingsDraft: {
            ...current.settingsDraft,
            [field]: value,
          },
        };
      });
    },
    selectPendingRequest: (requestId) => {
      setState((current) => ({
        ...current,
        selectedDetail: {
          kind: "pending_request",
          id: requestId,
        },
        noteDraft: "",
      }));
    },
    selectResolvedRequest: (requestId) => {
      setState((current) => ({
        ...current,
        selectedDetail: {
          kind: "resolved_request",
          id: requestId,
        },
        noteDraft: "",
      }));
    },
    selectResolvedAuto: (requestId) => {
      setState((current) => ({
        ...current,
        selectedDetail: {
          kind: "resolved_auto",
          id: requestId,
        },
        noteDraft: "",
      }));
    },
    setNoteDraft: (value) => {
      setState((current) => ({
        ...current,
        noteDraft: value,
      }));
    },
    decide,
  };
}
