import { invoke } from "@tauri-apps/api/core";

import type { LocalSecretCatalog } from "../../types";
import type {
  PasswordBackend,
  PasswordItem,
  PasswordOrigin,
} from "./workspaceTypes";

type TauriWindow = Window & { __TAURI_INTERNALS__?: object };

export type PasswordAdapterResult =
  | { kind: "live"; items: PasswordItem[] }
  | { kind: "fallback"; message: string };

function asPasswordItems(catalog: LocalSecretCatalog): PasswordItem[] {
  const grouped = new Map<string, PasswordItem>();
  for (const entry of catalog.literals) {
    const fieldKey = entry.metadata?.field_key?.trim() || "value";
    const itemId = entry.metadata?.item_id?.trim() || entry.resource;
    const current = grouped.get(itemId);
    const nextTags = Array.from(
      new Set([...(current?.tags ?? []), ...(entry.tags ?? [])]),
    );
    const item: PasswordItem = current ?? {
      id: itemId,
      title:
        entry.metadata?.item_title?.trim() ||
        entry.display_name ||
        entry.resource,
      vault: entry.metadata?.vault ?? "Plankton",
      group: entry.metadata?.group ?? entry.metadata?.section ?? "Credentials",
      tags: nextTags,
      username: entry.metadata?.username ?? "",
      notes: entry.description ?? "",
      updatedAt: "",
      backend: "plankton",
      origin: "local",
      fields: [],
    };
    item.tags = nextTags;
    item.fields.push({
      key: fieldKey,
      label: entry.metadata?.field_label?.trim() || "Value",
      value: "Resolved on demand",
      resourceId: entry.resource,
      secret: true,
    });
    grouped.set(itemId, item);
  }
  for (const entry of catalog.imports) {
    const parsed = parseFieldResource(entry.resource);
    const fieldKey =
      entry.metadata?.field_key ??
      parsed?.fieldId ??
      ("field" in entry ? entry.field : entry.key);
    const fieldLabel = entry.metadata?.field_label ?? fieldKey;
    const itemId = importedItemId(entry, parsed?.itemId);
    const backend: PasswordBackend =
      entry.provider_kind === "1password_cli"
        ? "one_password"
        : entry.provider_kind === "bitwarden_cli"
          ? "bitwarden"
          : "plankton";
    const current = grouped.get(itemId);
    const nextTags = Array.from(
      new Set([...(current?.tags ?? []), ...(entry.tags ?? [])]),
    );
    const item: PasswordItem = current ?? {
      id: itemId,
      title: entry.metadata?.item_title ?? importedItemTitle(entry, fieldKey),
      vault: importedVault(entry),
      group: entry.metadata?.group ?? entry.metadata?.section ?? "Credentials",
      tags: nextTags,
      username: entry.metadata?.username ?? "",
      notes: entry.description ?? "",
      updatedAt: entry.last_verified_at ?? entry.imported_at,
      backend,
      origin: importedOrigin(entry),
      fields: [],
    };
    item.tags = nextTags;
    item.fields.push({
      key: fieldKey,
      label: fieldLabel,
      value: "Resolved on demand",
      resourceId: entry.resource,
      secret: true,
    });
    grouped.set(itemId, item);
  }
  return [...grouped.values()];
}

function importedItemId(
  entry: LocalSecretCatalog["imports"][number],
  parsedItemId?: string,
): string {
  const metadataItemId = entry.metadata?.item_id?.trim();
  if (metadataItemId) return metadataItemId;
  if (parsedItemId) return parsedItemId;

  if (entry.provider_kind === "1password_cli") {
    return [
      "1password",
      entry.account_id ?? entry.account,
      entry.vault_id ?? entry.vault,
      entry.item_id ?? entry.item,
    ].join(":");
  }
  if (entry.provider_kind === "bitwarden_cli") {
    return [
      "bitwarden",
      entry.account,
      entry.organization ?? entry.collection ?? entry.folder ?? "personal",
      entry.item_id ?? entry.item,
    ].join(":");
  }
  if (entry.provider_kind === "dotenv_file") {
    return [
      "dotenv",
      entry.file_path,
      entry.namespace ?? "",
      entry.prefix ?? "",
    ].join(":");
  }
  return entry.resource;
}

function importedItemTitle(
  entry: LocalSecretCatalog["imports"][number],
  fieldKey: string,
): string {
  if (
    entry.provider_kind === "1password_cli" ||
    entry.provider_kind === "bitwarden_cli"
  ) {
    return entry.item;
  }
  if (entry.provider_kind === "dotenv_file") {
    return dotenvItemTitle(entry.file_path, entry.namespace, entry.prefix);
  }
  return titleWithoutFieldSuffix(entry.display_name, fieldKey);
}

function dotenvItemTitle(
  filePath: string,
  namespace?: string | null,
  prefix?: string | null,
): string {
  const explicitContext = namespace?.trim() || prefix?.trim();
  if (explicitContext) return explicitContext;
  const segments = filePath.split(/[\\/]/).filter(Boolean);
  const fileName = segments.at(-1) ?? ".env";
  if (fileName === ".env") {
    const parentName = segments.at(-2);
    return parentName ? `${parentName} environment` : "Environment variables";
  }
  return fileName;
}

function importedVault(entry: LocalSecretCatalog["imports"][number]): string {
  const metadataVault = entry.metadata?.vault?.trim();
  if (metadataVault) return metadataVault;
  if (entry.provider_kind === "1password_cli") return entry.vault;
  if (entry.provider_kind === "bitwarden_cli") {
    return (
      entry.collection ?? entry.folder ?? entry.organization ?? entry.account
    );
  }
  return "Plankton";
}

function importedOrigin(
  entry: LocalSecretCatalog["imports"][number],
): PasswordOrigin {
  if (entry.provider_kind === "1password_cli") return "one_password";
  if (entry.provider_kind === "bitwarden_cli") return "bitwarden";
  if (entry.provider_kind === "dotenv_file") return "dotenv";
  if (
    entry.metadata?.source_kind === "dotenv" ||
    entry.metadata?.item_title === ".env" ||
    entry.display_name.startsWith(".env:")
  ) {
    return "dotenv";
  }
  return "plankton_vault";
}

function parseFieldResource(
  resource: string,
): { itemId: string; fieldId: string } | null {
  const match = /^plankton:\/\/field\/([^/]+)\/([^/]+)$/.exec(resource);
  return match ? { itemId: match[1], fieldId: match[2] } : null;
}

function titleWithoutFieldSuffix(
  displayName: string,
  fieldKey: string,
): string {
  const suffix = `:${fieldKey}`;
  return displayName.endsWith(suffix)
    ? displayName.slice(0, -suffix.length)
    : displayName;
}

export function passwordItemIdForResource(resource: string): string {
  return parseFieldResource(resource)?.itemId ?? resource;
}

export async function loadPasswordItems(): Promise<PasswordAdapterResult> {
  if (!(window as TauriWindow).__TAURI_INTERNALS__) {
    return {
      kind: "fallback",
      message: "Daemon catalog is unavailable in this preview.",
    };
  }

  const catalog = await invoke<LocalSecretCatalog>(
    "list_secret_catalog_metadata",
  );
  return { kind: "live", items: asPasswordItems(catalog) };
}

export async function resolvePasswordValue(resource: string): Promise<string> {
  if (!(window as TauriWindow).__TAURI_INTERNALS__) {
    throw new Error(
      "Secret resolution is available only in the desktop runtime.",
    );
  }
  return invoke<string>("resolve_human_secret", { resource });
}
