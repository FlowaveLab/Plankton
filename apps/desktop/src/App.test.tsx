// @vitest-environment jsdom

import { act } from "react";
import ReactDOM from "react-dom/client";
import { afterEach, expect, it, vi } from "vitest";

import type { DesktopSettings } from "./types";

const SETTINGS: DesktopSettings = {
  locale: "en",
  default_policy_mode: "assisted",
  llm_approval_allow_enabled: true,
  llm_approval_deny_enabled: true,
  llm_approval_escalate_enabled: true,
  llm_auto_approve_password_edits: false,
  llm_auto_approve_password_renames: false,
  llm_auto_approve_password_refreshes: false,
  llm_auto_approve_password_deletes: false,
  provider_kind: "acp",
  request_template: "",
  llm_advice_template: "",
  openai_api_base: "",
  openai_api_key: "",
  openai_model: "",
  openai_temperature: 0,
  claude_api_base: "",
  claude_api_key: "",
  claude_model: "",
  claude_anthropic_version: "",
  claude_max_tokens: 512,
  claude_temperature: 0,
  claude_timeout_secs: 30,
  acp_profile: {
    agent_kind: "codex",
    version_mode: "latest",
  },
  acp_codex_program: "npx",
  acp_codex_args: "-y @agentclientprotocol/codex-acp@latest",
  acp_timeout_secs: 30,
};

const desktopApp = vi.hoisted(() => ({
  dismissError: vi.fn(),
  reloadSettings: vi.fn(),
  saveSettings: vi.fn(),
  setAcpProfile: vi.fn(),
  setPolicyMode: vi.fn(),
  setProviderKind: vi.fn(),
  setLocale: vi.fn(),
  setNoteDraft: vi.fn(),
  updateSettingsField: vi.fn(),
  decide: vi.fn(async () => {}),
}));

vi.mock("./hooks/useDesktopApp", () => ({
  useDesktopApp: () => ({
    state: {
      dashboard: null,
      settings: SETTINGS,
      settingsDraft: SETTINGS,
      locale: "en",
      pendingHandoffRequestId: null,
      lastHandoffRequestId: null,
      selectedDetail: null,
      noteDraft: "",
      errorMessage: null,
      settingsErrorMessage: null,
      settingsNoticeMessage: null,
      lastUpdatedAt: null,
      isLoading: false,
      isRefreshing: false,
      isSubmitting: false,
      isSettingsLoading: false,
      isSettingsSaving: false,
      pendingDecision: null,
    },
    hasUnsavedSettings: false,
    canSaveSettings: false,
    settingsValidationMessage: null,
    ...desktopApp,
  }),
}));

import App from "./App";

Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });

afterEach(() => {
  document.body.innerHTML = "";
  vi.clearAllMocks();
});

it("wires the shared configuration controller into the top-level policies page", () => {
  const container = document.createElement("div");
  document.body.appendChild(container);
  const root = ReactDOM.createRoot(container);
  act(() => root.render(<App />));

  const policies = Array.from(container.querySelectorAll("button")).find(
    (button) => button.textContent?.includes("Policies"),
  );
  act(() => policies?.click());

  expect(
    container.querySelector('[data-testid="policies-page-form"]'),
  ).not.toBeNull();
  expect(container.textContent).toContain("Request Routing");
  expect(container.textContent).not.toContain("Memory");
  expect(container.textContent).not.toContain("Settings categories");

  act(() => root.unmount());
});
