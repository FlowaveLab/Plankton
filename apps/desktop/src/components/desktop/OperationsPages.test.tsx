// @vitest-environment jsdom

import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { act, type ReactNode } from "react";
import ReactDOM from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import type {
  AccessRequest,
  DashboardData,
  DesktopSettings,
} from "../../types";
import { OperationsPage } from "./OperationsPages";

function chooseRadio(group: HTMLElement, value: string): void {
  const radio = Array.from(
    group.querySelectorAll<HTMLInputElement>('input[type="radio"]'),
  ).find((input) => input.value === value);
  if (!radio) throw new Error(`Missing radio option ${value}`);
  radio.click();
}

const workspaceStyles = readFileSync(
  resolve(process.cwd(), "src/components/desktop/workspace.css"),
  "utf8",
);

const { invoke } = vi.hoisted(() => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({ invoke }));
Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });

type RenderHarness = {
  container: HTMLDivElement;
  rerender: (node: ReactNode) => void;
  unmount: () => void;
};

type Deferred<T> = {
  promise: Promise<T>;
  resolve: (value: T) => void;
  reject: (reason: Error) => void;
};

function deferred<T>(): Deferred<T> {
  let resolve!: (value: T) => void;
  let reject!: (reason: Error) => void;
  const promise = new Promise<T>((onResolve, onReject) => {
    resolve = onResolve;
    reject = onReject;
  });
  return { promise, resolve, reject };
}

function diagnosticPage<T>(
  items: T[],
  total = items.length,
  page = 1,
): { items: T[]; page: number; page_size: number; total: number } {
  return { items, page, page_size: 20, total };
}

function diagnosticRecord(correlationId: string, userMessage: string) {
  return {
    error: {
      code: "timeout",
      user_message: userMessage,
      severity: "error",
      retryable: true,
      timestamp: "2026-07-30T12:00:00Z",
      correlation_id: correlationId,
      source: { kind: "acp" },
    },
    acknowledged_at: null,
  };
}

const SETTINGS: DesktopSettings = {
  locale: "en",
  default_policy_mode: "manual_only",
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
    version: null,
    program: null,
    args: [],
  },
  acp_codex_program: "npx",
  acp_codex_args: "-y @agentclientprotocol/codex-acp@latest",
  acp_timeout_secs: 30,
};

function render(node: ReactNode): RenderHarness {
  const container = document.createElement("div");
  document.body.appendChild(container);
  const root = ReactDOM.createRoot(container);
  act(() => root.render(node));
  return {
    container,
    rerender(node) {
      act(() => root.render(node));
    },
    unmount() {
      act(() => root.unmount());
      container.remove();
    },
  };
}

function changeInput(input: HTMLInputElement, value: string): void {
  const setter = Object.getOwnPropertyDescriptor(
    HTMLInputElement.prototype,
    "value",
  )?.set;
  setter?.call(input, value);
  input.dispatchEvent(new Event("input", { bubbles: true }));
}

function renderPage(
  view: "agents" | "connections" | "diagnostics" | "policies",
  settingsController = buildSettingsController(),
  locale: "en" | "zh-CN" = "en",
): RenderHarness {
  return render(
    <OperationsPage
      focusedRequestId={null}
      isSubmitting={false}
      locale={locale}
      noteDraft=""
      onDecision={async () => {}}
      onNavigate={() => {}}
      onNoteChange={() => {}}
      settingsController={settingsController}
      view={view}
    />,
  );
}

function buildSettingsController(
  overrides: Partial<{
    settings: DesktopSettings | null;
    settingsDraft: DesktopSettings | null;
    isLoading: boolean;
    isSaving: boolean;
    errorMessage: string | null;
    noticeMessage: string | null;
    hasUnsavedChanges: boolean;
    canSave: boolean;
    validationMessage: string | null;
  }> = {},
) {
  return {
    settings: SETTINGS,
    settingsDraft: SETTINGS,
    isLoading: false,
    isSaving: false,
    errorMessage: null,
    noticeMessage: null,
    hasUnsavedChanges: false,
    canSave: false,
    validationMessage: null,
    onSave: vi.fn(),
    onReload: vi.fn(),
    onPolicyModeChange: vi.fn(),
    onProviderKindChange: vi.fn(),
    onAcpProfileChange: vi.fn(),
    onFieldChange: vi.fn(),
    ...overrides,
  };
}

function policiesPage(
  settingsController = buildSettingsController(),
  locale: "en" | "zh-CN" = "en",
): ReactNode {
  return (
    <OperationsPage
      focusedRequestId={null}
      isSubmitting={false}
      locale={locale}
      noteDraft=""
      onDecision={async () => {}}
      onNavigate={() => {}}
      onNoteChange={() => {}}
      settingsController={settingsController}
      view="policies"
    />
  );
}

function evaluationRequest(
  evaluationState: AccessRequest["evaluation_state"],
  policyMode = "llm_automatic",
  error: string | null = null,
): AccessRequest {
  return {
    id: `request-${policyMode}-${evaluationState}`,
    context: {
      resource: "plankton://field/smoke/password",
      reason: "Verify automatic approval status",
      requested_by: "codex-smoke",
      script_path: null,
      call_chain: [],
      env_vars: {},
      metadata: {},
      resource_tags: [],
      resource_metadata: {},
      created_at: "2026-07-30T09:00:00Z",
    },
    policy_mode: policyMode,
    approval_status: "pending",
    evaluation_state: evaluationState,
    final_decision: null,
    provider_kind: "acp",
    rendered_prompt: "Evaluate request",
    llm_suggestion:
      error === null
        ? null
        : {
            template_id: "request-advice",
            template_version: "1",
            prompt_contract_version: "1",
            prompt_sha256: "test-sha256",
            suggested_decision: "escalate",
            risk_score: 1,
            rationale_summary: "Provider evaluation failed",
            provider_kind: "acp",
            provider_model: null,
            provider_response_id: null,
            x_request_id: null,
            provider_trace: null,
            usage: null,
            error,
            generated_at: "2026-07-30T09:00:00Z",
          },
    automatic_decision: null,
    created_at: "2026-07-30T09:00:00Z",
    updated_at: "2026-07-30T09:00:00Z",
    resolved_at: null,
  };
}

function renderRequests(
  request: AccessRequest | AccessRequest[],
  locale: "en" | "zh-CN" = "zh-CN",
  focusedRequestId: string | null = Array.isArray(request) ? null : request.id,
): RenderHarness {
  const requests = Array.isArray(request) ? request : [request];
  const dashboard: DashboardData = {
    pending_requests: requests,
    recent_audit_records: [],
  };
  return render(
    <OperationsPage
      dashboard={dashboard}
      focusedRequestId={focusedRequestId}
      isSubmitting={false}
      locale={locale}
      noteDraft=""
      onDecision={async () => {}}
      onNavigate={() => {}}
      onNoteChange={() => {}}
      settingsController={buildSettingsController()}
      view="requests"
    />,
  );
}

function annotatedRequest(completedUnits: number): AccessRequest {
  const request = evaluationRequest("running", "llm_automatic");
  return {
    ...request,
    context: {
      ...request.context,
      call_chain: [
        {
          pid: 501,
          ppid: 500,
          process_name: "python3",
          executable_path: "/usr/bin/python3",
          argv: [
            "python3",
            "/workspace/query.py",
            "--endpoint=https://api.example.test/v1",
          ],
          resolved_file_path: "/workspace/query.py",
          source: "os_probe",
          previewable: true,
          preview_status: "preview_ready",
          preview_text: [
            "from urllib.request import urlopen",
            "",
            'endpoint = "https://api.example.test/v1"',
            "response = urlopen(endpoint)",
          ].join("\n"),
          preview_error: null,
        },
      ],
    },
    llm_suggestion: {
      template_id: "request-advice",
      template_version: "1",
      prompt_contract_version: "1",
      prompt_sha256: "test-sha256",
      suggested_decision: "escalate",
      risk_score: 42,
      rationale_summary: "The endpoint and process handoff need review.",
      exposure_report: {
        chain_summary:
          "The request reads one credential and invokes a fixed network client.",
        node_assessments: [
          {
            node_index: 0,
            summary: "Python performs the outbound request.",
            capabilities: ["network request", "response handling"],
          },
        ],
        surfaces: [
          {
            surface: "llm_context",
            actual_level: 1,
            evidence_state: "observed",
            summary: "The caller receives the value.",
            annotations: [
              {
                reason: "Only the endpoint value is relevant here.",
                target: {
                  kind: "argument_quote",
                  node_index: 0,
                  argument_index: 2,
                  quote: "https://api.example.test",
                  occurrence: 0,
                },
              },
            ],
          },
          {
            surface: "network",
            actual_level: 1,
            evidence_state: "observed",
            summary: "A fixed destination is visible in source.",
            annotations: [
              {
                reason: "This exact source keyword constructs the target.",
                target: {
                  kind: "source_quote",
                  node_index: 0,
                  source_id: "file:/workspace/query.py",
                  start_line: 3,
                  end_line: 4,
                  quote: "endpoint",
                  occurrence: 0,
                },
              },
            ],
          },
          {
            surface: "local_persistence",
            actual_level: 0,
            evidence_state: "not_observed",
            summary: "No write is observed.",
            annotations: [
              {
                reason: "The stated intent is read-only.",
                target: { kind: "node", node_index: 0 },
              },
            ],
          },
          {
            surface: "terminal_log",
            actual_level: 0,
            evidence_state: "not_observed",
            summary: "No terminal output is observed.",
            annotations: [],
          },
          {
            surface: "process_propagation",
            actual_level: 1,
            evidence_state: "observed",
            summary: "Python receives the request parameters.",
            annotations: [
              {
                reason: "This node owns the network-capable process.",
                target: { kind: "node", node_index: 0 },
              },
            ],
          },
        ],
      },
      provider_kind: "acp",
      provider_model: "gpt-5.6",
      provider_response_id: null,
      x_request_id: null,
      provider_trace: {
        rendered_prompt: null,
        transport: "stdio",
        protocol: "acp",
        api_version: "1",
        output_format: "json",
        stop_reason: null,
        package_name: "codex-acp",
        package_version: "1.0.0",
        session_id: "session-1",
        client_request_id: "client-1",
        agent_name: "codex",
        agent_version: "1.0.0",
        beta_headers: [],
        review_progress: {
          state: completedUnits >= 10 ? "complete" : "running",
          completed_units: completedUnits,
          total_units: 10,
          error: null,
          updated_at: `2026-07-30T09:00:${String(completedUnits).padStart(2, "0")}Z`,
        },
      },
      usage: null,
      error: null,
      generated_at: "2026-07-30T09:00:00Z",
    },
  };
}

beforeEach(() => {
  Object.assign(window, { __TAURI_INTERNALS__: {} });
  invoke.mockResolvedValue(undefined);
});

afterEach(() => {
  document.body.innerHTML = "";
  delete (window as Window & { __TAURI_INTERNALS__?: object })
    .__TAURI_INTERNALS__;
  invoke.mockReset();
  vi.useRealTimers();
});

