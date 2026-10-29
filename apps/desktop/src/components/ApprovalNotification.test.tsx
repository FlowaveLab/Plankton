// @vitest-environment jsdom

import { act } from "react";
import ReactDOM from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import {
  ApprovalNotification,
  type ApprovalNotificationApi,
  type ApprovalNotificationData,
} from "./ApprovalNotification";

Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
const notification: ApprovalNotificationData = {
  id: "notification-1",
  request_id: "request-1",
  stage: "evaluating",
  resource: "plankton://field/service/token",
  resource_label: "Service API / Token",
  requested_by: "release-agent",
  reason: "Read the service status",
  decision_reason: "Only the allowed API receives the token",
};

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (reason: Error) => void;
  const promise = new Promise<T>((onResolve, onReject) => {
    resolve = onResolve;
    reject = onReject;
  });
  return { promise, resolve, reject };
}

function harness(
  initial: ApprovalNotificationData | null = notification,
  overrides: Partial<ApprovalNotificationApi> = {},
) {
  let onChange = (_notification: ApprovalNotificationData | null) => {};
  const unlisten = vi.fn();
  const api: ApprovalNotificationApi = {
    loadCurrent: vi.fn(async () => initial),
    loadLocale: vi.fn(async () => "zh-CN" as const),
    openFullDetails: vi.fn(async () => {}),
    dismiss: vi.fn(async () => {}),
    disable: vi.fn(async () => {}),
    subscribe: vi.fn(async (listener) => {
      onChange = listener;
      return unlisten;
    }),
    ...overrides,
  };
  const container = document.createElement("div");
  document.body.appendChild(container);
  const root = ReactDOM.createRoot(container);
  act(() => root.render(<ApprovalNotification api={api} />));
  return {
    api,
    container,
    unlisten,
    onChange: (next: ApprovalNotificationData | null) => onChange(next),
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
    (entry) =>
      entry.textContent?.trim() === name ||
      entry.getAttribute("aria-label") === name,
  );
  if (!found) throw new Error(`No button ${name}`);
  return found;
}
async function advance(milliseconds: number) {
  await act(async () => {
    await vi.advanceTimersByTimeAsync(milliseconds);
  });
}

beforeEach(() => {
  vi.useFakeTimers();
});
afterEach(() => {
  document.body.innerHTML = "";
  vi.useRealTimers();
  vi.restoreAllMocks();
});

