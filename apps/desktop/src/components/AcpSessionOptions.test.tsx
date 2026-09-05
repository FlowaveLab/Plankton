// @vitest-environment jsdom
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { invoke } from "@tauri-apps/api/core";
import { AcpSessionOptions } from "./AcpSessionOptions";
import type { AcpProbeResult, AcpProfile } from "../types";
vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
const profile: AcpProfile = { agent_kind: "codex", version_mode: "latest" };
const result: AcpProbeResult = {
  configured_selector: "adapter@current",
  program: "adapter",
  args: [],
  package_selector: "current",
  basic: { status: "passed" },
  readiness: { status: "not_run" },
  config_options: [
    {
      id: "model",
      name: "Model",
      category: "model",
      description: null,
      current_value: "future-model",
      options: [
        {
          value: "future-model",
          name: "Future model",
          description: null,
          group: "Provider",
        },
      ],
    },
    {
      id: "vendor-speed",
      name: "Speed",
      category: "_speed",
      description: "From agent",
      current_value: "normal",
      options: [
        { value: "normal", name: "Normal", group: null, description: null },
        { value: "turbo", name: "Turbo", group: null, description: null },
      ],
    },
  ],
};
let container: HTMLDivElement;
let root: Root;
beforeEach(() => {
  vi.useFakeTimers();
  vi.mocked(invoke).mockReset();
  container = document.createElement("div");
  document.body.append(container);
  root = createRoot(container);
});
afterEach(async () => {
  await act(async () => root.unmount());
  container.remove();
  vi.useRealTimers();
});
async function render(p = profile, onChange = vi.fn()) {
  await act(async () =>
    root.render(
      <AcpSessionOptions profile={p} zh disabled={false} onChange={onChange} />,
    ),
  );
  await act(async () => vi.advanceTimersByTimeAsync(300));
}
describe("dynamic ACP settings", () => {
  it("renders agent-provided groups and persists arbitrary option IDs", async () => {
    vi.mocked(invoke).mockResolvedValue(result);
    const onChange = vi.fn();
    await render(profile, onChange);
    expect(container.querySelector("optgroup")?.label).toBe("Provider");
    expect(container.textContent).toContain("Future model");
    const selects = container.querySelectorAll("select");
    await act(async () => {
      selects[1].value = "turbo";
      selects[1].dispatchEvent(new Event("change", { bubbles: true }));
    });
    expect(onChange).toHaveBeenCalledWith({
      ...profile,
      session_options: { "vendor-speed": "turbo" },
    });
    expect(invoke).toHaveBeenCalledWith("discover_acp_options", { profile });
  });
  it("discards stale responses after changing agents", async () => {
    let finishOld!: (value: AcpProbeResult) => void;
    vi.mocked(invoke)
      .mockImplementationOnce(
        () =>
          new Promise((resolve) => {
            finishOld = resolve;
          }),
      )
      .mockResolvedValueOnce({ ...result, config_options: [] });
    await render();
    await render({ ...profile, agent_kind: "open_code" });
    await act(async () => finishOld(result));
    expect(container.querySelectorAll("select")).toHaveLength(0);
    expect(container.textContent).toContain("未返回可配置选项");
  });
  it("reports removed selections and lets users reset them", async () => {
    vi.mocked(invoke).mockResolvedValue({
      ...result,
      rejected_options: ["fast"],
    });
    const onChange = vi.fn();
    await render(
      { ...profile, session_options: { fast: "on", model: "future-model" } },
      onChange,
    );
    expect(container.querySelector('[role="alert"]')?.textContent).toContain(
      "fast",
    );
    const reset = Array.from(container.querySelectorAll("button")).find(
      (button) => button.textContent === "使用当前默认值",
    )!;
    await act(async () => reset.click());
    expect(onChange).toHaveBeenCalledWith({
      ...profile,
      session_options: { model: "future-model" },
    });
  });
  it("reports adapter errors without inventing models", async () => {
    vi.mocked(invoke).mockRejectedValue(new Error("Authentication required"));
    await render();
    expect(container.querySelector('[role="alert"]')?.textContent).toBe(
      "Authentication required",
    );
    expect(container.querySelectorAll("select")).toHaveLength(0);
  });
});