describe("RequestsPage", () => {
  it("follows human → automatic → history and preserves an explicitly selected queue", async () => {
    const human = { ...evaluationRequest("failed"), id: "human" };
    const automatic = { ...evaluationRequest("running"), id: "automatic" };
    const resolved = {
      ...human,
      approval_status: "approved",
      resolved_at: "2026-09-05T12:00:00Z",
    };
    invoke.mockResolvedValue({
      items: [resolved],
      total: 1,
      page: 1,
      page_size: 8,
    });
    const props = {
      focusedRequestId: null,
      isSubmitting: false,
      locale: "en" as const,
      noteDraft: "",
      onDecision: async () => {},
      onNavigate: () => {},
      onNoteChange: () => {},
      settingsController: buildSettingsController(),
      view: "requests" as const,
    };
    const dashboard = (pending_requests: AccessRequest[]): DashboardData => ({
      pending_requests,
      recent_audit_records: [],
    });
    const view = render(
      <OperationsPage {...props} dashboard={dashboard([automatic, human])} />,
    );
    const checked = () =>
      view.container.querySelector<HTMLInputElement>(
        'fieldset[aria-label="Request status"] input:checked',
      )?.value;
    expect(checked()).toBe("awaiting");
    expect(
      view.container.querySelector('[aria-label="Request details"]')
        ?.textContent,
    ).toContain("Automatic evaluation failed");
    view.rerender(
      <OperationsPage {...props} dashboard={dashboard([automatic])} />,
    );
    expect(checked()).toBe("evaluating");
    view.rerender(<OperationsPage {...props} dashboard={dashboard([])} />);
    await act(async () => {});
    expect(checked()).toBe("completed");
    expect(view.container.textContent).toContain("Decision: Approved");
    act(() =>
      chooseRadio(
        view.container.querySelector('fieldset[aria-label="Request status"]')!,
        "completed",
      ),
    );
    view.rerender(<OperationsPage {...props} dashboard={dashboard([human])} />);
    expect(checked()).toBe("completed");
    const auto = Array.from(view.container.querySelectorAll("button")).find(
      (button) => button.textContent === "Auto select",
    );
    act(() => auto?.click());
    expect(checked()).toBe("awaiting");
    view.unmount();
  });

  it("refreshes history after a new decision and keeps navigation available on errors", async () => {
    invoke.mockResolvedValue({ items: [], total: 0, page: 1, page_size: 8 });
    const props = {
      focusedRequestId: null,
      isSubmitting: false,
      locale: "en" as const,
      noteDraft: "",
      onDecision: async () => {},
      onNavigate: () => {},
      onNoteChange: () => {},
      settingsController: buildSettingsController(),
      view: "requests" as const,
    };
    const view = render(
      <OperationsPage
        {...props}
        dashboard={{ pending_requests: [], recent_audit_records: [] }}
      />,
    );
    await act(async () => {});
    expect(
      invoke.mock.calls.filter(
        ([command]) => command === "list_resolved_requests",
      ),
    ).toHaveLength(1);
    invoke.mockRejectedValueOnce(new Error("History temporarily unavailable"));
    view.rerender(
      <OperationsPage
        {...props}
        dashboard={{
          pending_requests: [],
          recent_audit_records: [
            {
              id: "new-decision",
              request_id: "resolved",
              action: "automatic_decision_recorded",
              actor: "system",
              note: null,
              payload: {},
              created_at: "2026-09-05T12:00:00Z",
            },
          ],
        }}
      />,
    );
    await act(async () => {});
    expect(
      view.container.querySelector('[role="alert"]')?.textContent,
    ).toContain("History temporarily unavailable");
    expect(
      view.container.querySelector('fieldset[aria-label="Request status"]'),
    ).not.toBeNull();
    const retry = Array.from(view.container.querySelectorAll("button")).find(
      (button) => button.textContent === "Retry",
    );
    await act(async () => retry?.click());
    expect(view.container.textContent).toContain("No request history yet");
    view.unmount();
  });

  it("shows a loading state before the first dashboard snapshot", () => {
    const view = render(
      <OperationsPage
        dashboard={null}
        focusedRequestId={null}
        isSubmitting={false}
        locale="en"
        noteDraft=""
        onDecision={async () => {}}
        onNavigate={() => {}}
        onNoteChange={() => {}}
        settingsController={buildSettingsController()}
        view="requests"
      />,
    );

    expect(view.container.querySelector('[role="status"]')?.textContent).toBe(
      "Loading requests…",
    );
    view.unmount();
  });

  it("restores a progress line below each approval stage", () => {
    const view = renderRequests(
      evaluationRequest("not_required", "manual_only"),
    );
    const steps = view.container.querySelectorAll(".approval-state-rail > div");
    expect(
      Array.from(steps, (step) => step.getAttribute("data-complete")),
    ).toEqual(["true", "false", "false"]);
    expect(workspaceStyles).toMatch(
      /\.desktop-workspace \.approval-state-rail > div::after\s*\{[^}]*bottom: 0;[^}]*height: 3px;/s,
    );
    expect(workspaceStyles).toContain(
      '.approval-state-rail > div[data-complete="true"]::after',
    );
    view.unmount();
  });

  it("keeps manual controls outside the scrolling evidence and chat", () => {
    const request = {
      ...evaluationRequest("not_required"),
      policy_mode: "manual_only" as const,
    };
    const view = renderRequests(request);
    const controls = view.container.querySelector(".request-review-controls");
    const scroll = view.container.querySelector(".request-detail-scroll");
    const frame = view.container.querySelector(".request-detail-frame");
    expect(scroll?.contains(controls)).toBe(false);
    expect(frame?.lastElementChild).toBe(controls);
    expect(scroll?.parentElement).toBe(frame);
    expect(view.container.querySelector('[role="progressbar"]')).toBeNull();
    view.unmount();
  });

  it("shows an automatic request as automatic approval in progress", () => {
    const view = renderRequests(evaluationRequest("running"));

    expect(view.container.textContent).toContain("自动审批中");
    expect(view.container.textContent).toContain("LLM 正在检查请求上下文");
    expect(view.container.textContent).toContain(
      "可立即人工批准或拒绝，人工决定优先",
    );
    const decisionButtons = Array.from(
      view.container.querySelectorAll("button"),
    ).filter((button) => ["批准", "拒绝"].includes(button.textContent ?? ""));
    expect(decisionButtons).toHaveLength(2);
    expect(decisionButtons.every((button) => !button.disabled)).toBe(true);
    view.unmount();
  });

  it.each(["queued", "running"] as const)(
    "submits human decisions while the evaluation is %s",
    async (state) => {
      for (const mode of ["assisted", "llm_automatic"] as const) {
        for (const label of ["Approve", "Reject"]) {
          const request = evaluationRequest(state, mode);
          const onDecision = vi.fn(async () => {});
          const view = render(
            <OperationsPage
              dashboard={{
                pending_requests: [request],
                recent_audit_records: [],
              }}
              focusedRequestId={request.id}
              isSubmitting={false}
              locale="en"
              noteDraft="human decision"
              onDecision={onDecision}
              onNavigate={() => {}}
              onNoteChange={() => {}}
              settingsController={buildSettingsController()}
              view="requests"
            />,
          );
          const button = Array.from(
            view.container.querySelectorAll("button"),
          ).find((entry) => entry.textContent === label);
          expect(button?.disabled).toBe(false);
          await act(async () => button?.click());
          expect(onDecision).toHaveBeenCalledWith(
            request.id,
            label === "Approve" ? "approve_request" : "reject_request",
          );
          view.unmount();
        }
      }
    },
  );

  it("distinguishes assisted advice generation from automatic approval", () => {
    const view = renderRequests(
      evaluationRequest("queued", "assisted"),
      "zh-CN",
    );

    expect(view.container.textContent).toContain("正在生成 AI 建议");
    expect(view.container.textContent).not.toContain("自动审批中");
    view.unmount();
  });

  it("separates raw failed model output from the system decision and exposes evidence", () => {
    const request = evaluationRequest(
      "failed",
      "llm_automatic",
      "schema mismatch",
    );
    const trace = annotatedRequest(0).llm_suggestion?.provider_trace;
    if (!request.llm_suggestion || !trace) throw new Error("missing fixture");
    request.llm_suggestion.risk_score = 100;
    request.llm_suggestion.rationale_summary =
      "Provider suggestion unavailable";
    request.llm_suggestion.provider_trace = {
      ...trace,
      stop_reason: "decision_validation_failed",
      review_progress: null,
      session_id: "same-acp-session",
      decision_attempts: [
        {
          prompt: "actual staged decision prompt",
          raw_response: JSON.stringify({
            suggested_decision: "escalate",
            risk_score: 78,
            rationale_summary: "original model rationale",
            exposure_report: { unexpected: true },
          }),
          started_at: "2026-09-05T17:00:23Z",
          finished_at: "2026-09-05T17:00:51Z",
          tool_events: [{ kind: "read_file", output: "test evidence" }],
          validation_error: "schema mismatch",
          normalization: null,
          evidence_path: "/tmp/test-evidence.json",
        },
      ],
    };
    const view = renderRequests(request);
    expect(view.container.textContent).toContain(
      "模型输出校验失败，等待人工处理",
    );
    expect(view.container.textContent).toContain("失败 1");
    expect(view.container.textContent).toContain("原始建议（未通过校验）");
    expect(view.container.textContent).toContain("original model rationale");
    expect(view.container.textContent).not.toContain(
      "Provider suggestion unavailable",
    );
    expect(view.container.textContent).toContain(
      "actual staged decision prompt",
    );
    expect(view.container.textContent).toContain("same-acp-session");
    expect(view.container.textContent).toContain("test evidence");
    const risk = Array.from(view.container.querySelectorAll("dt")).find(
      (entry) => entry.textContent === "风险分",
    );
    expect(risk?.nextElementSibling?.textContent).toBe("78");
    view.unmount();
  });

  it("keeps a failed automatic evaluation visible for human review", () => {
    const view = renderRequests(
      evaluationRequest("failed", "llm_automatic", "ACP initialize timed out"),
    );

    expect(view.container.textContent).toContain("自动评估失败，等待人工处理");
    expect(view.container.textContent).toContain("ACP initialize timed out");
    view.unmount();
  });

  it("shows an escalated completed evaluation as awaiting human approval", () => {
    const view = renderRequests(evaluationRequest("completed"));

    expect(view.container.textContent).toContain("模型建议人工复核");
    view.unmount();
  });

  it("renders model Markdown in the full view while keeping technical information folded", async () => {
    const request = evaluationRequest(
      "completed",
      "llm_automatic",
      "temporary",
    );
    request.llm_suggestion = {
      ...request.llm_suggestion!,
      error: null,
      rationale_summary: "需要 **人工核对目标**，请求字段为 `TOKEN`。",
    };
    const view = renderRequests(request);
    await act(async () => {});
    expect(
      view.container.querySelector(
        ".request-llm-rationale .approval-markdown strong",
      )?.textContent,
    ).toBe("人工核对目标");
    expect(
      view.container.querySelector(
        ".request-llm-rationale .approval-markdown code",
      )?.textContent,
    ).toBe("TOKEN");
    expect(
      view.container
        .querySelector(".request-technical-details")
        ?.hasAttribute("open"),
    ).toBe(false);
    expect(
      view.container.querySelector(".approval-state-rail")?.textContent,
    ).toContain("模型评估完成");
    view.unmount();
  });

  it("marks execution files and preserves quoted source when a live preview is unavailable", async () => {
    const request = annotatedRequest(4);
    request.llm_suggestion!.exposure_report!.surfaces[0]!.annotations.push(
      {
        reason: "实际执行的 **本地脚本**。",
        target: {
          kind: "source_file",
          node_index: 0,
          source_id: "file:/workspace/check.py",
        },
      },
      {
        reason: "输出处理",
        target: {
          kind: "source_quote",
          node_index: 0,
          source_id: "file:/workspace/check.py",
          start_line: 8,
          end_line: 8,
          quote: "stdout=DEVNULL",
          occurrence: 0,
        },
      },
    );
    const view = renderRequests(request);
    await act(async () => {});
    const panel = view.container.querySelector(
      ".request-evidence-workbench__file",
    );
    expect(panel?.textContent).toContain("/workspace/check.py");
    expect(panel?.querySelector("mark")).not.toBeNull();
    expect(
      view.container.querySelector(".request-call-chain-list")?.textContent,
    ).toContain("stdout=DEVNULL");
    expect(
      view.container.querySelector(
        ".request-call-chain-list .approval-markdown strong",
      )?.textContent,
    ).toBe("本地脚本");
    view.unmount();
  });

  it("emphasizes collection, item and intent in pending and history, preserving identifiers and prior failures", async () => {
    const pending = evaluationRequest("not_required", "manual_only");
    pending.context.resource_metadata = {
      vault: "开发集合",
      section: "服务凭据",
      item_title: "发布服务",
      field_label: "访问令牌",
    };
    pending.context.reason = "读取发布配置以验证部署";
    const resolved: AccessRequest = {
      ...pending,
      id: "resolved-failure",
      approval_status: "approved",
      final_decision: "allow",
      evaluation_state: "failed",
      resolved_at: "2026-07-30T10:00:00Z",
      llm_suggestion: evaluationRequest(
        "failed",
        "llm_automatic",
        "original output error",
      ).llm_suggestion,
    };
    invoke.mockImplementation(
      async (command: string, args: { requestId?: string }) => {
        if (command === "list_related_requests")
          return args.requestId === resolved.id ? [resolved] : [pending];
        if (command === "list_resolved_requests")
          return { items: [resolved], total: 1, page: 1, page_size: 8 };
        return undefined;
      },
    );
    const view = renderRequests(
      [pending, { ...pending, id: "second" }],
      "zh-CN",
      null,
    );
    await act(async () => {});
    const row = view.container.querySelector(".request-row");
    expect(row?.querySelector("strong")?.textContent).toBe("发布服务");
    expect(row?.textContent).toContain("开发集合 / 服务凭据");
    expect(row?.textContent).toContain(pending.context.reason);
    expect(row?.textContent).not.toContain("plankton://");
    expect(
      view.container.querySelector(".request-identifiers")?.textContent,
    ).toContain(pending.context.resource);
    await act(async () => {
      chooseRadio(
        view.container.querySelector(".request-toolbar") as HTMLElement,
        "completed",
      );
    });
    expect(
      view.container.querySelector(".request-detail-heading h2")?.textContent,
    ).toBe("发布服务");
    expect(
      view.container.querySelector(".approval-state-rail")?.textContent,
    ).not.toContain("等待人工");
    expect(
      view.container.querySelector(".approval-state-rail")?.textContent,
    ).toContain("已批准");
    expect(view.container.textContent).toContain(
      "此前自动评估失败：original output error",
    );
    view.unmount();
  });

  it("loads related requests outside the filtered history page and preserves each decision", async () => {
    const pending = evaluationRequest("not_required", "manual_only");
    const resolved: AccessRequest = {
      ...pending,
      id: "other-page",
      approval_status: "approved",
      final_decision: "allow",
      resolved_at: "2026-07-30T10:00:00Z",
    };
    invoke.mockImplementation(async (command: string) => {
      if (command === "list_related_requests") return [pending, resolved];
      if (command === "list_resolved_requests")
        return { items: [resolved], total: 17, page: 1, page_size: 8 };
      return undefined;
    });
    const view = renderRequests(pending);
    await act(async () => {});
    expect(
      view.container.querySelectorAll(".request-dossier-entry"),
    ).toHaveLength(2);
    expect(
      view.container.querySelector(".request-dossier-list")?.textContent,
    ).toContain("已批准");
    await act(async () => {
      chooseRadio(
        view.container.querySelector(".request-toolbar") as HTMLElement,
        "completed",
      );
    });
    expect(
      view.container.querySelectorAll(".request-dossier-entry"),
    ).toHaveLength(2);
    expect(invoke).toHaveBeenCalledWith("list_related_requests", {
      requestId: resolved.id,
    });
    view.unmount();
  });

  it("keeps the resolved detail selected when approval is recorded during streamed evidence work", async () => {
    const runningDetail = {
      ...annotatedRequest(4),
      evaluation_state: "completed" as const,
    };
    const resolved = {
      ...runningDetail,
      approval_status: "approved" as const,
      final_decision: "allow" as const,
      resolved_at: "2026-07-30T09:01:00Z",
    };
    invoke.mockImplementation((command: string) => {
      if (command === "list_resolved_requests") {
        return Promise.resolve({
          items: [resolved],
          page: 1,
          page_size: 8,
          total: 1,
        });
      }
      return Promise.reject(new Error(`unexpected command ${command}`));
    });
    const onDecision = vi.fn(async () => {});
    const view = render(
      <OperationsPage
        dashboard={{
          pending_requests: [runningDetail],
          recent_audit_records: [],
        }}
        focusedRequestId={runningDetail.id}
        isSubmitting={false}
        locale="en"
        noteDraft=""
        onDecision={onDecision}
        onNavigate={() => {}}
        onNoteChange={() => {}}
        settingsController={buildSettingsController()}
        view="requests"
      />,
    );
    const approve = Array.from(
      view.container.querySelectorAll<HTMLButtonElement>("button"),
    ).find((button) => button.textContent === "Approve");
    await act(async () => approve?.click());
    await act(async () => {});

    expect(onDecision).toHaveBeenCalledWith(
      runningDetail.id,
      "approve_request",
    );
    expect(invoke).toHaveBeenCalledWith("list_resolved_requests", {
      page: 1,
      pageSize: 8,
      query: "",
    });
    expect(view.container.textContent).toContain(
      runningDetail.context.resource,
    );
    const progress = view.container.querySelector(
      '[aria-label="Adding approval evidence"]',
    );
    expect(progress?.getAttribute("aria-valuenow")).toBe("4");
    expect(progress?.getAttribute("aria-valuemax")).toBe("10");
    view.unmount();
  });

  it("loads history for an empty inbox, auto-opens one result, and handles no matches", async () => {
    invoke.mockResolvedValue({ items: [], total: 0, page: 1, page_size: 8 });
    const empty = renderRequests([], "en");
    await act(async () => {});
    expect(
      empty.container.querySelector('[data-state="empty"]')?.textContent,
    ).toContain("No request history yet");
    expect(empty.container.querySelector(".request-layout")).toBeNull();
    empty.unmount();

    const unselected = renderRequests(
      [
        evaluationRequest("completed"),
        {
          ...evaluationRequest("running"),
          id: "automatic-running",
        },
      ],
      "en",
      null,
    );
    const status = unselected.container.querySelector<HTMLFieldSetElement>(
      'fieldset[aria-label="Request status"]',
    );
    act(() => {
      if (!status) return;
      chooseRadio(status, "evaluating");
    });
    expect(
      unselected.container.querySelector('[aria-label="Request details"]')
        ?.textContent,
    ).toContain("Automatic approval in progress");
    expect(
      unselected.container.querySelector('[aria-label="Request list"]'),
    ).toBeNull();

    const search = unselected.container.querySelector<HTMLInputElement>(
      'input[aria-label="Search requests"]',
    );
    act(() => changeInput(search!, "does-not-exist"));
    expect(
      unselected.container.querySelector('[data-state="empty"]')?.textContent,
    ).toContain("No requests match");
    expect(unselected.container.querySelector(".request-layout")).toBeNull();
    unselected.unmount();
  });

  it("filters status groups, resets pagination, and only paginates beyond one page", async () => {
    const requests = Array.from({ length: 18 }, (_, index) => ({
      ...evaluationRequest(index === 0 ? "running" : "completed", "assisted"),
      id: `request-${index}`,
      context: {
        ...evaluationRequest("completed", "assisted").context,
        resource: `plankton://field/service/item-${index}`,
      },
    }));
    const view = renderRequests(requests, "en", null);

    const pagination = view.container.querySelector(
      '[aria-label="Request pagination"]',
    );
    expect(pagination).not.toBeNull();
    expect(
      pagination?.parentElement?.classList.contains("request-list-footer"),
    ).toBe(true);
    const next = view.container.querySelector<HTMLButtonElement>(
      'button[aria-label="Next request page"]',
    );
    act(() => next?.click());
    expect(pagination?.textContent).toContain("2 / 3");

    const filter = view.container.querySelector<HTMLFieldSetElement>(
      'fieldset[aria-label="Request status"]',
    );
    act(() => {
      if (!filter) return;
      chooseRadio(filter, "evaluating");
    });
    expect(
      view.container.querySelector('[aria-label="Request pagination"]'),
    ).toBeNull();
    expect(view.container.textContent).toContain("item-0");
    expect(view.container.textContent).not.toContain("item-10");

    await act(async () => chooseRadio(filter!, "awaiting"));
    expect(filter?.querySelector('input[value="all"]')).not.toBeNull();
    await act(async () => chooseRadio(filter!, "all"));
    expect(
      filter?.querySelector<HTMLInputElement>("input:checked")?.value,
    ).toBe("all");
    expect(
      view.container.querySelector('[aria-label="Request pagination"]')
        ?.textContent,
    ).toContain("1 / 3");
    expect(view.container.textContent).toContain("Generating AI advice");
    view.unmount();
  });

  it("loads completed requests from the paged resolved-request contract", async () => {
    const resolved = {
      ...evaluationRequest("completed"),
      id: "resolved-1",
      approval_status: "approved",
      final_decision: "allow",
      resolved_at: "2026-07-30T10:00:00Z",
      context: {
        ...evaluationRequest("completed").context,
        resource: "secret/resolved/item",
      },
    };
    invoke.mockImplementation((command: string, payload?: unknown) => {
      if (command === "list_resolved_requests") {
        expect(payload).toEqual({ page: 1, pageSize: 8, query: "" });
        return Promise.resolve({
          items: [resolved],
          page: 1,
          page_size: 8,
          total: 17,
        });
      }
      return Promise.reject(new Error(`unexpected command ${command}`));
    });
    const view = renderRequests([evaluationRequest("running")], "en", null);
    const filter = view.container.querySelector<HTMLFieldSetElement>(
      'fieldset[aria-label="Request status"]',
    );
    await act(async () => {
      if (!filter) return;
      chooseRadio(filter, "completed");
    });

    expect(view.container.textContent).toContain("secret/resolved/item");
    expect(
      view.container.querySelector('[aria-label="Request pagination"]')
        ?.textContent,
    ).toContain("1 / 3");
    expect(
      invoke.mock.calls.filter(
        ([command]) => command === "list_resolved_requests",
      ),
    ).toHaveLength(1);
    view.unmount();
  });

  it("applies a focused request only on handoff transition and preserves a user-selected page", () => {
    const requests = Array.from({ length: 18 }, (_, index) => ({
      ...evaluationRequest("completed", "assisted"),
      id: `focus-${index}`,
      context: {
        ...evaluationRequest("completed", "assisted").context,
        resource: `secret/focus/${index}`,
      },
    }));
    const dashboard: DashboardData = {
      pending_requests: requests,
      recent_audit_records: [],
    };
    const props = {
      dashboard,
      focusedRequestId: "focus-17",
      isSubmitting: false,
      locale: "en" as const,
      noteDraft: "",
      onDecision: async () => {},
      onNavigate: () => {},
      onNoteChange: vi.fn(),
      settingsController: buildSettingsController(),
      view: "requests" as const,
    };
    const view = render(<OperationsPage {...props} />);

    expect(
      view.container.querySelector('[aria-label="Request pagination"]')
        ?.textContent,
    ).toContain("3 / 3");
    const previous = view.container.querySelector<HTMLButtonElement>(
      'button[aria-label="Previous request page"]',
    );
    act(() => previous?.click());
    expect(
      view.container.querySelector('[aria-label="Request pagination"]')
        ?.textContent,
    ).toContain("2 / 3");

    view.rerender(
      <OperationsPage
        {...props}
        dashboard={{ ...dashboard, pending_requests: [...requests] }}
      />,
    );
    expect(
      view.container.querySelector('[aria-label="Request pagination"]')
        ?.textContent,
    ).toContain("2 / 3");
    view.unmount();
  });

  it("preserves long request context, rationale, and failures without clipping", () => {
    const long = "长上下文 very-long-context ".repeat(40);
    const request = {
      ...evaluationRequest("failed", "llm_automatic", long),
      context: {
        ...evaluationRequest("failed").context,
        reason: long,
        call_chain: [{ process_name: "long-running-command", argv: [long] }],
        metadata: { long },
      },
    };
    const view = renderRequests(request, "zh-CN");

    expect(view.container.textContent).toContain(long);
    expect(
      view.container.querySelector(".request-detail-scroll")?.classList,
    ).toContain("request-detail-scroll");
    expect(workspaceStyles).toMatch(
      /\.desktop-workspace \.page-split-pane-detail\s*\{[^}]*overflow: hidden;/s,
    );
    expect(workspaceStyles).toMatch(
      /\.desktop-workspace \.request-detail-scroll\s*\{[^}]*height: 100%;[^}]*overflow-y: auto;/s,
    );
    expect(workspaceStyles).toMatch(
      /\.desktop-workspace \.request-detail-scroll\s*\{[^}]*overscroll-behavior: auto;/s,
    );
    expect(workspaceStyles).toMatch(
      /\.desktop-workspace \.request-detail-frame > \.request-review-progress\s*\{[^}]*flex: 0 0 auto;[^}]*margin: 0;/s,
    );
    view.unmount();
  });

  it("shows structured process arguments and opens a highlighted script preview", () => {
    const request = {
      ...evaluationRequest("completed"),
      context: {
        ...evaluationRequest("completed").context,
        script_path: "/workspace/scripts/release.sh",
        call_chain: [
          {
            pid: 420,
            ppid: 41,
            process_name: "bash",
            executable_path: "/bin/bash",
            argv: [
              "/bin/bash",
              "/workspace/scripts/release.sh",
              "--target",
              "production",
            ],
            resolved_file_path: "/workspace/scripts/release.sh",
            source: "os_probe",
            previewable: true,
            preview_status: "preview_ready",
            preview_text:
              "#!/usr/bin/env bash\nif true; then\n  echo release\nfi\n",
            preview_error: null,
          },
        ],
      },
    };
    const view = renderRequests(request, "zh-CN");

    expect(view.container.textContent).toContain("进程与参数明细 · 4 项 argv");
    expect(view.container.textContent).toContain("--target");
    expect(view.container.textContent).toContain("production");
    expect(view.container.textContent).toContain("420 / 41");
    expect(view.container.textContent).toContain("系统进程探测");
    const chainEntry = view.container.querySelector(
      ".request-call-chain-entry",
    );
    expect(
      chainEntry?.querySelector(".request-call-chain-command")?.textContent,
    ).toContain("/bin/bash /workspace/scripts/release.sh --target production");
    expect(
      chainEntry?.querySelector<HTMLDetailsElement>(
        ".request-call-chain-technical",
      )?.open,
    ).toBe(false);
    expect(view.container.textContent).not.toContain("模型说明");
    expect(view.container.textContent).not.toContain(
      "模型可在需要时沿已识别路径自主读取",
    );

    const preview = view.container.querySelector<HTMLDetailsElement>(
      ".request-call-chain-preview",
    );
    expect(preview?.open).toBe(false);
    act(() => preview?.querySelector("summary")?.click());
    expect(preview?.open).toBe(true);
    expect(
      preview?.querySelector(".payload-code .hljs-keyword"),
    ).not.toBeNull();
    expect(preview?.textContent).toContain("本地源码预览");

    view.unmount();
  });

  it("aggregates exact evidence into one always-visible ledger with linked references", () => {
    const view = renderRequests(annotatedRequest(4), "zh-CN");

    expect(view.container.textContent).toContain(
      "The request reads one credential and invokes a fixed network client.",
    );
    expect(view.container.textContent).toContain(
      "Python performs the outbound request.",
    );
    expect(view.container.textContent).toContain(
      "network request · response handling",
    );
    expect(view.container.querySelector(".request-exposure-nodes")).toBeNull();
    expect(
      view.container.querySelector(
        ".request-call-chain-heading .request-call-chain-llm-note",
      )?.textContent,
    ).toContain("Python performs the outbound request.");
    const workbench = view.container.querySelector(
      ".request-evidence-workbench",
    );
    expect(workbench?.querySelector("details")).toBeNull();
    expect(
      workbench?.querySelectorAll(".request-evidence-workbench__row"),
    ).toHaveLength(3);
    expect(
      workbench?.querySelectorAll(".request-evidence-workbench__notes li"),
    ).toHaveLength(3);
    expect(view.container.textContent).not.toContain("红线已标出");
    expect(view.container.textContent).not.toContain("红色下划线只标记");
    const commandMarks = Array.from(
      view.container.querySelectorAll(
        ".request-evidence-workbench__command mark",
      ),
    );
    expect(commandMarks.length).toBeGreaterThan(0);
    expect(
      view.container.querySelector(
        '.request-evidence-workbench__command mark[data-reference-ids~="1"]',
      )?.textContent,
    ).toContain("https://api.example.test");
    expect(
      workbench?.querySelector(".request-evidence-workbench__origin pre mark")
        ?.textContent,
    ).toContain("endpoint");
    const reference = workbench?.querySelector<HTMLButtonElement>(
      '.request-evidence-reference[aria-label="Evidence 1"]',
    );
    act(() =>
      reference?.dispatchEvent(new MouseEvent("mouseover", { bubbles: true })),
    );
    expect(
      workbench?.querySelector(
        '.request-evidence-workbench__notes li[data-active="true"]',
      )?.textContent,
    ).toContain("Only the endpoint value is relevant here");
    expect(view.container.textContent).toContain(
      "This exact source keyword constructs the target",
    );
    view.unmount();
  });

  it("underlines an exact argument quote", () => {
    const request = annotatedRequest(4);
    request.llm_suggestion!.exposure_report!.surfaces[2]!.annotations[0] = {
      reason: "Exact endpoint copied from argv.",
      target: {
        kind: "argument_quote",
        node_index: 0,
        argument_index: 2,
        quote: "api.example.test",
        occurrence: 0,
      },
    };
    const view = renderRequests(request, "zh-CN");
    const note = Array.from(
      view.container.querySelectorAll<HTMLElement>(
        ".request-evidence-workbench__notes li",
      ),
    ).find((entry) => entry.textContent?.includes("Exact endpoint"));

    expect(note).toBeDefined();
    expect(
      view.container.querySelector(
        '.request-evidence-workbench__command mark[data-reference-ids~="3"]',
      )?.textContent,
    ).toContain("api.example.test");
    expect(
      view.container
        .querySelector(".request-evidence-workbench__command")
        ?.textContent?.replace(/\[\d+\]/g, ""),
    ).toContain("--endpoint=https://api.example.test/v1");
    view.unmount();
  });

  it("keeps a narrow reference boundary inside a broader evidence span", () => {
    const request = annotatedRequest(4);
    request.llm_suggestion!.exposure_report!.surfaces[4]!.annotations.push({
      reason: "The complete parameter carries related process evidence.",
      target: {
        kind: "argument_span",
        node_index: 0,
        start: {
          argument_index: 2,
          quote: "--endpoint=",
          occurrence: 0,
        },
        end: { argument_index: 2, quote: "/v1", occurrence: 0 },
      },
    });
    const view = renderRequests(request, "zh-CN");
    const exactText = Array.from(
      view.container.querySelectorAll<HTMLElement>(
        '.request-evidence-workbench__command mark[data-reference-ids~="1"]',
      ),
    )
      .map((mark) => mark.textContent?.replace(/\[\d+\]/g, "") ?? "")
      .join("");
    const broadText = Array.from(
      view.container.querySelectorAll<HTMLElement>(
        '.request-evidence-workbench__command mark[data-reference-ids~="4"]',
      ),
    )
      .map((mark) => mark.textContent?.replace(/\[\d+\]/g, "") ?? "")
      .join("");

    expect(exactText).toBe("https://api.example.test");
    expect(broadText).toBe("--endpoint=https://api.example.test/v1");
    expect(
      view.container.querySelectorAll(
        '.request-evidence-reference[data-reference-id="1"]',
      ),
    ).toHaveLength(1);
    expect(
      view.container.querySelectorAll(
        '.request-evidence-reference[data-reference-id="4"]',
      ),
    ).toHaveLength(1);
    view.unmount();
  });

  it("keeps the original full command visible for node-only references", () => {
    const request = annotatedRequest(4);
    const report = request.llm_suggestion!.exposure_report!;
    report.surfaces.forEach((surface) => {
      surface.annotations = [];
    });
    report.surfaces[1]!.annotations = [
      {
        reason: "The whole process is the available evidence boundary.",
        target: { kind: "node", node_index: 0 },
      },
    ];
    const view = renderRequests(request, "zh-CN");
    const workbench = view.container.querySelector(
      ".request-evidence-workbench",
    );

    expect(workbench?.textContent).toContain("原始完整命令");
    expect(
      workbench?.querySelector(".request-evidence-workbench__command")
        ?.textContent,
    ).toContain("--endpoint=https://api.example.test/v1");
    expect(
      workbench?.querySelector(".request-evidence-workbench__node")
        ?.textContent,
    ).toContain("/workspace/query.py");
    view.unmount();
  });

  it("scrolls a bounded command panel and cycles every location for one reference", () => {
    const request = annotatedRequest(4);
    request.llm_suggestion!.exposure_report!.surfaces[0]!.annotations[0] = {
      reason: "The full argument handoff is relevant.",
      target: {
        kind: "argument_span",
        node_index: 0,
        start: { argument_index: 0, quote: "python3", occurrence: 0 },
        end: {
          argument_index: 2,
          quote: "api.example.test",
          occurrence: 0,
        },
      },
    };
    const view = renderRequests(request, "zh-CN");
    const command = view.container.querySelector<HTMLElement>(
      ".request-evidence-workbench__command",
    );
    const targets = Array.from(
      command?.querySelectorAll<HTMLElement>('mark[data-reference-ids~="1"]') ??
        [],
    );
    const note = view.container.querySelector<HTMLButtonElement>(
      'button[aria-label="定位证据 1"]',
    );
    const scrollTo = vi.fn();
    expect(targets.length).toBeGreaterThan(1);
    expect(workspaceStyles.replace(/\s+/g, " ")).toContain(
      ":is(.desktop-workspace, .compact-approval) .request-evidence-workbench__command {",
    );
    expect(workspaceStyles).toContain("max-height: 240px;");
    expect(workspaceStyles).toMatch(
      /\.request-evidence-workbench__command\s*\{[^}]*overscroll-behavior: auto;/s,
    );
    if (!command || !note) throw new Error("evidence controls are missing");
    Object.defineProperties(command, {
      clientHeight: { configurable: true, value: 100 },
      scrollHeight: { configurable: true, value: 600 },
      scrollTo: { configurable: true, value: scrollTo },
    });
    command.getBoundingClientRect = () =>
      ({ top: 0, bottom: 100, height: 100 }) as DOMRect;
    targets.forEach((target, index) => {
      target.getBoundingClientRect = () =>
        ({
          top: 180 + index * 80,
          bottom: 200 + index * 80,
          height: 20,
        }) as DOMRect;
    });

    act(() => note.click());
    const firstTop = scrollTo.mock.calls[0]?.[0]?.top;
    act(() => note.click());
    const secondTop = scrollTo.mock.calls[1]?.[0]?.top;

    expect(scrollTo).toHaveBeenCalledTimes(2);
    expect(scrollTo.mock.calls[0]?.[0]?.behavior).toBe("smooth");
    expect(secondTop).toBeGreaterThan(firstTop);
    view.unmount();
  });

  it("merges duplicate locations into one reference and combines their text", () => {
    const request = annotatedRequest(4);
    request.llm_suggestion!.exposure_report!.surfaces[1]!.annotations.push({
      reason: "The same endpoint also defines the network boundary.",
      target: {
        kind: "argument_quote",
        node_index: 0,
        argument_index: 2,
        quote: "https://api.example.test",
        occurrence: 0,
      },
    });
    const view = renderRequests(request, "zh-CN");
    const firstMark = view.container.querySelector(
      '.request-evidence-workbench__command mark[data-reference-ids~="1"]',
    );
    const firstNote = view.container.querySelector(
      ".request-evidence-workbench__notes li",
    );

    expect(
      firstMark?.querySelectorAll(".request-evidence-reference"),
    ).toHaveLength(1);
    expect(firstMark?.textContent).toContain("[1]");
    expect(firstNote?.textContent).toContain("LLM 回显 · 网络发送");
    expect(firstNote?.textContent).toContain(
      "Only the endpoint value is relevant here；The same endpoint also defines the network boundary。",
    );
    expect(
      view.container.querySelectorAll(".request-evidence-workbench__notes li"),
    ).toHaveLength(3);
    view.unmount();
  });

  it("keeps heredoc syntax highlighting and evidence underlines on the same source", () => {
    const request = annotatedRequest(4);
    const heredoc =
      "python3 <<'PY'\nfor item in range(2):\n    print(item)\nPY";
    const firstNode = request.context.call_chain[0];
    if (!firstNode) {
      throw new Error("expected a structured call-chain node");
    }
    request.context.call_chain[0] = {
      ...firstNode,
      process_name: "zsh",
      executable_path: "/bin/zsh",
      argv: ["zsh", "-lc", heredoc],
    };
    const report = request.llm_suggestion!.exposure_report!;
    for (const surface of report.surfaces) surface.annotations = [];
    report.surfaces[0]!.annotations = [
      {
        reason: "The loop consumes the credential-bearing input.",
        target: {
          kind: "argument_quote",
          node_index: 0,
          argument_index: 2,
          quote: "for",
          occurrence: 0,
        },
      },
    ];
    const view = renderRequests(request, "zh-CN");
    const highlightedKeyword = view.container.querySelector(
      '.request-evidence-workbench__command .payload-code[data-language="python heredoc"] .hljs-keyword',
    );

    expect(highlightedKeyword?.querySelector("mark")?.textContent).toContain(
      "for[1]",
    );
    expect(
      view.container.querySelectorAll(
        ".request-evidence-workbench__command .payload-code",
      ),
    ).toHaveLength(1);
    view.unmount();
  });

  it("renders one validated evidence span across multiple argv items", () => {
    const request = annotatedRequest(4);
    const node = request.context.call_chain[0]!;
    request.context.call_chain[0] = {
      ...node,
      argv: ["python3", "-c", "requests.post('https://example.test')"],
    };
    const report = request.llm_suggestion!.exposure_report!;
    for (const surface of report.surfaces) surface.annotations = [];
    report.surfaces[1]!.annotations = [
      {
        reason: "The Python child sends the request.",
        target: {
          kind: "argument_span",
          node_index: 0,
          start: {
            argument_index: 0,
            quote: "python3",
            occurrence: 0,
          },
          end: {
            argument_index: 2,
            quote: "requests.post",
            occurrence: 0,
          },
        },
      },
    ];

    const view = renderRequests(request, "zh-CN");
    const marks = Array.from(
      view.container.querySelectorAll(
        '.request-evidence-workbench__command mark[data-reference-ids~="1"]',
      ),
    ).map((mark) => mark.textContent?.replace("[1]", ""));

    expect(marks).toEqual(["python3", "-c", "requests.post"]);
    view.unmount();
  });

  it("shows only unexpected surface explanations directly below the model rationale", () => {
    const request = annotatedRequest(4);
    const report = request.llm_suggestion!.exposure_report!;
    report.surfaces[3] = {
      ...report.surfaces[3]!,
      evidence_state: "observed",
    };
    const view = renderRequests(request, "zh-CN");
    const annotations = view.container.querySelector(
      ".request-exposure-annotations",
    );
    const radar = view.container.querySelector(".exposure-radar");

    expect(annotations?.textContent).not.toContain("终端 / 日志");
    expect(
      Array.from(
        annotations?.querySelectorAll<HTMLDetailsElement>(
          "details.is-breached",
        ) ?? [],
      ).every((details) => details.open),
    ).toBe(true);
    expect(
      Array.from(
        annotations?.querySelectorAll<HTMLDetailsElement>(
          "details:not(.is-breached)",
        ) ?? [],
      ).length,
    ).toBe(0);
    expect(
      Boolean(
        (annotations?.compareDocumentPosition(radar!) ?? 0) &
        Node.DOCUMENT_POSITION_FOLLOWING,
      ),
    ).toBe(true);
    view.unmount();
  });

  it("accumulates LLM evidence in place when the dashboard refreshes", () => {
    const initial = annotatedRequest(2);
    initial.llm_suggestion!.exposure_report!.surfaces[1]!.annotations = [];
    const props = {
      focusedRequestId: initial.id,
      isSubmitting: false,
      locale: "zh-CN" as const,
      noteDraft: "",
      onDecision: async () => {},
      onNavigate: () => {},
      onNoteChange: () => {},
      settingsController: buildSettingsController(),
      view: "requests" as const,
    };
    const view = render(
      <OperationsPage
        {...props}
        dashboard={{ pending_requests: [initial], recent_audit_records: [] }}
      />,
    );

    const initialProgress = view.container.querySelector(
      '[role="progressbar"]',
    );
    expect(initialProgress?.getAttribute("aria-valuenow")).toBe("2");
    expect(initialProgress?.getAttribute("aria-valuemax")).toBe("10");
    expect(view.container.textContent).not.toContain("正在补充详细解释");
    expect(
      initialProgress
        ?.closest(".request-review-progress")
        ?.getAttribute("data-has-message"),
    ).toBe("false");
    expect(
      initialProgress?.closest(".request-detail-frame")?.firstElementChild,
    ).toBe(initialProgress?.parentElement);
    expect(
      view.container.querySelectorAll(".request-evidence-workbench__notes li"),
    ).toHaveLength(2);

    view.rerender(
      <OperationsPage
        {...props}
        dashboard={{
          pending_requests: [annotatedRequest(4)],
          recent_audit_records: [],
        }}
      />,
    );

    expect(
      view.container
        .querySelector('[role="progressbar"]')
        ?.getAttribute("aria-valuenow"),
    ).toBe("4");
    expect(view.container.textContent).not.toContain("正在补充详细解释");
    expect(
      view.container.querySelectorAll(".request-evidence-workbench__notes li"),
    ).toHaveLength(3);
    expect(view.container.textContent).toContain(
      "This exact source keyword constructs the target",
    );
    view.unmount();
  });

  it("shows interrupted detail generation as a terminal partial result", () => {
    const request = annotatedRequest(4);
    const progress = request.llm_suggestion?.provider_trace?.review_progress;
    if (!progress) throw new Error("review progress fixture is missing");
    progress.state = "partial";
    progress.error = "invalid trailing enrichment frame";

    const view = renderRequests(request, "zh-CN");

    expect(view.container.textContent).toContain("详细解释部分完成 · 4/10");
    expect(view.container.textContent).toContain(
      "已停止生成；现有标记仍可查看",
    );
    expect(view.container.textContent).not.toContain("正在补充详细解释 · 4/10");
    view.unmount();
  });

  it("keeps the progress rail active while the agent repairs a validation error", () => {
    const request = annotatedRequest(4);
    const progress = request.llm_suggestion?.provider_trace?.review_progress;
    if (!progress) throw new Error("review progress fixture is missing");
    progress.error =
      "Automatic repair 1/2: provider response was not valid JSON";

    const view = renderRequests(request, "zh-CN");

    expect(view.container.textContent).toContain("正在自动修复详细解释 · 4/10");
    expect(view.container.textContent).toContain(
      "校验错误已反馈给 Agent，当前进度会保留",
    );
    expect(
      view.container
        .querySelector('[role="progressbar"]')
        ?.getAttribute("aria-valuenow"),
    ).toBe("4");
    expect(view.container.textContent).not.toContain("已停止生成");
    view.unmount();
  });

  it("aggregates related resources by semantic call chain and preserves LLM approval evidence", () => {
    const chain = {
      pid: 420,
      ppid: 41,
      process_name: "plankton",
      executable_path: "/usr/local/bin/plankton",
      argv: ["plankton", "get", "item-a"],
      resolved_file_path: "/usr/local/bin/plankton",
      source: "os_probe",
      previewable: false,
      preview_status: "not_previewable",
      preview_text: null,
      preview_error: null,
    };
    const source = {
      ...evaluationRequest("completed"),
      id: "batch-source",
      created_at: "2026-07-30T09:00:00Z",
      context: {
        ...evaluationRequest("completed").context,
        resource: "plankton://field/item-a/secret-id",
        resource_metadata: {
          item_id: "item-a",
          field_key: "SECRET_ID",
          field_label: "Secret ID",
        },
        call_chain: [chain],
      },
      llm_suggestion: {
        template_id: "llm_advice_request",
        template_version: "5",
        prompt_contract_version: "prompt_context.v5",
        prompt_sha256: "prompt-sha",
        suggested_decision: "allow",
        rationale_summary: "Read-only access with bounded output.",
        risk_score: 18,
        batch_decisions: [
          {
            resource_selector: "SECRET_ID",
            suggested_decision: "allow",
            rationale_summary: "Identifier is required for the same lookup.",
            risk_score: 14,
          },
          {
            resource_selector: "SECRET_KEY",
            suggested_decision: "escalate",
            rationale_summary: "Key access needs a human check.",
            risk_score: 61,
          },
        ],
        json_repair_strategy: "strict" as const,
        provider_kind: "acp",
        provider_model: "gpt-5.6",
        provider_response_id: "response-1",
        x_request_id: "request-1",
        provider_trace: {
          rendered_prompt: null,
          transport: "stdio",
          protocol: "acp",
          api_version: "1",
          output_format: "json",
          stop_reason: "end_turn",
          package_name: "codex-acp",
          package_version: "1.0.0",
          session_id: "session-1",
          client_request_id: "client-1",
          agent_name: "codex",
          agent_version: "1.0.0",
          beta_headers: [],
        },
        usage: {
          prompt_tokens: 100,
          completion_tokens: 20,
          total_tokens: 120,
        },
        error: null,
        generated_at: "2026-07-30T09:00:01Z",
      },
      automatic_decision: {
        auto_disposition: "allow",
        decision_source: "llm_suggestion",
        matched_rule_ids: ["llm_suggested_allow"],
        secret_exposure_risk: false,
        provider_called: true,
        suggested_decision: "allow",
        risk_score: 18,
        template_id: "llm_advice_request",
        template_version: "5",
        prompt_contract_version: "prompt_context.v5",
        provider_kind: "acp",
        provider_model: "gpt-5.6",
        x_request_id: "request-1",
        provider_response_id: "response-1",
        auto_rationale_summary: "Allowed by LLM and guardrails.",
        fail_closed: false,
        evaluated_at: "2026-07-30T09:00:02Z",
      },
    } satisfies AccessRequest;
    const reused = {
      ...evaluationRequest("completed"),
      id: "batch-reused",
      created_at: "2026-07-30T09:00:03Z",
      context: {
        ...source.context,
        resource: "plankton://field/item-a/secret-key",
        resource_metadata: {
          item_id: "item-a",
          field_key: "SECRET_KEY",
          field_label: "Secret key",
        },
        call_chain: [
          {
            ...chain,
            pid: 991,
            ppid: 990,
            preview_status: "io_error",
            preview_error: "preview changed",
          },
        ],
      },
      automatic_decision: {
        ...source.automatic_decision,
        auto_disposition: "escalate",
        decision_source: "batch_ticket",
        provider_called: false,
        suggested_decision: "escalate",
        risk_score: 61,
        batch_source_request_id: source.id,
        auto_rationale_summary: "Reused the source request batch decision.",
      },
      llm_suggestion: null,
    } satisfies AccessRequest;
    const unrelated = {
      ...reused,
      id: "different-reason",
      context: { ...reused.context, reason: "Different invocation purpose" },
      automatic_decision: null,
    } satisfies AccessRequest;

    const view = renderRequests(
      [source, reused, unrelated],
      "zh-CN",
      source.id,
    );

    expect(
      view.container.querySelectorAll(".request-dossier-entry"),
    ).toHaveLength(2);
    expect(view.container.textContent).toContain(
      "plankton://field/item-a/secret-id",
    );
    expect(view.container.textContent).toContain(
      "plankton://field/item-a/secret-key",
    );
    expect(view.container.textContent).not.toContain("different-reason");
    expect(view.container.textContent).toContain("同次模型调用返回的资源决策");
    expect(view.container.textContent).toContain("SECRET_KEY");
    expect(view.container.textContent).toContain("gpt-5.6");
    expect(view.container.textContent).toContain("严格解析");
    expect(view.container.textContent).toContain("batch-source");
    expect(
      view.container.querySelectorAll(".request-call-chain-list"),
    ).toHaveLength(1);
    view.unmount();
  });

  it("deepens the code-agent boundary and highlights multiline heredoc arguments", () => {
    const request = {
      ...evaluationRequest("completed"),
      context: {
        ...evaluationRequest("completed").context,
        call_chain: [
          {
            process_name: "launchd",
            executable_path: "/sbin/launchd",
            argv: ["/sbin/launchd"],
          },
          {
            process_name: "codex",
            executable_path: "/opt/homebrew/bin/codex",
            argv: ["codex", "exec"],
          },
          {
            process_name: "zsh",
            executable_path: "/bin/zsh",
            argv: [
              "zsh",
              "-lc",
              "python3 <<'PY'\nfor item in range(2):\n    print(item)\nPY",
            ],
          },
        ],
      },
    };
    const view = renderRequests(request, "zh-CN");

    const entries = view.container.querySelectorAll(
      ".request-call-chain-entry",
    );
    expect(entries).toHaveLength(3);
    expect(entries[0]?.classList).not.toContain(
      "request-call-chain-entry-agent-scope",
    );
    expect(entries[1]?.classList).toContain(
      "request-call-chain-entry-agent-scope",
    );
    expect(entries[1]?.getAttribute("data-agent-start")).toBe("true");
    expect(entries[2]?.classList).toContain(
      "request-call-chain-entry-agent-scope",
    );
    expect(
      entries[2]?.querySelector(
        '.request-call-chain-arguments code[data-language="python heredoc"] .hljs-keyword',
      ),
    ).not.toBeNull();
    view.unmount();
  });
});

