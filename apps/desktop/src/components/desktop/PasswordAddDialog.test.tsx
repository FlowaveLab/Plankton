// @vitest-environment jsdom

import { act, useState, type JSX } from "react";
import ReactDOM from "react-dom/client";
import { afterEach, describe, expect, it, vi } from "vitest";

import { defaultExposurePolicy } from "../ExposurePolicy";

import { PasswordAddDialog } from "./PasswordAddDialog";

const { invoke } = vi.hoisted(() => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({ invoke }));
Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });

type Deferred<T> = {
  promise: Promise<T>;
  reject: (reason: Error) => void;
  resolve: (value: T) => void;
};

function deferred<T>(): Deferred<T> {
  let rejectPromise: ((reason: Error) => void) | null = null;
  let resolvePromise: ((value: T) => void) | null = null;
  const promise = new Promise<T>((resolve, reject) => {
    resolvePromise = resolve;
    rejectPromise = reject;
  });
  return {
    promise,
    reject(reason) {
      if (!rejectPromise) throw new Error("deferred promise is not ready");
      rejectPromise(reason);
    },
    resolve(value) {
      if (!resolvePromise) throw new Error("deferred promise is not ready");
      resolvePromise(value);
    },
  };
}

function setInputValue(input: HTMLInputElement, value: string): void {
  Object.getOwnPropertyDescriptor(
    HTMLInputElement.prototype,
    "value",
  )?.set?.call(input, value);
  input.dispatchEvent(new Event("input", { bubbles: true }));
}

function inputByLabel(
  container: HTMLElement,
  labelText: string,
): HTMLInputElement | null {
  const label = Array.from(container.querySelectorAll("label")).find((entry) =>
    entry.textContent?.startsWith(labelText),
  );
  return label?.querySelector("input") ?? null;
}

afterEach(() => {
  document.body.innerHTML = "";
  invoke.mockReset();
});