describe("ApprovalNotification", () => {
  it.each([
    ["evaluating", "自动审批中"],
    ["approved", "访问已通过"],
    ["rejected", "访问已拒绝"],
    ["failed", "审批未完成"],
  ] as const)(
    "shows the resource, request reason, and %s result without human approval controls",
    async (stage, label) => {
      const view = harness({ ...notification, stage });
      await flush();
      expect(view.container.textContent).toContain(label);
      expect(view.container.textContent).toContain(notification.resource_label);
      expect(
        view.container
          .querySelector(".approval-notification__resource")
          ?.getAttribute("title"),
      ).toBe(notification.resource);
      expect(view.container.textContent).toContain(notification.reason);
      expect(view.container.textContent).toContain(notification.requested_by);
      if (stage === "evaluating")
        expect(
          view.container.querySelector(".approval-notification__decision"),
        ).toBeNull();
      else
        expect(view.container.textContent).toContain(
          notification.decision_reason,
        );
      expect(view.container.querySelector("textarea")).toBeNull();
      expect(view.container.querySelector("h1")).toBeNull();
      expect(view.api.openFullDetails).not.toHaveBeenCalled();
      expect(view.container.textContent).not.toContain("需要你审批");
      view.unmount();
    },
  );

  it("keeps evaluation visible and expires only five seconds after a terminal update", async () => {
    const view = harness();
    await flush();
    await advance(60000);
    expect(view.api.dismiss).not.toHaveBeenCalled();
    act(() =>
      view.onChange({ ...notification, id: "approved-1", stage: "approved" }),
    );
    await advance(4999);
    expect(view.api.dismiss).not.toHaveBeenCalled();
    await advance(1);
    expect(view.api.dismiss).toHaveBeenCalledExactlyOnceWith("approved-1");
    view.unmount();
  });

  it("starts the terminal timer even when a stage update reuses the notification id", async () => {
    const view = harness();
    await flush();
    act(() => view.onChange({ ...notification, stage: "rejected" }));
    await advance(5000);
    expect(view.api.dismiss).toHaveBeenCalledExactlyOnceWith(notification.id);
    view.unmount();
  });

  it("cancels the old timer when a new notification arrives", async () => {
    const view = harness({ ...notification, stage: "approved" });
    await flush();
    await advance(4000);
    act(() =>
      view.onChange({ ...notification, id: "new-result", stage: "rejected" }),
    );
    await advance(1000);
    expect(view.api.dismiss).not.toHaveBeenCalled();
    await advance(4000);
    expect(view.api.dismiss).toHaveBeenCalledExactlyOnceWith("new-result");
    view.unmount();
  });

  it("clears the notification and its timer when human review takes over", async () => {
    const view = harness({ ...notification, stage: "approved" });
    await flush();
    act(() => view.onChange(null));
    await advance(5000);
    expect(view.container.textContent).not.toContain(
      notification.resource_label,
    );
    expect(view.api.dismiss).not.toHaveBeenCalled();
    expect(button(view.container, "查看详情").disabled).toBe(true);
    view.unmount();
  });

  it.each(["close", "escape"])(
    "dismisses the current id through %s without approving or muting",
    async (action) => {
      const view = harness();
      await flush();
      await act(async () => {
        if (action === "close") button(view.container, "关闭提示").click();
        else
          window.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape" }));
      });
      expect(view.api.dismiss).toHaveBeenCalledExactlyOnceWith(notification.id);
      expect(view.api.disable).not.toHaveBeenCalled();
      expect(view.api.openFullDetails).not.toHaveBeenCalled();
      view.unmount();
    },
  );

  it("persistently disables automatic notifications through the dedicated action", async () => {
    const view = harness();
    await flush();
    await act(async () => button(view.container, "不再提示").click());
    expect(view.api.disable).toHaveBeenCalledOnce();
    expect(view.api.dismiss).not.toHaveBeenCalled();
    expect(view.api.openFullDetails).not.toHaveBeenCalled();
    view.unmount();
  });

  it("opens the request id and dismisses only that notification", async () => {
    const view = harness();
    await flush();
    await act(async () => button(view.container, "查看详情").click());
    expect(view.api.openFullDetails).toHaveBeenCalledExactlyOnceWith(
      notification.request_id,
    );
    expect(view.api.dismiss).toHaveBeenCalledExactlyOnceWith(notification.id);
    view.unmount();
  });

  it.each(["open", "disable", "dismiss"])(
    "keeps failed %s actions visible instead of silently expiring them",
    async (action) => {
      const view = harness({ ...notification, stage: "approved" });
      await flush();
      const method =
        action === "open"
          ? view.api.openFullDetails
          : action === "disable"
            ? view.api.disable
            : view.api.dismiss;
      vi.mocked(method).mockRejectedValue(new Error("unavailable"));
      await act(async () =>
        button(
          view.container,
          action === "open"
            ? "查看详情"
            : action === "disable"
              ? "不再提示"
              : "关闭提示",
        ).click(),
      );
      expect(
        view.container.querySelector('[role="alert"]')?.textContent,
      ).toContain("操作未完成");
      expect(view.container.textContent).toContain(notification.resource_label);
      await advance(6000);
      expect(method).toHaveBeenCalledOnce();
      if (action !== "dismiss") expect(view.api.dismiss).not.toHaveBeenCalled();
      view.unmount();
    },
  );

  it("does not apply an old action failure to a newer notification", async () => {
    const pending = deferred<void>();
    const view = harness(
      { ...notification, stage: "approved" },
      { dismiss: vi.fn(() => pending.promise) },
    );
    await flush();
    await advance(5000);
    act(() =>
      view.onChange({
        ...notification,
        id: "new-evaluation",
        stage: "evaluating",
        resource_label: "New resource",
      }),
    );
    await act(async () => pending.reject(new Error("old dismissal failed")));
    expect(view.container.textContent).toContain("New resource");
    expect(view.container.querySelector('[role="alert"]')).toBeNull();
    expect(button(view.container, "关闭提示").disabled).toBe(false);
    view.unmount();
  });

  it("subscribes before reading current and never replaces a newer event with a stale read", async () => {
    const initialRead = deferred<ApprovalNotificationData | null>();
    const view = harness(notification, {
      loadCurrent: vi.fn(() => initialRead.promise),
    });
    expect(view.api.subscribe).toHaveBeenCalledOnce();
    expect(view.api.loadCurrent).not.toHaveBeenCalled();
    await flush();
    expect(view.api.loadCurrent).toHaveBeenCalledOnce();
    act(() =>
      view.onChange({
        ...notification,
        id: "new-event",
        resource_label: "Newest resource",
      }),
    );
    await act(async () => initialRead.resolve(notification));
    expect(view.container.textContent).toContain("Newest resource");
    expect(view.container.textContent).not.toContain(
      notification.resource_label,
    );
    view.unmount();
    expect(view.unlisten).toHaveBeenCalledOnce();
  });

  it("cancels terminal expiry on unmount", async () => {
    const view = harness({ ...notification, stage: "failed" });
    await flush();
    view.unmount();
    await advance(5000);
    expect(view.api.dismiss).not.toHaveBeenCalled();
  });
});