describe("PoliciesPage", () => {
  it("promotes policy controls without a settings category rail", () => {
    const controller = buildSettingsController({
      settingsDraft: { ...SETTINGS, default_policy_mode: "assisted" },
      hasUnsavedChanges: true,
      canSave: true,
    });
    const view = render(policiesPage(controller));

    expect(
      view.container.querySelector('[data-testid="policies-page-form"]'),
    ).not.toBeNull();
    expect(
      view.container.querySelector('[data-testid="settings-save-bar"]'),
    ).not.toBeNull();
    const policyBody = view.container.querySelector(
      '[data-testid="policies-page-body"]',
    );
    const saveBar = view.container.querySelector(
      '[data-testid="settings-save-bar"]',
    );
    expect(policyBody?.contains(saveBar)).toBe(false);
    expect(saveBar?.parentElement?.getAttribute("data-testid")).toBe(
      "policies-page-form",
    );
    expect(view.container.textContent).toContain("Request Routing");
    expect(view.container.textContent).toContain("Password Change Permissions");
    expect(view.container.textContent).toContain("LLM Behavior");
    expect(view.container.textContent).not.toContain("Memory");
    expect(view.container.textContent).toContain("Unsaved changes");
    expect(view.container.textContent).not.toContain("Providers");
    expect(view.container.textContent).not.toContain("Open Passwords");
    expect(view.container.textContent).not.toContain("Open Connections");
    expect(
      view.container.querySelector('nav[aria-label="Settings categories"]'),
    ).toBeNull();

    view.unmount();
  });

  it("keeps password change auto-approval permissions fail-closed until LLM automatic is selected", () => {
    const manualController = buildSettingsController();
    const view = render(policiesPage(manualController));
    const manualDelete = view.container.querySelector<HTMLInputElement>(
      '[data-settings-field="llm_auto_approve_password_deletes"]',
    );
    expect(manualDelete?.type).toBe("checkbox");
    expect(manualDelete?.disabled).toBe(true);
    expect(view.container.textContent).toContain(
      "Select LLM Automatic above to activate these permissions",
    );

    const automaticController = buildSettingsController({
      settingsDraft: {
        ...SETTINGS,
        default_policy_mode: "llm_automatic",
        llm_auto_approve_password_edits: true,
      },
    });
    view.rerender(policiesPage(automaticController));
    const automaticDelete = view.container.querySelector<HTMLInputElement>(
      '[data-settings-field="llm_auto_approve_password_deletes"]',
    );
    expect(automaticDelete?.disabled).toBe(false);
    act(() => automaticDelete?.click());
    expect(automaticController.onFieldChange).toHaveBeenCalledWith(
      "llm_auto_approve_password_deletes",
      "true",
    );

    view.unmount();
  });

  it("keeps allow required and prevents disabling the final non-allow outcome", () => {
    const controller = buildSettingsController({
      settingsDraft: {
        ...SETTINGS,
        llm_approval_deny_enabled: false,
        llm_approval_escalate_enabled: true,
      },
    });
    const view = render(policiesPage(controller));
    const allow = view.container.querySelector<HTMLInputElement>(
      '[data-settings-field="llm_approval_allow_enabled"]',
    );
    const deny = view.container.querySelector<HTMLInputElement>(
      '[data-settings-field="llm_approval_deny_enabled"]',
    );
    const escalate = view.container.querySelector<HTMLInputElement>(
      '[data-settings-field="llm_approval_escalate_enabled"]',
    );

    expect(allow?.checked).toBe(true);
    expect(allow?.disabled).toBe(true);
    expect(deny?.disabled).toBe(false);
    expect(escalate?.disabled).toBe(true);
    expect(view.container.textContent).toContain("Last fallback");

    act(() => deny?.click());
    expect(controller.onFieldChange).toHaveBeenCalledWith(
      "llm_approval_deny_enabled",
      "true",
    );

    view.unmount();
  });

  it("blocks an invalid ACP draft and directs correction to Agents & Models", () => {
    const invalidPinned: DesktopSettings = {
      ...SETTINGS,
      acp_profile: {
        agent_kind: "codex",
        version_mode: "pinned",
        version: "latest",
        program: null,
        args: [],
      },
    };
    const controller = buildSettingsController({
      settingsDraft: invalidPinned,
      hasUnsavedChanges: true,
      canSave: false,
      validationMessage:
        "Pinned ACP profiles require an exact semantic version such as 1.2.3.",
    });
    const view = render(policiesPage(controller));

    expect(
      view.container.querySelector('[data-testid="settings-save-validation"]')
        ?.textContent,
    ).toContain("exact semantic version");
    const openAgents = view.container.querySelector<HTMLButtonElement>(
      '[data-testid="open-agents-settings"]',
    );
    expect(openAgents?.textContent).toBe("Open Agents & Models");
    expect(
      view.container.querySelector('input[placeholder="1.2.3"]'),
    ).toBeNull();
    expect(
      view.container.querySelectorAll(
        '[role="alert"][data-testid="settings-save-validation"]',
      ),
    ).toHaveLength(1);
    const save = view.container.querySelector<HTMLButtonElement>(
      '[data-testid="save-settings-button"]',
    );
    expect(save?.disabled).toBe(true);
    expect(controller.onSave).not.toHaveBeenCalled();

    view.unmount();
  });

  it("keeps policy form focus while surfacing save errors and notices", () => {
    const controller = buildSettingsController({
      settingsDraft: { ...SETTINGS, llm_advice_template: "Review decisions" },
      errorMessage: "The settings file is read-only.",
      hasUnsavedChanges: true,
      canSave: true,
    });
    const view = render(policiesPage(controller));

    const bodyBefore = view.container.querySelector<HTMLElement>(
      '[data-testid="policies-page-body"]',
    );
    const inputBefore = view.container.querySelector<HTMLTextAreaElement>(
      '[data-settings-field="llm_advice_template"]',
    );
    expect(
      view.container.querySelector('[data-testid="settings-error-banner"]')
        ?.textContent,
    ).toContain("read-only");
    act(() => {
      if (bodyBefore) bodyBefore.scrollTop = 240;
      inputBefore?.focus();
      inputBefore?.setSelectionRange(3, 3);
    });

    view.rerender(
      policiesPage(
        buildSettingsController({
          settingsDraft: {
            ...SETTINGS,
            llm_advice_template: "Review decisions",
          },
          noticeMessage: "Desktop settings saved",
        }),
      ),
    );

    const bodyAfter = view.container.querySelector<HTMLElement>(
      '[data-testid="policies-page-body"]',
    );
    const inputAfter = view.container.querySelector<HTMLTextAreaElement>(
      '[data-settings-field="llm_advice_template"]',
    );
    expect(bodyAfter).toBe(bodyBefore);
    expect(bodyAfter?.scrollTop).toBe(240);
    expect(inputAfter).toBe(inputBefore);
    expect(document.activeElement).toBe(inputAfter);
    expect(inputAfter?.selectionStart).toBe(3);
    expect(
      view.container.querySelector('[data-testid="settings-notice-banner"]')
        ?.textContent,
    ).toContain("Desktop settings saved");
    expect(
      view.container.querySelector('[data-testid="settings-error-banner"]'),
    ).toBeNull();

    view.unmount();
  });

  it("keeps provider and ACP controls exclusively on Agents & Models", () => {
    const controller = buildSettingsController({
      settingsDraft: {
        ...SETTINGS,
        openai_api_key: "openai-placeholder",
        claude_api_key: "claude-placeholder",
      },
    });
    const view = render(policiesPage(controller));
    expect(
      view.container.querySelector('[data-settings-field="openai_api_key"]'),
    ).toBeNull();
    expect(
      view.container.querySelector('[data-testid="settings-acp-section"]'),
    ).toBeNull();

    view.unmount();

    const chinese = render(policiesPage(controller, "zh-CN"));
    expect(chinese.container.textContent).toContain("策略");
    expect(chinese.container.textContent).toContain("密码库变更权限");
    expect(chinese.container.textContent).not.toContain("打开密码库");
    chinese.unmount();
  });

  it("uses guided loading and retry states when policies are unavailable", () => {
    const loading = render(
      policiesPage(
        buildSettingsController({
          settings: null,
          settingsDraft: null,
          isLoading: true,
        }),
      ),
    );
    expect(
      loading.container.querySelector('.settings-loading[role="status"]')
        ?.textContent,
    ).toContain("Loading policies");
    expect(loading.container.querySelector(".page-header-status")).toBeNull();
    loading.unmount();

    const controller = buildSettingsController({
      settings: null,
      settingsDraft: null,
      isLoading: false,
      errorMessage: "settings.toml is unavailable",
    });
    const failed = render(policiesPage(controller));
    expect(
      failed.container.querySelector('[data-state="error"]')?.textContent,
    ).toContain("settings.toml is unavailable");
    expect(failed.container.querySelector(".page-header-status")).toBeNull();
    expect(failed.container.textContent).not.toContain("All changes saved");
    const retry = Array.from(
      failed.container.querySelectorAll<HTMLButtonElement>("button"),
    ).find((button) => button.textContent === "Retry loading");
    act(() => retry?.click());
    expect(controller.onReload).toHaveBeenCalledTimes(1);
    failed.unmount();

    const missingSource = render(
      policiesPage(
        buildSettingsController({
          settings: null,
          settingsDraft: SETTINGS,
          isLoading: false,
        }),
      ),
    );
    expect(
      missingSource.container.querySelector(".settings-save-bar")?.textContent,
    ).toContain("Policies unavailable");
    expect(
      missingSource.container.querySelector(".settings-save-bar")?.textContent,
    ).not.toContain("All changes saved");
    missingSource.unmount();
  });

  it("does not duplicate ACP editing on the policies page", () => {
    const controller = buildSettingsController();
    const view = render(policiesPage(controller));
    expect(
      view.container.querySelector('[data-testid="settings-acp-version-mode"]'),
    ).toBeNull();
    expect(view.container.textContent).not.toContain("Test connection");

    view.unmount();
  });
});