describe("PasswordAddDialog", () => {
  it("requires human review and final confirmation for editable 1Password imports", async () => {
    invoke.mockImplementation((command: string) => {
      if (command === "preview_password_draft")
        return Promise.resolve({
          descriptor: {
            kind: "one_password",
            account: "team",
            fields: [{ key: "TOKEN", reference: "op://Work/Service/password" }],
          },
          entries: [{ key: "TOKEN", value: "test-imported-token" }],
        });
      if (command === "confirm_password_draft")
        return Promise.resolve({
          draft_id: "op-draft",
          destination: "plankton:default",
          resource_ids: [],
        });
      return Promise.resolve([]);
    });
    const container = document.createElement("div");
    document.body.appendChild(container);
    const root = ReactDOM.createRoot(container);
    await act(async () =>
      root.render(
        <PasswordAddDialog
          draftId="op-draft"
          locale="en"
          onClose={() => {}}
          onCommitted={() => {}}
        />,
      ),
    );
    expect(container.textContent).toContain("Import from 1Password");
    expect(
      container.querySelector<HTMLDetailsElement>(
        ".collection-exposure-profile",
      )?.open,
    ).toBe(false);
    expect(container.textContent).toContain("op://Work/Service/password");
    const input = container.querySelector<HTMLInputElement>(
      '[aria-label="TOKEN password"]',
    )!;
    expect(input.type).toBe("password");
    expect(input.readOnly).toBe(false);
    await act(async () => setInputValue(input, "human-corrected-token"));
    expect(
      invoke.mock.calls.some(
        ([command]) => command === "confirm_password_draft",
      ),
    ).toBe(false);
    await act(async () =>
      Array.from(container.querySelectorAll("button"))
        .find((button) => button.textContent === "Next: review and save")
        ?.click(),
    );
    expect(
      invoke.mock.calls.some(
        ([command]) => command === "confirm_password_draft",
      ),
    ).toBe(false);
    await act(async () =>
      Array.from(container.querySelectorAll("button"))
        .find((button) => button.textContent === "Confirm and save")
        ?.click(),
    );
    expect(invoke).toHaveBeenCalledWith(
      "confirm_password_draft",
      expect.objectContaining({
        draftId: "op-draft",
        values: { TOKEN: "human-corrected-token" },
      }),
    );
    act(() => root.unmount());
  });

  it("shows an LLM-suggested Direct field and lets a human protect it without changing its value", async () => {
    invoke.mockImplementation((command: string) => {
      if (command === "preview_password_draft")
        return Promise.resolve({
          descriptor: { kind: "environment", names: ["TOKEN"] },
          entries: [{ key: "TOKEN", value: "test-direct-token" }],
          suggested_layout: {
            default_exposure_policy: {
              access_mode: "direct",
              breach_action: "human_review",
              surfaces: [],
            },
          },
        });
      return Promise.resolve([]);
    });
    const container = document.createElement("div");
    document.body.appendChild(container);
    const root = ReactDOM.createRoot(container);
    await act(async () =>
      root.render(
        <PasswordAddDialog
          draftId="direct"
          locale="en"
          onClose={() => {}}
          onCommitted={() => {}}
        />,
      ),
    );
    const input = container.querySelector<HTMLInputElement>(
      '[aria-label="TOKEN password"]',
    )!;
    expect(input.type).toBe("text");
    expect(input.value).toBe("test-direct-token");
    expect(input.readOnly).toBe(false);
    await act(async () =>
      container
        .querySelector<HTMLInputElement>(
          '.exposure-policy-mode input[value="protected"]',
        )
        ?.click(),
    );
    expect(input.type).toBe("password");
    expect(input.value).toBe("test-direct-token");
    act(() => root.unmount());
  });

  it("inherits the collection default and submits only customized field overrides", async () => {
    invoke.mockImplementation((command: string) => {
      if (command === "preview_password_draft")
        return Promise.resolve({
          descriptor: { kind: "environment", names: ["A", "B"] },
          entries: [
            { key: "A", value: "test-a" },
            { key: "B", value: "test-b" },
          ],
        });
      if (command === "confirm_password_draft")
        return Promise.resolve({
          draft_id: "inheritance",
          destination: "plankton:default",
          resource_ids: [],
        });
      return Promise.resolve([]);
    });
    const container = document.createElement("div");
    document.body.appendChild(container);
    const root = ReactDOM.createRoot(container);
    await act(async () =>
      root.render(
        <PasswordAddDialog
          draftId="inheritance"
          locale="en"
          onClose={() => {}}
          onCommitted={() => {}}
        />,
      ),
    );
    expect(
      container.querySelectorAll(".field-exposure-profile canvas"),
    ).toHaveLength(0);
    await act(async () =>
      container
        .querySelector<HTMLInputElement>(
          '.field-exposure-profile input[value="custom"]',
        )
        ?.click(),
    );
    expect(
      container.querySelectorAll(".field-exposure-profile canvas"),
    ).toHaveLength(1);
    await act(async () =>
      container
        .querySelector<HTMLInputElement>(
          '.collection-exposure-profile input[value="direct"]',
        )
        ?.click(),
    );
    expect(
      container.querySelector<HTMLInputElement>('[aria-label="A password"]')
        ?.type,
    ).toBe("password");
    expect(
      container.querySelector<HTMLInputElement>('[aria-label="B password"]')
        ?.type,
    ).toBe("text");
    await act(async () =>
      Array.from(container.querySelectorAll("button"))
        .find((button) => button.textContent === "Next: review and save")
        ?.click(),
    );
    await act(async () =>
      Array.from(container.querySelectorAll("button"))
        .find((button) => button.textContent === "Confirm and save")
        ?.click(),
    );
    expect(invoke).toHaveBeenCalledWith(
      "confirm_password_draft",
      expect.objectContaining({
        layout: expect.objectContaining({
          default_exposure_policy: {
            ...defaultExposurePolicy(),
            access_mode: "direct",
          },
          field_exposure_policies: { A: defaultExposurePolicy() },
        }),
      }),
    );
    act(() => root.unmount());
  });

  it("loads selectable Plankton vaults and honors a CLI destination suggestion", async () => {
    invoke.mockImplementation((command: string) => {
      if (command === "preview_password_draft") {
        return Promise.resolve({
          descriptor: { kind: "environment", names: ["TOKEN"] },
          entries: [{ key: "TOKEN", value: "secret" }],
          suggested_destination: {
            kind: "plankton",
            vault_id: "work",
          },
        });
      }
      if (command === "list_backend_connections") return Promise.resolve([]);
      if (command === "list_local_vaults") {
        return Promise.resolve([
          { id: "default", label: "Default" },
          { id: "work", label: "Work" },
        ]);
      }
      return Promise.reject(new Error(`unexpected command ${command}`));
    });
    const container = document.createElement("div");
    document.body.appendChild(container);
    const root = ReactDOM.createRoot(container);
    await act(async () => {
      root.render(
        <PasswordAddDialog
          draftId="draft-work"
          locale="en"
          onClose={() => {}}
          onCommitted={() => {}}
        />,
      );
    });

    const vaultLabel = Array.from(container.querySelectorAll("label")).find(
      (label) => label.textContent?.startsWith("Vault"),
    );
    const vaultSelect = vaultLabel?.querySelector("select");
    expect(vaultSelect?.value).toBe("work");
    expect(
      Array.from(vaultSelect?.options ?? []).map((option) => option.text),
    ).toEqual(["Default", "Work"]);
    act(() => root.unmount());
  });

  it("uses the CLI title as an editable default in final confirmation", async () => {
    invoke.mockImplementation((command: string) => {
      if (command === "preview_password_draft") {
        return Promise.resolve({
          descriptor: { kind: "file", path: "/workspace/example/.env" },
          entries: [{ key: "TOKEN", value: "secret" }],
          suggested_item_title: "Example production",
        });
      }
      if (command === "list_backend_connections") return Promise.resolve([]);
      return Promise.reject(new Error(`unexpected command ${command}`));
    });
    const container = document.createElement("div");
    document.body.appendChild(container);
    const root = ReactDOM.createRoot(container);
    await act(async () => {
      root.render(
        <PasswordAddDialog
          draftId="draft-title"
          locale="en"
          onClose={() => {}}
          onCommitted={() => {}}
        />,
      );
    });

    const titleInput = inputByLabel(container, "Item title");
    expect(titleInput?.value).toBe("Example production");
    await act(async () => {
      if (titleInput) setInputValue(titleInput, "Example staging");
    });
    act(() => {
      Array.from(container.querySelectorAll("button"))
        .find((button) => button.textContent === "Next: review and save")
        ?.click();
    });
    expect(container.textContent).toContain("Item “Example staging”");
    act(() => root.unmount());
  });

  it("derives a meaningful localized title for an unnamed .env draft", async () => {
    invoke.mockImplementation((command: string) => {
      if (command === "preview_password_draft") {
        return Promise.resolve({
          descriptor: { kind: "file", path: "/workspace/example/.env" },
          entries: [{ key: "TOKEN", value: "secret" }],
        });
      }
      if (command === "list_backend_connections") return Promise.resolve([]);
      return Promise.reject(new Error(`unexpected command ${command}`));
    });
    const container = document.createElement("div");
    document.body.appendChild(container);
    const root = ReactDOM.createRoot(container);
    await act(async () => {
      root.render(
        <PasswordAddDialog
          draftId="draft-env-title"
          locale="zh-CN"
          onClose={() => {}}
          onCommitted={() => {}}
        />,
      );
    });
    expect(inputByLabel(container, "条目标题")?.value).toBe("example 环境变量");
    act(() => root.unmount());
  });

  it("fully resets a final revealed draft before loading a replacement draft", async () => {
    const nextPreview = deferred<{
      descriptor: { kind: string; names: string[] };
      entries: Array<{ key: string; value: string }>;
    }>();
    invoke.mockImplementation(
      (command: string, args?: { draftId?: string }) => {
        if (command === "preview_password_draft") {
          if (args?.draftId === "draft-old") {
            return Promise.resolve({
              descriptor: { kind: "environment", names: ["OLD_TOKEN"] },
              entries: [{ key: "OLD_TOKEN", value: "old-secret" }],
            });
          }
          if (args?.draftId === "draft-new") {
            return nextPreview.promise;
          }
        }
        if (command === "list_backend_connections") {
          return Promise.resolve([]);
        }
        return Promise.reject(new Error(`unexpected command ${command}`));
      },
    );
    const container = document.createElement("div");
    document.body.appendChild(container);
    const root = ReactDOM.createRoot(container);
    await act(async () => {
      root.render(
        <PasswordAddDialog
          draftId="draft-old"
          locale="en"
          onClose={() => {}}
          onCommitted={() => {}}
        />,
      );
    });

    act(() => {
      Array.from(container.querySelectorAll("button"))
        .find((button) =>
          button.getAttribute("aria-label")?.startsWith("Show "),
        )
        ?.click();
    });
    expect(
      container.querySelector<HTMLInputElement>(
        '[aria-label="OLD_TOKEN password"]',
      )?.value,
    ).toBe("old-secret");
    const oldSection = inputByLabel(container, "Section");
    const oldTags = inputByLabel(container, "Tags");
    const oldVault = inputByLabel(container, "Vault");
    await act(async () => {
      if (oldSection) setInputValue(oldSection, "Old section");
      if (oldTags) setInputValue(oldTags, "old-tag");
      if (oldVault) setInputValue(oldVault, "old-vault");
    });
    act(() => {
      Array.from(container.querySelectorAll("button"))
        .find((button) => button.textContent === "Next: review and save")
        ?.click();
    });
    expect(container.textContent).toContain("Confirm and save");

    await act(async () => {
      root.render(
        <PasswordAddDialog
          draftId="draft-new"
          locale="en"
          onClose={() => {}}
          onCommitted={() => {}}
        />,
      );
    });
    expect(container.textContent).toContain("Loading draft");
    expect(container.textContent).not.toContain("Confirm and save");
    expect(container.textContent).not.toContain("old-secret");
    expect(container.textContent).not.toContain("new-secret");

    await act(async () => {
      nextPreview.resolve({
        descriptor: { kind: "environment", names: ["NEW_TOKEN"] },
        entries: [{ key: "NEW_TOKEN", value: "new-secret" }],
      });
      await nextPreview.promise;
    });

    expect(container.textContent).toContain("NEW_TOKEN");
    expect(container.textContent).not.toContain("new-secret");
    expect(inputByLabel(container, "Item title")?.value).toBe("NEW_TOKEN");
    expect(inputByLabel(container, "Section")?.value).toBe("Credentials");
    expect(inputByLabel(container, "Tags")?.value).toBe("");
    expect(inputByLabel(container, "Vault")?.value).toBe("default");
    expect(container.textContent).toContain("Next: review and save");
    expect(container.textContent).not.toContain("Confirm and save");

    act(() => {
      Array.from(container.querySelectorAll("button"))
        .find((button) => button.textContent === "Next: review and save")
        ?.click();
    });
    expect(container.textContent).toContain("Confirm and save");
    act(() => root.unmount());
  });

  it("ignores a stale preview that resolves after the draft id changes", async () => {
    const stalePreview = deferred<{
      descriptor: { kind: string; names: string[] };
      entries: Array<{ key: string; value: string }>;
    }>();
    const currentPreview = deferred<{
      descriptor: { kind: string; names: string[] };
      entries: Array<{ key: string; value: string }>;
    }>();
    invoke.mockImplementation(
      (command: string, args?: { draftId?: string }) => {
        if (command === "preview_password_draft") {
          return args?.draftId === "draft-stale"
            ? stalePreview.promise
            : currentPreview.promise;
        }
        if (command === "list_backend_connections") {
          return Promise.resolve([]);
        }
        return Promise.reject(new Error(`unexpected command ${command}`));
      },
    );
    const container = document.createElement("div");
    document.body.appendChild(container);
    const root = ReactDOM.createRoot(container);
    await act(async () => {
      root.render(
        <PasswordAddDialog
          draftId="draft-stale"
          locale="en"
          onClose={() => {}}
          onCommitted={() => {}}
        />,
      );
    });
    await act(async () => {
      root.render(
        <PasswordAddDialog
          draftId="draft-current"
          locale="en"
          onClose={() => {}}
          onCommitted={() => {}}
        />,
      );
    });

    await act(async () => {
      stalePreview.resolve({
        descriptor: { kind: "environment", names: ["STALE_TOKEN"] },
        entries: [{ key: "STALE_TOKEN", value: "stale-secret" }],
      });
      await stalePreview.promise;
    });
    expect(container.textContent).not.toContain("STALE_TOKEN");
    expect(container.textContent).not.toContain("stale-secret");

    await act(async () => {
      currentPreview.resolve({
        descriptor: { kind: "environment", names: ["CURRENT_TOKEN"] },
        entries: [{ key: "CURRENT_TOKEN", value: "current-secret" }],
      });
      await currentPreview.promise;
    });
    expect(container.textContent).toContain("CURRENT_TOKEN");
    expect(container.textContent).not.toContain("current-secret");
    act(() => root.unmount());
  });

  it("does not render a stale commit failure into a replacement draft", async () => {
    const staleCommit = deferred<{
      draft_id: string;
      destination: string;
      resource_ids: string[];
    }>();
    invoke.mockImplementation(
      (command: string, args?: { draftId?: string }) => {
        if (command === "preview_password_draft") {
          const isOld = args?.draftId === "draft-old";
          return Promise.resolve({
            descriptor: {
              kind: "environment",
              names: [isOld ? "OLD_TOKEN" : "NEW_TOKEN"],
            },
            entries: [
              {
                key: isOld ? "OLD_TOKEN" : "NEW_TOKEN",
                value: isOld ? "old-secret" : "new-secret",
              },
            ],
          });
        }
        if (command === "list_backend_connections") {
          return Promise.resolve([]);
        }
        if (command === "confirm_password_draft") {
          return staleCommit.promise;
        }
        return Promise.reject(new Error(`unexpected command ${command}`));
      },
    );
    const container = document.createElement("div");
    document.body.appendChild(container);
    const root = ReactDOM.createRoot(container);
    await act(async () => {
      root.render(
        <PasswordAddDialog
          draftId="draft-old"
          locale="en"
          onClose={() => {}}
          onCommitted={() => {}}
        />,
      );
    });
    await act(async () => {
      Array.from(container.querySelectorAll<HTMLButtonElement>("button"))
        .find((button) => button.textContent === "Next: review and save")
        ?.click();
    });
    await act(async () => {
      Array.from(container.querySelectorAll<HTMLButtonElement>("button"))
        .find((button) => button.textContent === "Confirm and save")
        ?.click();
    });

    await act(async () => {
      root.render(
        <PasswordAddDialog
          draftId="draft-new"
          locale="en"
          onClose={() => {}}
          onCommitted={() => {}}
        />,
      );
    });
    expect(container.textContent).toContain("NEW_TOKEN");
    expect(container.textContent).not.toContain("new-secret");

    await act(async () => {
      staleCommit.reject(new Error("SENTINEL_STALE_COMMIT_FAILURE"));
      await staleCommit.promise.catch(() => undefined);
    });

    const finalText = container.textContent;
    act(() => root.unmount());
    expect(finalText).toContain("NEW_TOKEN");
    expect(finalText).toContain("Next: review and save");
    expect(finalText).not.toContain("Password draft could not be saved");
    expect(finalText).not.toContain("SENTINEL_STALE_COMMIT_FAILURE");
  });

  it("reports a successful commit identity without updating state after unmount", async () => {
    const pendingCommit = deferred<{
      draft_id: string;
      destination: string;
      resource_ids: string[];
    }>();
    invoke.mockImplementation((command: string) => {
      if (command === "preview_password_draft") {
        return Promise.resolve({
          descriptor: { kind: "environment", names: ["TOKEN"] },
          entries: [{ key: "TOKEN", value: "secret" }],
        });
      }
      if (command === "list_backend_connections") {
        return Promise.resolve([]);
      }
      if (command === "confirm_password_draft") {
        return pendingCommit.promise;
      }
      return Promise.reject(new Error(`unexpected command ${command}`));
    });
    const committed = vi.fn();
    const close = vi.fn();
    const consoleError = vi
      .spyOn(console, "error")
      .mockImplementation(() => undefined);
    const container = document.createElement("div");
    document.body.appendChild(container);
    const root = ReactDOM.createRoot(container);
    try {
      await act(async () => {
        root.render(
          <PasswordAddDialog
            draftId="draft-unmounted"
            locale="en"
            onClose={close}
            onCommitted={committed}
          />,
        );
      });
      await act(async () => {
        Array.from(container.querySelectorAll<HTMLButtonElement>("button"))
          .find((button) => button.textContent === "Next: review and save")
          ?.click();
      });
      await act(async () => {
        Array.from(container.querySelectorAll<HTMLButtonElement>("button"))
          .find((button) => button.textContent === "Confirm and save")
          ?.click();
      });
      const headerClose = container.querySelector<HTMLButtonElement>(
        ".page-modal-header .page-icon-button",
      );
      const headerCloseDisabled = headerClose?.disabled;
      const headerCloseBusy = headerClose?.getAttribute("aria-busy");
      await act(async () => {
        headerClose?.click();
      });
      act(() => root.unmount());

      const receipt = {
        draft_id: "draft-unmounted",
        destination: "plankton:default",
        resource_ids: ["plankton://field/draft-unmounted/token"],
      };
      await act(async () => {
        pendingCommit.resolve(receipt);
        await pendingCommit.promise;
      });

      expect(headerCloseDisabled).toBe(true);
      expect(headerCloseBusy).toBe("true");
      expect(close).not.toHaveBeenCalled();
      expect(committed).toHaveBeenCalledWith("draft-unmounted", receipt);
      expect(consoleError).not.toHaveBeenCalled();
    } finally {
      consoleError.mockRestore();
    }
  });

  it("previews real draft fields and commits the exact Plankton destination", async () => {
    invoke.mockImplementation((command: string) => {
      if (command === "preview_password_draft") {
        return Promise.resolve({
          descriptor: { kind: "file" },
          entries: [{ key: "service.token", value: "actual-secret" }],
        });
      }
      if (command === "list_backend_connections") {
        return Promise.resolve([]);
      }
      if (command === "confirm_password_draft") {
        return Promise.resolve({
          draft_id: "draft-1",
          destination: "plankton:default",
          resource_ids: ["plankton://field/draft-1/service-token"],
        });
      }
      return Promise.reject(new Error(`unexpected command ${command}`));
    });
    const committed = vi.fn();
    const container = document.createElement("div");
    document.body.appendChild(container);
    const root = ReactDOM.createRoot(container);
    await act(async () => {
      root.render(
        <PasswordAddDialog
          draftId="draft-1"
          locale="en"
          onClose={() => {}}
          onCommitted={committed}
        />,
      );
    });

    expect(
      container.querySelector<HTMLInputElement>(
        '[aria-label="service.token field label"]',
      )?.value,
    ).toBe("service.token");
    expect(container.textContent).not.toContain("actual-secret");
    const sourceContext = container.querySelector(
      ".password-add-source-context",
    );
    expect(sourceContext?.textContent).toContain("CLI · file");
    expect(sourceContext?.textContent).toContain("Draft ID");
    expect((sourceContext as HTMLDetailsElement).open).toBe(false);
    expect(
      container.querySelector<HTMLDetailsElement>(".password-add-optional")
        ?.open,
    ).toBe(false);
    expect(
      container.querySelector<HTMLDetailsElement>(
        ".password-add-field-exposure > details",
      )?.open,
    ).toBe(false);
    expect(sourceContext?.closest(".password-add-review-grid")).not.toBeNull();
    const reveal = Array.from(container.querySelectorAll("button")).find(
      (button) => button.getAttribute("aria-label")?.startsWith("Show "),
    );
    await act(async () => reveal?.click());
    expect(
      container.querySelector<HTMLInputElement>(
        '[aria-label="service.token password"]',
      )?.type,
    ).toBe("text");
    expect(
      container.querySelector<HTMLInputElement>(
        '[aria-label="service.token password"]',
      )?.value,
    ).toBe("actual-secret");
    expect(container.querySelector(".password-add-field-row")).not.toBeNull();
    expect(
      container.querySelector(".password-add-field-identity input"),
    ).not.toBeNull();
    expect(
      container.querySelector(
        ".password-add-field-value .secret-input-control input",
      ),
    ).not.toBeNull();
    expect(
      container.querySelector(".password-add-field-exposure"),
    ).not.toBeNull();

    await act(async () => {
      setInputValue(
        container.querySelector<HTMLInputElement>(
          '[aria-label="service.token password"]',
        )!,
        "human-edited-secret",
      );
    });
    const review = Array.from(container.querySelectorAll("button")).find(
      (button) => button.textContent === "Next: review and save",
    );
    act(() => review?.click());
    expect(invoke).not.toHaveBeenCalledWith(
      "confirm_password_draft",
      expect.anything(),
    );
    const confirm = Array.from(container.querySelectorAll("button")).find(
      (button) => button.textContent === "Confirm and save",
    );
    await act(async () => {
      confirm?.click();
    });

    expect(invoke).toHaveBeenCalledWith("confirm_password_draft", {
      draftId: "draft-1",
      destination: { kind: "plankton", vault_id: "default" },
      layout: {
        item_title: "Imported file",
        section: "Credentials",
        tags: [],
        description: null,
        field_labels: { "service.token": "service.token" },
        field_resources: {},
        default_exposure_policy: defaultExposurePolicy(),
        field_exposure_policies: {},
      },
      values: { "service.token": "human-edited-secret" },
    });
    expect(committed).toHaveBeenCalledOnce();
    act(() => root.unmount());
  });

  it("offers only enabled create-capable external backends", async () => {
    invoke.mockImplementation((command: string) => {
      if (command === "preview_password_draft") {
        return Promise.resolve({
          descriptor: { kind: "environment" },
          entries: [{ key: "TOKEN", value: "actual-secret" }],
        });
      }
      if (command === "list_backend_connections") {
        return Promise.resolve([
          {
            id: "onepassword",
            backend_kind: "one_password",
            display_name: "1Password",
            enabled: true,
            capabilities: ["read", "create"],
            config: { account: "account-1" },
          },
          {
            id: "bitwarden",
            backend_kind: "bitwarden",
            display_name: "Bitwarden",
            enabled: false,
            capabilities: ["read", "create"],
          },
        ]);
      }
      if (command === "confirm_password_draft") {
        return Promise.resolve({
          draft_id: "draft-2",
          destination: "external:onepassword:Private",
          resource_ids: ["plankton://field/draft-2/token"],
        });
      }
      if (command === "list_onepassword_vaults_command") {
        return Promise.resolve([{ id: "Private", label: "Private" }]);
      }
      return Promise.reject(new Error(`unexpected command ${command}`));
    });
    const container = document.createElement("div");
    document.body.appendChild(container);
    const root = ReactDOM.createRoot(container);
    await act(async () => {
      root.render(
        <PasswordAddDialog
          draftId="draft-2"
          locale="en"
          onClose={() => {}}
          onCommitted={() => {}}
        />,
      );
    });

    const backend = Array.from(container.querySelectorAll("select")).find(
      (select) => select.textContent?.includes("1Password"),
    );
    expect(backend?.textContent).toContain("1Password");
    expect(backend?.textContent).not.toContain("Bitwarden");
    if (backend) {
      await act(async () => {
        Object.getOwnPropertyDescriptor(
          HTMLSelectElement.prototype,
          "value",
        )?.set?.call(backend, "onepassword");
        backend.dispatchEvent(new Event("change", { bubbles: true }));
      });
    }
    const review = Array.from(container.querySelectorAll("button")).find(
      (button) => button.textContent === "Next: review and save",
    );
    act(() => review?.click());
    const confirm = Array.from(container.querySelectorAll("button")).find(
      (button) => button.textContent === "Confirm and save",
    );
    await act(async () => {
      confirm?.click();
    });

    expect(invoke).toHaveBeenCalledWith("confirm_password_draft", {
      draftId: "draft-2",
      destination: {
        kind: "external",
        binding_id: "onepassword",
        vault_id: "Private",
      },
      layout: {
        item_title: "Environment secrets (1 fields)",
        section: "Credentials",
        tags: [],
        description: null,
        field_labels: { TOKEN: "TOKEN" },
        field_resources: {},
        default_exposure_policy: defaultExposurePolicy(),
        field_exposure_policies: {},
      },
      values: { TOKEN: "actual-secret" },
    });
    act(() => root.unmount());
  });

  it("collects every manual field locally before committing one aggregated draft", async () => {
    invoke.mockImplementation((command: string) => {
      if (command === "preview_password_draft") {
        return Promise.resolve({
          descriptor: { kind: "manual", keys: ["CLIENT_ID", "CLIENT_SECRET"] },
          entries: [
            { key: "CLIENT_ID", value: "" },
            { key: "CLIENT_SECRET", value: "" },
          ],
          suggested_item_title: "Example credentials",
        });
      }
      if (command === "list_backend_connections") return Promise.resolve([]);
      if (command === "confirm_password_draft") {
        return Promise.resolve({
          draft_id: "draft-manual",
          destination: "plankton:default",
          resource_ids: ["secret/client-id", "secret/client-secret"],
        });
      }
      return Promise.reject(new Error(`unexpected command ${command}`));
    });
    const container = document.createElement("div");
    document.body.appendChild(container);
    const root = ReactDOM.createRoot(container);
    await act(async () => {
      root.render(
        <PasswordAddDialog
          draftId="draft-manual"
          locale="en"
          onClose={() => {}}
          onCommitted={() => {}}
        />,
      );
    });

    const review = Array.from(container.querySelectorAll("button")).find(
      (button) => button.textContent === "Next: review and save",
    );
    expect(review?.disabled).toBe(true);
    const clientId = container.querySelector<HTMLInputElement>(
      '[aria-label="CLIENT_ID password"]',
    );
    const clientSecret = container.querySelector<HTMLInputElement>(
      '[aria-label="CLIENT_SECRET password"]',
    );
    expect(clientId?.type).toBe("password");
    expect(clientSecret?.type).toBe("password");
    await act(async () => {
      if (clientId) setInputValue(clientId, "human-client-id");
      if (clientSecret) setInputValue(clientSecret, "human-client-secret");
    });
    expect(review?.disabled).toBe(false);
    act(() => review?.click());
    expect(container.textContent).not.toContain("human-client-id");
    expect(container.textContent).not.toContain("human-client-secret");
    const confirm = Array.from(container.querySelectorAll("button")).find(
      (button) => button.textContent === "Confirm and save",
    );
    await act(async () => {
      confirm?.click();
    });

    expect(invoke).toHaveBeenCalledWith(
      "confirm_password_draft",
      expect.objectContaining({
        draftId: "draft-manual",
        values: {
          CLIENT_ID: "human-client-id",
          CLIENT_SECRET: "human-client-secret",
        },
      }),
    );
    act(() => root.unmount());
  });

  it("announces and focuses final confirmation before trapping Tab navigation", async () => {
    invoke.mockImplementation((command: string) => {
      if (command === "preview_password_draft") {
        return Promise.resolve({
          descriptor: { kind: "environment", names: ["TOKEN"] },
          entries: [{ key: "TOKEN", value: "secret" }],
        });
      }
      if (command === "list_backend_connections") {
        return Promise.resolve([]);
      }
      return Promise.reject(new Error(`unexpected command ${command}`));
    });
    const container = document.createElement("div");
    document.body.appendChild(container);
    const root = ReactDOM.createRoot(container);
    await act(async () => {
      root.render(
        <PasswordAddDialog
          draftId="draft-focus"
          locale="en"
          onClose={() => {}}
          onCommitted={() => {}}
        />,
      );
    });

    await act(async () => {
      Array.from(container.querySelectorAll<HTMLButtonElement>("button"))
        .find((button) => button.textContent === "Next: review and save")
        ?.click();
    });
    const heading = container.querySelector<HTMLElement>(
      '[data-testid="password-final-confirmation-heading"]',
    );
    const headingTabIndex = heading?.tabIndex;
    const headingLive = heading?.getAttribute("aria-live");
    const headingFocused = document.activeElement === heading;

    if (heading) {
      await act(async () => {
        document.dispatchEvent(
          new KeyboardEvent("keydown", { bubbles: true, key: "Tab" }),
        );
      });
    }
    const forwardFocus =
      document.activeElement ===
      container.querySelector(".page-modal-header .page-icon-button");
    heading?.focus();
    if (heading) {
      await act(async () => {
        document.dispatchEvent(
          new KeyboardEvent("keydown", {
            bubbles: true,
            key: "Tab",
            shiftKey: true,
          }),
        );
      });
    }
    const backwardFocus = document.activeElement?.textContent;
    act(() => root.unmount());
    expect(headingTabIndex).toBe(-1);
    expect(headingLive).toBe("polite");
    expect(headingFocused).toBe(true);
    expect(forwardFocus).toBe(true);
    expect(backwardFocus).toBe("Confirm and save");
  });

  it("localizes the Chinese dialog close control", async () => {
    invoke.mockImplementation((command: string) => {
      if (command === "preview_password_draft") {
        return Promise.resolve({
          descriptor: { kind: "environment", names: ["TOKEN"] },
          entries: [{ key: "TOKEN", value: "secret" }],
        });
      }
      if (command === "list_backend_connections") {
        return Promise.resolve([]);
      }
      return Promise.reject(new Error(`unexpected command ${command}`));
    });
    const container = document.createElement("div");
    document.body.appendChild(container);
    const root = ReactDOM.createRoot(container);
    await act(async () => {
      root.render(
        <PasswordAddDialog
          draftId="draft-zh-close"
          locale="zh-CN"
          onClose={() => {}}
          onCommitted={() => {}}
        />,
      );
    });
    const localizedClose = container.querySelector(
      '[aria-label="关闭保存密码草稿对话框"]',
    );
    act(() => root.unmount());
    expect(localizedClose).not.toBeNull();
  });

  it("keeps a scrollable body between sticky modal chrome and restores focus after Escape", async () => {
    invoke.mockImplementation((command: string) => {
      if (command === "preview_password_draft") {
        return Promise.resolve({
          descriptor: { kind: "environment", names: ["TOKEN"] },
          entries: Array.from({ length: 16 }, (_, index) => ({
            key: `TOKEN_${index + 1}`,
            value: `secret-${index + 1}`,
          })),
        });
      }
      if (command === "list_backend_connections") {
        return Promise.resolve([]);
      }
      return Promise.reject(new Error(`unexpected command ${command}`));
    });
    const close = vi.fn();

    function Harness(): JSX.Element {
      const [open, setOpen] = useState(false);
      return (
        <div className="desktop-workspace">
          <main className="workspace-content">
            <button onClick={() => setOpen(true)} type="button">
              Open password dialog
            </button>
            {open ? (
              <PasswordAddDialog
                draftId="draft-long"
                locale="en"
                onClose={() => {
                  close();
                  setOpen(false);
                }}
                onCommitted={() => {}}
              />
            ) : null}
          </main>
        </div>
      );
    }

    const container = document.createElement("div");
    document.body.appendChild(container);
    const root = ReactDOM.createRoot(container);
    await act(async () => {
      root.render(<Harness />);
    });
    const opener = Array.from(container.querySelectorAll("button")).find(
      (button) => button.textContent === "Open password dialog",
    );
    opener?.focus();
    await act(async () => {
      opener?.click();
    });

    const dialog = container.querySelector('[role="dialog"]');
    expect(dialog?.querySelector(".page-modal-header")).not.toBeNull();
    expect(dialog?.querySelector(".page-modal-body")).not.toBeNull();
    expect(dialog?.querySelector(".page-modal-footer")).not.toBeNull();
    expect(dialog?.querySelector('[data-dialog-initial-focus="true"]')).toBe(
      document.activeElement,
    );
    expect(document.body.style.overflow).toBe("hidden");
    expect(
      container.querySelector<HTMLElement>(".workspace-content")?.style
        .overflow,
    ).toBe("hidden");

    await act(async () => {
      document.dispatchEvent(
        new KeyboardEvent("keydown", { bubbles: true, key: "Escape" }),
      );
    });
    expect(close).toHaveBeenCalledOnce();
    expect(container.querySelector('[role="dialog"]')).toBeNull();
    expect(document.activeElement).toBe(opener);
    expect(document.body.style.overflow).toBe("");
    expect(
      container.querySelector<HTMLElement>(".workspace-content")?.style
        .overflow,
    ).toBe("");
    act(() => root.unmount());
  });
});
