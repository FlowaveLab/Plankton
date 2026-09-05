// @vitest-environment jsdom

import { act, type ReactNode } from "react";
import ReactDOM from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const tauri = vi.hoisted(() => ({
  emit: vi.fn(),
  hide: vi.fn(),
  invoke: vi.fn(),
}));

vi.mock("@tauri-apps/api/core", () => ({ invoke: tauri.invoke }));
vi.mock("@tauri-apps/api/event", () => ({ emit: tauri.emit }));
vi.mock("@tauri-apps/api/window", () => ({
  getCurrentWindow: () => ({ hide: tauri.hide }),
}));

import { PASSWORD_CATALOG_CHANGED_EVENT } from "../passwordCatalogEvents";
import { PasswordChangeConfirmation } from "./PasswordChangeConfirmation";

Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });

type RenderHarness = {
  container: HTMLDivElement;
  unmount: () => void;
};

function render(node: ReactNode): RenderHarness {
  const container = document.createElement("div");
  document.body.appendChild(container);
  const root = ReactDOM.createRoot(container);
  act(() => root.render(node));
  return {
    container,
    unmount() {
      act(() => root.unmount());
      container.remove();
    },
  };
}

function pendingChange() {
  const titleEntry = {
    path: "/title",
    label: "Title",
    before: ".env",
    after: "示例内部服务凭据",
    impact: "metadata",
  } as const;
  return {
    batch_id: "batch-1",
    change_id: "change-1",
    version: 2,
    state: "pending_confirmation",
    reason: "整理密码标题",
    requested_by: "codex",
    diff: {
      items: [
        {
          record_id: "record-1",
          item_id: "item-example",
          title: "示例内部服务凭据",
          vaults: ["work"],
          entries: [titleEntry],
        },
        {
          record_id: "record-2",
          item_id: "item-example",
          title: "示例内部服务凭据",
          vaults: ["work"],
          entries: [titleEntry],
        },
      ],
      changed_items: 2,
      changed_fields: 0,
      breaking_changes: 0,
    },
    updated_at: "2020-01-01T00:00:00Z",
  } as const;
}

beforeEach(() => {
  tauri.emit.mockReset().mockResolvedValue(undefined);
  tauri.hide.mockReset().mockResolvedValue(undefined);
  tauri.invoke.mockReset();
});

afterEach(() => {
  document.body.innerHTML = "";
});

describe("PasswordChangeConfirmation", () => {
  it("renders field-level records as one logical password item", async () => {
    tauri.invoke.mockResolvedValue([pendingChange()]);
    const view = render(<PasswordChangeConfirmation />);

    await act(async () => {
      await Promise.resolve();
    });

    expect(view.container.querySelectorAll(".item-diff")).toHaveLength(1);
    expect(view.container.textContent).toContain("1 个密码项");
    expect(view.container.textContent).toContain("2 个字段");
    expect(view.container.textContent).toContain("确认 1 项更改");
    expect(view.container.textContent).toContain("保险库：work");
    expect(view.container.textContent?.match(/示例内部服务凭据/g)).toHaveLength(
      2,
    );
    view.unmount();
  });

  it("notifies the password page after a committed confirmation", async () => {
    tauri.invoke.mockImplementation((command: string) => {
      if (command === "pending_password_changes") {
        const pendingCalls = tauri.invoke.mock.calls.filter(
          ([calledCommand]) => calledCommand === "pending_password_changes",
        ).length;
        return Promise.resolve(pendingCalls === 1 ? [pendingChange()] : []);
      }
      if (command === "confirm_password_change_command") {
        return Promise.resolve({ ...pendingChange(), state: "committed" });
      }
      return Promise.reject(new Error(`unexpected command ${command}`));
    });
    const view = render(<PasswordChangeConfirmation />);
    await act(async () => {
      await Promise.resolve();
    });

    const confirm = Array.from(
      view.container.querySelectorAll<HTMLButtonElement>("button"),
    ).find((button) => button.textContent === "确认 1 项更改");
    await act(async () => {
      confirm?.click();
      await Promise.resolve();
      await Promise.resolve();
    });

    expect(tauri.emit).toHaveBeenCalledWith(PASSWORD_CATALOG_CHANGED_EVENT, {
      change_id: "change-1",
    });
    expect(tauri.hide).toHaveBeenCalled();
    view.unmount();
  });
});