describe("AgentsPage", () => {
  it("configures an OpenAI-compatible endpoint, token, and model on the runtime page", () => {
    const openAiSettings: DesktopSettings = {
      ...SETTINGS,
      provider_kind: "openai_compatible",
      openai_api_base: "https://models.example.test/v1",
      openai_api_key: "saved-token",
      openai_model: "custom-model",
    };
    const view = renderPage(
      "agents",
      buildSettingsController({
        settings: openAiSettings,
        settingsDraft: openAiSettings,
      }),
    );

    const section = view.container.querySelector(
      '[data-testid="agents-openai-section"]',
    );
    expect(section?.textContent).toContain("OpenAI Compatible");
    expect(section?.textContent).toContain("Base URL");
    expect(section?.textContent).toContain("API Key");
    expect(section?.textContent).toContain("Model");
    expect(
      Array.from(section?.querySelectorAll("input") ?? []).map(
        (input) => input.value,
      ),
    ).toEqual(
      expect.arrayContaining([
        "https://models.example.test/v1",
        "saved-token",
        "custom-model",
      ]),
    );
    expect(
      view.container.querySelector('[data-testid="settings-acp-section"]'),
    ).toBeNull();
    view.unmount();
  });

  it("centers one readable controller form with a separate runtime status panel", () => {
    const view = renderPage("agents");

    expect(
      view.container.querySelector('[data-testid="agents-controller"]'),
    ).not.toBeNull();
    expect(
      view.container.querySelector('[data-testid="agents-runtime-status"]'),
    ).not.toBeNull();
    expect(view.container.querySelectorAll(".agents-controller")).toHaveLength(
      1,
    );
    expect(invoke).not.toHaveBeenCalledWith("desktop_settings");
    view.unmount();
  });

  it("keeps custom command controls inside an advanced section only", () => {
    const customSettings: DesktopSettings = {
      ...SETTINGS,
      acp_profile: {
        agent_kind: "custom",
        version_mode: "custom",
        version: null,
        program: "/opt/bin/custom-acp",
        args: ["serve"],
      },
    };
    const view = renderPage(
      "agents",
      buildSettingsController({
        settings: customSettings,
        settingsDraft: customSettings,
      }),
    );

    const advanced = view.container.querySelector("details.agents-advanced");
    expect(advanced?.textContent).toContain("Program");
    expect(advanced?.textContent).toContain("Arguments");
    expect(
      view.container.querySelector(
        '[data-testid="settings-acp-version-mode"] input[value="custom"]',
      ),
    ).not.toBeNull();
    view.unmount();
  });

  it("edits and saves the shared settings draft without loading a second controller", () => {
    const controller = buildSettingsController({
      settingsDraft: {
        ...SETTINGS,
        llm_advice_template: "Keep this unsaved General change",
      },
      hasUnsavedChanges: true,
      canSave: true,
    });
    const view = renderPage("agents", controller);

    expect(invoke).not.toHaveBeenCalledWith("desktop_settings");
    const versionMode = view.container.querySelector<HTMLFieldSetElement>(
      '[data-testid="settings-acp-version-mode"]',
    );
    act(() => {
      if (!versionMode) return;
      chooseRadio(versionMode, "pinned");
    });
    expect(controller.onAcpProfileChange).toHaveBeenCalledWith(
      expect.objectContaining({ version_mode: "pinned" }),
    );

    const save = view.container.querySelector<HTMLButtonElement>(
      '[data-testid="agents-save-settings-button"]',
    );
    act(() => save?.click());
    expect(controller.onSave).toHaveBeenCalledTimes(1);
    expect(invoke).not.toHaveBeenCalledWith(
      "save_desktop_settings",
      expect.anything(),
    );
    view.unmount();
  });

  it("distinguishes the configured selector from the resolved ACP runtime version", async () => {
    invoke.mockImplementation((command: string) => {
      if (command === "test_acp_connection") {
        return Promise.resolve({
          configured_selector: "@agentclientprotocol/codex-acp@latest",
          program: "npx",
          args: ["-y", "@agentclientprotocol/codex-acp@latest"],
          package_name: "@agentclientprotocol/codex-acp",
          package_selector: "latest",
          agent_name: "codex-acp",
          agent_version: "0.42.0",
          protocol_version: "1",
          basic: { status: "passed", error: null },
          readiness: { status: "passed", error: null },
        });
      }
      return Promise.reject(new Error(`unexpected command ${command}`));
    });
    const view = renderPage("agents");

    await act(async () => {});
    expect(view.container.textContent).toContain(
      "@agentclientprotocol/codex-acp@latest",
    );

    const testConnection = Array.from(
      view.container.querySelectorAll("button"),
    ).find((button) => button.textContent === "Test connection");
    expect(testConnection).toBeDefined();
    await act(async () => {
      testConnection?.click();
    });

    expect(view.container.textContent).toContain("Adapter runtime");
    expect(view.container.textContent).toContain("0.42.0");
    expect(view.container.textContent).toContain("Basic connection");
    expect(view.container.textContent).toContain("Model readiness");
    expect(view.container.textContent).toContain(
      "npx -y @agentclientprotocol/codex-acp@latest",
    );
    view.unmount();
  });

  it("shows a model-readiness incompatibility without hiding the successful basic check", async () => {
    invoke.mockImplementation((command: string) => {
      if (command === "test_acp_connection") {
        return Promise.resolve({
          configured_selector: "@agentclientprotocol/codex-acp@latest",
          program: "npx",
          args: ["-y", "@agentclientprotocol/codex-acp@latest"],
          package_name: "@agentclientprotocol/codex-acp",
          package_selector: "latest",
          agent_name: "codex-acp",
          agent_version: "0.42.0",
          protocol_version: "1",
          basic: { status: "passed", error: null },
          readiness: {
            status: "failed",
            error: {
              kind: "protocol",
              code: -32099,
              message: "Codex model protocol version incompatible",
              data: { adapterProtocol: 1, runtimeProtocol: 2 },
            },
          },
        });
      }
      return Promise.reject(new Error(`unexpected command ${command}`));
    });
    const view = renderPage("agents");

    await act(async () => {});
    const testConnection = Array.from(
      view.container.querySelectorAll("button"),
    ).find((button) => button.textContent === "Test connection");
    await act(async () => {
      testConnection?.click();
    });

    expect(view.container.textContent).toContain("Basic connection");
    expect(view.container.textContent).toContain("Passed");
    expect(view.container.textContent).toContain("Model readiness");
    expect(view.container.textContent).toContain("Failed");
    expect(view.container.textContent).toContain("Protocol · -32099");
    expect(view.container.textContent).toContain(
      "Codex model protocol version incompatible",
    );
    expect(view.container.textContent).toContain('"runtimeProtocol":2');
    view.unmount();
  });

  it("keeps ACP probe failures visible", async () => {
    invoke.mockImplementation((command: string) => {
      if (command === "test_acp_connection") {
        return Promise.reject(new Error("ACP initialize timed out"));
      }
      return Promise.reject(new Error(`unexpected command ${command}`));
    });
    const view = renderPage("agents");

    await act(async () => {});
    const testConnection = Array.from(
      view.container.querySelectorAll("button"),
    ).find((button) => button.textContent === "Test connection");
    await act(async () => {
      testConnection?.click();
    });

    expect(
      view.container.querySelector('[role="alert"]')?.textContent,
    ).toContain("ACP initialize timed out");
    view.unmount();
  });

  it("blocks saving an empty pinned version and shows the validation error", async () => {
    const invalidPinnedSettings: DesktopSettings = {
      ...SETTINGS,
      acp_profile: {
        agent_kind: "codex",
        version_mode: "pinned",
        version: null,
        program: null,
        args: [],
      },
    };
    const controller = buildSettingsController({
      settingsDraft: invalidPinnedSettings,
      hasUnsavedChanges: true,
      canSave: false,
      validationMessage:
        "Pinned ACP profiles require an exact semantic version such as 1.2.3.",
    });
    const view = renderPage("agents", controller);

    await act(async () => {});

    expect(
      view.container.querySelector('[role="alert"]')?.textContent,
    ).toContain("Pinned ACP profiles require an exact semantic version");
    const save = Array.from(view.container.querySelectorAll("button")).find(
      (button) => button.textContent === "Save",
    );
    expect(save?.disabled).toBe(true);
    expect(controller.onSave).not.toHaveBeenCalled();
    expect(invoke).not.toHaveBeenCalled();
    view.unmount();
  });
});

