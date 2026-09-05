// @vitest-environment jsdom

import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { act, type ReactNode } from "react";
import ReactDOM from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { PasswordManagementView } from "../PasswordManagementView";
import { defaultExposurePolicy } from "../ExposurePolicy";
import { PasswordVaultPage } from "./PasswordVaultPage";
import type { PasswordItem } from "./workspaceTypes";

const {
  invoke,
  listeners,
  listen,
  loadPasswordItems,
  resolvePasswordValue,
  writeText,
} = vi.hoisted(() => ({
  invoke: vi.fn(),
  listeners: new Map<string, () => void>(),
  listen: vi.fn(),
  loadPasswordItems: vi.fn(),
  resolvePasswordValue: vi.fn(),
  writeText: vi.fn(),
}));
vi.mock("@tauri-apps/api/core", () => ({ invoke }));
vi.mock("@tauri-apps/api/event", () => ({ listen }));
vi.mock("./passwordAdapter", () => ({
  loadPasswordItems,
  passwordItemIdForResource: (resource: string) => resource,
  resolvePasswordValue,
}));

function visibleFieldValue(element: Element | null): string | null {
  const input = element?.querySelector("input");
  return input
    ? input.type === "text"
      ? input.value
      : "••••••••"
    : (element?.textContent ?? null);
}

Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });

const passwordVaultStyles = readFileSync(
  resolve(process.cwd(), "src/components/desktop/password-vault.css"),
  "utf8",
);
const legacyStyles = readFileSync(
  resolve(process.cwd(), "src/styles.css"),
  "utf8",
);

type RenderHarness = {
  container: HTMLDivElement;
  rerender: (node: ReactNode) => Promise<void>;
  unmount: () => void;
};

type Deferred<T> = {
  promise: Promise<T>;
  reject: (reason: Error) => void;
  resolve: (value: T) => void;
};

