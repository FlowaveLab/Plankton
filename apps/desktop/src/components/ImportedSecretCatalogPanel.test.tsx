// @vitest-environment jsdom

import { act } from "react";
import ReactDOM from "react-dom/client";
import { afterEach, describe, expect, it, vi } from "vitest";

import { ImportedSecretCatalogPanel } from "./ImportedSecretCatalogPanel";

Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });

afterEach(() => {
  document.body.innerHTML = "";
});

describe("ImportedSecretCatalogPanel reveal errors", () => {
  it("catches a rejected reveal and renders only a safe inline error", async () => {
    const providerStderr =
      "SENTINEL_IMPORTED_REVEAL_STDERR password=do-not-render";
    const onReveal = vi.fn(async () => {
      throw new Error(providerStderr);
    });
    const container = document.createElement("div");
    document.body.appendChild(container);
    const root = ReactDOM.createRoot(container);
    await act(async () => {
      root.render(
        <ImportedSecretCatalogPanel
          catalog={{
            catalog_path: "/tmp/plankton-secrets.toml",
            literals: [],
            imports: [
              {
                resource: "secret/env/TOKEN",
                display_name: "Deploy token",
                description: null,
                tags: ["production"],
                imported_at: "2026-07-30T00:00:00Z",
                last_verified_at: null,
                provider_kind: "dotenv_file",
                file_path: "/tmp/service.env",
                namespace: "production",
                prefix: "APP_",
                key: "TOKEN",
              },
            ],
          }}
          errorMessage={null}
          isLoading={false}
          locale="en"
          noticeMessage={null}
          onDelete={async () => {}}
          onRefreshImported={async () => {}}
          onRename={async () => {}}
          onReload={async () => {}}
          onReveal={onReveal}
          onSaveImported={async () => {}}
          onSaveLiteral={async () => {}}
        />,
      );
    });
    const reveal = Array.from(
      container.querySelectorAll<HTMLButtonElement>("button"),
    ).find(
      (button) => button.getAttribute("aria-label") === "Show secret value",
    );
    expect(reveal).toBeDefined();

    await act(async () => {
      reveal?.click();
      await Promise.resolve();
    });

    expect(onReveal).toHaveBeenCalledWith("secret/env/TOKEN");
    expect(
      container.querySelector('[data-testid="imported-secret-reveal-error"]')
        ?.textContent,
    ).toContain(
      "Secret value could not be revealed. Check Diagnostics and try again.",
    );
    expect(container.textContent).not.toContain(providerStderr);
    act(() => root.unmount());
  });

  it("moves an imported resource key while leaving locator refresh as a separate action", async () => {
    const onRename = vi.fn(async () => {});
    const confirm = vi.spyOn(window, "confirm").mockReturnValue(true);
    const container = document.createElement("div");
    document.body.appendChild(container);
    const root = ReactDOM.createRoot(container);
    await act(async () => {
      root.render(
        <ImportedSecretCatalogPanel
          catalog={{
            catalog_path: "/tmp/plankton-secrets.toml",
            literals: [],
            imports: [
              {
                resource: "secret/env/TOKEN",
                display_name: "Deploy token",
                tags: [],
                imported_at: "2026-07-30T00:00:00Z",
                provider_kind: "dotenv_file",
                file_path: "/tmp/service.env",
                key: "TOKEN",
              },
            ],
          }}
          errorMessage={null}
          isLoading={false}
          locale="en"
          noticeMessage={null}
          onDelete={async () => {}}
          onRefreshImported={async () => {}}
          onRename={onRename}
          onReload={async () => {}}
          onReveal={async () => "secret"}
          onSaveImported={async () => {}}
          onSaveLiteral={async () => {}}
        />,
      );
    });
    const resourceInput = container.querySelector<HTMLInputElement>(
      '[data-testid="imported-secret-resource"] input',
    );
    await act(async () => {
      if (!resourceInput) throw new Error("resource input should render");
      Object.getOwnPropertyDescriptor(
        HTMLInputElement.prototype,
        "value",
      )?.set?.call(resourceInput, "secret/env/renamed-token");
      resourceInput.dispatchEvent(new Event("input", { bubbles: true }));
    });
    const move = container.querySelector<HTMLButtonElement>(
      '[data-testid="imported-secret-rename"]',
    );
    expect(move?.disabled).toBe(false);
    await act(async () => {
      move?.click();
      await Promise.resolve();
    });

    expect(onRename).toHaveBeenCalledWith(
      "secret/env/TOKEN",
      "secret/env/renamed-token",
    );
    confirm.mockRestore();
    act(() => root.unmount());
  });

  it("defaults to an item tree and keeps the raw resource key tree available", async () => {
    const container = document.createElement("div");
    document.body.appendChild(container);
    const root = ReactDOM.createRoot(container);
    await act(async () => {
      root.render(
        <ImportedSecretCatalogPanel
          catalog={{
            catalog_path: "/tmp/plankton-secrets.toml",
            literals: [],
            imports: ["password", "username"].map((field) => ({
              resource: `example/csighub/${field}`,
              display_name: `csighub:${field}`,
              tags: [],
              imported_at: "2026-07-30T00:00:00Z",
              provider_kind: "1password_cli" as const,
              account: "work",
              vault: "Private",
              item: "csighub",
              item_id: "item-1",
              field,
            })),
          }}
          errorMessage={null}
          isLoading={false}
          locale="en"
          noticeMessage={null}
          onDelete={async () => {}}
          onRefreshImported={async () => {}}
          onRename={async () => {}}
          onReload={async () => {}}
          onReveal={async () => "secret"}
          onSaveImported={async () => {}}
          onSaveLiteral={async () => {}}
        />,
      );
    });

    const treePanel = container.querySelector(
      '[data-testid="imported-secret-tree-panel"]',
    );
    expect(treePanel?.textContent).toContain("Item Tree");
    expect(treePanel?.textContent).toContain("1Password");
    expect(treePanel?.textContent).toContain("Private");
    expect(treePanel?.textContent).not.toContain("example");

    await act(async () => {
      container
        .querySelector<HTMLButtonElement>(
          '[data-testid="catalog-tree-mode-resource"]',
        )
        ?.click();
    });
    expect(treePanel?.textContent).toContain("Resource Key Tree");
    expect(treePanel?.textContent).toContain("example");
    act(() => root.unmount());
  });
});