describe("visible operation code labels", () => {
  const resolvedRequest: AccessRequest = {
    ...evaluationRequest("completed", "manual_only"),
    approval_status: "approved",
    final_decision: "allow",
    resolved_at: "2026-07-30T12:00:00Z",
  };

  it("translates request policy, status, and decision codes", () => {
    const english = renderRequests(resolvedRequest, "en");
    expect(english.container.textContent).toContain("Manual only");
    expect(english.container.textContent).toContain("Decision: Approved");
    english.unmount();

    const chinese = renderRequests(resolvedRequest, "zh-CN");
    expect(chinese.container.textContent).toContain("仅人工审批");
    expect(chinese.container.textContent).toContain("决定：已批准");
    chinese.unmount();
  });

  it("translates sync kind/status, agent mode, and daemon health in both locales", async () => {
    invoke.mockImplementation((command: string) => {
      if (command === "list_backend_connections") return Promise.resolve([]);
      if (command === "list_sync_credential_resources") {
        return Promise.resolve([]);
      }
      if (command === "list_local_vaults") return Promise.resolve([]);
      if (command === "list_sync_connections") {
        return Promise.resolve([
          {
            vault_id: "default",
            adapter_id: "primary",
            remote_revision: null,
            last_attempt_at: null,
            last_success_at: null,
            status: "idle",
            error_id: null,
            config: { kind: "local_folder", directory: "/tmp/vault" },
          },
        ]);
      }
      if (command === "daemon_health") {
        return Promise.resolve({
          protocol_version: 1,
          health: "ready",
          pid: 42,
          started_at: "2026-07-30T12:00:00Z",
        });
      }
      if (command === "list_diagnostic_errors") {
        return Promise.resolve(diagnosticPage([]));
      }
      return Promise.reject(new Error(`unexpected command ${command}`));
    });

    const englishConnections = renderPage("connections");
    await act(async () => {});
    expect(englishConnections.container.textContent).toContain(
      "Local folder · Idle",
    );
    expect(englishConnections.container.textContent).not.toContain(
      "local_folder",
    );
    englishConnections.unmount();
    const chineseConnections = renderPage(
      "connections",
      buildSettingsController(),
      "zh-CN",
    );
    await act(async () => {});
    expect(chineseConnections.container.textContent).toContain(
      "本地文件夹 · 空闲",
    );
    chineseConnections.unmount();

    const englishAgents = renderPage("agents");
    expect(
      englishAgents.container.querySelector(
        '[data-testid="agents-runtime-status"]',
      )?.textContent,
    ).toContain("ProtocolACP");
    englishAgents.unmount();
    const chineseAgents = renderPage(
      "agents",
      buildSettingsController(),
      "zh-CN",
    );
    expect(
      chineseAgents.container.querySelector(
        '[data-testid="agents-runtime-status"]',
      )?.textContent,
    ).toContain("协议ACP");
    chineseAgents.unmount();

    const englishDiagnostics = renderPage("diagnostics");
    await act(async () => {});
    expect(
      englishDiagnostics.container.querySelector(".daemon-status-strip")
        ?.textContent,
    ).toContain("DaemonReady");
    englishDiagnostics.unmount();
    const chineseDiagnostics = renderPage(
      "diagnostics",
      buildSettingsController(),
      "zh-CN",
    );
    await act(async () => {});
    expect(
      chineseDiagnostics.container.querySelector(".daemon-status-strip")
        ?.textContent,
    ).toContain("Daemon正常");
    chineseDiagnostics.unmount();
  });
});