function deferred<T>(): Deferred<T> {
  let rejectPromise: ((reason: Error) => void) | null = null;
  let resolvePromise: ((value: T) => void) | null = null;
  const promise = new Promise<T>((resolvePromiseValue, rejectPromiseValue) => {
    resolvePromise = resolvePromiseValue;
    rejectPromise = rejectPromiseValue;
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

function passwordItem(
  index: number,
  options?: {
    backend?: PasswordItem["backend"];
    fields?: PasswordItem["fields"];
    notes?: string;
    tags?: string[];
    title?: string;
    vault?: string;
  },
): PasswordItem {
  const id = `item-${index}`;
  return {
    id,
    backend: options?.backend ?? "plankton",
    origin: "local",
    title: options?.title ?? `Password ${index}`,
    vault: options?.vault ?? "Primary",
    group: "Credentials",
    tags: options?.tags ?? [],
    username: `user-${index}`,
    notes: options?.notes ?? `Notes ${index}`,
    updatedAt: "2026-07-29T01:00:00Z",
    fields: options?.fields ?? [
      {
        key: "password",
        label: "Password",
        value: "Resolved on demand",
        resourceId: `plankton://field/${id}/password`,
        secret: true,
      },
    ],
  };
}

async function render(node: ReactNode): Promise<RenderHarness> {
  const container = document.createElement("div");
  document.body.appendChild(container);
  const root = ReactDOM.createRoot(container);
  await act(async () => {
    root.render(node);
  });
  return {
    container,
    async rerender(nextNode) {
      await act(async () => {
        root.render(nextNode);
      });
    },
    unmount() {
      act(() => root.unmount());
      container.remove();
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

function setTextareaValue(textarea: HTMLTextAreaElement, value: string): void {
  Object.getOwnPropertyDescriptor(
    HTMLTextAreaElement.prototype,
    "value",
  )?.set?.call(textarea, value);
  textarea.dispatchEvent(new Event("input", { bubbles: true }));
}

function setSelectValue(
  select: HTMLSelectElement | HTMLFieldSetElement,
  value: string,
): void {
  if (select instanceof HTMLFieldSetElement) {
    const radio = Array.from(
      select.querySelectorAll<HTMLInputElement>('input[type="radio"]'),
    ).find((input) => input.value === value);
    if (!radio) throw new Error(`Missing radio option ${value}`);
    radio.click();
    return;
  }
  Object.getOwnPropertyDescriptor(
    HTMLSelectElement.prototype,
    "value",
  )?.set?.call(select, value);
  select.dispatchEvent(new Event("change", { bubbles: true }));
}

function installPasswordVaultStyles(): CSSStyleSheet {
  const style = document.createElement("style");
  style.textContent = passwordVaultStyles;
  document.head.appendChild(style);
  const sheet = style.sheet;
  if (!(sheet instanceof CSSStyleSheet)) {
    throw new Error(
      "Password vault stylesheet did not produce a CSSStyleSheet",
    );
  }
  return sheet;
}

function installStyles(css: string): CSSStyleSheet {
  const style = document.createElement("style");
  style.textContent = css;
  document.head.appendChild(style);
  const sheet = style.sheet;
  if (!(sheet instanceof CSSStyleSheet)) {
    throw new Error("Stylesheet did not produce a CSSStyleSheet");
  }
  return sheet;
}

function splitSelectorList(selectorText: string): string[] {
  const selectors: string[] = [];
  let depth = 0;
  let start = 0;
  for (let index = 0; index < selectorText.length; index += 1) {
    const character = selectorText[index];
    if (character === "(" || character === "[") depth += 1;
    if (character === ")" || character === "]") depth -= 1;
    if (character === "," && depth === 0) {
      selectors.push(selectorText.slice(start, index).trim());
      start = index + 1;
    }
  }
  selectors.push(selectorText.slice(start).trim());
  return selectors;
}

function allSelectors(rules: CSSRuleList): string[] {
  return Array.from(rules).flatMap((rule) => {
    if ("selectorText" in rule) {
      return splitSelectorList((rule as CSSStyleRule).selectorText);
    }
    return "cssRules" in rule
      ? allSelectors((rule as CSSMediaRule).cssRules)
      : [];
  });
}

function styleRule(
  sheet: CSSStyleSheet,
  selector: string,
): CSSStyleRule | undefined {
  return Array.from(sheet.cssRules)
    .reverse()
    .find(
      (rule) =>
        "selectorText" in rule &&
        (rule as CSSStyleRule).selectorText
          .split(",")
          .map((entry) => entry.trim())
          .includes(selector),
    ) as CSSStyleRule | undefined;
}

function mediaRule(
  sheet: CSSStyleSheet,
  condition: string,
  selector: string,
): CSSStyleRule | undefined {
  const media = Array.from(sheet.cssRules).find(
    (rule) =>
      "conditionText" in rule &&
      (rule as CSSMediaRule).conditionText === condition,
  ) as CSSMediaRule | undefined;
  return Array.from(media?.cssRules ?? [])
    .reverse()
    .find(
      (rule) =>
        "selectorText" in rule &&
        (rule as CSSStyleRule).selectorText
          .split(",")
          .map((entry) => entry.trim())
          .includes(selector),
    ) as CSSStyleRule | undefined;
}

beforeEach(() => {
  invoke.mockReset();
  listeners.clear();
  listen
    .mockReset()
    .mockImplementation(
      (eventName: string, listener: () => void): Promise<() => void> => {
        listeners.set(eventName, listener);
        return Promise.resolve(() => listeners.delete(eventName));
      },
    );
  loadPasswordItems.mockReset();
  resolvePasswordValue.mockReset();
  writeText.mockReset();
  Object.defineProperty(navigator, "clipboard", {
    configurable: true,
    value: { writeText },
  });
});

afterEach(() => {
  vi.useRealTimers();
  document.body.innerHTML = "";
  document.head.innerHTML = "";
});

describe("PasswordVaultPage pagination and filtering", () => {
  it("edits a persisted collection default without turning inherited fields into overrides", async () => {
    const item = passwordItem(1);
    loadPasswordItems.mockResolvedValue({ kind: "live", items: [item] });
    resolvePasswordValue.mockResolvedValue("test-only");
    invoke.mockImplementation((command: string) => {
      if (command === "list_password_catalog_metadata_command")
        return Promise.resolve({
          revision: "inheritance-revision",
          items: [
            {
              record_id: "record-1",
              item_id: "item-1",
              title: item.title,
              tags: [],
              metadata: {},
              default_exposure_policy: defaultExposurePolicy(),
              fields: [
                {
                  resource_id: item.fields[0].resourceId,
                  label: "Password",
                  provider_kind: "local_literal",
                  has_value: true,
                  exposure_policy: defaultExposurePolicy(),
                  inherits_exposure_policy: true,
                },
              ],
            },
          ],
        });
      if (command === "submit_desktop_password_change")
        return Promise.resolve({ state: "committed" });
      return Promise.resolve([]);
    });
    const view = await render(
      <PasswordVaultPage
        incomingDraftId={null}
        locale="en"
        onDraftConsumed={() => {}}
      />,
    );
    expect(
      view.container.querySelector(".password-field-exposure-summary")
        ?.textContent,
    ).toContain("Inherits defaults");
    await act(async () =>
      Array.from(view.container.querySelectorAll("button"))
        .find((button) => button.textContent === "Edit entry")
        ?.click(),
    );
    expect(
      view.container.querySelector(".field-exposure-profile canvas"),
    ).toBeNull();
    await act(async () =>
      view.container
        .querySelector<HTMLInputElement>(
          '.collection-exposure-profile input[value="direct"]',
        )
        ?.click(),
    );
    expect(
      view.container.querySelector<HTMLInputElement>(
        '[aria-label="Password password value"]',
      )?.type,
    ).toBe("text");
    await act(async () =>
      Array.from(view.container.querySelectorAll("button"))
        .find((button) => button.textContent === "Save")
        ?.click(),
    );
    expect(
      view.container.querySelector('[role="dialog"]')?.textContent,
    ).toContain("Default exposure profile");
    await act(async () =>
      Array.from(view.container.querySelectorAll('[role="dialog"] button'))
        .find((button) => button.textContent === "Confirm")
        ?.dispatchEvent(new MouseEvent("click", { bubbles: true })),
    );
    expect(invoke).toHaveBeenCalledWith(
      "submit_desktop_password_change",
      expect.objectContaining({
        operations: [
          {
            operation: "set_item_exposure_policy",
            item_id: "record-1",
            policy: { ...defaultExposurePolicy(), access_mode: "direct" },
          },
        ],
      }),
    );
    view.unmount();
  });

  it("shows every field exposure surface in detail and keeps exposure editing visible", async () => {
    const sheet = installPasswordVaultStyles();
    resolvePasswordValue.mockResolvedValue("direct-test-only");
    const item = passwordItem(1);
    loadPasswordItems.mockResolvedValue({ kind: "live", items: [item] });
    invoke.mockImplementation((command: string) => {
      if (command === "list_password_catalog_metadata_command") {
        return Promise.resolve({
          revision: "exposure-detail-revision",
          items: [
            {
              record_id: "record-exposure-detail",
              item_id: "password-1",
              title: "Password 1",
              tags: [],
              metadata: {},
              fields: [
                {
                  resource_id: item.fields[0].resourceId,
                  label: "Password",
                  provider_kind: "local_literal",
                  vault: "Primary",
                  has_value: true,
                  exposure_policy: {
                    access_mode: "direct",
                    breach_action: "human_review",
                    surfaces: [
                      {
                        surface: "llm_context",
                        max_level: 1,
                        note: "Visible to the model.",
                      },
                      {
                        surface: "network",
                        max_level: 1,
                        network_allowlist: [
                          { kind: "exact_domain", domain: "api.example.com" },
                        ],
                        note: "Only the declared endpoint.",
                      },
                      {
                        surface: "local_persistence",
                        max_level: 0,
                        note: "No files.",
                      },
                      {
                        surface: "terminal_log",
                        max_level: 0,
                        note: "No logs.",
                      },
                      {
                        surface: "process_propagation",
                        max_level: 1,
                        note: "Declared process only.",
                      },
                    ],
                  },
                },
              ],
            },
          ],
        });
      }
      return Promise.reject(new Error(`unexpected command ${command}`));
    });
    const view = await render(
      <PasswordVaultPage
        incomingDraftId={null}
        locale="en"
        onDraftConsumed={() => {}}
      />,
    );

    const summary = view.container.querySelector(
      ".password-field-exposure-summary .exposure-policy-summary",
    );
    expect(resolvePasswordValue).toHaveBeenCalledWith(
      item.fields[0].resourceId,
    );
    expect(
      visibleFieldValue(view.container.querySelector(".field-value")),
    ).toBe("direct-test-only");
    expect(summary?.textContent).toContain("Direct · get requires no approval");
    expect(summary?.textContent).toContain("LLM context");
    expect(summary?.textContent).toContain("Local storage");
    expect(summary?.textContent).toContain("Visible to the model.");
    expect(summary?.textContent).toContain("domain:api.example.com");
    const disclosure = summary?.closest("details");
    expect(disclosure?.open).toBe(false);
    expect(
      view.container.querySelector<HTMLDetailsElement>(
        ".collection-exposure-summary",
      )?.open,
    ).toBe(false);
    await act(async () => disclosure?.querySelector("summary")?.click());
    expect(disclosure?.open).toBe(true);

    await act(async () =>
      view.container
        .querySelector<HTMLButtonElement>('[aria-label="Hide Password"]')
        ?.click(),
    );
    expect(
      visibleFieldValue(view.container.querySelector(".field-value")),
    ).toBe("••••••••");
    const loadsBeforeRefresh = loadPasswordItems.mock.calls.length;
    vi.useFakeTimers();
    await act(async () => {
      listeners.get("plankton://password-catalog-changed")?.();
      await vi.advanceTimersByTimeAsync(100);
    });
    vi.useRealTimers();
    expect(loadPasswordItems).toHaveBeenCalledTimes(loadsBeforeRefresh + 1);
    expect(
      visibleFieldValue(view.container.querySelector(".field-value")),
    ).toBe("••••••••");

    const editExposure = Array.from(
      summary?.querySelectorAll<HTMLButtonElement>("button") ?? [],
    ).find((button) => button.textContent === "Edit exposure");
    await act(async () => editExposure?.click());

    expect(
      view.container.querySelector(".password-edit-exposure"),
    ).not.toBeNull();
    expect(
      view.container.querySelector(".vault-layout.is-editing-entry"),
    ).not.toBeNull();
    expect(
      view.container.querySelector(".password-edit-context")?.textContent,
    ).toContain("Local value");
    expect(
      view.container.querySelector(
        ".password-edit-exposure .exposure-policy-workbench",
      ),
    ).not.toBeNull();
    expect(
      view.container.querySelector(".password-edit-exposure details"),
    ).toBeNull();
    expect(
      view.container.querySelector<HTMLInputElement>(
        ".password-edit-exposure .exposure-policy-mode .choice-group input:checked",
      )?.value,
    ).toBe("direct");
    const fieldExposure = view.container.querySelector<HTMLDetailsElement>(
      ".password-edit-exposure",
    );
    expect(fieldExposure?.open).toBe(false);
    await act(async () => fieldExposure?.querySelector("summary")?.click());
    expect(fieldExposure?.open).toBe(true);
    const defaults = view.container.querySelector<HTMLDetailsElement>(
      ".collection-exposure-profile",
    );
    expect(defaults?.open).toBe(false);
    expect(defaults?.parentElement).toBe(
      view.container.querySelector(".password-edit-fields")?.parentElement,
    );
    const radar = view.container.querySelector<HTMLCanvasElement>(
      ".password-edit-exposure canvas",
    );
    await act(async () =>
      radar?.dispatchEvent(
        new KeyboardEvent("keydown", { key: "ArrowRight", bubbles: true }),
      ),
    );
    expect(
      view.container.querySelector('[aria-label="Network settings"]'),
    ).not.toBeNull();
    expect(view.container.textContent).toContain("Network allowlist");
    const levelRadio = view.container.querySelector<HTMLInputElement>(
      '.password-edit-exposure .exposure-axis-editor input[type="radio"]',
    );
    expect(levelRadio).not.toBeNull();
    if (!levelRadio) throw new Error("Missing exposure level radio");
    const levelLabel = levelRadio.closest("label");
    expect(levelLabel).not.toBeNull();
    if (!levelLabel) throw new Error("Missing exposure level label");
    const radioRule = styleRule(
      sheet,
      '.desktop-workspace .exposure-axis-editor fieldset input[type="radio"]',
    );
    expect(radioRule?.style.width).toBe("14px");
    expect(radioRule?.style.minHeight).toBe("14px");
    expect(
      styleRule(
        sheet,
        ".desktop-workspace .exposure-axis-editor fieldset label",
      )?.style.display,
    ).toBe("flex");
    const allowlist = view.container.querySelector<HTMLTextAreaElement>(
      ".password-edit-exposure .exposure-network-allowlist textarea",
    );
    await act(async () => {
      if (allowlist) setTextareaValue(allowlist, "");
    });
    const save = Array.from(
      view.container.querySelectorAll<HTMLButtonElement>("button"),
    ).find((button) => button.textContent === "Save");
    await act(async () => save?.click());
    expect(
      view.container.querySelector('[role="alert"]')?.textContent,
    ).toContain(
      "controlled network exposure requires at least one allowlist destination",
    );
    view.unmount();
  });

  it("keeps item deletion in the selected entry overflow menu", async () => {
    const item = passwordItem(1);
    const metadata = {
      revision: "delete-revision",
      items: [
        {
          record_id: "record-delete",
          item_id: "password-1",
          title: "Password 1",
          tags: [],
          metadata: {},
          fields: [
            {
              resource_id: item.fields[0].resourceId,
              label: "Password",
              provider_kind: "local_literal",
              vault: "Primary",
              has_value: true,
            },
          ],
        },
      ],
    };
    loadPasswordItems.mockResolvedValue({ kind: "live", items: [item] });
    invoke.mockImplementation((command: string) => {
      if (command === "list_password_catalog_metadata_command") {
        return Promise.resolve(metadata);
      }
      if (command === "submit_desktop_password_change") {
        return Promise.resolve({ state: "committed" });
      }
      return Promise.reject(new Error(`unexpected command ${command}`));
    });
    const view = await render(
      <PasswordVaultPage
        incomingDraftId={null}
        locale="en"
        onDraftConsumed={() => {}}
      />,
    );

    expect(
      view.container.querySelector('[aria-label="Delete Password 1"]'),
    ).toBeNull();
    const menuTrigger = view.container.querySelector<HTMLButtonElement>(
      '[aria-label="More entry actions"]',
    );
    expect(menuTrigger).not.toBeNull();
    await act(async () => menuTrigger?.click());
    const menu = view.container.querySelector<HTMLElement>('[role="menu"]');
    expect(menu?.textContent).toContain("Delete entry");
    const deleteButton =
      menu?.querySelector<HTMLButtonElement>('[role="menuitem"]');
    await act(async () => deleteButton?.click());

    const dialog = view.container.querySelector<HTMLElement>('[role="dialog"]');
    expect(dialog?.textContent).toContain("Confirm deletion");
    expect(dialog?.textContent).toContain(
      "source files are not deleted upstream",
    );
    const confirm = Array.from(
      dialog?.querySelectorAll<HTMLButtonElement>("button") ?? [],
    ).find((button) => button.textContent === "Delete entry");
    expect(confirm?.className).toBe("danger");
    await act(async () => confirm?.click());

    expect(invoke).toHaveBeenCalledWith("submit_desktop_password_change", {
      operations: [{ operation: "delete_item", item_id: "password-1" }],
      reason: "",
    });
    view.unmount();
  });

  it("replaces the WebView context menu with entry deletion on right click", async () => {
    const item = passwordItem(1);
    loadPasswordItems.mockResolvedValue({ kind: "live", items: [item] });
    invoke.mockImplementation((command: string) => {
      if (command === "list_password_catalog_metadata_command") {
        return Promise.resolve({
          revision: "context-menu-revision",
          items: [
            {
              record_id: "record-context-menu",
              item_id: "password-1",
              title: "Password 1",
              tags: [],
              metadata: {},
              fields: [
                {
                  resource_id: item.fields[0].resourceId,
                  label: "Password",
                  provider_kind: "local_literal",
                  vault: "Primary",
                  has_value: true,
                },
              ],
            },
          ],
        });
      }
      return Promise.reject(new Error(`unexpected command ${command}`));
    });
    const view = await render(
      <PasswordVaultPage
        incomingDraftId={null}
        locale="en"
        onDraftConsumed={() => {}}
      />,
    );
    const row = Array.from(
      view.container.querySelectorAll<HTMLButtonElement>(".password-row"),
    ).find((button) => button.textContent?.includes("Password 1"));
    const contextMenuEvent = new MouseEvent("contextmenu", {
      bubbles: true,
      cancelable: true,
      clientX: 240,
      clientY: 180,
    });

    await act(async () => row?.dispatchEvent(contextMenuEvent));

    expect(contextMenuEvent.defaultPrevented).toBe(true);
    const menu = document.body.querySelector<HTMLElement>(
      ".password-row-context-menu[role='menu']",
    );
    expect(menu?.textContent).toContain("Delete entry");
    expect(menu?.textContent).not.toContain("Reload");
    await act(async () =>
      menu?.querySelector<HTMLButtonElement>("[role='menuitem']")?.click(),
    );
    expect(
      view.container.querySelector<HTMLElement>('[role="dialog"]')?.textContent,
    ).toContain("Confirm deletion");
    view.unmount();
  });

  it("reloads catalog metadata when deletion is requested after an initial metadata failure", async () => {
    const item = passwordItem(1);
    let metadataLoads = 0;
    loadPasswordItems.mockResolvedValue({ kind: "live", items: [item] });
    invoke.mockImplementation((command: string) => {
      if (command !== "list_password_catalog_metadata_command") {
        return Promise.reject(new Error(`unexpected command ${command}`));
      }
      metadataLoads += 1;
      if (metadataLoads === 1) {
        return Promise.reject(new Error("temporary metadata failure"));
      }
      return Promise.resolve({
        revision: "recovered-revision",
        items: [
          {
            record_id: "record-recovered",
            item_id: "password-1",
            title: "Password 1",
            tags: [],
            metadata: {},
            fields: [
              {
                resource_id: item.fields[0].resourceId,
                label: "Password",
                provider_kind: "local_literal",
                has_value: true,
              },
            ],
          },
        ],
      });
    });
    const view = await render(
      <PasswordVaultPage
        incomingDraftId={null}
        locale="en"
        onDraftConsumed={() => {}}
      />,
    );

    await act(async () =>
      view.container
        .querySelector<HTMLButtonElement>('[aria-label="More entry actions"]')
        ?.click(),
    );
    await act(async () =>
      view.container
        .querySelector<HTMLButtonElement>('[role="menuitem"]')
        ?.click(),
    );

    expect(metadataLoads).toBe(2);
    expect(
      view.container.querySelector<HTMLElement>('[role="dialog"]')?.textContent,
    ).toContain("Confirm deletion");
    view.unmount();
  });

  it("stages one field and its stored value for deletion from the editor", async () => {
    const item = passwordItem(1);
    const metadata = {
      revision: "field-delete-revision",
      items: [
        {
          record_id: "record-field-delete",
          item_id: "password-1",
          title: "Password 1",
          tags: [],
          metadata: {},
          fields: [
            {
              resource_id: item.fields[0].resourceId,
              label: "Password",
              provider_kind: "local_literal",
              vault: "Primary",
              has_value: true,
            },
            {
              resource_id: "local/password-1/username",
              label: "Username",
              provider_kind: "local_literal",
              vault: "Primary",
              has_value: true,
            },
          ],
        },
      ],
    };
    loadPasswordItems.mockResolvedValue({ kind: "live", items: [item] });
    invoke.mockImplementation((command: string) => {
      if (command === "list_password_catalog_metadata_command") {
        return Promise.resolve(metadata);
      }
      if (command === "submit_desktop_password_change") {
        return Promise.resolve({ state: "committed" });
      }
      return Promise.reject(new Error(`unexpected command ${command}`));
    });
    const view = await render(
      <PasswordVaultPage
        incomingDraftId={null}
        locale="en"
        onDraftConsumed={() => {}}
      />,
    );

    const editButton = Array.from(
      view.container.querySelectorAll<HTMLButtonElement>("button"),
    ).find((button) => button.textContent === "Edit entry");
    await act(async () => editButton?.click());
    await act(async () =>
      view.container
        .querySelector<HTMLButtonElement>(
          '[aria-label="Delete Password field"]',
        )
        ?.click(),
    );

    expect(view.container.textContent).toContain(
      "This field and its stored value will be deleted when you save.",
    );
    expect(view.container.textContent).toContain("Undo");
    const saveButton = Array.from(
      view.container.querySelectorAll<HTMLButtonElement>("button"),
    ).find((button) => button.textContent === "Save");
    await act(async () => saveButton?.click());

    const dialog = view.container.querySelector<HTMLElement>('[role="dialog"]');
    expect(dialog?.textContent).toContain("1 field and its stored value");
    expect(dialog?.textContent).toContain("Delete field: Password");
    const confirm = Array.from(
      dialog?.querySelectorAll<HTMLButtonElement>("button") ?? [],
    ).find((button) => button.textContent === "Delete and save");
    expect(confirm?.className).toBe("danger");
    await act(async () => confirm?.click());

    expect(invoke).toHaveBeenCalledWith("submit_desktop_password_change", {
      operations: [
        {
          operation: "delete_field",
          resource_id: item.fields[0].resourceId,
        },
      ],
      reason: "",
    });
    view.unmount();
  });

  it("warns that deleting the final field also removes the entry", async () => {
    const item = passwordItem(1);
    loadPasswordItems.mockResolvedValue({ kind: "live", items: [item] });
    invoke.mockImplementation((command: string) => {
      if (command === "list_password_catalog_metadata_command") {
        return Promise.resolve({
          revision: "last-field-revision",
          items: [
            {
              record_id: "record-last-field",
              item_id: "password-1",
              title: "Password 1",
              tags: [],
              metadata: {},
              fields: [
                {
                  resource_id: item.fields[0].resourceId,
                  label: "Password",
                  provider_kind: "local_literal",
                  vault: "Primary",
                  has_value: true,
                },
              ],
            },
          ],
        });
      }
      return Promise.reject(new Error(`unexpected command ${command}`));
    });
    const view = await render(
      <PasswordVaultPage
        incomingDraftId={null}
        locale="en"
        onDraftConsumed={() => {}}
      />,
    );

    const editButton = Array.from(
      view.container.querySelectorAll<HTMLButtonElement>("button"),
    ).find((button) => button.textContent === "Edit entry");
    await act(async () => editButton?.click());
    await act(async () =>
      view.container
        .querySelector<HTMLButtonElement>(
          '[aria-label="Delete Password field"]',
        )
        ?.click(),
    );
    expect(view.container.textContent).toContain(
      "Saving now will delete the entire entry because no fields remain.",
    );
    const saveButton = Array.from(
      view.container.querySelectorAll<HTMLButtonElement>("button"),
    ).find((button) => button.textContent === "Save");
    await act(async () => saveButton?.click());
    expect(
      view.container.querySelector<HTMLElement>('[role="dialog"]')?.textContent,
    ).toContain("this entry will also be removed from Plankton");
    view.unmount();
  });

  it("refreshes after a cross-window password catalog change", async () => {
    vi.useFakeTimers();
    loadPasswordItems
      .mockResolvedValueOnce({
        kind: "live",
        items: [passwordItem(1, { title: "Before" })],
      })
      .mockResolvedValueOnce({
        kind: "live",
        items: [passwordItem(1, { title: "After" })],
      });
    const view = await render(
      <PasswordVaultPage
        incomingDraftId={null}
        locale="en"
        onDraftConsumed={() => {}}
      />,
    );

    expect(view.container.textContent).toContain("Before");
    await act(async () => {
      listeners.get("plankton://password-catalog-changed")?.();
      await vi.advanceTimersByTimeAsync(80);
    });

    expect(loadPasswordItems).toHaveBeenCalledTimes(2);
    expect(view.container.textContent).toContain("After");
    expect(view.container.textContent).not.toContain("Before");
    view.unmount();
    vi.useRealTimers();
  });

  it("shows an explicit full-height loading state on initial and locale reloads", async () => {
    const sheet = installPasswordVaultStyles();
    const initialLoad = deferred<{
      kind: "live";
      items: PasswordItem[];
    }>();
    const localeReload = deferred<{
      kind: "live";
      items: PasswordItem[];
    }>();
    loadPasswordItems
      .mockReturnValueOnce(initialLoad.promise)
      .mockReturnValueOnce(localeReload.promise);
    const view = await render(
      <PasswordVaultPage
        incomingDraftId={null}
        locale="en"
        onDraftConsumed={() => {}}
      />,
    );

    const initialStatus = view.container.querySelector(
      '[data-testid="password-vault-loading"][role="status"]',
    );
    expect(initialStatus?.textContent).toContain("Loading passwords");
    expect(view.container.querySelector(".vault-layout")).toBeNull();
    expect(view.container.textContent).not.toContain("0 results");
    expect(view.container.textContent).not.toContain("No passwords yet");
    expect(
      styleRule(
        sheet,
        ".desktop-workspace .password-vault-shell .password-vault-loading",
      )?.style.minHeight,
    ).toContain("100vh");

    await act(async () => {
      initialLoad.resolve({
        kind: "live",
        items: [passwordItem(1)],
      });
      await initialLoad.promise;
    });
    expect(view.container.textContent).toContain("Password 1");

    await view.rerender(
      <PasswordVaultPage
        incomingDraftId={null}
        locale="zh-CN"
        onDraftConsumed={() => {}}
      />,
    );
    const localizedStatus = view.container.querySelector(
      '[data-testid="password-vault-loading"][role="status"]',
    );
    expect(localizedStatus?.textContent).toContain("正在加载密码");
    expect(view.container.querySelector(".vault-layout")).toBeNull();
    expect(view.container.textContent).not.toContain("0 个结果");
    expect(view.container.textContent).not.toContain("Password 1");

    await act(async () => {
      localeReload.resolve({
        kind: "live",
        items: [passwordItem(2)],
      });
      await localeReload.promise;
    });
    view.unmount();
  });

  it("hides pagination for an empty catalog and a single page", async () => {
    loadPasswordItems.mockResolvedValueOnce({ kind: "live", items: [] });
    const empty = await render(
      <PasswordVaultPage
        incomingDraftId={null}
        locale="en"
        onDraftConsumed={() => {}}
      />,
    );

    expect(empty.container.querySelector(".page-pagination")).toBeNull();
    expect(empty.container.textContent).toContain("No passwords yet");
    expect(empty.container.textContent).not.toContain("Manage catalog");
    expect(empty.container.textContent).toContain("Add or import");
    expect(empty.container.textContent).not.toContain("Show CLI help");
    empty.unmount();

    loadPasswordItems.mockResolvedValueOnce({
      kind: "live",
      items: [passwordItem(1)],
    });
    const singlePage = await render(
      <PasswordVaultPage
        incomingDraftId={null}
        locale="en"
        onDraftConsumed={() => {}}
      />,
    );

    expect(singlePage.container.querySelector(".page-pagination")).toBeNull();
    expect(singlePage.container.textContent).toContain("Password 1");
    singlePage.unmount();
  });

  it("keeps search and the list pane visible when a search has no matches", async () => {
    loadPasswordItems.mockResolvedValueOnce({
      kind: "live",
      items: [passwordItem(1)],
    });
    const view = await render(
      <PasswordVaultPage
        incomingDraftId={null}
        locale="zh-CN"
        onDraftConsumed={() => {}}
      />,
    );
    const search = view.container.querySelector<HTMLInputElement>(
      '[aria-label="搜索密码条目"]',
    );

    await act(async () => {
      if (search) setInputValue(search, "不存在的密码");
    });

    const inlineEmpty = view.container.querySelector(
      '[data-testid="password-list-empty"]',
    );
    expect(search).not.toBeNull();
    expect(search?.value).toBe("不存在的密码");
    expect(view.container.querySelector(".vault-layout")).not.toBeNull();
    expect(inlineEmpty?.parentElement?.classList).toContain(
      "password-list-scroll",
    );
    expect(inlineEmpty?.textContent).toContain("没有匹配的密码");
    expect(view.container.querySelector(".page-empty-state")).toBeNull();

    await act(async () => {
      inlineEmpty?.querySelector<HTMLButtonElement>("button")?.click();
    });
    expect(search?.value).toBe("");
    expect(
      view.container.querySelector(".password-row")?.textContent,
    ).toContain("Password 1");
    view.unmount();
  });

  it("anchors two-page pagination after the independently scrolling list", async () => {
    loadPasswordItems.mockResolvedValue({
      kind: "live",
      items: Array.from({ length: 9 }, (_, index) => passwordItem(index + 1)),
    });
    const view = await render(
      <PasswordVaultPage
        incomingDraftId={null}
        locale="en"
        onDraftConsumed={() => {}}
      />,
    );

    const list = view.container.querySelector(".password-list-scroll");
    expect(list).not.toBeNull();
    expect(
      list?.nextElementSibling?.classList.contains("page-pagination"),
    ).toBe(true);
    expect(view.container.querySelector(".page-pagination")?.textContent).toBe(
      "1 / 2",
    );
    view.unmount();
  });

  it("selects the first item on the page when pagination changes", async () => {
    loadPasswordItems.mockResolvedValue({
      kind: "live",
      items: Array.from({ length: 9 }, (_, index) => passwordItem(index + 1)),
    });
    const view = await render(
      <PasswordVaultPage
        incomingDraftId={null}
        locale="en"
        onDraftConsumed={() => {}}
      />,
    );

    const next = view.container.querySelector<HTMLButtonElement>(
      '[aria-label="Next page"]',
    );
    await act(async () => {
      next?.click();
    });

    expect(view.container.querySelector(".page-pagination")?.textContent).toBe(
      "2 / 2",
    );
    expect(
      view.container.querySelector(".password-list")?.textContent,
    ).toContain("Password 9");
    expect(
      view.container.querySelector(".item-detail-pane h2")?.textContent,
    ).toBe("Password 9");

    view.unmount();
  });

  it("resets a later page and detail selection when a tag filter narrows results", async () => {
    loadPasswordItems.mockResolvedValue({
      kind: "live",
      items: [
        passwordItem(1, { tags: ["production"] }),
        ...Array.from({ length: 8 }, (_, index) =>
          passwordItem(index + 2, { tags: ["development"] }),
        ),
      ],
    });
    const view = await render(
      <PasswordVaultPage
        incomingDraftId={null}
        locale="en"
        onDraftConsumed={() => {}}
      />,
    );

    await act(async () => {
      view.container
        .querySelector<HTMLButtonElement>('[aria-label="Next page"]')
        ?.click();
    });
    const tagInput = view.container.querySelector<HTMLInputElement>(
      '.vault-sidebar [aria-label="Filter by tags"]',
    );
    await act(async () => {
      if (tagInput) {
        setInputValue(tagInput, "production");
      }
    });

    expect(view.container.querySelector(".page-pagination")).toBeNull();
    expect(
      view.container.querySelector(".password-list")?.textContent,
    ).toContain("Password 1");
    expect(
      view.container.querySelector(".password-list")?.textContent,
    ).not.toContain("Password 9");
    expect(
      view.container.querySelector(".item-detail-pane h2")?.textContent,
    ).toBe("Password 1");

    view.unmount();
  });

  it("resets a later page when the vault tree narrows results", async () => {
    loadPasswordItems.mockResolvedValue({
      kind: "live",
      items: [
        passwordItem(1, { vault: "Primary" }),
        ...Array.from({ length: 8 }, (_, index) =>
          passwordItem(index + 2, { vault: "Archive" }),
        ),
      ],
    });
    const view = await render(
      <PasswordVaultPage
        incomingDraftId={null}
        locale="en"
        onDraftConsumed={() => {}}
      />,
    );

    await act(async () => {
      view.container
        .querySelector<HTMLButtonElement>('[aria-label="Next page"]')
        ?.click();
    });
    const primaryVault = view.container.querySelector<HTMLSelectElement>(
      '.vault-sidebar [aria-label="Filter by vault"]',
    );
    await act(async () => {
      if (primaryVault) setSelectValue(primaryVault, "Primary");
    });

    expect(view.container.querySelector(".page-pagination")).toBeNull();
    expect(
      view.container.querySelector(".item-detail-pane h2")?.textContent,
    ).toBe("Password 1");

    view.unmount();
  });

  it("shows only backends present in the catalog and supports hiding one class", async () => {
    loadPasswordItems.mockResolvedValue({
      kind: "live",
      items: [
        passwordItem(1, { backend: "plankton", title: "Local password" }),
        passwordItem(2, {
          backend: "one_password",
          title: "Connected password",
        }),
      ],
    });
    const view = await render(
      <PasswordVaultPage
        incomingDraftId={null}
        locale="en"
        onDraftConsumed={() => {}}
      />,
    );

    const backendButtons = Array.from(
      view.container.querySelectorAll<HTMLButtonElement>(
        ".vault-sidebar .backend-filter",
      ),
    );
    expect(backendButtons.map((button) => button.textContent)).toEqual([
      "Local (Plankton)",
      "1Password",
    ]);
    expect(view.container.textContent).not.toContain("Bitwarden");

    await act(async () => {
      backendButtons
        .find((button) => button.textContent === "1Password")
        ?.click();
    });
    expect(
      view.container.querySelector(".password-list")?.textContent,
    ).toContain("Local password");
    expect(
      view.container.querySelector(".password-list")?.textContent,
    ).not.toContain("Connected password");
    view.unmount();
  });

  it("reports distinct title, notes, field-key, and tag search matches", async () => {
    loadPasswordItems.mockResolvedValue({
      kind: "live",
      items: [
        passwordItem(1, {
          title: "Deployment login",
          notes: "Rotated nightly by release automation",
          tags: ["production"],
          fields: [
            {
              key: "client_secret",
              label: "Client secret",
              value: "Resolved on demand",
              resourceId: "plankton://field/item-1/client-secret",
              secret: true,
            },
          ],
        }),
      ],
    });
    const view = await render(
      <PasswordVaultPage
        incomingDraftId={null}
        locale="en"
        onDraftConsumed={() => {}}
      />,
    );
    const search = view.container.querySelector<HTMLInputElement>(
      '[aria-label="Search password items"]',
    );
    expect(search).not.toBeNull();

    for (const [query, reason] of [
      ["deployment", "Matched title"],
      ["release automation", "Matched notes"],
      ["client_secret", "Matched field key: client_secret"],
      ["production", "Matched tag: production"],
    ]) {
      await act(async () => {
        if (search) setInputValue(search, query);
      });
      expect(
        view.container.querySelector(".password-row small")?.textContent,
      ).toContain(reason);
    }

    view.unmount();
  });

  it("deduplicates tag chips and toggles them case-insensitively", async () => {
    loadPasswordItems.mockResolvedValue({
      kind: "live",
      items: [
        passwordItem(1, { tags: ["Production", "production", "Shared"] }),
      ],
    });
    const view = await render(
      <PasswordVaultPage
        incomingDraftId={null}
        locale="en"
        onDraftConsumed={() => {}}
      />,
    );

    const chips = view.container.querySelectorAll(
      ".password-filter-tags .tag-filter",
    );
    expect(Array.from(chips, (chip) => chip.textContent)).toEqual([
      "#Production",
      "#Shared",
    ]);
    const tagInput = view.container.querySelector<HTMLInputElement>(
      '.vault-sidebar [aria-label="Filter by tags"]',
    );
    await act(async () => {
      if (tagInput) setInputValue(tagInput, "production, PRODUCTION");
    });
    expect(
      view.container
        .querySelector<HTMLButtonElement>(".password-filter-tags .tag-filter")
        ?.getAttribute("aria-pressed"),
    ).toBe("true");

    await act(async () => {
      Array.from(
        view.container.querySelectorAll<HTMLButtonElement>(
          ".password-filter-tags .tag-filter",
        ),
      )
        .find((button) => button.textContent === "#Shared")
        ?.click();
    });
    expect(tagInput?.value).toBe("production, Shared");
    view.unmount();
  });

  it("re-resolves every copy and keeps resolver failures out of the DOM", async () => {
    loadPasswordItems.mockResolvedValue({
      kind: "live",
      items: [passwordItem(1)],
    });
    resolvePasswordValue
      .mockResolvedValueOnce("displayed-old-secret")
      .mockResolvedValueOnce("fresh-copy-secret");
    writeText.mockResolvedValueOnce(undefined);
    const view = await render(
      <PasswordVaultPage
        incomingDraftId={null}
        locale="en"
        onDraftConsumed={() => {}}
      />,
    );

    const reveal = view.container.querySelector<HTMLButtonElement>(
      '[aria-label="Show Password"]',
    );
    expect(reveal?.querySelector(".lucide-eye")).not.toBeNull();
    await act(async () => {
      reveal?.click();
    });
    expect(
      visibleFieldValue(view.container.querySelector(".field-value")),
    ).toBe("displayed-old-secret");
    expect(
      view.container.querySelector(
        '[aria-label="Hide Password"] .lucide-eye-off',
      ),
    ).not.toBeNull();

    await act(async () => {
      view.container
        .querySelector<HTMLButtonElement>('[aria-label="Copy Password"]')
        ?.click();
    });
    expect(resolvePasswordValue).toHaveBeenCalledTimes(2);
    expect(writeText).toHaveBeenCalledWith("fresh-copy-secret");
    expect(
      view.container.querySelector('[role="status"]')?.textContent,
    ).toContain("Password copied");

    await act(async () => {
      view.container
        .querySelector<HTMLButtonElement>('[aria-label="Hide Password"]')
        ?.click();
    });
    expect(
      visibleFieldValue(view.container.querySelector(".field-value")),
    ).toBe("••••••••");

    const providerStderr =
      "SENTINEL_PROVIDER_STDERR password=hunter2 internal=/tmp/vault";
    resolvePasswordValue.mockRejectedValueOnce(new Error(providerStderr));
    await act(async () => {
      view.container
        .querySelector<HTMLButtonElement>('[aria-label="Copy Password"]')
        ?.click();
    });
    const alert = view.container.querySelector('[role="alert"]');
    expect(alert?.textContent).toContain(
      "Password could not be copied. Check Diagnostics and try again.",
    );
    expect(view.container.textContent).not.toContain(providerStderr);
    view.unmount();
  });

  it("clears revealed values across selection and filter transitions", async () => {
    const sharedField = {
      key: "password",
      label: "Password",
      value: "Resolved on demand",
      resourceId: "plankton://field/shared/password",
      secret: true,
    };
    loadPasswordItems.mockResolvedValue({
      kind: "live",
      items: [
        passwordItem(1, { fields: [sharedField] }),
        passwordItem(2, { fields: [sharedField] }),
      ],
    });
    resolvePasswordValue
      .mockResolvedValueOnce("first-reveal")
      .mockResolvedValueOnce("second-reveal");
    const view = await render(
      <PasswordVaultPage
        incomingDraftId={null}
        locale="en"
        onDraftConsumed={() => {}}
      />,
    );

    await act(async () => {
      view.container
        .querySelector<HTMLButtonElement>('[aria-label="Show Password"]')
        ?.click();
    });
    expect(
      visibleFieldValue(view.container.querySelector(".field-value")),
    ).toBe("first-reveal");

    await act(async () => {
      Array.from(
        view.container.querySelectorAll<HTMLButtonElement>(".password-row"),
      )
        .find((button) => button.textContent?.includes("Password 2"))
        ?.click();
    });
    expect(
      visibleFieldValue(view.container.querySelector(".field-value")),
    ).toBe("••••••••");
    await act(async () => {
      Array.from(
        view.container.querySelectorAll<HTMLButtonElement>(".password-row"),
      )
        .find((button) => button.textContent?.includes("Password 1"))
        ?.click();
    });
    expect(
      visibleFieldValue(view.container.querySelector(".field-value")),
    ).toBe("••••••••");

    await act(async () => {
      view.container
        .querySelector<HTMLButtonElement>('[aria-label="Show Password"]')
        ?.click();
    });
    expect(
      visibleFieldValue(view.container.querySelector(".field-value")),
    ).toBe("second-reveal");
    await act(async () => {
      const search = view.container.querySelector<HTMLInputElement>(
        '[aria-label="Search password items"]',
      );
      if (search) setInputValue(search, "Password 1");
    });
    expect(
      visibleFieldValue(view.container.querySelector(".field-value")),
    ).toBe("••••••••");
    expect(resolvePasswordValue).toHaveBeenCalledTimes(2);
    view.unmount();
  });

  it("lets the newest same-item reveal win after a query conceal transition", async () => {
    const firstReveal = deferred<string>();
    const secondReveal = deferred<string>();
    loadPasswordItems.mockResolvedValue({
      kind: "live",
      items: [passwordItem(1)],
    });
    resolvePasswordValue
      .mockReturnValueOnce(firstReveal.promise)
      .mockReturnValueOnce(secondReveal.promise);
    const view = await render(
      <PasswordVaultPage
        incomingDraftId={null}
        locale="en"
        onDraftConsumed={() => {}}
      />,
    );

    await act(async () => {
      view.container
        .querySelector<HTMLButtonElement>('[aria-label="Show Password"]')
        ?.click();
    });
    await act(async () => {
      const search = view.container.querySelector<HTMLInputElement>(
        '[aria-label="Search password items"]',
      );
      if (search) setInputValue(search, "Password 1");
    });
    const replacementReveal = view.container.querySelector<HTMLButtonElement>(
      '[aria-label="Show Password"]',
    );
    expect(replacementReveal?.disabled).toBe(false);
    await act(async () => {
      replacementReveal?.click();
    });

    await act(async () => {
      secondReveal.resolve("newest-secret");
      await secondReveal.promise;
    });
    expect(
      visibleFieldValue(view.container.querySelector(".field-value")),
    ).toBe("newest-secret");
    await act(async () => {
      firstReveal.resolve("stale-secret");
      await firstReveal.promise;
    });
    expect(
      visibleFieldValue(view.container.querySelector(".field-value")),
    ).toBe("newest-secret");
    expect(view.container.textContent).not.toContain("stale-secret");
    expect(resolvePasswordValue).toHaveBeenCalledTimes(2);
    view.unmount();
  });

  it("ignores an older same-item reveal failure after a tag-mode transition", async () => {
    const staleReveal = deferred<string>();
    const currentReveal = deferred<string>();
    loadPasswordItems.mockResolvedValue({
      kind: "live",
      items: [passwordItem(1, { tags: ["production"] })],
    });
    resolvePasswordValue
      .mockReturnValueOnce(staleReveal.promise)
      .mockReturnValueOnce(currentReveal.promise);
    const view = await render(
      <PasswordVaultPage
        incomingDraftId={null}
        locale="en"
        onDraftConsumed={() => {}}
      />,
    );

    await act(async () => {
      view.container
        .querySelector<HTMLButtonElement>('[aria-label="Show Password"]')
        ?.click();
    });
    await act(async () => {
      const tagMode = view.container.querySelector<HTMLFieldSetElement>(
        '.vault-sidebar [aria-label="Tag match mode"]',
      );
      if (tagMode) setSelectValue(tagMode, "all");
    });
    await act(async () => {
      view.container
        .querySelector<HTMLButtonElement>('[aria-label="Show Password"]')
        ?.click();
    });
    await act(async () => {
      currentReveal.resolve("current-secret");
      await currentReveal.promise;
    });
    await act(async () => {
      staleReveal.reject(new Error("SENTINEL_STALE_REVEAL_FAILURE"));
      await staleReveal.promise.catch(() => undefined);
    });

    expect(
      visibleFieldValue(view.container.querySelector(".field-value")),
    ).toBe("current-secret");
    expect(view.container.querySelector('[role="alert"]')).toBeNull();
    expect(view.container.textContent).not.toContain(
      "SENTINEL_STALE_REVEAL_FAILURE",
    );
    view.unmount();
  });

  it("keeps visibility independent while another field is still loading", async () => {
    const staleSuccess = deferred<string>();
    loadPasswordItems.mockResolvedValue({
      kind: "live",
      items: [
        passwordItem(1, {
          fields: [
            {
              key: "field_a",
              label: "Field A",
              value: "Resolved on demand",
              resourceId: "plankton://field/item-1/a",
              secret: true,
            },
            {
              key: "field_b",
              label: "Field B",
              value: "Resolved on demand",
              resourceId: "plankton://field/item-1/b",
              secret: true,
            },
          ],
        }),
      ],
    });
    resolvePasswordValue
      .mockResolvedValueOnce("field-a-first")
      .mockReturnValueOnce(staleSuccess.promise);
    const view = await render(
      <PasswordVaultPage
        incomingDraftId={null}
        locale="en"
        onDraftConsumed={() => {}}
      />,
    );
    const fieldValue = (label: string): string | null =>
      visibleFieldValue(
        Array.from(
          view.container.querySelectorAll<HTMLElement>(".password-field-row"),
        )
          .find((row) => row.querySelector("strong")?.textContent === label)
          ?.querySelector(".field-value") ?? null,
      );
    await act(async () =>
      view.container
        .querySelector<HTMLButtonElement>('[aria-label="Show Field A"]')
        ?.click(),
    );
    await act(async () =>
      view.container
        .querySelector<HTMLButtonElement>('[aria-label="Show Field B"]')
        ?.click(),
    );
    await act(async () =>
      view.container
        .querySelector<HTMLButtonElement>('[aria-label="Hide Field A"]')
        ?.click(),
    );
    expect(fieldValue("Field A")).toBe("••••••••");
    expect(
      view.container.querySelector<HTMLButtonElement>(
        '[aria-label="Show Field B"]',
      )?.disabled,
    ).toBe(true);
    await act(async () => {
      staleSuccess.resolve("field-b-success");
      await staleSuccess.promise;
    });
    expect(fieldValue("Field B")).toBe("field-b-success");
    expect(fieldValue("Field A")).toBe("••••••••");
    view.unmount();
  });

  it("requires a fresh reveal after leaving and returning across pages", async () => {
    const sharedField = {
      key: "password",
      label: "Password",
      value: "Resolved on demand",
      resourceId: "plankton://field/shared/password",
      secret: true,
    };
    loadPasswordItems.mockResolvedValue({
      kind: "live",
      items: Array.from({ length: 9 }, (_, index) =>
        passwordItem(index + 1, { fields: [sharedField] }),
      ),
    });
    resolvePasswordValue.mockResolvedValueOnce("page-one-secret");
    const view = await render(
      <PasswordVaultPage
        incomingDraftId={null}
        locale="en"
        onDraftConsumed={() => {}}
      />,
    );

    await act(async () => {
      view.container
        .querySelector<HTMLButtonElement>('[aria-label="Show Password"]')
        ?.click();
    });
    expect(
      visibleFieldValue(view.container.querySelector(".field-value")),
    ).toBe("page-one-secret");
    await act(async () => {
      view.container
        .querySelector<HTMLButtonElement>('[aria-label="Next page"]')
        ?.click();
    });
    expect(
      visibleFieldValue(view.container.querySelector(".field-value")),
    ).toBe("••••••••");
    await act(async () => {
      view.container
        .querySelector<HTMLButtonElement>('[aria-label="Previous page"]')
        ?.click();
    });
    expect(
      visibleFieldValue(view.container.querySelector(".field-value")),
    ).toBe("••••••••");
    expect(
      view.container.querySelector('[aria-label="Show Password"]'),
    ).not.toBeNull();
    view.unmount();
  });

  it("shows a localized safe reveal error without rendering provider stderr", async () => {
    loadPasswordItems.mockResolvedValue({
      kind: "live",
      items: [passwordItem(1)],
    });
    const providerStderr =
      "SENTINEL_REVEAL_STDERR raw-secret=never-render-this";
    resolvePasswordValue.mockRejectedValueOnce(new Error(providerStderr));
    const view = await render(
      <PasswordVaultPage
        incomingDraftId={null}
        locale="zh-CN"
        onDraftConsumed={() => {}}
      />,
    );

    await act(async () => {
      view.container
        .querySelector<HTMLButtonElement>('[aria-label="显示Password"]')
        ?.click();
    });

    expect(
      view.container.querySelector('[role="alert"]')?.textContent,
    ).toContain("无法显示Password。请查看诊断信息后重试。");
    expect(view.container.textContent).not.toContain(providerStderr);
    view.unmount();
  });

  it("keeps long values inside the keyboard-safe outer overflow region", async () => {
    const sheet = installPasswordVaultStyles();
    const longValue = "value-".repeat(80);
    const longResourceId = `plankton://field/${"item-".repeat(50)}/token`;
    loadPasswordItems.mockResolvedValue({
      kind: "live",
      items: [
        passwordItem(1, {
          fields: [
            {
              key: "verbose_identifier",
              label: "Long value",
              value: longValue,
              resourceId: longResourceId,
              secret: false,
            },
          ],
        }),
      ],
    });
    const view = await render(
      <PasswordVaultPage
        incomingDraftId={null}
        locale="en"
        onDraftConsumed={() => {}}
      />,
    );

    expect(
      visibleFieldValue(view.container.querySelector(".field-value")),
    ).toBe(longValue);
    expect(
      view.container.querySelector(".field-resource-id")?.textContent,
    ).toBe(longResourceId);
    expect(view.container.querySelector(".item-detail-scroll")).not.toBeNull();
    expect(view.container.querySelector(".item-detail-actions")).not.toBeNull();
    expect(
      styleRule(
        sheet,
        ".desktop-workspace .password-vault-shell .item-detail-scroll",
      )?.style.overflow,
    ).toBe("auto");
    const fieldValueRule = styleRule(
      sheet,
      ".desktop-workspace .password-vault-shell .field-value",
    );
    expect(fieldValueRule?.style.overflow).not.toBe("auto");
    expect(fieldValueRule?.style.maxHeight).toBe("");
    expect(
      view.container.querySelector(".field-value")?.hasAttribute("tabindex"),
    ).toBe(false);
    view.unmount();
  });

  it("opens a keyboard-reachable narrow filter drawer instead of removing filters", async () => {
    loadPasswordItems.mockResolvedValue({
      kind: "live",
      items: [passwordItem(1, { tags: ["production"] })],
    });
    const view = await render(
      <PasswordVaultPage
        incomingDraftId={null}
        locale="en"
        onDraftConsumed={() => {}}
      />,
    );

    const trigger = view.container.querySelector<HTMLButtonElement>(
      '[aria-label="Open password filters"]',
    );
    expect(trigger).not.toBeNull();
    await act(async () => {
      trigger?.click();
    });

    const drawer = view.container.querySelector<HTMLElement>(
      '[data-page-drawer="true"]',
    );
    expect(drawer?.getAttribute("aria-label")).toBe("Password filters");
    expect(
      drawer?.querySelector('[aria-label="Filter by vault"]'),
    ).not.toBeNull();
    expect(
      drawer?.querySelector('[aria-label="Filter by tags"]'),
    ).not.toBeNull();
    expect(
      drawer?.querySelector('[aria-label="Tag match mode"]'),
    ).not.toBeNull();
    expect(document.activeElement).not.toBe(trigger);
    view.unmount();
  });

  it("shows an incoming CLI draft while inline entry editing is open", async () => {
    loadPasswordItems.mockResolvedValue({
      kind: "live",
      items: [passwordItem(1)],
    });
    invoke.mockImplementation((command: string) => {
      if (command === "list_password_catalog_metadata_command") {
        return Promise.resolve({
          revision: "test-revision",
          items: [
            {
              record_id: "record-1",
              item_id: "password-1",
              title: "Password 1",
              description: "Notes 1",
              tags: [],
              metadata: {},
              fields: [
                {
                  resource_id: "plankton://field/item-1/password",
                  label: "Password",
                  provider_kind: "local_literal",
                  has_value: true,
                },
              ],
            },
          ],
        });
      }
      if (command === "list_secret_catalog_metadata") {
        return Promise.resolve({
          catalog_path: "/tmp/plankton-secrets.toml",
          imports: [],
          literals: [],
        });
      }
      if (command === "list_onepassword_accounts_command") {
        return Promise.resolve([]);
      }
      if (command === "preview_password_draft") {
        return Promise.resolve({
          descriptor: { kind: "environment", names: ["NEW_TOKEN"] },
          entries: [{ key: "NEW_TOKEN", value: "new-cli-secret" }],
        });
      }
      if (command === "list_backend_connections") {
        return Promise.resolve([]);
      }
      return Promise.reject(new Error(`unexpected command ${command}`));
    });
    const view = await render(
      <PasswordVaultPage
        incomingDraftId={null}
        locale="en"
        onDraftConsumed={() => {}}
      />,
    );
    const editEntry = Array.from(
      view.container.querySelectorAll<HTMLButtonElement>("button"),
    ).find((button) => button.textContent === "Edit entry");
    await act(async () => {
      editEntry?.click();
    });
    expect(view.container.textContent).toContain("EDIT ENTRY");
    expect(view.container.textContent).not.toContain("Local Secret Catalog");

    await view.rerender(
      <PasswordVaultPage
        incomingDraftId="draft-from-cli"
        locale="en"
        onDraftConsumed={() => {}}
      />,
    );

    expect(view.container.querySelector('[role="dialog"]')).not.toBeNull();
    expect(view.container.textContent).toContain("draft-from-cli");
    expect(view.container.textContent).toContain("NEW_TOKEN");
    expect(view.container.textContent).not.toContain("new-cli-secret");
    view.unmount();
  });

  it("edits metadata in place without requiring a reason", async () => {
    const item = passwordItem(1);
    loadPasswordItems.mockResolvedValue({ kind: "live", items: [item] });
    invoke.mockImplementation((command: string) => {
      if (command === "list_password_catalog_metadata_command") {
        return Promise.resolve({
          revision: "test-revision",
          items: [
            {
              record_id: "record-1",
              item_id: "password-1",
              title: "Password 1",
              description: "Notes 1",
              tags: [],
              metadata: {},
              fields: [
                {
                  resource_id: item.fields[0].resourceId,
                  label: "Password",
                  provider_kind: "local_literal",
                  has_value: true,
                },
              ],
            },
          ],
        });
      }
      if (command === "submit_desktop_password_change") {
        return Promise.resolve({ state: "committed" });
      }
      return Promise.reject(new Error(`unexpected command ${command}`));
    });

    const view = await render(
      <PasswordVaultPage
        incomingDraftId={null}
        locale="en"
        onDraftConsumed={() => {}}
      />,
    );
    const edit = Array.from(
      view.container.querySelectorAll<HTMLButtonElement>("button"),
    ).find((button) => button.textContent === "Edit entry");
    await act(async () => edit?.click());
    const title = view.container.querySelector<HTMLInputElement>(
      ".password-inline-editor input",
    );
    await act(async () => {
      if (title) setInputValue(title, "Production API");
    });
    const save = Array.from(
      view.container.querySelectorAll<HTMLButtonElement>("button"),
    ).find((button) => button.textContent === "Save");
    await act(async () => save?.click());

    const dialog = view.container.querySelector<HTMLElement>('[role="dialog"]');
    expect(dialog?.textContent).toContain("Confirm entry changes");
    const confirm = Array.from(
      dialog?.querySelectorAll<HTMLButtonElement>("button") ?? [],
    ).find((button) => button.textContent === "Confirm");
    expect(confirm?.disabled).toBe(false);
    await act(async () => confirm?.click());

    expect(invoke).toHaveBeenCalledWith("submit_desktop_password_change", {
      operations: [
        {
          operation: "update_item",
          item_id: "password-1",
          title: "Production API",
        },
      ],
      reason: "",
    });
    view.unmount();
  });

  it("loads and edits a local encrypted-vault value only in the human editor", async () => {
    const item = passwordItem(1);
    loadPasswordItems.mockResolvedValue({ kind: "live", items: [item] });
    resolvePasswordValue.mockResolvedValue("old-test-secret");
    invoke.mockImplementation((command: string) => {
      if (command === "list_password_catalog_metadata_command") {
        return Promise.resolve({
          revision: "test-revision",
          items: [
            {
              record_id: "record-1",
              item_id: "password-1",
              title: "Password 1",
              tags: [],
              metadata: {},
              fields: [
                {
                  resource_id: item.fields[0].resourceId,
                  label: "Password",
                  provider_kind: "keepassxc_cli",
                  has_value: true,
                },
              ],
            },
          ],
        });
      }
      if (command === "update_local_password_values") {
        return Promise.resolve();
      }
      return Promise.reject(new Error(`unexpected command ${command}`));
    });

    const view = await render(
      <PasswordVaultPage
        incomingDraftId={null}
        locale="en"
        onDraftConsumed={() => {}}
      />,
    );
    await act(async () => {
      Array.from(view.container.querySelectorAll<HTMLButtonElement>("button"))
        .find((button) => button.textContent === "Edit entry")
        ?.click();
    });
    await act(async () => {
      Array.from(view.container.querySelectorAll<HTMLButtonElement>("button"))
        .find((button) => button.getAttribute("aria-label") === "Show Password")
        ?.click();
      await Promise.resolve();
    });
    const value = view.container.querySelector<HTMLInputElement>(
      '[aria-label="Password password value"]',
    );
    expect(value?.type).toBe("text");
    await act(async () => {
      if (value) setInputValue(value, "new-test-secret");
    });
    await act(async () => {
      Array.from(view.container.querySelectorAll<HTMLButtonElement>("button"))
        .find((button) => button.textContent === "Save")
        ?.click();
    });
    const dialog = view.container.querySelector<HTMLElement>('[role="dialog"]');
    expect(dialog?.textContent).not.toContain("old-test-secret");
    expect(dialog?.textContent).not.toContain("new-test-secret");
    await act(async () => {
      Array.from(dialog?.querySelectorAll<HTMLButtonElement>("button") ?? [])
        .find((button) => button.textContent === "Confirm")
        ?.click();
    });

    expect(invoke).toHaveBeenCalledWith("update_local_password_values", {
      request: {
        source_record_id: "record-1",
        expected_revision: "test-revision",
        values: {
          [item.fields[0].resourceId]: "new-test-secret",
        },
      },
    });
    expect(invoke).not.toHaveBeenCalledWith(
      "submit_desktop_password_change",
      expect.anything(),
    );
    view.unmount();
  });

  it("splits selected fields into a new password item through confirmation", async () => {
    const source = passwordItem(1, {
      fields: [
        {
          key: "username",
          label: "Username",
          value: "Resolved on demand",
          resourceId: "secret/source/username",
          secret: true,
        },
        {
          key: "password",
          label: "Password",
          value: "Resolved on demand",
          resourceId: "secret/source/password",
          secret: true,
        },
      ],
    });
    loadPasswordItems.mockResolvedValue({ kind: "live", items: [source] });
    invoke.mockImplementation((command: string) => {
      if (command === "list_password_catalog_metadata_command") {
        return Promise.resolve({
          revision: "test-revision",
          items: [
            {
              record_id: "record-source",
              item_id: "source-item",
              title: "Source",
              tags: [],
              metadata: {},
              fields: source.fields.map((field) => ({
                resource_id: field.resourceId,
                label: field.label,
                provider_kind: "local_literal",
                has_value: true,
              })),
            },
          ],
        });
      }
      if (command === "submit_desktop_password_change") {
        return Promise.resolve({ state: "committed" });
      }
      return Promise.reject(new Error(`unexpected command ${command}`));
    });

    const view = await render(
      <PasswordVaultPage
        incomingDraftId={null}
        locale="en"
        onDraftConsumed={() => {}}
      />,
    );
    const organize = Array.from(
      view.container.querySelectorAll<HTMLButtonElement>("button"),
    ).find((button) => button.textContent === "Organize fields");
    await act(async () => organize?.click());

    const organizer =
      view.container.querySelector<HTMLElement>('[role="dialog"]');
    const targetInputs = organizer?.querySelectorAll<HTMLInputElement>(
      ".password-organize-target-grid input",
    );
    await act(async () => {
      if (targetInputs?.[0]) setInputValue(targetInputs[0], "Split target");
      if (targetInputs?.[1]) setInputValue(targetInputs[1], "split-target");
    });
    const review = Array.from(
      organizer?.querySelectorAll<HTMLButtonElement>("button") ?? [],
    ).find((button) => button.textContent === "Review changes");
    await act(async () => review?.click());

    const confirmation =
      view.container.querySelector<HTMLElement>('[role="dialog"]');
    expect(confirmation?.textContent).toContain("Confirm field organization");
    const reason = confirmation?.querySelector<HTMLTextAreaElement>("textarea");
    await act(async () => {
      if (reason) setTextareaValue(reason, "Separate service credentials");
    });
    const confirm = Array.from(
      confirmation?.querySelectorAll<HTMLButtonElement>("button") ?? [],
    ).find((button) => button.textContent === "Confirm");
    await act(async () => confirm?.click());

    expect(invoke).toHaveBeenCalledWith("submit_desktop_password_change", {
      operations: [
        {
          operation: "move_field",
          resource_id: "secret/source/username",
          target_item_id: "split-target",
          target_title: "Split target",
        },
        {
          operation: "move_field",
          resource_id: "secret/source/password",
          target_item_id: "split-target",
          target_title: "Split target",
        },
      ],
      reason: "Separate service credentials",
    });
    view.unmount();
  });

  it("creates a local-value-verified dedupe operation without revealing values", async () => {
    const duplicate = passwordItem(1, { title: "Duplicate" });
    const canonical = passwordItem(2, { title: "Canonical" });
    loadPasswordItems.mockResolvedValue({
      kind: "live",
      items: [duplicate, canonical],
    });
    invoke.mockImplementation((command: string) => {
      if (command === "list_password_catalog_metadata_command") {
        return Promise.resolve({
          revision: "test-revision",
          items: [duplicate, canonical].map((item, index) => ({
            record_id: `record-${index + 1}`,
            item_id: item.id,
            title: item.title,
            tags: [],
            metadata: {},
            fields: item.fields.map((field) => ({
              resource_id: field.resourceId,
              label: field.label,
              provider_kind: "local_literal",
              has_value: true,
            })),
          })),
        });
      }
      if (command === "submit_desktop_password_change") {
        return Promise.resolve({ state: "committed" });
      }
      return Promise.reject(new Error(`unexpected command ${command}`));
    });

    const view = await render(
      <PasswordVaultPage
        incomingDraftId={null}
        locale="en"
        onDraftConsumed={() => {}}
      />,
    );
    const organize = Array.from(
      view.container.querySelectorAll<HTMLButtonElement>("button"),
    ).find((button) => button.textContent === "Organize fields");
    await act(async () => organize?.click());
    const organizer =
      view.container.querySelector<HTMLElement>('[role="dialog"]');
    const action =
      organizer?.querySelector<HTMLFieldSetElement>(".choice-group");
    await act(async () => {
      if (action) setSelectValue(action, "dedupe");
    });
    expect(organizer?.textContent).toContain(
      "compares the two stored values locally",
    );
    const review = Array.from(
      organizer?.querySelectorAll<HTMLButtonElement>("button") ?? [],
    ).find((button) => button.textContent === "Review changes");
    await act(async () => review?.click());
    const confirmation =
      view.container.querySelector<HTMLElement>('[role="dialog"]');
    const reason = confirmation?.querySelector<HTMLTextAreaElement>("textarea");
    await act(async () => {
      if (reason) setTextareaValue(reason, "Remove verified duplicate");
    });
    const confirm = Array.from(
      confirmation?.querySelectorAll<HTMLButtonElement>("button") ?? [],
    ).find((button) => button.textContent === "Confirm");
    await act(async () => confirm?.click());

    expect(invoke).toHaveBeenCalledWith("submit_desktop_password_change", {
      operations: [
        {
          operation: "delete_duplicate_field",
          resource_id: duplicate.fields[0].resourceId,
          canonical_resource_id: canonical.fields[0].resourceId,
        },
      ],
      reason: "Remove verified duplicate",
    });
    expect(view.container.textContent).not.toContain("Resolved on demand");
    view.unmount();
  });

  it("opens imported entries when empty tags are omitted from metadata", async () => {
    const item = passwordItem(1, { backend: "one_password" });
    loadPasswordItems.mockResolvedValue({ kind: "live", items: [item] });
    invoke.mockImplementation((command: string) => {
      if (command === "list_password_catalog_metadata_command") {
        return Promise.resolve({
          revision: "test-revision",
          items: [
            {
              record_id: "record-1",
              item_id: "password-1",
              title: "Password 1",
              fields: [
                {
                  resource_id: item.fields[0].resourceId,
                  label: "Password",
                  provider_kind: "1password_cli",
                  has_value: true,
                },
              ],
            },
          ],
        });
      }
      return Promise.reject(new Error(`unexpected command ${command}`));
    });

    const view = await render(
      <PasswordVaultPage
        incomingDraftId={null}
        locale="en"
        onDraftConsumed={() => {}}
      />,
    );
    const edit = Array.from(
      view.container.querySelectorAll<HTMLButtonElement>("button"),
    ).find((button) => button.textContent === "Edit entry");
    await act(async () => edit?.click());

    expect(view.container.textContent).toContain("EDIT ENTRY");
    const inputs = view.container.querySelectorAll<HTMLInputElement>(
      ".password-inline-editor input",
    );
    expect(inputs[2]?.value).toBe("");
    view.unmount();
  });

  it("consumes a committed draft before a catalog refresh failure and never retries the commit", async () => {
    const staleReveal = deferred<string>();
    const refreshStderr =
      "SENTINEL_REFRESH_STDERR secret=do-not-render refresh failed";
    loadPasswordItems
      .mockResolvedValueOnce({
        kind: "live",
        items: [passwordItem(1)],
      })
      .mockRejectedValueOnce(new Error(refreshStderr));
    resolvePasswordValue.mockReturnValueOnce(staleReveal.promise);
    invoke.mockImplementation((command: string) => {
      if (command === "preview_password_draft") {
        return Promise.resolve({
          descriptor: { kind: "environment", names: ["TOKEN"] },
          entries: [{ key: "TOKEN", value: "commit-secret" }],
        });
      }
      if (command === "list_backend_connections") {
        return Promise.resolve([]);
      }
      if (command === "confirm_password_draft") {
        return Promise.resolve({
          draft_id: "draft-commit-once",
          destination: "plankton:default",
          resource_ids: ["plankton://field/committed/token"],
        });
      }
      return Promise.reject(new Error(`unexpected command ${command}`));
    });
    const consumed = vi.fn();
    const view = await render(
      <PasswordVaultPage
        incomingDraftId="draft-commit-once"
        locale="en"
        onDraftConsumed={consumed}
      />,
    );
    await act(async () => {
      view.container
        .querySelector<HTMLButtonElement>('[aria-label="Show Password"]')
        ?.click();
    });
    expect(
      view.container.querySelector<HTMLButtonElement>(
        '[aria-label="Show Password"]',
      )?.disabled,
    ).toBe(true);
    const review = Array.from(
      view.container.querySelectorAll<HTMLButtonElement>("button"),
    ).find((button) => button.textContent === "Next: review and save");
    await act(async () => {
      review?.click();
    });
    const confirm = Array.from(
      view.container.querySelectorAll<HTMLButtonElement>("button"),
    ).find((button) => button.textContent === "Confirm and save");
    await act(async () => {
      confirm?.click();
      await Promise.resolve();
      await Promise.resolve();
    });

    expect(
      invoke.mock.calls.filter(
        ([command]) => command === "confirm_password_draft",
      ),
    ).toHaveLength(1);
    expect(consumed).toHaveBeenCalledOnce();
    expect(view.container.querySelector('[role="dialog"]')).toBeNull();
    expect(view.container.textContent).not.toContain("Confirm and save");
    expect(
      view.container.querySelector('[role="alert"]')?.textContent,
    ).toContain("Saved, but the password catalog could not be refreshed.");
    expect(view.container.textContent).not.toContain(refreshStderr);
    await act(async () => {
      staleReveal.resolve("revealed-after-commit");
      await staleReveal.promise;
    });
    expect(view.container.textContent).not.toContain("revealed-after-commit");
    expect(
      visibleFieldValue(view.container.querySelector(".field-value")),
    ).toBe("••••••••");
    view.unmount();
  });

  it("keeps a replacement draft when the previous draft commit resolves late", async () => {
    const staleCommit = deferred<{
      draft_id: string;
      destination: string;
      resource_ids: string[];
    }>();
    loadPasswordItems
      .mockResolvedValueOnce({
        kind: "live",
        items: [passwordItem(1)],
      })
      .mockRejectedValueOnce(new Error("stale A refresh failed"))
      .mockResolvedValueOnce({
        kind: "live",
        items: [passwordItem(2)],
      });
    invoke.mockImplementation(
      (command: string, args?: { draftId?: string }) => {
        if (command === "preview_password_draft") {
          const draftId = args?.draftId;
          return Promise.resolve({
            descriptor: {
              kind: "environment",
              names: [draftId === "draft-a" ? "A_TOKEN" : "B_TOKEN"],
            },
            entries: [
              {
                key: draftId === "draft-a" ? "A_TOKEN" : "B_TOKEN",
                value: draftId === "draft-a" ? "secret-a" : "secret-b",
              },
            ],
          });
        }
        if (command === "list_backend_connections") {
          return Promise.resolve([]);
        }
        if (command === "list_local_vaults") {
          return Promise.resolve([{ id: "default", label: "Default" }]);
        }
        if (command === "confirm_password_draft") {
          if (args?.draftId === "draft-a") {
            return staleCommit.promise;
          }
          return Promise.resolve({
            draft_id: "draft-b",
            destination: "plankton:default",
            resource_ids: ["plankton://field/draft-b/token"],
          });
        }
        return Promise.reject(new Error(`unexpected command ${command}`));
      },
    );
    const consumed = vi.fn();
    const view = await render(
      <PasswordVaultPage
        incomingDraftId="draft-a"
        locale="en"
        onDraftConsumed={consumed}
      />,
    );

    await act(async () => {
      Array.from(view.container.querySelectorAll<HTMLButtonElement>("button"))
        .find((button) => button.textContent === "Next: review and save")
        ?.click();
    });
    await act(async () => {
      Array.from(view.container.querySelectorAll<HTMLButtonElement>("button"))
        .find((button) => button.textContent === "Confirm and save")
        ?.click();
    });

    await view.rerender(
      <PasswordVaultPage
        incomingDraftId="draft-b"
        locale="en"
        onDraftConsumed={consumed}
      />,
    );
    expect(view.container.textContent).toContain("draft-b");
    expect(view.container.textContent).toContain("B_TOKEN");
    expect(view.container.textContent).not.toContain("secret-b");
    expect(view.container.textContent).toContain("Next: review and save");

    await act(async () => {
      staleCommit.resolve({
        draft_id: "draft-a",
        destination: "plankton:default",
        resource_ids: ["plankton://field/draft-a/token"],
      });
      await staleCommit.promise;
      await Promise.resolve();
    });

    try {
      expect(consumed).not.toHaveBeenCalled();
      expect(view.container.querySelector('[role="dialog"]')).not.toBeNull();
      expect(view.container.textContent).toContain("draft-b");
      expect(view.container.textContent).toContain("B_TOKEN");
      expect(view.container.textContent).not.toContain("secret-b");
      expect(view.container.textContent).not.toContain(
        "stale A refresh failed",
      );
      expect(view.container.querySelector('[role="alert"]')).toBeNull();
      expect(loadPasswordItems).toHaveBeenCalledTimes(2);

      await act(async () => {
        Array.from(view.container.querySelectorAll<HTMLButtonElement>("button"))
          .find((button) => button.textContent === "Next: review and save")
          ?.click();
      });
      await act(async () => {
        Array.from(view.container.querySelectorAll<HTMLButtonElement>("button"))
          .find((button) => button.textContent === "Confirm and save")
          ?.click();
        await Promise.resolve();
        await Promise.resolve();
      });

      expect(consumed).toHaveBeenCalledOnce();
      expect(
        invoke.mock.calls.filter(
          ([command]) => command === "confirm_password_draft",
        ),
      ).toHaveLength(2);
      expect(loadPasswordItems).toHaveBeenCalledTimes(3);
      expect(view.container.querySelector('[role="dialog"]')).toBeNull();
    } finally {
      view.unmount();
    }
  });

  it("localizes visible and accessible password controls in Chinese", async () => {
    loadPasswordItems.mockResolvedValue({
      kind: "live",
      items: [passwordItem(1, { tags: ["生产"] })],
    });
    const view = await render(
      <PasswordVaultPage
        incomingDraftId={null}
        locale="zh-CN"
        onDraftConsumed={() => {}}
      />,
    );

    expect(
      view.container.querySelector('[aria-label="搜索密码条目"]'),
    ).not.toBeNull();
    const filterTrigger = view.container.querySelector<HTMLButtonElement>(
      '[aria-label="打开密码筛选"]',
    );
    expect(filterTrigger).not.toBeNull();
    await act(async () => {
      filterTrigger?.click();
    });
    const drawer = view.container.querySelector('[data-page-drawer="true"]');
    expect(drawer?.getAttribute("aria-label")).toBe("密码筛选");
    expect(
      drawer?.querySelector('[aria-label="关闭密码筛选抽屉"]'),
    ).not.toBeNull();
    expect(drawer?.querySelector('[aria-label="按保险库筛选"]')).not.toBeNull();
    expect(drawer?.querySelector('[aria-label="按标签筛选"]')).not.toBeNull();
    expect(drawer?.querySelector('[aria-label="标签匹配模式"]')).not.toBeNull();
    view.unmount();
  });

  it("keeps independent scroll columns, fixed controls, and the reachable narrow filter contract", () => {
    const sheet = installPasswordVaultStyles();
    for (const selector of [
      ".desktop-workspace .password-vault-shell .vault-sidebar",
      ".desktop-workspace .password-vault-shell .password-list-scroll",
      ".desktop-workspace .password-vault-shell .item-detail-scroll",
    ]) {
      const rule = styleRule(sheet, selector);
      expect(rule?.style.overflow).toBe("auto");
      expect(rule?.style.getPropertyValue("scrollbar-gutter")).toBe("stable");
    }

    const tagMode = styleRule(
      sheet,
      ".desktop-workspace .password-vault-shell .password-filter-surface select",
    );
    expect(tagMode?.style.height).toBe("40px");
    expect(tagMode?.style.minHeight).toBe("40px");
    const detailActions = styleRule(
      sheet,
      ".desktop-workspace .password-vault-shell .item-detail-actions",
    );
    expect(detailActions?.style.position).toBe("sticky");
    expect(detailActions?.style.bottom).toBe("0px");
    const fieldAction = styleRule(
      sheet,
      ".desktop-workspace .password-vault-shell .field-actions button",
    );
    expect(fieldAction?.style.minHeight).toBe("40px");
    expect(fieldAction?.style.minWidth).toBe("40px");

    expect(
      mediaRule(
        sheet,
        "(max-width: 620px)",
        ".desktop-workspace .password-vault-shell .password-filter-trigger",
      )?.style.display,
    ).toBe("inline-flex");
    expect(
      mediaRule(
        sheet,
        "(max-width: 620px)",
        ".desktop-workspace .password-vault-shell .vault-sidebar",
      )?.style.display,
    ).toBe("none");
  });
});

describe("PasswordManagementView information architecture", () => {
  it("keeps catalog management flat and does not probe optional providers", async () => {
    invoke.mockImplementation((command: string) => {
      if (command === "list_secret_catalog_metadata") {
        return Promise.resolve({
          catalog_path: "/tmp/plankton-secrets.toml",
          imports: [],
          literals: [],
        });
      }
      return Promise.reject(new Error(`unexpected command ${command}`));
    });
    const view = await render(
      <PasswordManagementView locale="en" surface="catalog" />,
    );
    const sections = Array.from(
      view.container.querySelectorAll<HTMLElement>(
        "[data-password-primary-section]",
      ),
    );
    expect(
      sections.map((section) => section.dataset.passwordPrimarySection),
    ).toEqual(["catalog"]);
    expect(invoke).toHaveBeenCalledTimes(1);
    expect(invoke).toHaveBeenCalledWith("list_secret_catalog_metadata");
    view.unmount();
  });

  it("starts with a clear manual form and reveals import controls on demand", async () => {
    invoke.mockImplementation((command: string) => {
      if (command === "list_backend_connections") {
        return Promise.resolve([
          {
            backend_kind: "one_password",
            enabled: true,
          },
          {
            backend_kind: "bitwarden",
            enabled: false,
          },
        ]);
      }
      if (command === "list_onepassword_accounts_command") {
        return Promise.resolve([]);
      }
      return Promise.reject(new Error(`unexpected command ${command}`));
    });
    const view = await render(
      <PasswordManagementView locale="en" surface="import" />,
    );
    const sections = Array.from(
      view.container.querySelectorAll<HTMLElement>(
        "[data-password-primary-section]",
      ),
    );
    expect(
      sections.map((section) => section.dataset.passwordPrimarySection),
    ).toEqual(["manual"]);
    expect(sections[0]?.querySelector("h2")?.textContent).toBe(
      "Create a password",
    );
    expect(
      view.container.querySelector("[data-testid=password-provider-options]"),
    ).toBeNull();

    act(() => {
      view.container
        .querySelector<HTMLButtonElement>(
          '[data-testid="password-entry-mode-import"]',
        )
        ?.click();
    });
    const importSections = Array.from(
      view.container.querySelectorAll<HTMLElement>(
        "[data-password-primary-section]",
      ),
    );
    expect(
      importSections.map((section) => section.dataset.passwordPrimarySection),
    ).toEqual(["add", "sources"]);
    expect(
      importSections
        .find((section) => section.dataset.passwordPrimarySection === "add")
        ?.querySelector("h2")?.textContent,
    ).toBe("Optional details");
    const providerText = view.container.querySelector(
      '[data-testid="password-provider-options"]',
    )?.textContent;
    expect(providerText).toContain("1Password CLI");
    expect(providerText).toContain("Dotenv File");
    expect(providerText).not.toContain("Bitwarden CLI");
    expect(invoke).not.toHaveBeenCalledWith("list_secret_catalog_metadata");
    view.unmount();
  });

  it("saves a manually entered password with generated identifiers and field metadata", async () => {
    const onDraftCreated = vi.fn();
    invoke.mockImplementation((command: string) => {
      if (command === "list_backend_connections") {
        return Promise.resolve([]);
      }
      if (command === "create_password_draft_command") {
        return Promise.resolve({ draft_id: "manual-draft-1" });
      }
      if (command === "list_secret_catalog_metadata") {
        return Promise.resolve({
          catalog_path: "/tmp/plankton-secrets.toml",
          imports: [],
          literals: [],
        });
      }
      return Promise.reject(new Error(`unexpected command ${command}`));
    });
    const view = await render(
      <PasswordManagementView
        locale="en"
        onDraftCreated={onDraftCreated}
        surface="import"
      />,
    );

    await act(async () => {
      setInputValue(
        view.container.querySelector<HTMLInputElement>(
          '[data-testid="manual-secret-title"]',
        )!,
        "GitHub Production",
      );
      setInputValue(
        view.container.querySelector<HTMLInputElement>(
          '[data-testid="manual-secret-value"]',
        )!,
        "secret-value",
      );
      setInputValue(
        view.container.querySelector<HTMLInputElement>(
          '[data-testid="manual-secret-tags"]',
        )!,
        "production, shared",
      );
    });

    await act(async () => {
      view.container
        .querySelector<HTMLButtonElement>(
          '[data-testid="manual-secret-submit"]',
        )
        ?.click();
    });

    expect(invoke).toHaveBeenCalledWith("create_password_draft_command", {
      input: {
        descriptor: { kind: "environment", names: ["password"] },
        entries: [{ key: "password", value: "secret-value" }],
        suggested_item_title: "GitHub Production",
        suggested_destination: null,
        suggested_layout: {
          description: null,
          tags: ["production", "shared"],
          field_labels: { password: "Password" },
          field_resources: {
            password: "plankton://field/github-production/password",
          },
        },
      },
    });
    expect(onDraftCreated).toHaveBeenCalledWith("manual-draft-1");
    expect(
      view.container
        .querySelector('[data-testid="manual-secret-value"]')
        ?.getAttribute("type"),
    ).toBe("password");
    expect(view.container.textContent).not.toContain("secret-value");
    view.unmount();
  });

  it("combines keys from multiple dotenv files into one transfer selection", async () => {
    invoke.mockImplementation(
      (command: string, payload?: { filePath?: string }) => {
        if (command === "list_secret_catalog_metadata") {
          return Promise.resolve({
            catalog_path: "/tmp/plankton-secrets.toml",
            imports: [],
            literals: [],
          });
        }
        if (command === "list_onepassword_accounts_command") {
          return Promise.resolve([]);
        }
        if (command === "pick_dotenv_file_command") {
          return Promise.resolve(["/tmp/alpha.env", "/tmp/beta.env"]);
        }
        if (command === "inspect_dotenv_file_command") {
          const filePath = payload?.filePath ?? "";
          const key = filePath.includes("alpha") ? "ALPHA_TOKEN" : "BETA_TOKEN";
          return Promise.resolve({
            file_path: filePath,
            groups: [
              {
                id: "all",
                label: "All keys",
                namespace: null,
                prefix: null,
                key_count: 1,
              },
            ],
            keys: [{ group_id: "all", label: key, full_key: key }],
          });
        }
        if (command === "import_secret_sources") {
          return Promise.resolve({
            catalog_path: "/tmp/plankton-secrets.toml",
            receipts: [],
          });
        }
        return Promise.reject(new Error(`unexpected command ${command}`));
      },
    );
    const view = await render(<PasswordManagementView locale="en" />);

    act(() => {
      view.container
        .querySelector<HTMLButtonElement>(
          '[data-testid="password-provider-option-dotenv_file"]',
        )
        ?.click();
    });
    await act(async () => {
      view.container
        .querySelector<HTMLButtonElement>(
          '[data-testid="dotenv-choose-file-button"]',
        )
        ?.click();
    });
    await act(async () => {});

    expect(view.container.textContent).toContain("alpha.env");
    expect(view.container.textContent).toContain("beta.env");
    expect(
      view.container.querySelector('[data-testid="dotenv-group-picker"]'),
    ).toBeNull();

    for (const key of ["ALPHA_TOKEN", "BETA_TOKEN"]) {
      act(() => {
        Array.from(
          view.container.querySelectorAll<HTMLButtonElement>(
            '[data-testid="dotenv-key-picker-option"]',
          ),
        )
          .find((button) => button.textContent?.includes(key))
          ?.click();
      });
    }
    await act(async () => {});

    await act(async () => {
      view.container
        .querySelector<HTMLButtonElement>(
          '[data-testid="password-import-submit"]',
        )
        ?.click();
    });
    const importCall = invoke.mock.calls.find(
      ([command]) => command === "import_secret_sources",
    );
    expect(
      importCall?.[1]?.spec.imports.map(
        (entry: { source_locator: { file_path: string } }) =>
          entry.source_locator.file_path,
      ),
    ).toEqual(["/tmp/alpha.env", "/tmp/beta.env"]);
    view.unmount();
  });

  it("announces picker loading and exposes its busy state", async () => {
    const accounts =
      deferred<
        Array<{ id: string; label: string; subtitle?: string | null }>
      >();
    invoke.mockImplementation((command: string) => {
      if (command === "list_secret_catalog_metadata") {
        return Promise.resolve({
          catalog_path: "/tmp/plankton-secrets.toml",
          imports: [],
          literals: [],
        });
      }
      if (command === "list_onepassword_accounts_command") {
        return accounts.promise;
      }
      return Promise.reject(new Error(`unexpected command ${command}`));
    });
    const view = await render(<PasswordManagementView locale="zh-CN" />);
    const picker = view.container.querySelector(
      '[data-testid="onepassword-account-picker"]',
    );
    expect(picker?.getAttribute("aria-busy")).toBe("true");
    const loading = picker?.querySelector(
      '[data-testid="onepassword-account-picker-loading"]',
    );
    expect(loading?.getAttribute("role")).toBe("status");
    expect(loading?.getAttribute("aria-live")).toBe("polite");
    expect(loading?.textContent).toContain("加载账号中");

    await act(async () => {
      accounts.resolve([]);
      await accounts.promise;
    });
    expect(picker?.getAttribute("aria-busy")).toBe("false");
    expect(
      picker?.querySelector(
        '[data-testid="onepassword-account-picker-loading"]',
      ),
    ).toBeNull();
    view.unmount();
  });

  it("scopes password management legacy selectors away from compact surfaces", () => {
    const selectors = allSelectors(installStyles(legacyStyles).cssRules);
    const scopedRoots = [".desktop-workspace", ".password-management-view"];
    const managedSelector =
      /\.(?:password-|provider-|queue-|detail-|settings-|catalog-|imported-|template-|boundary-|field-(?:optional|hint)|section-copy|panel(?:[.:\s>#-]|$)|alert(?:[.:\s>#]|$))/;
    const unscoped = selectors.filter(
      (selector) =>
        managedSelector.test(selector) &&
        !scopedRoots.some((root) => selector.startsWith(root)),
    );
    expect(unscoped).toEqual([]);
  });

  it("presents Catalog, Add or Import, and Sources as primary sections with advanced templates collapsed", async () => {
    invoke.mockImplementation((command: string) => {
      if (command === "list_secret_catalog_metadata") {
        return Promise.resolve({
          catalog_path: "/tmp/plankton-secrets.toml",
          imports: [],
          literals: [],
        });
      }
      if (command === "list_onepassword_accounts_command") {
        return Promise.resolve([]);
      }
      return Promise.reject(new Error(`unexpected command ${command}`));
    });
    const view = await render(<PasswordManagementView locale="en" />);

    const sections = Array.from(
      view.container.querySelectorAll<HTMLElement>(
        "[data-password-primary-section]",
      ),
    );
    expect(
      sections.map((section) => section.querySelector("h2")?.textContent),
    ).toEqual(["Catalog", "Add or Import", "Sources"]);
    const advanced = view.container.querySelector<HTMLDetailsElement>(
      '[data-testid="password-advanced-templates"]',
    );
    expect(advanced).not.toBeNull();
    expect(advanced?.open).toBe(false);
    expect(
      advanced?.querySelector('[data-testid="password-template-section"]'),
    ).not.toBeNull();
    expect(
      view.container.querySelector("[data-testid=password-management-header]"),
    ).toBeNull();
    view.unmount();
  });
});
