// @vitest-environment jsdom

import { act } from "react";
import ReactDOM from "react-dom/client";
import { afterEach, describe, expect, it, vi } from "vitest";

import {
  ApprovalNotification,
  type ApprovalNotificationApi,
} from "./ApprovalNotification";
import type { CompactApprovalRequest } from "./CompactApproval";

Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });

const request: CompactApprovalRequest = {
  id: "request-1",
  resource: "plankton://field/service/token",
  requested_by: "release-agent",
  reason: "Read service status",
  context: "/workspace/client.py",
  call_chain: [],
  suggestion: "Review destination",
  suggested_decision: "escalate",
  risk_score: 24,
  approval_status: "pending",
  evaluation_state: "completed",
};

function harness(initial: CompactApprovalRequest[] = [request]) {
  let onChange = () => {};
  const api: ApprovalNotificationApi = {
    loadRequests: vi.fn(async () => initial),
    loadLocale: vi.fn(async () => "zh-CN" as const),
    openFullDetails: vi.fn(async () => {}),
    dismiss: vi.fn(async () => {}),
    subscribe: vi.fn(async (listener) => {
      onChange = listener;
      return () => {};
    }),
  };
  const container = document.createElement("div");
  document.body.appendChild(container);
  const root = ReactDOM.createRoot(container);
  act(() => root.render(<ApprovalNotification api={api} />));
  return {
    api,
    container,
    onChange: () => onChange(),
    unmount() {
      act(() => root.unmount());
      container.remove();
    },
  };
}

async function flush() {
  await act(async () => {
    await Promise.resolve();
    await Promise.resolve();
  });
}
function button(container: HTMLElement, name: string) {
  const found = Array.from(container.querySelectorAll("button")).find(
    (entry) => entry.textContent?.trim() === name,
  );
  if (!found) throw new Error(`No button ${name}`);
  return found;
}
afterEach(() => {
  document.body.innerHTML = "";
  vi.restoreAllMocks();
});

describe("ApprovalNotification", () => {
  it("shows a small localized summary and opens details only after a click", async () => {
    const view = harness();
    await flush();
    expect(view.container.textContent).toContain("需要你审批");
    expect(view.container.textContent).toContain(request.resource);
    expect(view.api.openFullDetails).not.toHaveBeenCalled();
    expect(view.container.querySelector("textarea")).toBeNull();
    expect(view.container.textContent).not.toContain(request.context);
    await act(async () => button(view.container, "查看并审批").click());
    expect(view.api.openFullDetails).toHaveBeenCalledWith(request.id);
    view.unmount();
  });

  it("Later and Escape dismiss the notification without approving anything", async () => {
    const view = harness();
    await flush();
    await act(async () => button(view.container, "稍后处理").click());
    expect(view.api.dismiss).toHaveBeenCalledOnce();
    await act(async () =>
      window.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape" })),
    );
    expect(view.api.dismiss).toHaveBeenCalledTimes(2);
    expect(view.api.openFullDetails).not.toHaveBeenCalled();
    view.unmount();
  });

  it("announces new queued requests and opens the newest one", async () => {
    const view = harness();
    await flush();
    vi.mocked(view.api.loadRequests).mockResolvedValue([
      request,
      { ...request, id: "request-2", resource: "secret/second-token" },
    ]);
    await act(async () => view.onChange());
    expect(view.container.textContent).toContain("2 项待审批");
    expect(view.container.textContent).toContain("secret/second-token");
    await act(async () => button(view.container, "查看并审批").click());
    expect(view.api.openFullDetails).toHaveBeenCalledWith("request-2");
    view.unmount();
  });

  it("advances when the newest of two requests resolves elsewhere", async () => {
    const latest = {
      ...request,
      id: "request-2",
      resource: "secret/second-token",
    };
    const view = harness([request, latest]);
    await flush();
    expect(view.container.textContent).toContain(latest.resource);
    vi.mocked(view.api.loadRequests).mockResolvedValue([request]);
    await act(async () => view.onChange());
    expect(view.container.textContent).toContain(request.resource);
    expect(view.container.textContent).not.toContain(latest.resource);
    expect(view.container.textContent).not.toContain("2 项待审批");
    await act(async () => button(view.container, "查看并审批").click());
    expect(view.api.openFullDetails).toHaveBeenCalledWith(request.id);
    view.unmount();
  });

  it("does not offer review for a request already resolved elsewhere", async () => {
    const view = harness([{ ...request, approval_status: "approved" }]);
    await flush();
    expect(view.container.textContent).toContain("审批已处理");
    expect(button(view.container, "查看并审批").disabled).toBe(true);
    view.unmount();
  });

  it("surfaces a failed open without losing the pending request", async () => {
    const view = harness();
    await flush();
    vi.mocked(view.api.openFullDetails).mockRejectedValue(
      new Error("window unavailable"),
    );
    await act(async () => button(view.container, "查看并审批").click());
    expect(
      view.container.querySelector('[role="alert"]')?.textContent,
    ).toContain("操作未完成");
    expect(view.container.textContent).toContain(request.resource);
    view.unmount();
  });
});