describe("AuditPage", () => {
  function auditDashboard(count: number): DashboardData {
    return {
      pending_requests: [],
      recent_audit_records: Array.from({ length: count }, (_, index) => ({
        id: `audit-${index}`,
        request_id: `request-${index}`,
        action: index % 2 === 0 ? "request_approved" : "request_rejected",
        actor: index % 3 === 0 ? "alice" : "daemon",
        note: index === 0 ? "A long audit note ".repeat(30) : null,
        payload: {
          result: index % 2 === 0 ? "approved" : "rejected",
          nested: { index, context: "审计上下文 ".repeat(20) },
        },
        created_at: `2026-07-${String(30 - (index % 20)).padStart(2, "0")}T12:00:00Z`,
      })),
    };
  }

  it("uses a real empty state without a fake numbered record", () => {
    const view = render(
      <OperationsPage
        dashboard={auditDashboard(0)}
        focusedRequestId={null}
        isSubmitting={false}
        locale="en"
        noteDraft=""
        onDecision={async () => {}}
        onNavigate={() => {}}
        onNoteChange={() => {}}
        settingsController={buildSettingsController()}
        view="audit"
      />,
    );

    expect(view.container.querySelector('[data-state="empty"]')).not.toBeNull();
    expect(view.container.textContent).not.toContain("00");
    view.unmount();
  });

  it("keeps audit loading distinct from an empty audit trail", () => {
    const view = render(
      <OperationsPage
        dashboard={null}
        focusedRequestId={null}
        isSubmitting={false}
        locale="en"
        noteDraft=""
        onDecision={async () => {}}
        onNavigate={() => {}}
        onNoteChange={() => {}}
        settingsController={buildSettingsController()}
        view="audit"
      />,
    );

    expect(view.container.querySelector('[role="status"]')?.textContent).toBe(
      "Loading audit events…",
    );
    expect(view.container.querySelector('[data-state="empty"]')).toBeNull();
    view.unmount();
  });

  it("groups approvals, resets pagination, and opens keyboard-accessible event detail", () => {
    const view = render(
      <OperationsPage
        dashboard={auditDashboard(105)}
        focusedRequestId={null}
        isSubmitting={false}
        locale="en"
        noteDraft=""
        onDecision={async () => {}}
        onNavigate={() => {}}
        onNoteChange={() => {}}
        settingsController={buildSettingsController()}
        view="audit"
      />,
    );

    expect(view.container.querySelector(".audit-approval-list")).not.toBeNull();
    expect(
      view.container.querySelector('[aria-label="Audit pagination"]'),
    ).not.toBeNull();
    const next = view.container.querySelector<HTMLButtonElement>(
      'button[aria-label="Next audit page"]',
    );
    act(() => next?.click());
    expect(
      view.container.querySelector('[aria-label="Audit pagination"]')
        ?.textContent,
    ).toContain("2 / 6");

    const actor = view.container.querySelector<HTMLSelectElement>(
      'select[aria-label="Audit actor"]',
    );
    act(() => {
      if (!actor) return;
      actor.value = "alice";
      actor.dispatchEvent(new Event("change", { bubbles: true }));
    });
    expect(
      view.container.querySelector('[aria-label="Audit pagination"]')
        ?.textContent,
    ).toContain("1 / 2");

    const first = view.container.querySelector<HTMLButtonElement>(
      ".audit-approval-row",
    );
    act(() => first?.click());
    expect(first?.getAttribute("aria-expanded")).toBe("true");
    const historicalChat = view.container.querySelector(
      ".audit-approval-detail .approval-chat",
    );
    expect(historicalChat).not.toBeNull();
    expect(historicalChat?.textContent).toContain("Chat with the review agent");
    expect((historicalChat as HTMLDetailsElement | null)?.open).toBe(false);
    expect(workspaceStyles).toMatch(
      /\.audit-approval-detail\s*>\s*\.approval-chat\s*\{[^}]*grid-column:\s*1\s*\/\s*-1/s,
    );
    const event =
      view.container.querySelector<HTMLButtonElement>(".audit-phase-event");
    act(() => event?.click());
    const drawer = view.container.querySelector('[role="dialog"]');
    expect(drawer?.textContent).toContain("Event details");
    expect(drawer?.textContent).toContain('"nested"');
    expect(drawer?.textContent).toContain("审计上下文");
    act(() =>
      document.dispatchEvent(
        new KeyboardEvent("keydown", { key: "Escape", bubbles: true }),
      ),
    );
    expect(view.container.querySelector('[role="dialog"]')).toBeNull();
    view.unmount();
  });

  it("keeps node assessments and precise exposure annotations in audit history", () => {
    const exposureReport = {
      chain_summary: "The CLI hands one identifier to a bounded query process.",
      node_assessments: [
        {
          node_index: 0,
          summary: "Requests one provider-neutral field.",
          capabilities: ["credential read", "stdout"],
        },
      ],
      surfaces: [
        {
          surface: "llm_context",
          actual_level: 1,
          evidence_state: "observed",
          summary: "The identifier is returned to the requesting model.",
          annotations: [
            {
              reason: "The get subcommand selects this field.",
              target: {
                kind: "argument_quote",
                node_index: 0,
                argument_index: 2,
                quote: "field/demo",
                occurrence: 0,
              },
            },
          ],
        },
        {
          surface: "network",
          actual_level: 1,
          evidence_state: "observed",
          summary: "Sent only to the declared endpoint.",
          annotations: [
            {
              reason: "The request target is fixed here.",
              target: {
                kind: "source_quote",
                node_index: 0,
                source_id: "file:/workspace/query.py",
                start_line: 14,
                end_line: 18,
                quote: "endpoint",
                occurrence: 0,
              },
            },
          ],
        },
        ...["local_persistence", "terminal_log", "process_propagation"].map(
          (surface) => ({
            surface,
            actual_level: surface === "process_propagation" ? 1 : 0,
            evidence_state: "not_observed",
            summary: `No unexpected ${surface} exposure.`,
            annotations: [],
          }),
        ),
      ],
    };
    const dashboard: DashboardData = {
      pending_requests: [],
      recent_audit_records: [
        {
          id: "audit-submit",
          request_id: "request-with-exposure",
          action: "request_submitted",
          actor: "alice",
          note: "Run one bounded query.",
          payload: {
            resource: "SECRET_ID",
            policy_mode: "assisted",
            resource_metadata: {
              credential_exposure_policy_v1: JSON.stringify({
                access_mode: "protected",
                breach_action: "human_review",
                surfaces: [
                  { surface: "llm_context", max_level: 0 },
                  { surface: "network", max_level: 1 },
                  { surface: "local_persistence", max_level: 0 },
                  { surface: "terminal_log", max_level: 0 },
                  { surface: "process_propagation", max_level: 1 },
                ],
              }),
            },
          },
          created_at: "2026-07-30T12:00:00Z",
        },
        {
          id: "audit-model",
          request_id: "request-with-exposure",
          action: "llm_suggestion_generated",
          actor: "acp",
          note: "One surface needs human review.",
          payload: {
            suggested_decision: "escalate",
            risk_score: 47,
            exposure_report: exposureReport,
            provider_trace: {
              review_progress: {
                state: "complete",
                completed_units: 7,
                total_units: 7,
                error: null,
                updated_at: "2026-07-30T12:00:01Z",
              },
            },
          },
          created_at: "2026-07-30T12:00:01Z",
        },
        {
          id: "audit-approval",
          request_id: "request-with-exposure",
          action: "approval_recorded",
          actor: "desktop-reviewer",
          note: "Reviewed with evidence.",
          payload: { decision: "allow", approval_status: "approved" },
          created_at: "2026-07-30T12:00:02Z",
        },
      ],
    };
    const view = render(
      <OperationsPage
        dashboard={dashboard}
        focusedRequestId={null}
        isSubmitting={false}
        locale="en"
        noteDraft=""
        onDecision={async () => {}}
        onNavigate={() => {}}
        onNoteChange={() => {}}
        settingsController={buildSettingsController()}
        view="audit"
      />,
    );

    act(() =>
      view.container
        .querySelector<HTMLButtonElement>(".audit-approval-row")
        ?.click(),
    );
    const snapshot = view.container.querySelector(".audit-exposure-snapshot");
    expect(snapshot?.textContent).not.toContain(
      "Requests one provider-neutral field.",
    );
    expect(snapshot?.textContent).not.toContain(
      "The get subcommand selects this field.",
    );
    expect(snapshot?.querySelector("ol")).toBeNull();
    expect(snapshot?.querySelector(".request-review-progress")).toBeNull();
    expect(view.container.querySelector(".request-review-progress")).toBeNull();
    view.unmount();
  });

  it("loads local call-chain evidence into audit history and renders precise source marks", async () => {
    const request = {
      ...annotatedRequest(10),
      id: "request-audit-evidence",
    };
    invoke.mockImplementation(async (command: string) => {
      if (command === "request_evidence") return request;
      throw new Error(`Unexpected command: ${command}`);
    });
    const dashboard: DashboardData = {
      pending_requests: [],
      recent_audit_records: [
        {
          id: "audit-submit",
          request_id: request.id,
          action: "request_submitted",
          actor: "codex",
          note: "Inspect a fixed endpoint.",
          payload: { resource: "SECRET_ID", policy_mode: "llm_automatic" },
          created_at: "2026-07-30T12:00:00Z",
        },
        {
          id: "audit-model",
          request_id: request.id,
          action: "llm_suggestion_generated",
          actor: "acp",
          note: "Evidence complete.",
          payload: {
            suggested_decision: "escalate",
            risk_score: 42,
            exposure_report: request.llm_suggestion?.exposure_report,
            provider_trace: request.llm_suggestion?.provider_trace,
          },
          created_at: "2026-07-30T12:00:01Z",
        },
      ],
    };
    const view = render(
      <OperationsPage
        dashboard={dashboard}
        focusedRequestId={null}
        isSubmitting={false}
        locale="zh-CN"
        noteDraft=""
        onDecision={async () => {}}
        onNavigate={() => {}}
        onNoteChange={() => {}}
        settingsController={buildSettingsController()}
        view="audit"
      />,
    );

    await act(async () => {
      view.container
        .querySelector<HTMLButtonElement>(".audit-approval-row")
        ?.click();
      await Promise.resolve();
      await Promise.resolve();
    });

    expect(invoke).toHaveBeenCalledWith("request_evidence", {
      requestId: request.id,
    });
    const evidence = view.container.querySelector(".audit-call-chain-evidence");
    expect(evidence?.textContent).toContain("审批调用链与精确标记");
    expect(evidence?.textContent).toContain("/workspace/query.py");
    expect(
      evidence?.querySelector(".request-evidence-workbench details"),
    ).toBeNull();
    expect(
      evidence?.querySelector(".request-evidence-workbench__origin pre mark")
        ?.textContent,
    ).toContain("endpoint");
    expect(
      evidence?.querySelector(".request-evidence-workbench__node")?.textContent,
    ).toContain("/workspace/query.py");
    view.unmount();
  });

  it("condenses streamed detail events in grouped audit view while preserving their count", () => {
    const updates = Array.from({ length: 10 }, (_, index) => ({
      id: `audit-detail-${index}`,
      request_id: "request-streamed-details",
      action: "llm_review_details_updated",
      actor: "acp",
      note: `Annotation update ${index + 1}`,
      payload: {
        provider_trace: {
          review_progress: {
            state: index === 9 ? "complete" : "running",
            completed_units: index + 1,
            total_units: 10,
            error: null,
            updated_at: `2026-07-30T12:00:${String(index).padStart(2, "0")}Z`,
          },
        },
      },
      created_at: `2026-07-30T12:00:${String(index).padStart(2, "0")}Z`,
    }));
    const dashboard: DashboardData = {
      pending_requests: [],
      recent_audit_records: [
        {
          id: "audit-submit",
          request_id: "request-streamed-details",
          action: "request_submitted",
          actor: "codex",
          note: "Stream annotations.",
          payload: { resource: "SECRET_ID", policy_mode: "llm_automatic" },
          created_at: "2026-07-30T11:59:59Z",
        },
        ...updates,
      ],
    };
    const view = render(
      <OperationsPage
        dashboard={dashboard}
        focusedRequestId={null}
        isSubmitting={false}
        locale="zh-CN"
        noteDraft=""
        onDecision={async () => {}}
        onNavigate={() => {}}
        onNoteChange={() => {}}
        settingsController={buildSettingsController()}
        view="audit"
      />,
    );

    act(() =>
      view.container
        .querySelector<HTMLButtonElement>(".audit-approval-row")
        ?.click(),
    );
    const updateRows = Array.from(
      view.container.querySelectorAll(".audit-phase-event"),
    ).filter((row) => row.textContent?.includes("AI 审批细节已更新"));
    expect(updateRows).toHaveLength(1);
    expect(updateRows[0]?.textContent).toContain("10 次注解增量");
    view.unmount();
  });

  it("maps approval payload results, translates codes, and omits sensitive payload fields", () => {
    const dashboard: DashboardData = {
      pending_requests: [],
      recent_audit_records: [
        {
          id: "audit-approval",
          request_id: "request-approval",
          action: "approval_recorded",
          actor: "reviewer",
          note: null,
          payload: {
            approval_status: "approved",
            decision: "allow",
            password: "test-placeholder-value",
            nested: { authorization: "test-placeholder-header" },
          },
          created_at: "2026-07-30T12:00:00Z",
        },
      ],
    };
    const view = render(
      <OperationsPage
        dashboard={dashboard}
        focusedRequestId={null}
        isSubmitting={false}
        locale="en"
        noteDraft=""
        onDecision={async () => {}}
        onNavigate={() => {}}
        onNoteChange={() => {}}
        settingsController={buildSettingsController()}
        view="audit"
      />,
    );

    expect(view.container.textContent).toContain("Approved");
    const group = view.container.querySelector<HTMLButtonElement>(
      ".audit-approval-row",
    );
    act(() => group?.click());
    expect(view.container.textContent).toContain("Approval recorded");
    const event =
      view.container.querySelector<HTMLButtonElement>(".audit-phase-event");
    act(() => event?.click());
    expect(view.container.textContent).not.toContain("password");
    expect(view.container.textContent).not.toContain("authorization");
    expect(view.container.textContent).not.toContain("test-placeholder-value");
    expect(view.container.textContent).not.toContain("test-placeholder-header");
    view.unmount();

    const chinese = render(
      <OperationsPage
        dashboard={dashboard}
        focusedRequestId={null}
        isSubmitting={false}
        locale="zh-CN"
        noteDraft=""
        onDecision={async () => {}}
        onNavigate={() => {}}
        onNoteChange={() => {}}
        settingsController={buildSettingsController()}
        view="audit"
      />,
    );
    act(() =>
      chinese.container
        .querySelector<HTMLButtonElement>(".audit-approval-row")
        ?.click(),
    );
    expect(chinese.container.textContent).toContain("审批已记录");
    expect(chinese.container.textContent).toContain("已批准");
    chinese.unmount();
  });

  it("classifies real audit actions from only their action-specific payload contracts", () => {
    const dashboard: DashboardData = {
      pending_requests: [],
      recent_audit_records: [
        {
          id: "audit-llm-failed",
          request_id: "request-llm-failed",
          action: "llm_suggestion_failed",
          actor: "acp",
          note: "Provider timed out",
          payload: {
            approval_status: "pending",
            suggested_decision: "escalate",
          },
          created_at: "2026-07-30T12:03:00Z",
        },
        {
          id: "audit-auto-deny",
          request_id: "request-auto-deny",
          action: "automatic_decision_recorded",
          actor: "system_auto",
          note: "High risk",
          payload: {
            auto_disposition: "deny",
            decision: "deny",
            approval_status: "rejected",
          },
          created_at: "2026-07-30T12:02:00Z",
        },
        {
          id: "audit-auto-escalate",
          request_id: "request-auto-escalate",
          action: "automatic_escalated_to_human",
          actor: "system_auto",
          note: "Human review required",
          payload: {
            auto_disposition: "escalate",
            decision: "escalate",
          },
          created_at: "2026-07-30T12:01:00Z",
        },
        {
          id: "audit-llm-generated",
          request_id: "request-llm-generated",
          action: "llm_suggestion_generated",
          actor: "acp",
          note: "Advice generated",
          payload: {
            approval_status: "pending",
            suggested_decision: "allow",
          },
          created_at: "2026-07-30T12:00:00Z",
        },
      ],
    };
    const view = render(
      <OperationsPage
        dashboard={dashboard}
        focusedRequestId={null}
        isSubmitting={false}
        locale="en"
        noteDraft=""
        onDecision={async () => {}}
        onNavigate={() => {}}
        onNoteChange={() => {}}
        settingsController={buildSettingsController()}
        view="audit"
      />,
    );

    const rawEvents = Array.from(
      view.container.querySelectorAll<HTMLButtonElement>(
        ".request-history-views button",
      ),
    ).find((button) => button.textContent === "Raw events");
    act(() => rawEvents?.click());

    expect(
      Array.from(
        view.container.querySelectorAll<HTMLElement>(".audit-result"),
        (result) => result.textContent,
      ),
    ).toEqual(["Failed", "Rejected", "Human review requested", "Generated"]);
    expect(view.container.textContent).not.toContain(
      "AI advice failedrejected",
    );
    view.unmount();
  });
});

