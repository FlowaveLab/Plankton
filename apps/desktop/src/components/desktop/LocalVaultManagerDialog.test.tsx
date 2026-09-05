// @vitest-environment jsdom

import { act } from "react";
import ReactDOM from "react-dom/client";
import { afterEach, describe, expect, it, vi } from "vitest";

import { LocalVaultManagerDialog } from "./LocalVaultManagerDialog";

const { invoke } = vi.hoisted(() => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({ invoke }));
Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });

function setInput(input: HTMLInputElement, value: string): void {
  Object.getOwnPropertyDescriptor(
    HTMLInputElement.prototype,
    "value",
  )?.set?.call(input, value);
  input.dispatchEvent(new Event("input", { bubbles: true }));
}

afterEach(() => {
  document.body.innerHTML = "";
  invoke.mockReset();
});

describe("LocalVaultManagerDialog", () => {
  it("requires the exact vault name after warning about disappearing passwords", async () => {
    invoke.mockImplementation((command: string) => {
      if (command === "list_local_vaults") {
        return Promise.resolve([
          {
            id: "work",
            file_name: "work.kdbx",
            unlock_file_name: ".work.unlock",
            label: "work",
            subtitle: "Encrypted KDBX4",
            exists: true,
            unlock_file_exists: false,
          },
        ]);
      }
      if (command === "preview_local_vault_deletion") {
        return Promise.resolve({
          vault_id: "work",
          item_count: 3,
          field_count: 5,
        });
      }
      if (command === "delete_local_vault") {
        return Promise.resolve({ vault_id: "work", removed_fields: 5 });
      }
      return Promise.reject(new Error(`unexpected command ${command}`));
    });
    const container = document.createElement("div");
    document.body.appendChild(container);
    const root = ReactDOM.createRoot(container);
    await act(async () => {
      root.render(
        <LocalVaultManagerDialog
          locale="en"
          onChanged={() => {}}
          onClose={() => {}}
        />,
      );
    });
    await act(async () => {
      Array.from(container.querySelectorAll("button"))
        .find((button) => button.textContent === "Delete")
        ?.click();
    });

    expect(container.textContent).toContain("Passwords will disappear");
    expect(container.textContent).toContain("3 password items and 5 fields");
    const deleteButton = Array.from(
      container.querySelectorAll<HTMLButtonElement>("button"),
    ).find((button) => button.textContent === "Delete vault");
    expect(deleteButton?.disabled).toBe(true);
    const confirmationInput = Array.from(
      container.querySelectorAll("input"),
    ).find((input) =>
      input.closest("label")?.textContent?.includes("Type work"),
    );
    await act(async () => {
      if (confirmationInput) setInput(confirmationInput, "work");
    });
    expect(deleteButton?.disabled).toBe(false);
    await act(async () => deleteButton?.click());

    expect(invoke).toHaveBeenCalledWith("delete_local_vault", {
      vaultId: "work",
      confirmation: "work",
    });
    act(() => root.unmount());
  });

  it("imports a missing unlock file locally and can reveal it for secure transfer", async () => {
    let unlockReady = false;
    invoke.mockImplementation((command: string, payload?: unknown) => {
      const vault = {
        id: "work",
        file_name: "work.kdbx",
        unlock_file_name: ".work.unlock",
        label: "work",
        subtitle: unlockReady
          ? "Encrypted KDBX4 · unlock ready"
          : "Encrypted KDBX4 · unlock required",
        exists: true,
        unlock_file_exists: unlockReady,
      };
      if (command === "list_local_vaults") return Promise.resolve([vault]);
      if (command === "pick_local_vault_unlock_file") {
        expect(payload).toEqual({ vaultId: "work" });
        unlockReady = true;
        return Promise.resolve({ ...vault, unlock_file_exists: true });
      }
      if (command === "reveal_local_vault_unlock_file") {
        expect(payload).toEqual({ vaultId: "work" });
        return Promise.resolve();
      }
      return Promise.reject(new Error(`unexpected command ${command}`));
    });
    const container = document.createElement("div");
    document.body.appendChild(container);
    const root = ReactDOM.createRoot(container);
    await act(async () => {
      root.render(
        <LocalVaultManagerDialog
          locale="en"
          onChanged={() => {}}
          onClose={() => {}}
        />,
      );
    });

    expect(container.textContent).toContain(
      "transfer its unlock file separately through a secure channel",
    );
    await act(async () => {
      Array.from(container.querySelectorAll("button"))
        .find((button) => button.textContent === "Choose unlock file")
        ?.click();
    });
    expect(container.textContent).toContain(".work.unlock · ready");
    await act(async () => {
      Array.from(container.querySelectorAll("button"))
        .find((button) => button.textContent === "Show unlock file")
        ?.click();
    });
    expect(invoke).toHaveBeenCalledWith("reveal_local_vault_unlock_file", {
      vaultId: "work",
    });
    act(() => root.unmount());
  });
});
