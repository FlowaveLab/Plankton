// @vitest-environment jsdom

import { beforeEach, describe, expect, it, vi } from "vitest";

import type { LocalSecretCatalog } from "../../types";
import {
  loadPasswordItems,
  passwordItemIdForResource,
} from "./passwordAdapter";

const { invoke } = vi.hoisted(() => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({ invoke }));

const FIRST_FIELD_RESOURCE = "plankton://field/item-123/api-token";
const SECOND_FIELD_RESOURCE = "plankton://field/item-123/service-user";

beforeEach(() => {
  invoke.mockReset();
  Object.assign(window, { __TAURI_INTERNALS__: {} });
});

describe("passwordAdapter", () => {
  it("groups fields by item while preserving metadata and opaque resource ids", async () => {
    const catalog = {
      catalog_path: "/metadata-only/catalog.json",
      literals: [],
      imports: [
        {
          provider_kind: "keepassxc_cli",
          database: "/vaults/primary.kdbx",
          entry: "item-123-api-token",
          field: "password",
          unlock_secret_file: "/vaults/.primary.unlock",
          executable: "/Applications/Plankton.app/keepassxc-cli",
          executable_sha256: "sha256",
          resource: FIRST_FIELD_RESOURCE,
          display_name: "Deploy service:API token",
          description: "Production deployment credentials",
          tags: ["plankton", "production"],
          metadata: {
            vault: "Primary",
            item_id: "item-123",
            item_title: "Deploy service",
            section: "Credentials",
            field_key: "api_token",
            field_label: "API token",
            username: "deploy-bot",
          },
          value: "must-never-reach-the-ui",
          imported_at: "2026-07-29T01:00:00Z",
          last_verified_at: "2026-07-29T02:00:00Z",
        },
        {
          provider_kind: "keepassxc_cli",
          database: "/vaults/primary.kdbx",
          entry: "item-123-service-user",
          field: "password",
          unlock_secret_file: "/vaults/.primary.unlock",
          executable: "/Applications/Plankton.app/keepassxc-cli",
          executable_sha256: "sha256",
          resource: SECOND_FIELD_RESOURCE,
          display_name: "Deploy service:Service user",
          description: "Production deployment credentials",
          tags: ["plankton", "platform"],
          metadata: {
            vault: "Primary",
            item_id: "item-123",
            item_title: "Deploy service",
            section: "Credentials",
            field_key: "service_user",
            field_label: "Service user",
            username: "deploy-bot",
          },
          value: "another-secret-that-must-not-leak",
          imported_at: "2026-07-29T01:00:00Z",
          last_verified_at: "2026-07-29T02:00:00Z",
        },
      ],
    } satisfies LocalSecretCatalog;
    invoke.mockResolvedValue(catalog);

    const result = await loadPasswordItems();

    expect(result).toEqual({
      kind: "live",
      items: [
        {
          id: "item-123",
          backend: "plankton",
          origin: "plankton_vault",
          title: "Deploy service",
          vault: "Primary",
          group: "Credentials",
          tags: ["plankton", "production", "platform"],
          username: "deploy-bot",
          notes: "Production deployment credentials",
          updatedAt: "2026-07-29T02:00:00Z",
          fields: [
            {
              key: "api_token",
              label: "API token",
              value: "Resolved on demand",
              resourceId: FIRST_FIELD_RESOURCE,
              secret: true,
            },
            {
              key: "service_user",
              label: "Service user",
              value: "Resolved on demand",
              resourceId: SECOND_FIELD_RESOURCE,
              secret: true,
            },
          ],
        },
      ],
    });
    expect(JSON.stringify(result)).not.toContain("must-never-reach-the-ui");
    expect(JSON.stringify(result)).not.toContain(
      "another-secret-that-must-not-leak",
    );
    expect(invoke).toHaveBeenCalledWith("list_secret_catalog_metadata");
  });

  it("uses explicit group metadata for legacy local entries", async () => {
    invoke.mockResolvedValue({
      catalog_path: "/metadata-only/catalog.json",
      imports: [],
      literals: [
        {
          resource: "secret/local-token",
          value: "redacted-by-the-backend",
          display_name: "Local token",
          tags: ["local"],
          metadata: {
            vault: "Plankton",
            group: "Legacy imports",
            item_title: "Local token entry",
            field_key: "password",
            field_label: "Password",
          },
        },
      ],
    } satisfies LocalSecretCatalog);

    const result = await loadPasswordItems();

    expect(result.kind).toBe("live");
    if (result.kind !== "live") {
      throw new Error("expected live password items");
    }
    expect(result.items[0]?.group).toBe("Legacy imports");
    expect(result.items[0]?.title).toBe("Local token entry");
    expect(result.items[0]?.fields[0]?.key).toBe("password");
    expect(result.items[0]?.fields[0]?.label).toBe("Password");
    expect(result.items[0]?.fields[0]?.resourceId).toBe("secret/local-token");
    expect(JSON.stringify(result.items)).not.toContain(
      "redacted-by-the-backend",
    );
  });

  it("groups local fields by item metadata without exposing stored values", async () => {
    invoke.mockResolvedValue({
      catalog_path: "/metadata-only/catalog.json",
      imports: [],
      literals: [
        {
          resource: "secret/local/username",
          value: "user-that-must-stay-hidden",
          display_name: "Service username",
          tags: ["local"],
          metadata: {
            item_id: "local-service",
            item_title: "Local service",
            field_key: "username",
            field_label: "Username",
          },
        },
        {
          resource: "secret/local/password",
          value: "password-that-must-stay-hidden",
          display_name: "Service password",
          tags: ["credential"],
          metadata: {
            item_id: "local-service",
            item_title: "Local service",
            field_key: "password",
            field_label: "Password",
          },
        },
      ],
    } satisfies LocalSecretCatalog);

    const result = await loadPasswordItems();

    expect(result.kind).toBe("live");
    if (result.kind !== "live") throw new Error("expected live items");
    expect(result.items).toHaveLength(1);
    expect(result.items[0]).toMatchObject({
      id: "local-service",
      title: "Local service",
      tags: ["local", "credential"],
    });
    expect(result.items[0]?.fields.map((field) => field.key)).toEqual([
      "username",
      "password",
    ]);
    expect(JSON.stringify(result)).not.toContain("must-stay-hidden");
  });

  it("maps connected references to backend filters while dotenv imports remain Plankton entries", async () => {
    invoke.mockResolvedValue({
      catalog_path: "/metadata-only/catalog.json",
      literals: [],
      imports: [
        {
          resource: "secret/op/token",
          display_name: "1Password token",
          tags: [],
          metadata: { item_id: "op-item" },
          imported_at: "2026-08-05T00:00:00Z",
          provider_kind: "1password_cli",
          account: "work",
          vault: "Engineering",
          item: "Deploy",
          field: "token",
        },
        {
          resource: "secret/bw/token",
          display_name: "Bitwarden token",
          tags: [],
          metadata: { item_id: "bw-item" },
          imported_at: "2026-08-05T00:00:00Z",
          provider_kind: "bitwarden_cli",
          account: "work",
          item: "Deploy",
          field: "token",
        },
        {
          resource: "secret/env/token",
          display_name: "Imported token",
          tags: [],
          metadata: { item_id: "env-item" },
          imported_at: "2026-08-05T00:00:00Z",
          provider_kind: "dotenv_file",
          file_path: "/tmp/.env",
          key: "TOKEN",
        },
      ],
    } satisfies LocalSecretCatalog);

    const result = await loadPasswordItems();
    expect(result.kind).toBe("live");
    if (result.kind !== "live") {
      throw new Error("expected live password items");
    }
    expect(result.items.map((item) => item.backend)).toEqual([
      "one_password",
      "bitwarden",
      "plankton",
    ]);
    expect(result.items.map((item) => item.origin)).toEqual([
      "one_password",
      "bitwarden",
      "dotenv",
    ]);
  });

  it("groups legacy connected fields by locator item id and preserves the real vault", async () => {
    invoke.mockResolvedValue({
      catalog_path: "/metadata-only/catalog.json",
      literals: [],
      imports: [
        {
          resource: "example/csighub/password",
          display_name: "csighub:password",
          tags: [],
          metadata: {},
          imported_at: "2026-08-05T00:00:00Z",
          provider_kind: "1password_cli",
          account: "work",
          vault: "Private",
          item: "csighub",
          item_id: "same-item-id",
          field: "password",
        },
        {
          resource: "example/csighub/username",
          display_name: "csighub:username",
          tags: [],
          metadata: {},
          imported_at: "2026-08-05T00:00:00Z",
          provider_kind: "1password_cli",
          account: "work",
          vault: "Private",
          item: "csighub",
          item_id: "same-item-id",
          field: "username",
        },
      ],
    } satisfies LocalSecretCatalog);

    const result = await loadPasswordItems();
    expect(result.kind).toBe("live");
    if (result.kind !== "live") throw new Error("expected live items");
    expect(result.items).toHaveLength(1);
    expect(result.items[0]).toMatchObject({
      title: "csighub",
      vault: "Private",
      origin: "one_password",
    });
    expect(result.items[0]?.fields.map((field) => field.key)).toEqual([
      "password",
      "username",
    ]);
  });

  it("groups dotenv keys by file and uses a meaningful title", async () => {
    invoke.mockResolvedValue({
      catalog_path: "/metadata-only/catalog.json",
      literals: [],
      imports: ["TOKEN", "USERNAME"].map((key) => ({
        resource: `secret/project/${key.toLowerCase()}`,
        display_name: key,
        tags: [],
        metadata: {},
        imported_at: "2026-08-05T00:00:00Z",
        provider_kind: "dotenv_file" as const,
        file_path: "/workspace/project/.env",
        key,
      })),
    } satisfies LocalSecretCatalog);

    const result = await loadPasswordItems();
    expect(result.kind).toBe("live");
    if (result.kind !== "live") throw new Error("expected live items");
    expect(result.items).toHaveLength(1);
    expect(result.items[0]).toMatchObject({
      title: "project environment",
      origin: "dotenv",
    });
    expect(result.items[0]?.fields).toHaveLength(2);
  });

  it("derives a stable item id from a field resource id", () => {
    expect(passwordItemIdForResource(FIRST_FIELD_RESOURCE)).toBe("item-123");
    expect(passwordItemIdForResource("secret/legacy-token")).toBe(
      "secret/legacy-token",
    );
  });
});