describe("DiagnosticsPage", () => {
  it("keeps initial loading, first-load error, no-match, and healthy states distinct", async () => {
    const health = deferred<{
      protocol_version: number;
      health: string;
      pid: number;
      started_at: string;
    }>();
    const records = deferred<ReturnType<typeof diagnosticPage>>();
    invoke.mockImplementation((command: string) => {
      if (command === "daemon_health") return health.promise;
      if (command === "list_diagnostic_errors") return records.promise;
      return Promise.reject(new Error(`unexpected command ${command}`));
    });
    const loading = renderPage("diagnostics");
    expect(
      loading.container.querySelector(
        '.diagnostic-errors-section [role="status"]',
      )?.textContent,
    ).toContain("Loading diagnostic errors");
    expect(loading.container.textContent).not.toContain("System healthy");
    loading.unmount();

    invoke.mockImplementation((command: string) => {
      if (command === "daemon_health") {
        return Promise.resolve({
          protocol_version: 1,
          health: "ready",
          pid: 42,
          started_at: "2026-07-30T12:00:00Z",
        });
      }
      if (command === "list_diagnostic_errors") {
        return Promise.reject(new Error("diagnostic page unavailable"));
      }
      return Promise.reject(new Error(`unexpected command ${command}`));
    });
    const failed = renderPage("diagnostics");
    await act(async () => {});
    expect(
      failed.container.querySelector(
        '.diagnostic-errors-section [data-state="error"]',
      )?.textContent,
    ).toContain("diagnostic page unavailable");
    expect(failed.container.textContent).not.toContain("System healthy");
    failed.unmount();

    let filter = "unacknowledged";
    invoke.mockImplementation((command: string, payload?: unknown) => {
      if (command === "daemon_health") {
        return Promise.resolve({
          protocol_version: 1,
          health: "ready",
          pid: 42,
          started_at: "2026-07-30T12:00:00Z",
        });
      }
      if (command === "list_diagnostic_errors") {
        filter =
          (payload as { severity: string | null }).severity ?? "unacknowledged";
        return Promise.resolve(
          filter === "warning"
            ? diagnosticPage([])
            : diagnosticPage([
                {
                  error: {
                    code: "timeout",
                    user_message: "Timed out",
                    severity: "error",
                    retryable: true,
                    timestamp: "2026-07-30T12:00:00Z",
                    correlation_id: "correlation-loading-states",
                    source: { kind: "acp" },
                  },
                  acknowledged_at: null,
                },
              ]),
        );
      }
      return Promise.reject(new Error(`unexpected command ${command}`));
    });
    const noMatch = renderPage("diagnostics");
    await act(async () => {});
    const severity = noMatch.container.querySelector<HTMLSelectElement>(
      'select[aria-label="Diagnostic severity"]',
    );
    await act(async () => {
      if (!severity) return;
      severity.value = "warning";
      severity.dispatchEvent(new Event("change", { bubbles: true }));
    });
    expect(
      noMatch.container.querySelector('[data-state="empty"]')?.textContent,
    ).toContain("No errors match");
    expect(noMatch.container.textContent).not.toContain("System healthy");
    noMatch.unmount();

    invoke.mockImplementation((command: string) => {
      if (command === "daemon_health") {
        return Promise.resolve({
          protocol_version: 1,
          health: "ready",
          pid: 42,
          started_at: "2026-07-30T12:00:00Z",
        });
      }
      if (command === "list_diagnostic_errors") {
        return Promise.resolve(diagnosticPage([]));
      }
      return Promise.reject(new Error(`unexpected command ${command}`));
    });
    const healthy = renderPage("diagnostics");
    await act(async () => {});
    expect(healthy.container.textContent).toContain("System healthy");
    healthy.unmount();
  });

  it("ignores stale refresh generations when filters start a newer page request", async () => {
    const oldHealth = deferred<{
      protocol_version: number;
      health: string;
      pid: number;
      started_at: string;
    }>();
    const oldRecords = deferred<ReturnType<typeof diagnosticPage>>();
    const newHealth = deferred<{
      protocol_version: number;
      health: string;
      pid: number;
      started_at: string;
    }>();
    const newRecords = deferred<ReturnType<typeof diagnosticPage>>();
    let healthCalls = 0;
    let recordCalls = 0;
    invoke.mockImplementation((command: string) => {
      if (command === "daemon_health") {
        healthCalls += 1;
        return healthCalls === 1 ? oldHealth.promise : newHealth.promise;
      }
      if (command === "list_diagnostic_errors") {
        recordCalls += 1;
        return recordCalls === 1 ? oldRecords.promise : newRecords.promise;
      }
      return Promise.reject(new Error(`unexpected command ${command}`));
    });
    const view = renderPage("diagnostics");
    const severity = view.container.querySelector<HTMLSelectElement>(
      'select[aria-label="Diagnostic severity"]',
    );
    act(() => {
      if (!severity) return;
      severity.value = "warning";
      severity.dispatchEvent(new Event("change", { bubbles: true }));
    });
    await act(async () => {
      newHealth.resolve({
        protocol_version: 2,
        health: "ready",
        pid: 99,
        started_at: "new-generation",
      });
      newRecords.resolve(diagnosticPage([]));
    });
    await act(async () => {
      oldHealth.resolve({
        protocol_version: 1,
        health: "degraded",
        pid: 1,
        started_at: "old-generation",
      });
      oldRecords.resolve(diagnosticPage([]));
    });

    expect(view.container.textContent).toContain("new-generation");
    expect(view.container.textContent).not.toContain("old-generation");
    expect(view.container.textContent).not.toContain("Degraded");
    view.unmount();
  });

  it("probes the unsaved ACP draft without loading a second settings source", async () => {
    const draftProfile = {
      agent_kind: "codex" as const,
      version_mode: "pinned" as const,
      version: "9.8.7",
      program: null,
      args: [],
    };
    invoke.mockImplementation((command: string, payload?: unknown) => {
      if (command === "daemon_health") {
        return Promise.resolve({
          protocol_version: 1,
          health: "ready",
          pid: 42,
          started_at: "2026-07-29T13:00:00Z",
        });
      }
      if (command === "list_diagnostic_errors") {
        return Promise.resolve(diagnosticPage([]));
      }
      if (command === "test_acp_connection") {
        expect(payload).toEqual({ profile: draftProfile });
        return Promise.resolve({
          configured_selector: "@agentclientprotocol/codex-acp@9.8.7",
          program: "npx",
          args: ["-y", "@agentclientprotocol/codex-acp@9.8.7"],
          package_name: "@agentclientprotocol/codex-acp",
          package_selector: "9.8.7",
          agent_name: "codex-acp",
          agent_version: "0.42.0",
          protocol_version: "1",
          basic: { status: "passed", error: null },
          readiness: {
            status: "failed",
            error: {
              kind: "protocol",
              code: -32099,
              message: "Codex model protocol version incompatible",
              data: { runtimeProtocol: 2 },
            },
          },
        });
      }
      return Promise.reject(new Error(`unexpected command ${command}`));
    });
    const view = renderPage(
      "diagnostics",
      buildSettingsController({
        settingsDraft: {
          ...SETTINGS,
          acp_profile: draftProfile,
        },
        hasUnsavedChanges: true,
      }),
    );

    await act(async () => {});
    const testAcp = Array.from(view.container.querySelectorAll("button")).find(
      (button) => button.textContent === "Run ACP probes",
    );
    expect(testAcp).toBeDefined();
    await act(async () => {
      testAcp?.click();
    });

    expect(view.container.textContent).toContain("Basic connection");
    expect(view.container.textContent).toContain("Model readiness");
    expect(view.container.textContent).toContain(
      "npx -y @agentclientprotocol/codex-acp@9.8.7",
    );
    expect(view.container.textContent).toContain(
      "Codex model protocol version incompatible",
    );
    expect(invoke).not.toHaveBeenCalledWith("desktop_settings");
    view.unmount();
  });

  it("shows diagnostic records even when the daemon health request fails", async () => {
    invoke.mockImplementation((command: string) => {
      if (command === "daemon_health") {
        return Promise.reject(new Error("daemon health unavailable"));
      }
      if (command === "list_diagnostic_errors") {
        return Promise.resolve(
          diagnosticPage([
            {
              error: {
                code: "backend_failed",
                user_message: "Backend command failed",
                internal_message: "exit status 17",
                public_context: { backend: "onepassword" },
                internal_context: { executable: "/usr/local/bin/op" },
                severity: "error",
                retryable: true,
                timestamp: "2026-07-29T14:00:00Z",
                correlation_id: "11111111-2222-3333-4444-555555555555",
                source: { kind: "backend", backend_id: "onepassword" },
              },
              acknowledged_at: null,
            },
          ]),
        );
      }
      return Promise.reject(new Error(`unexpected command ${command}`));
    });
    const view = renderPage("diagnostics");

    await act(async () => {});

    expect(view.container.textContent).toContain("daemon health unavailable");
    expect(view.container.textContent).toContain("Backend command failed");
    expect(view.container.textContent).toContain("Error");
    expect(view.container.textContent).toContain("2026-07-29T14:00:00Z");
    expect(view.container.textContent).toContain("Backend · onepassword");
    expect(view.container.textContent).toContain(
      "11111111-2222-3333-4444-555555555555",
    );
    expect(view.container.textContent).toContain("exit status 17");
    expect(view.container.textContent).toContain("/usr/local/bin/op");
    view.unmount();
  });

  it("refreshes health and diagnostics on demand", async () => {
    let healthCall = 0;
    invoke.mockImplementation((command: string) => {
      if (command === "daemon_health") {
        healthCall += 1;
        return Promise.resolve({
          protocol_version: 1,
          health: healthCall === 1 ? "degraded" : "ready",
          pid: 42,
          started_at: "2026-07-29T13:00:00Z",
        });
      }
      if (command === "list_diagnostic_errors") {
        return Promise.resolve(diagnosticPage([]));
      }
      return Promise.reject(new Error(`unexpected command ${command}`));
    });
    const view = renderPage("diagnostics");

    await act(async () => {});
    expect(view.container.textContent).toContain("Degraded");
    const refresh = Array.from(view.container.querySelectorAll("button")).find(
      (button) => button.textContent === "Refresh",
    );
    expect(refresh).toBeDefined();
    await act(async () => {
      refresh?.click();
    });

    expect(view.container.textContent).toContain("Ready");
    view.unmount();
  });

  it("keeps acknowledgement failures visible", async () => {
    invoke.mockImplementation((command: string) => {
      if (command === "daemon_health") {
        return Promise.resolve({
          protocol_version: 1,
          health: "ready",
          pid: 42,
          started_at: "2026-07-29T13:00:00Z",
        });
      }
      if (command === "list_diagnostic_errors") {
        return Promise.resolve(
          diagnosticPage([
            {
              error: {
                code: "timeout",
                user_message: "ACP initialize timed out",
                public_context: {},
                internal_context: {},
                severity: "error",
                retryable: true,
                timestamp: "2026-07-29T14:00:00Z",
                correlation_id: "11111111-2222-3333-4444-555555555555",
                source: { kind: "acp" },
              },
              acknowledged_at: null,
            },
          ]),
        );
      }
      if (command === "acknowledge_diagnostic_error") {
        return Promise.reject(new Error("diagnostic store is unavailable"));
      }
      return Promise.reject(new Error(`unexpected command ${command}`));
    });
    const view = renderPage("diagnostics");

    await act(async () => {});
    const acknowledge = Array.from(
      view.container.querySelectorAll("button"),
    ).find((button) => button.textContent === "Acknowledge");
    await act(async () => {
      acknowledge?.click();
    });

    expect(view.container.textContent).toContain(
      "diagnostic store is unavailable",
    );
    expect(view.container.textContent).toContain("ACP initialize timed out");
    view.unmount();
  });

  it("linearizes a successful acknowledgement after an older refresh started", async () => {
    const correlationId = "11111111-2222-3333-4444-555555555555";
    const record = diagnosticRecord(correlationId, "Must not resurrect");
    const staleRefresh = deferred<ReturnType<typeof diagnosticPage>>();
    const postAcknowledgement = deferred<ReturnType<typeof diagnosticPage>>();
    const acknowledgement = deferred<boolean>();
    let pageCalls = 0;
    invoke.mockImplementation((command: string) => {
      if (command === "daemon_health") {
        return Promise.resolve({
          protocol_version: 1,
          health: "ready",
          pid: 42,
          started_at: "2026-07-29T13:00:00Z",
        });
      }
      if (command === "list_diagnostic_errors") {
        pageCalls += 1;
        if (pageCalls === 1) return Promise.resolve(diagnosticPage([record]));
        if (pageCalls === 2) return staleRefresh.promise;
        return postAcknowledgement.promise;
      }
      if (command === "acknowledge_diagnostic_error") {
        return acknowledgement.promise;
      }
      return Promise.reject(new Error(`unexpected command ${command}`));
    });
    const view = renderPage("diagnostics");
    await act(async () => {});
    const refresh = Array.from(view.container.querySelectorAll("button")).find(
      (button) => button.textContent === "Refresh",
    );
    act(() => refresh?.click());
    const acknowledge = Array.from(
      view.container.querySelectorAll("button"),
    ).find((button) => button.textContent === "Acknowledge");
    act(() => acknowledge?.click());

    await act(async () => acknowledgement.resolve(true));
    expect(view.container.textContent).not.toContain("Must not resurrect");
    await act(async () => staleRefresh.resolve(diagnosticPage([record])));
    expect(view.container.textContent).not.toContain("Must not resurrect");
    await act(async () => postAcknowledgement.resolve(diagnosticPage([])));
    expect(view.container.textContent).not.toContain("Must not resurrect");
    view.unmount();
  });

  it("keeps an acknowledgement authoritative when it starts before a refresh", async () => {
    const correlationId = "22222222-3333-4444-5555-666666666666";
    const record = diagnosticRecord(correlationId, "Ack started first");
    const staleRefresh = deferred<ReturnType<typeof diagnosticPage>>();
    const postAcknowledgement = deferred<ReturnType<typeof diagnosticPage>>();
    const acknowledgement = deferred<boolean>();
    let pageCalls = 0;
    invoke.mockImplementation((command: string) => {
      if (command === "daemon_health") {
        return Promise.resolve({
          protocol_version: 1,
          health: "ready",
          pid: 42,
          started_at: "2026-07-29T13:00:00Z",
        });
      }
      if (command === "list_diagnostic_errors") {
        pageCalls += 1;
        if (pageCalls === 1) return Promise.resolve(diagnosticPage([record]));
        if (pageCalls === 2) return staleRefresh.promise;
        return postAcknowledgement.promise;
      }
      if (command === "acknowledge_diagnostic_error") {
        return acknowledgement.promise;
      }
      return Promise.reject(new Error(`unexpected command ${command}`));
    });
    const view = renderPage("diagnostics");
    await act(async () => {});
    const acknowledge = Array.from(
      view.container.querySelectorAll("button"),
    ).find((button) => button.textContent === "Acknowledge");
    act(() => acknowledge?.click());
    const refresh = Array.from(view.container.querySelectorAll("button")).find(
      (button) => button.textContent === "Refresh",
    );
    act(() => refresh?.click());

    await act(async () => acknowledgement.resolve(true));
    expect(view.container.textContent).not.toContain("Ack started first");
    await act(async () => staleRefresh.resolve(diagnosticPage([record])));
    expect(view.container.textContent).not.toContain("Ack started first");
    await act(async () => postAcknowledgement.resolve(diagnosticPage([])));
    expect(view.container.textContent).not.toContain("Ack started first");
    view.unmount();
  });

  it("applies only the newest of several concurrent refreshes", async () => {
    const second = deferred<ReturnType<typeof diagnosticPage>>();
    const third = deferred<ReturnType<typeof diagnosticPage>>();
    let pageCalls = 0;
    invoke.mockImplementation((command: string) => {
      if (command === "daemon_health") {
        return Promise.resolve({
          protocol_version: 1,
          health: "ready",
          pid: 42,
          started_at: "2026-07-29T13:00:00Z",
        });
      }
      if (command === "list_diagnostic_errors") {
        pageCalls += 1;
        if (pageCalls === 1) return Promise.resolve(diagnosticPage([]));
        if (pageCalls === 2) return second.promise;
        return third.promise;
      }
      return Promise.reject(new Error(`unexpected command ${command}`));
    });
    const view = renderPage("diagnostics");
    await act(async () => {});
    const severity = view.container.querySelector<HTMLSelectElement>(
      'select[aria-label="Diagnostic severity"]',
    );
    act(() => {
      if (!severity) return;
      severity.value = "warning";
      severity.dispatchEvent(new Event("change", { bubbles: true }));
      severity.value = "critical";
      severity.dispatchEvent(new Event("change", { bubbles: true }));
    });
    await act(async () =>
      third.resolve(
        diagnosticPage([
          diagnosticRecord(
            "33333333-4444-5555-6666-777777777777",
            "Newest refresh",
          ),
        ]),
      ),
    );
    await act(async () =>
      second.resolve(
        diagnosticPage([
          diagnosticRecord(
            "44444444-5555-6666-7777-888888888888",
            "Stale refresh",
          ),
        ]),
      ),
    );

    expect(view.container.textContent).toContain("Newest refresh");
    expect(view.container.textContent).not.toContain("Stale refresh");
    view.unmount();
  });

  it("shows a daemon strip, healthy empty state, filters, and conditional pagination for 100+ errors", async () => {
    const errors = Array.from({ length: 20 }, (_, index) => ({
      error: {
        code: index % 2 === 0 ? "timeout" : "backend_failed",
        user_message: `Diagnostic ${index} ${"完整错误 ".repeat(20)}`,
        internal_message: `stack-${index} ${"trace ".repeat(30)}`,
        public_context: { item: String(index) },
        internal_context: {},
        severity: index % 2 === 0 ? "error" : "warning",
        retryable: true,
        timestamp: "2026-07-30T14:00:00Z",
        correlation_id: `correlation-${index}`,
        source: { kind: "acp" },
      },
      acknowledged_at: null,
    }));
    invoke.mockImplementation((command: string, payload?: unknown) => {
      if (command === "daemon_health") {
        return Promise.resolve({
          protocol_version: 1,
          health: "ready",
          pid: 42,
          started_at: "2026-07-29T13:00:00Z",
        });
      }
      if (command === "list_diagnostic_errors") {
        const request = payload as {
          acknowledgement: string;
          page: number;
          pageSize: number;
          severity: string | null;
        };
        expect(request.pageSize).toBe(20);
        if (request.acknowledgement === "acknowledged") {
          return Promise.resolve(
            diagnosticPage(
              [
                {
                  ...errors[0],
                  acknowledged_at: "2026-07-30T15:00:00Z",
                },
              ],
              1,
            ),
          );
        }
        expect(request).toEqual({
          acknowledgement: "unacknowledged",
          page: 1,
          pageSize: 20,
          severity: null,
        });
        return Promise.resolve(diagnosticPage(errors, 105));
      }
      return Promise.reject(new Error(`unexpected command ${command}`));
    });
    const view = renderPage("diagnostics");
    await act(async () => {});

    expect(view.container.querySelector(".daemon-status-strip")).not.toBeNull();
    expect(
      view.container.querySelector(".diagnostic-error-list"),
    ).not.toBeNull();
    expect(
      view.container.querySelector('[aria-label="Diagnostic pagination"]'),
    ).not.toBeNull();
    const ackFilter = view.container.querySelector<HTMLFieldSetElement>(
      'fieldset[aria-label="Acknowledgement status"]',
    );
    await act(async () => {
      if (!ackFilter) return;
      chooseRadio(ackFilter, "acknowledged");
    });
    expect(view.container.textContent).toContain("Diagnostic 0");
    expect(view.container.textContent).not.toContain("Diagnostic 1 ");
    expect(view.container.textContent).toContain("完整错误");
    view.unmount();

    invoke.mockImplementation((command: string) => {
      if (command === "daemon_health") {
        return Promise.resolve({
          protocol_version: 1,
          health: "ready",
          pid: 42,
          started_at: "2026-07-29T13:00:00Z",
        });
      }
      if (command === "list_diagnostic_errors") {
        return Promise.resolve(diagnosticPage([]));
      }
      return Promise.reject(new Error(`unexpected command ${command}`));
    });
    const healthy = renderPage("diagnostics");
    await act(async () => {});
    expect(
      healthy.container.querySelector('[data-state="empty"]')?.textContent,
    ).toContain("No diagnostic errors");
    expect(
      healthy.container.querySelector('[aria-label="Diagnostic pagination"]'),
    ).toBeNull();
    healthy.unmount();
  });
});

