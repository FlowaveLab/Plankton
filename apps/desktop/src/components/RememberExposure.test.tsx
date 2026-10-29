// @vitest-environment jsdom
import { act } from "react";
import ReactDOM from "react-dom/client";
import { afterEach, describe, expect, it, vi } from "vitest";

import type { AccessRequest, CredentialExposureReport } from "../types";
import { defaultExposurePolicy, EXPOSURE_SURFACES } from "./ExposurePolicy";
import { deriveExposureCandidate, RememberExposure } from "./RememberExposure";

const { invoke } = vi.hoisted(() => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({ invoke }));

afterEach(() => {
  delete (window as Window & { __TAURI_INTERNALS__?: object })
    .__TAURI_INTERNALS__;
  invoke.mockReset();
});

function confirmedReport(): CredentialExposureReport {
  return {
    chain_summary:
      "The declared consumer sends the credential to the named API only.",
    node_assessments: [],
    surfaces: EXPOSURE_SURFACES.map((surface) => ({
      surface,
      actual_level:
        surface === "network" || surface === "process_propagation" ? 1 : 0,
      evidence_state:
        surface === "network" || surface === "process_propagation"
          ? "observed"
          : "not_observed",
      summary: "Established by the captured consumer.",
      network_destinations:
        surface === "network" ? ["https://api.example.com/v1/status"] : [],
      annotations: [],
    })),
  };
}

describe("minimal confirmed exposure candidate", () => {
  it("adds exact known hosts, normalizes and deduplicates URLs, and never chooses unrestricted network", () => {
    const original = defaultExposurePolicy();
    const report = confirmedReport();
    const network = report.surfaces.find(
      (entry) => entry.surface === "network",
    )!;
    network.actual_level = 2;
    network.network_destinations = [
      "https://API.EXAMPLE.COM./v1",
      "api.example.com:443",
    ];
    const result = deriveExposureCandidate(original, report);
    expect(result.issue).toBeNull();
    expect(
      result.policy.surfaces.find((entry) => entry.surface === "network"),
    ).toEqual({
      surface: "network",
      max_level: 1,
      network_allowlist: [{ kind: "exact_domain", domain: "api.example.com" }],
    });
    expect(
      result.policy.surfaces.filter((entry) => entry.surface !== "network"),
    ).toEqual(original.surfaces.filter((entry) => entry.surface !== "network"));
    expect(original).toEqual(defaultExposurePolicy());
  });

  it("preserves existing permitted domains and constraints while adding only the uncovered host", () => {
    const original = defaultExposurePolicy();
    original.breach_action = "deny";
    const network = original.surfaces.find(
      (entry) => entry.surface === "network",
    )!;
    network.max_level = 1;
    network.note = "Production API only";
    network.network_allowlist = [
      { kind: "exact_domain", domain: "old.example.test" },
      { kind: "subdomains_of", domain: "internal.test", include_apex: false },
    ];
    const report = confirmedReport();
    report.surfaces.find(
      (entry) => entry.surface === "network",
    )!.network_destinations = [
      "https://service.internal.test",
      "https://api.example.com",
    ];
    const result = deriveExposureCandidate(original, report);
    expect(result.policy.breach_action).toBe("deny");
    expect(
      result.policy.surfaces.find((entry) => entry.surface === "network"),
    ).toEqual({
      ...network,
      network_allowlist: [
        ...network.network_allowlist,
        { kind: "exact_domain", domain: "api.example.com" },
      ],
    });
  });

  it("does not activate a dormant broad allowlist when networking was disabled", () => {
    const original = defaultExposurePolicy();
    original.surfaces.find(
      (entry) => entry.surface === "network",
    )!.network_allowlist = [{ kind: "regex", pattern: ".*" }];
    const network = deriveExposureCandidate(
      original,
      confirmedReport(),
    ).policy.surfaces.find((entry) => entry.surface === "network")!;
    expect(network.network_allowlist).toEqual([
      { kind: "exact_domain", domain: "api.example.com" },
    ]);
  });

  it("preserves existing regex permissions and adds explicit observed hosts without simulating Rust regex", () => {
    const original = defaultExposurePolicy();
    const network = original.surfaces.find(
      (entry) => entry.surface === "network",
    )!;
    network.max_level = 1;
    network.network_allowlist = [
      { kind: "regex", pattern: "api\\.example\\.com" },
    ];
    const result = deriveExposureCandidate(original, confirmedReport());
    expect(
      result.policy.surfaces.find((entry) => entry.surface === "network")!
        .network_allowlist,
    ).toEqual([
      ...network.network_allowlist,
      { kind: "exact_domain", domain: "api.example.com" },
    ]);
  });

  it.each([
    "unknown",
    "missing_target",
    "wildcard",
    "incomplete",
    "uncontrolled",
  ])("does not propose permissions from %s evidence", (problem) => {
    const original = defaultExposurePolicy();
    const report = confirmedReport();
    const network = report.surfaces.find(
      (entry) => entry.surface === "network",
    )!;
    if (problem === "unknown") {
      network.evidence_state = "unknown";
      network.actual_level = 2;
    }
    if (problem === "missing_target") network.network_destinations = [];
    if (problem === "wildcard")
      network.network_destinations = ["https://*.example.com"];
    if (problem === "incomplete") report.surfaces.pop();
    if (problem === "uncontrolled") {
      report.surfaces[0].evidence_state = "observed";
      report.surfaces[0].actual_level = 2;
    }
    const result = deriveExposureCandidate(original, report);
    expect(result.issue).not.toBeNull();
    expect(result.policy).toEqual(original);
  });
});

Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });

