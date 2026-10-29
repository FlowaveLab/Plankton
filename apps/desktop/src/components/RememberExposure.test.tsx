// @vitest-environment jsdom
import { act } from "react";
import ReactDOM from "react-dom/client";
import { describe, expect, it, vi } from "vitest";

import type { AccessRequest } from "../types";
import { defaultExposurePolicy } from "./ExposurePolicy";
import { RememberExposure } from "./RememberExposure";

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
    act(() => container.querySelector<HTMLButtonElement>("button")!.click());
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
    act(() => container.querySelector<HTMLButtonElement>("button")!.click());
    expect(container.textContent).toContain("保存范围后仍需按该模式审批");
    act(() => root.unmount());
    container.remove();
  });
});