describe("ConnectionsPage", () => {
  it("shows setup, health, and enabled state while preserving health-check failures", async () => {
    invoke.mockImplementation((command: string) => {
      if (command === "list_backend_connections") {
        return Promise.resolve([
          {
            id: "onepassword",
            backend_kind: "one_password",
            display_name: "1Password",
            enabled: false,
            capabilities: ["search", "read"],
            setup_status: "setup_required",
            health: "not_checked",
            detail: "Install the CLI and run `op signin`.",
          },
        ]);
      }
      if (command === "list_sync_connections") return Promise.resolve([]);
      if (command === "list_local_vaults") return Promise.resolve([]);
      if (command === "list_sync_credential_resources") {
        return Promise.resolve([]);
      }
      if (command === "check_backend_connection_health") {
        return Promise.reject(
          new Error(
            "1Password has no signed-in account; run `op signin` first",
          ),
        );
      }
      return Promise.reject(new Error(`unexpected command ${command}`));
    });
    const view = renderPage("connections");

    await act(async () => {});
    expect(view.container.textContent).toContain("Setup required");
    expect(view.container.textContent).toContain("Not checked");
    expect(view.container.textContent).toContain("Off");

    const checkHealth = Array.from(
      view.container.querySelectorAll("button"),
    ).find((button) => button.textContent === "Check health");
    await act(async () => {
      checkHealth?.click();
    });

    expect(view.container.textContent).toContain("Health check failed");
    expect(view.container.textContent).toContain(
      "1Password has no signed-in account; run `op signin` first",
    );
    expect(view.container.textContent).toContain("Off");
    view.unmount();
  });

  it("saves only a credential resource reference for HTTP sync", async () => {
    let savePayload: unknown;
    invoke.mockImplementation((command: string, payload?: unknown) => {
      if (command === "list_backend_connections") return Promise.resolve([]);
      if (command === "list_sync_connections") return Promise.resolve([]);
      if (command === "list_local_vaults") return Promise.resolve([]);
      if (command === "list_sync_credential_resources") {
        return Promise.resolve([
          {
            resource: "plankton://field/sync/token",
            display_name: "Sync bearer token",
          },
        ]);
      }
      if (command === "save_sync_connection") {
        savePayload = payload;
        return Promise.resolve({
          vault_id: "default",
          adapter_id: "primary",
          remote_revision: null,
          last_attempt_at: null,
          last_success_at: null,
          status: "idle",
          error_id: null,
          config: {
            kind: "custom_http",
            endpoint: "https://sync.example.test/vault",
            bearer_token_resource: "plankton://field/sync/token",
          },
        });
      }
      return Promise.reject(new Error(`unexpected command ${command}`));
    });
    const view = renderPage("connections");
    await act(async () => {});

    expect(
      view.container.querySelector('fieldset[aria-label="Sync adapter type"]'),
    ).toBeNull();
    const addSync = Array.from(view.container.querySelectorAll("button")).find(
      (button) => button.textContent === "Add sync destination",
    );
    act(() => addSync?.click());
    expect(view.container.querySelector('[role="dialog"]')).not.toBeNull();

    const type = view.container.querySelector(
      'fieldset[aria-label="Sync adapter type"]',
    ) as HTMLFieldSetElement;
    await act(async () => {
      chooseRadio(type, "custom_http");
    });
    const endpoint = view.container.querySelector(
      'input[aria-label="Sync endpoint"]',
    ) as HTMLInputElement;
    const credential = view.container.querySelector(
      'select[aria-label="Available credential resources"]',
    ) as HTMLSelectElement;
    await act(async () => {
      changeInput(endpoint, "https://sync.example.test/vault");
      credential.value = "plankton://field/sync/token";
      credential.dispatchEvent(new Event("change", { bubbles: true }));
    });
    expect(
      view.container.querySelector(
        'input[aria-label="Credential resource ID"]',
      ),
    ).toBeNull();
    expect(view.container.textContent).toContain("Sync bearer token");
    const save = Array.from(view.container.querySelectorAll("button")).find(
      (button) => button.textContent === "Save sync connection",
    );
    await act(async () => {
      save?.click();
    });

    expect(savePayload).toEqual({
      vaultId: "default",
      adapterId: "primary",
      enabled: true,
      config: {
        kind: "custom_http",
        endpoint: "https://sync.example.test/vault",
        bearer_token_resource: "plankton://field/sync/token",
      },
    });
    expect(JSON.stringify(savePayload)).not.toContain('"bearer_token":');
    expect(view.container.querySelector('[role="dialog"]')).toBeNull();

    act(() => addSync?.click());
    expect(
      view.container.querySelector<HTMLSelectElement>(
        'fieldset[aria-label="Sync adapter type"] input:checked',
      )?.value,
    ).toBe("local_folder");
    expect(
      view.container.querySelector<HTMLInputElement>(
        'input[aria-label="Vault ID"]',
      )?.value,
    ).toBe("default");
    expect(
      view.container.querySelector<HTMLInputElement>(
        'input[aria-label="Connection ID"]',
      )?.value,
    ).toBe("primary");
    expect(
      view.container.querySelector<HTMLInputElement>(
        'input[aria-label="Sync path"]',
      )?.value,
    ).toBe("");
    view.unmount();
  });

  it("selects multiple local vaults for one Git destination", async () => {
    type SaveSyncPayload = {
      vaultId: string;
      adapterId: string;
      enabled: boolean;
      config: {
        kind: string;
        repository: string;
        repository_url: string;
        blob_path: string;
        remote: string;
        branch: string;
      };
    };
    const savePayloads: SaveSyncPayload[] = [];
    let preparePayload: unknown;
    invoke.mockImplementation((command: string, payload?: unknown) => {
      if (command === "list_backend_connections") return Promise.resolve([]);
      if (command === "list_sync_connections") return Promise.resolve([]);
      if (command === "list_sync_credential_resources") {
        return Promise.resolve([]);
      }
      if (command === "list_local_vaults") {
        return Promise.resolve([
          { id: "default", file_name: "default.kdbx" },
          { id: "personal", file_name: "personal.kdbx" },
          { id: "work", file_name: "work.kdbx" },
        ]);
      }
      if (command === "prepare_git_sync_repository") {
        preparePayload = payload;
        return Promise.resolve({
          directory: "/tmp/encrypted-vaults",
          branch: "vault-sync",
        });
      }
      if (command === "save_sync_connection") {
        const saved = payload as SaveSyncPayload;
        savePayloads.push(saved);
        return Promise.resolve({
          vault_id: saved.vaultId,
          adapter_id: saved.adapterId,
          remote_revision: null,
          last_attempt_at: null,
          last_success_at: null,
          status: "idle",
          error_id: null,
          config: saved.config,
        });
      }
      return Promise.reject(new Error(`unexpected command ${command}`));
    });
    const view = renderPage("connections");
    await act(async () => {});

    const addSync = Array.from(view.container.querySelectorAll("button")).find(
      (button) => button.textContent === "Add sync destination",
    );
    act(() => addSync?.click());
    const type = view.container.querySelector<HTMLFieldSetElement>(
      'fieldset[aria-label="Sync adapter type"]',
    );
    await act(async () => {
      if (!type) return;
      chooseRadio(type, "git");
    });

    expect(
      view.container.querySelector('input[aria-label="Vault ID"]'),
    ).toBeNull();
    expect(view.container.textContent).toContain("default.kdbx");
    expect(view.container.textContent).toContain("work.kdbx");
    expect(view.container.textContent).toContain("1 of 3 selected");

    const work = view.container.querySelector<HTMLInputElement>(
      '.sync-vault-option input[value="work"]',
    );
    const repositoryUrl = view.container.querySelector<HTMLInputElement>(
      'input[aria-label="Git repository URL"]',
    );
    const branch = view.container.querySelector<HTMLInputElement>(
      'input[aria-label="Git branch"]',
    );
    await act(async () => {
      work?.click();
      if (repositoryUrl) {
        changeInput(
          repositoryUrl,
          "https://github.com/example/encrypted-vaults.git",
        );
      }
      if (branch) changeInput(branch, "vault-sync");
    });

    const save = Array.from(view.container.querySelectorAll("button")).find(
      (button) => button.textContent === "Save 2 vaults",
    );
    await act(async () => {
      save?.click();
    });

    expect(preparePayload).toEqual({
      repositoryUrl: "https://github.com/example/encrypted-vaults.git",
      directory: null,
      branch: "vault-sync",
      createBranchIfMissing: true,
    });
    expect(savePayloads).toEqual([
      {
        vaultId: "default",
        adapterId: "primary",
        enabled: true,
        config: {
          kind: "git",
          repository: "/tmp/encrypted-vaults",
          repository_url: "https://github.com/example/encrypted-vaults.git",
          blob_path: "default.kdbx",
          remote: "origin",
          branch: "vault-sync",
        },
      },
      {
        vaultId: "work",
        adapterId: "primary",
        enabled: true,
        config: {
          kind: "git",
          repository: "/tmp/encrypted-vaults",
          repository_url: "https://github.com/example/encrypted-vaults.git",
          blob_path: "work.kdbx",
          remote: "origin",
          branch: "vault-sync",
        },
      },
    ]);
    expect(view.container.textContent).toContain("default · primary");
    expect(view.container.textContent).toContain("work · primary");
    view.unmount();
  });

  it("uses the system folder picker instead of editable directory fields", async () => {
    invoke.mockImplementation((command: string) => {
      if (command === "list_backend_connections") return Promise.resolve([]);
      if (command === "list_sync_connections") return Promise.resolve([]);
      if (command === "list_sync_credential_resources") {
        return Promise.resolve([]);
      }
      if (command === "list_local_vaults") return Promise.resolve([]);
      if (command === "pick_sync_directory") {
        return Promise.resolve("/tmp/chosen-sync-folder");
      }
      return Promise.reject(new Error(`unexpected command ${command}`));
    });
    const view = renderPage("connections");
    await act(async () => {});

    const addSync = Array.from(view.container.querySelectorAll("button")).find(
      (button) => button.textContent === "Add sync destination",
    );
    act(() => addSync?.click());
    const path = view.container.querySelector<HTMLInputElement>(
      'input[aria-label="Sync path"]',
    );
    expect(path?.readOnly).toBe(true);
    const choose = Array.from(view.container.querySelectorAll("button")).find(
      (button) => button.textContent === "Choose folder",
    );
    await act(async () => choose?.click());

    expect(path?.value).toBe("/tmp/chosen-sync-folder");
    expect(invoke).toHaveBeenCalledWith("pick_sync_directory");
    view.unmount();
  });

  it("bootstraps a remote vault ID when this computer has no local vault", async () => {
    invoke.mockImplementation((command: string) => {
      if (command === "list_backend_connections") return Promise.resolve([]);
      if (command === "list_sync_connections") return Promise.resolve([]);
      if (command === "list_sync_credential_resources") {
        return Promise.resolve([]);
      }
      if (command === "list_local_vaults") return Promise.resolve([]);
      return Promise.reject(new Error(`unexpected command ${command}`));
    });
    const view = renderPage("connections");
    await act(async () => {});
    const addSync = Array.from(view.container.querySelectorAll("button")).find(
      (button) => button.textContent === "Add sync destination",
    );
    act(() => addSync?.click());
    const type = view.container.querySelector<HTMLFieldSetElement>(
      'fieldset[aria-label="Sync adapter type"]',
    );
    act(() => {
      if (!type) return;
      chooseRadio(type, "git");
    });

    expect(
      view.container.querySelector<HTMLInputElement>(
        'input[aria-label="Remote vault ID"]',
      )?.value,
    ).toBe("default");
    expect(view.container.textContent).toContain(
      "choose the unlock file transferred securely from the original computer",
    );
    expect(view.container.textContent).toContain(
      "First sync: 1 vault selected",
    );
    view.unmount();
  });

  it("shows missing unlock guidance and switches to file-manager reveal after import", async () => {
    const connection = {
      vault_id: "default",
      adapter_id: "primary",
      remote_revision: null,
      last_attempt_at: null,
      last_success_at: null,
      status: "idle",
      error_id: null,
      config: {
        kind: "git",
        repository_url: "https://example.test/encrypted-vaults.git",
        repository: "/tmp/encrypted-vaults",
        blob_path: "default.kdbx",
        remote: "origin",
        branch: "main",
      },
    };
    let unlockReady = false;
    invoke.mockImplementation((command: string, payload?: unknown) => {
      if (command === "list_backend_connections") return Promise.resolve([]);
      if (command === "list_sync_connections") {
        return Promise.resolve([connection]);
      }
      if (command === "list_sync_credential_resources") {
        return Promise.resolve([]);
      }
      if (command === "list_local_vaults") {
        return Promise.resolve([
          {
            id: "default",
            file_name: "default.kdbx",
            unlock_file_name: ".default.unlock",
            label: "default",
            subtitle: "Encrypted KDBX4",
            exists: true,
            unlock_file_exists: unlockReady,
          },
        ]);
      }
      if (command === "pick_local_vault_unlock_file") {
        expect(payload).toEqual({ vaultId: "default" });
        unlockReady = true;
        return Promise.resolve({
          id: "default",
          file_name: "default.kdbx",
          unlock_file_name: ".default.unlock",
          label: "default",
          subtitle: "Encrypted KDBX4 · unlock ready",
          exists: true,
          unlock_file_exists: true,
        });
      }
      if (command === "reveal_local_vault_unlock_file") {
        expect(payload).toEqual({ vaultId: "default" });
        return Promise.resolve();
      }
      return Promise.reject(new Error(`unexpected command ${command}`));
    });
    const view = renderPage("connections");
    await act(async () => {});

    expect(view.container.textContent).toContain("Unlock file required");
    expect(view.container.textContent).toContain(
      "Transfer it securely from the original computer",
    );
    await act(async () => {
      Array.from(view.container.querySelectorAll("button"))
        .find((button) => button.textContent === "Choose unlock file")
        ?.click();
    });
    expect(view.container.textContent).toContain("Unlock file ready");
    await act(async () => {
      Array.from(view.container.querySelectorAll("button"))
        .find((button) => button.textContent === "Show in file manager")
        ?.click();
    });
    expect(invoke).toHaveBeenCalledWith("reveal_local_vault_unlock_file", {
      vaultId: "default",
    });
    view.unmount();
  });

  it("does not offer arbitrary text entry for sync credentials", async () => {
    invoke.mockImplementation((command: string) => {
      if (command === "list_backend_connections") return Promise.resolve([]);
      if (command === "list_sync_connections") return Promise.resolve([]);
      if (command === "list_local_vaults") return Promise.resolve([]);
      if (command === "list_sync_credential_resources") {
        return Promise.resolve([
          {
            resource: "secret/sync/credential",
            display_name: "Approved sync credential",
          },
        ]);
      }
      return Promise.reject(new Error(`unexpected command ${command}`));
    });
    const view = renderPage("connections");
    await act(async () => {});
    const addSync = Array.from(view.container.querySelectorAll("button")).find(
      (button) => button.textContent === "Add sync destination",
    );
    act(() => addSync?.click());
    const kind = view.container.querySelector<HTMLFieldSetElement>(
      'fieldset[aria-label="Sync adapter type"]',
    );
    act(() => {
      if (!kind) return;
      chooseRadio(kind, "webdav");
    });

    expect(
      view.container.querySelector(
        'select[aria-label="Available credential resources"]',
      ),
    ).not.toBeNull();
    expect(
      view.container.querySelector(
        'input[aria-label="Credential resource ID"]',
      ),
    ).toBeNull();
    expect(view.container.textContent).toContain("Approved sync credential");
    view.unmount();
  });

  it("separates backend and encrypted sync groups with the add action in the page header", async () => {
    invoke.mockImplementation((command: string) => {
      if (command === "list_backend_connections") return Promise.resolve([]);
      if (command === "list_sync_connections") return Promise.resolve([]);
      if (command === "list_local_vaults") return Promise.resolve([]);
      if (command === "list_sync_credential_resources") {
        return Promise.resolve([]);
      }
      return Promise.reject(new Error(`unexpected command ${command}`));
    });
    const view = renderPage("connections");
    await act(async () => {});

    expect(
      view.container.querySelector('[aria-labelledby="backend-group-title"]'),
    ).not.toBeNull();
    expect(
      view.container.querySelector('[aria-labelledby="sync-group-title"]'),
    ).not.toBeNull();
    expect(
      Array.from(view.container.querySelectorAll("button")).some(
        (button) => button.textContent === "Add sync destination",
      ),
    ).toBe(true);
    expect(view.container.querySelector(".sync-form")).toBeNull();
    view.unmount();
  });

  it("keeps technical conflict history secondary and offers one automatic retry", async () => {
    const failedSync = {
      vault_id: "work",
      adapter_id: "remote",
      remote_revision: "42",
      last_attempt_at: "2026-07-29T15:00:00Z",
      last_success_at: "2026-07-28T12:00:00Z",
      status: "error",
      error_id: "conflict-123",
      config: { kind: "webdav", endpoint: "https://sync.example.test" },
    };
    invoke.mockImplementation((command: string, payload?: unknown) => {
      if (command === "list_backend_connections") return Promise.resolve([]);
      if (command === "list_sync_connections") {
        return Promise.resolve([failedSync]);
      }
      if (command === "list_local_vaults") {
        return Promise.resolve([
          {
            id: "work",
            file_name: "work.kdbx",
            unlock_file_name: ".work.unlock",
            label: "work",
            subtitle: "Encrypted KDBX4 · unlock ready",
            exists: true,
            unlock_file_exists: true,
          },
        ]);
      }
      if (command === "list_sync_credential_resources") {
        return Promise.resolve([]);
      }
      if (command === "run_sync_connection") {
        expect(payload).toEqual({
          vaultId: "work",
          adapterId: "remote",
          direction: "sync",
        });
        return Promise.reject(new Error("remote version conflict"));
      }
      return Promise.reject(new Error(`unexpected command ${command}`));
    });
    const view = renderPage("connections");

    await act(async () => {});
    expect(view.container.textContent).toContain("Last attempt");
    expect(view.container.textContent).toContain("2026-07-29T15:00:00Z");
    expect(view.container.textContent).toContain("Last success");
    expect(view.container.textContent).toContain("2026-07-28T12:00:00Z");
    expect(view.container.textContent).toContain("Remote revision");
    expect(view.container.textContent).toContain("42");
    expect(view.container.textContent).toContain("conflict-123");
    expect(view.container.textContent).toContain("Retry available");

    expect(view.container.textContent).toContain("Technical details");
    const sync = Array.from(view.container.querySelectorAll("button")).find(
      (button) => button.textContent === "Sync again",
    );
    await act(async () => {
      sync?.click();
    });
    expect(view.container.textContent).toContain("remote version conflict");
    view.unmount();
  });

  it("reports a successful automatic KDBX merge in user language", async () => {
    const connection = {
      vault_id: "work",
      adapter_id: "origin",
      remote_revision: "8",
      last_attempt_at: null,
      last_success_at: null,
      status: "idle",
      error_id: null,
      config: { kind: "git", repository: "/tmp/work", branch: "main" },
    };
    const vault = {
      id: "work",
      file_name: "work.kdbx",
      unlock_file_name: ".work.unlock",
      label: "work",
      subtitle: "Encrypted KDBX4 · unlock ready",
      exists: true,
      unlock_file_exists: true,
    };
    invoke.mockImplementation((command: string, payload?: unknown) => {
      if (command === "list_backend_connections") return Promise.resolve([]);
      if (command === "list_sync_connections") {
        return Promise.resolve([connection]);
      }
      if (command === "list_sync_credential_resources") {
        return Promise.resolve([]);
      }
      if (command === "list_local_vaults") return Promise.resolve([vault]);
      if (command === "run_sync_connection") {
        expect(payload).toEqual({
          vaultId: "work",
          adapterId: "origin",
          direction: "sync",
        });
        return Promise.resolve({
          connection: {
            ...connection,
            remote_revision: "9",
            last_success_at: "2026-08-06T13:45:00Z",
          },
          completion: "merged",
        });
      }
      return Promise.reject(new Error(`unexpected command ${command}`));
    });
    const view = renderPage("connections");
    await act(async () => {});

    const sync = Array.from(view.container.querySelectorAll("button")).find(
      (button) => button.textContent === "Sync now",
    );
    await act(async () => sync?.click());

    expect(view.container.textContent).toContain(
      "Local and remote changes were merged and synchronized",
    );
    expect(view.container.textContent).toContain(
      "Both original copies were backed up",
    );
    expect(view.container.textContent).not.toContain("Pull");
    expect(view.container.textContent).not.toContain("Push");
    view.unmount();
  });
});