const request: AccessRequest = {
  id: "remember-test",
  policy_mode: "llm_automatic",
  approval_status: "pending",
  evaluation_state: "completed",
  final_decision: null,
  provider_kind: "acp",
  rendered_prompt: "",
  llm_suggestion: null,
  automatic_decision: null,
  created_at: "2026-10-29T08:00:00Z",
  updated_at: "2026-10-29T08:00:00Z",
  resolved_at: null,
  context: {
    resource: "secret/test",
    reason: "Query status",
    requested_by: "test",
    script_path: null,
    call_chain: [],
    env_vars: {},
    metadata: {},
    resource_metadata: {},
    created_at: "2026-10-29T08:00:00Z",
  },
};

describe("remember exposure approval", () => {
  it("shows the quick action and added domain before expansion and approves against the latest policy", async () => {
    Object.assign(window, { __TAURI_INTERNALS__: {} });
    const current = {
      ...defaultExposurePolicy(),
      breach_action: "deny" as const,
    };
    invoke.mockResolvedValue(current);
    const container = document.createElement("div");
    document.body.appendChild(container);
    const root = ReactDOM.createRoot(container);
    const onApprove = vi.fn().mockResolvedValue(undefined);
    await act(async () =>
      root.render(
        <RememberExposure
          request={{
            ...request,
            llm_suggestion: { exposure_report: confirmedReport() },
          }}
          zh
          busy={false}
          onApprove={onApprove}
        />,
      ),
    );
    const quick = container.querySelector<HTMLButtonElement>(
      ".remember-exposure-quick",
    )!;
    expect(quick.textContent).toBe("应用本次范围并批准");
    expect(quick.disabled).toBe(false);
    expect(container.querySelector(".remember-exposure-body")).toBeNull();
    expect(
      container.querySelector(".remember-exposure-preview")!.textContent,
    ).toContain("api.example.com");
    await act(async () => quick.click());
    expect(onApprove).toHaveBeenCalledWith({
      expectedExposurePolicy: current,
      exposurePolicy: deriveExposureCandidate(current, confirmedReport())
        .policy,
    });
    act(() => root.unmount());
    container.remove();
  });

  it("keeps the quick entry visible but disabled for unknown evidence and permits manual adjustment", () => {
    const report = confirmedReport();
    report.surfaces[0].actual_level = 2;
    report.surfaces[0].evidence_state = "unknown";
    const container = document.createElement("div");
    document.body.appendChild(container);
    const root = ReactDOM.createRoot(container);
    act(() =>
      root.render(
        <RememberExposure
          request={{ ...request, llm_suggestion: { exposure_report: report } }}
          zh
          busy={false}
          onApprove={vi.fn()}
        />,
      ),
    );
    expect(
      container.querySelector<HTMLButtonElement>(".remember-exposure-quick")!
        .disabled,
    ).toBe(true);
    expect(container.textContent).toContain("仍有未知证据");
    expect(
      container.querySelector<HTMLButtonElement>(".remember-exposure-toggle")!
        .disabled,
    ).toBe(false);
    act(() => root.unmount());
    container.remove();
  });
  it("requires an explicit change, keeps protected mode, and submits the edited scope and expected policy together", async () => {
    const container = document.createElement("div");
    document.body.appendChild(container);
    const root = ReactDOM.createRoot(container);
    const onApprove = vi.fn().mockResolvedValue(undefined);
    act(() =>
      root.render(
        <RememberExposure
          request={request}
          zh
          busy={false}
          onApprove={onApprove}
        />,
      ),
    );
    act(() =>
      container
        .querySelector<HTMLButtonElement>(".remember-exposure-toggle")!
        .click(),
    );
    const submit = () =>
      Array.from(container.querySelectorAll<HTMLButtonElement>("button")).find(
        (b) => b.textContent === "通过并记住暴露域",
      )!;
    expect(submit().disabled).toBe(true);
    expect(container.textContent).not.toContain("直接可见");
    const radar = container.querySelector<HTMLCanvasElement>("canvas")!;
    act(() =>
      radar.dispatchEvent(
        new KeyboardEvent("keydown", { key: "ArrowRight", bubbles: true }),
      ),
    );
    const controlled = Array.from(
      container.querySelectorAll<HTMLInputElement>('input[type="radio"]'),
    ).find((input) =>
      input.parentElement?.textContent?.includes("1 · 受控使用"),
    )!;
    act(() => controlled.click());
    expect(submit().disabled).toBe(true);
    const input = container.querySelector<HTMLTextAreaElement>(
      ".exposure-network-allowlist textarea",
    )!;
    const setter = Object.getOwnPropertyDescriptor(
      HTMLTextAreaElement.prototype,
      "value",
    )!.set!;
    act(() => input.focus());
    const text = "domain:api.example.com\nsubdomain:example.net";
    for (let i = 1; i <= text.length; i++) {
      act(() => {
        setter.call(input, text.slice(0, i));
        input.dispatchEvent(new Event("input", { bubbles: true }));
      });
      expect(input.value).toBe(text.slice(0, i));
    }
    act(() => input.blur());
    expect(input.value).toBe(text);
    expect(submit().disabled).toBe(false);
    await act(async () => submit().click());
    expect(onApprove).toHaveBeenCalledWith({
      expectedExposurePolicy: defaultExposurePolicy(),
      exposurePolicy: {
        ...defaultExposurePolicy(),
        surfaces: defaultExposurePolicy().surfaces.map((surface) =>
          surface.surface === "network"
            ? {
                ...surface,
                max_level: 1,
                network_allowlist: [
                  { kind: "exact_domain", domain: "api.example.com" },
                  {
                    kind: "subdomains_of",
                    domain: "example.net",
                    include_apex: false,
                  },
                ],
              }
            : surface,
        ),
      },
    });
    act(() => root.unmount());
    container.remove();
  });

  it("explains that remembering scope does not override manual review", () => {
    const container = document.createElement("div");
    document.body.appendChild(container);
    const root = ReactDOM.createRoot(container);
    act(() =>
      root.render(
        <RememberExposure
          request={{ ...request, policy_mode: "manual_only" }}
          zh
          busy={false}
          onApprove={vi.fn()}
        />,
      ),
    );
    act(() =>
      container
        .querySelector<HTMLButtonElement>(".remember-exposure-toggle")!
        .click(),
    );
    expect(container.textContent).toContain("保存范围后仍需按该模式审批");
    act(() => root.unmount());
    container.remove();
  });
});
