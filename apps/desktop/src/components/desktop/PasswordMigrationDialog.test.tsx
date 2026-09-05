// @vitest-environment jsdom

import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { act, type JSX } from "react";
import ReactDOM from "react-dom/client";
import { afterEach, describe, expect, it, vi } from "vitest";

import { PasswordMigrationDialog } from "./PasswordMigrationDialog";

const { invoke } = vi.hoisted(() => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({ invoke }));
Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });

const passwordVaultStyles = readFileSync(
  resolve(process.cwd(), "src/components/desktop/password-vault.css"),
  "utf8",
);

function styleRule(selector: string): CSSStyleRule | undefined {
  const style = document.createElement("style");
  style.textContent = passwordVaultStyles;
  document.head.appendChild(style);
  const sheet = style.sheet;
  if (!(sheet instanceof CSSStyleSheet)) {
    throw new Error(
      "Password vault stylesheet did not produce a CSSStyleSheet",
    );
  }
  return Array.from(sheet.cssRules).find(
    (rule) =>
      "selectorText" in rule &&
      (rule as CSSStyleRule).selectorText === selector,
  ) as CSSStyleRule | undefined;
}

const item = {
  record_id: "record-source",
  item_id: "source",
  title: "Production API",
  fields: [
    {
      resource_id: "plankton://field/source/token",
      provider_kind: "keepassxc_cli",
      vault: "personal",
    },
  ],
};

function Harness(props: {
  onCompleted: (receipt: object) => void;
}): JSX.Element {
  return (
    <PasswordMigrationDialog
      catalogRevision="revision-1"
      item={item}
      locale="en"
      onClose={() => {}}
      onCompleted={props.onCompleted}
    />
  );
}

afterEach(() => {
  document.body.innerHTML = "";
  invoke.mockReset();
});

describe("PasswordMigrationDialog", () => {
  it("keeps migration radios compact beside readable option copy", () => {
    const radioRule = styleRule(
      '.desktop-workspace .password-migration-mode input[type="radio"]',
    );
    const copyRule = styleRule(
      ".desktop-workspace .password-migration-mode span",
    );

    expect(radioRule?.style.width).toBe("16px");
    expect(radioRule?.style.minHeight).toBe("16px");
    expect(radioRule?.style.flex).toBe("0 0 16px");
    expect(copyRule?.style.flex).toBe("1 1 auto");
    expect(copyRule?.style.minWidth).toBe("0");
  });

  it("lists local vaults and submits a verified copy request", async () => {
    const onCompleted = vi.fn();
    invoke.mockImplementation((command: string) => {
      if (command === "list_backend_connections") return Promise.resolve([]);
      if (command === "list_local_vaults") {
        return Promise.resolve([
          { id: "default", label: "Default" },
          { id: "work", label: "Work" },
        ]);
      }
      if (command === "migrate_password_item") {
        return Promise.resolve({
          migration_id: "migration-1",
          mode: "copy",
          destination: "plankton:work",
          resource_ids: ["plankton://field/target/token"],
          source_deleted: false,
        });
      }
      return Promise.reject(new Error(`unexpected command ${command}`));
    });
    const container = document.createElement("div");
    document.body.appendChild(container);
    const root = ReactDOM.createRoot(container);
    await act(async () => root.render(<Harness onCompleted={onCompleted} />));

    const vaultSelect = Array.from(container.querySelectorAll("select")).find(
      (select) => select.closest("label")?.textContent?.startsWith("Vault"),
    );
    await act(async () => {
      if (vaultSelect) {
        Object.getOwnPropertyDescriptor(
          HTMLSelectElement.prototype,
          "value",
        )?.set?.call(vaultSelect, "work");
        vaultSelect.dispatchEvent(new Event("change", { bubbles: true }));
      }
    });
    await act(async () => {
      Array.from(container.querySelectorAll("button"))
        .find((button) => button.textContent === "Verify and copy")
        ?.click();
    });

    expect(invoke).toHaveBeenCalledWith("migrate_password_item", {
      request: {
        source_record_id: "record-source",
        expected_revision: "revision-1",
        destination: { kind: "plankton", vault_id: "work" },
        mode: "copy",
      },
    });
    expect(onCompleted).toHaveBeenCalledOnce();
    act(() => root.unmount());
  });

  it("does not offer destructive move for file-backed fields", async () => {
    invoke.mockImplementation((command: string) => {
      if (command === "list_backend_connections") return Promise.resolve([]);
      if (command === "list_local_vaults") {
        return Promise.resolve([{ id: "default", label: "Default" }]);
      }
      return Promise.reject(new Error(`unexpected command ${command}`));
    });
    const container = document.createElement("div");
    document.body.appendChild(container);
    const root = ReactDOM.createRoot(container);
    await act(async () => {
      root.render(
        <PasswordMigrationDialog
          catalogRevision="revision-1"
          item={{
            ...item,
            fields: [{ ...item.fields[0], provider_kind: "dotenv_file" }],
          }}
          locale="en"
          onClose={() => {}}
          onCompleted={() => {}}
        />,
      );
    });

    const moveRadio = Array.from(
      container.querySelectorAll<HTMLInputElement>('input[type="radio"]'),
    ).find((input) =>
      input.closest("label")?.textContent?.includes("Remove source"),
    );
    expect(moveRadio?.disabled).toBe(true);
    expect(container.textContent).toContain(
      "Unavailable for file or literal sources",
    );
    act(() => root.unmount());
  });
});
