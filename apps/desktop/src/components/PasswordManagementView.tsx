import { SecretInput } from "./SecretInput";
import { invoke } from "@tauri-apps/api/core";
import { useEffect, useState, type JSX, type ReactNode } from "react";

import { ImportedSecretCatalogPanel } from "./ImportedSecretCatalogPanel";
import { getBrowserStorage } from "../browserStorage";
import { formatTimestamp } from "../formatters";
import { t, translateCode, type Locale } from "../i18n";
import type {
  BitwardenContainerOption,
  DotenvInspection,
  DotenvKeyOption,
  ImportFieldOption,
  ImportPickerOption,
  ImportedSecretBatchReceipt,
  ImportedSecretReceipt,
  ImportedSecretReference,
  ImportedSecretReferenceUpdate,
  LocalSecretCatalog,
  LocalSecretLiteralUpsert,
  SecretImportBatchSpec,
  SecretImportSpec,
  SecretSourceLocator,
} from "../types";

type SecretImportProviderKind = SecretSourceLocator["provider_kind"];
type GuidedImportProviderKind =
  | "1password_cli"
  | "bitwarden_cli"
  | "dotenv_file";
type PasswordEntryMode = "manual" | "import";

type CommonImportDraft = {
  resource: string;
  displayName: string;
  description: string;
  tags: string;
  metadata: string;
};

type ManualSecretDraft = {
  title: string;
  value: string;
  fieldLabel: string;
  description: string;
  tags: string;
  resource: string;
};

type OnePasswordDraft = {
  account: string;
  accountId: string;
  vault: string;
  item: string;
  field: string;
  vaultId: string;
  itemId: string;
  fieldId: string;
};

type BitwardenDraft = {
  account: string;
  organization: string;
  collection: string;
  folder: string;
  item: string;
  field: string;
  itemId: string;
};

type DotenvDraft = {
  filePath: string;
  namespace: string;
  prefix: string;
  key: string;
};

type DotenvSelectableKey = {
  id: string;
  filePath: string;
  fileLabel: string;
  group: DotenvInspection["groups"][number] | null;
  option: DotenvKeyOption;
};

type ProviderOption = {
  kind: GuidedImportProviderKind;
  descriptionKey:
    | "provider1passwordCliDesc"
    | "providerBitwardenCliDesc"
    | "providerDotenvFileDesc";
  scopeKey:
    | "importScope1password"
    | "importScopeBitwarden"
    | "importScopeDotenv";
};

type ResourceTemplateMode = "default" | "custom";
type ResourceTemplateTokenMap = Record<string, string>;
type ResourcePreviewResult = {
  missingTokens: string[];
  resource: string | null;
};
type FieldOptionsByResourceId = Record<string, ImportFieldOption[]>;
type SavedResourceTemplate = {
  id: string;
  name: string;
  providerKind: SecretImportProviderKind;
  template: string;
  createdAt: string;
  updatedAt: string;
};

type PickerRenderableOption = {
  id: string;
  label: string;
  subtitle?: string | null;
};

type PickerSectionProps = {
  title: string;
  caption?: string;
  dataTestId: string;
  options: PickerRenderableOption[];
  selectedId: string | null;
  onSelect: (id: string) => void;
  emptyMessage: string;
  loading: boolean;
  searchQuery?: string;
  onSearchQueryChange?: (value: string) => void;
  searchPlaceholder?: string;
};

type LocatorFieldProps = {
  dataTestId: string;
  label: string;
  value: string;
  onChange: (value: string) => void;
  optionalLabel: string;
  optional?: boolean;
  hint?: string;
  disabled?: boolean;
};

type MultiPickerSectionProps = {
  title: string;
  caption?: string;
  helper?: string;
  locale: Locale;
  dataTestId: string;
  options: PickerRenderableOption[];
  selectedIds: string[];
  onToggleSelect: (id: string) => void;
  emptyMessage: string;
  loading: boolean;
  searchQuery?: string;
  onSearchQueryChange?: (value: string) => void;
  searchPlaceholder?: string;
};

type PasswordManagementViewProps = {
  locale: Locale;
  onCatalogChange?: () => Promise<void> | void;
  onDraftCreated?: (draftId: string) => void;
  surface?: "catalog" | "full" | "import";
};

type BackendConnectionSummary = {
  backend_kind: "local" | "one_password" | "bitwarden" | "custom";
  enabled: boolean;
};

const PROVIDER_OPTIONS: ProviderOption[] = [
  {
    kind: "1password_cli",
    descriptionKey: "provider1passwordCliDesc",
    scopeKey: "importScope1password",
  },
  {
    kind: "bitwarden_cli",
    descriptionKey: "providerBitwardenCliDesc",
    scopeKey: "importScopeBitwarden",
  },
  {
    kind: "dotenv_file",
    descriptionKey: "providerDotenvFileDesc",
    scopeKey: "importScopeDotenv",
  },
];

const EMPTY_COMMON_DRAFT: CommonImportDraft = {
  resource: "",
  displayName: "",
  description: "",
  tags: "",
  metadata: "",
};

const EMPTY_MANUAL_SECRET_DRAFT: ManualSecretDraft = {
  title: "",
  value: "",
  fieldLabel: "Password",
  description: "",
  tags: "",
  resource: "",
};

const EMPTY_ONEPASSWORD_DRAFT: OnePasswordDraft = {
  account: "",
  accountId: "",
  vault: "",
  item: "",
  field: "",
  vaultId: "",
  itemId: "",
  fieldId: "",
};

const EMPTY_BITWARDEN_DRAFT: BitwardenDraft = {
  account: "",
  organization: "",
  collection: "",
  folder: "",
  item: "",
  field: "",
  itemId: "",
};

const EMPTY_DOTENV_DRAFT: DotenvDraft = {
  filePath: "",
  namespace: "",
  prefix: "",
  key: "",
};
const SAVED_RESOURCE_TEMPLATES_STORAGE_KEY =
  "plankton.desktop.saved-resource-templates";

function optionalValue(value: string): string | null {
  const trimmed = value.trim();
  return trimmed.length > 0 ? trimmed : null;
}

function parseTags(value: string): string[] {
  return value
    .split(/[\n,]/)
    .map((entry) => entry.trim())
    .filter((entry) => entry.length > 0);
}

function parseMetadataDraft(value: string): {
  metadata: Record<string, string>;
  invalidLines: string[];
} {
  const metadata: Record<string, string> = {};
  const invalidLines: string[] = [];

  for (const rawLine of value.split("\n")) {
    const line = rawLine.trim();
    if (line.length === 0) {
      continue;
    }

    const separatorIndex = line.indexOf("=");
    if (separatorIndex <= 0 || separatorIndex === line.length - 1) {
      invalidLines.push(line);
      continue;
    }

    const key = line.slice(0, separatorIndex).trim();
    const nextValue = line.slice(separatorIndex + 1).trim();
    if (key.length === 0 || nextValue.length === 0) {
      invalidLines.push(line);
      continue;
    }

    metadata[key] = nextValue;
  }

  return {
    metadata,
    invalidLines,
  };
}

function rawErrorMessage(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

function providerDisplayName(locale: Locale, providerKind: string): string {
  if (providerKind === "1password_cli") {
    return translateCode(locale, "1password_cli");
  }
  if (providerKind === "bitwarden_cli") {
    return translateCode(locale, "bitwarden_cli");
  }
  if (providerKind === "dotenv_file") {
    return translateCode(locale, "dotenv_file");
  }
  return providerKind;
}

function formatUserFacingImportError(locale: Locale, error: unknown): string {
  const message = rawErrorMessage(error);

  const emptyFieldMatch = message.match(
    /failed to verify imported source for (.+): (\w+) source for .+ contained field (.+), but its value was empty/i,
  );
  if (emptyFieldMatch) {
    const [, resource, providerKind, field] = emptyFieldMatch;
    const providerName = providerDisplayName(locale, providerKind);
    return sectionCaption(
      locale,
      `Import failed: ${providerName} field ${field} for ${resource} is empty. Deselect it or add a value in the provider, then retry.`,
      `导入失败：${providerName} 中 ${resource} 的字段 ${field} 为空。请取消选择该字段，或先在提供方中补上值后重试。`,
    );
  }

  const missingFieldMatch = message.match(
    /failed to verify imported source for (.+): (\w+) source for .+ did not contain field (.+)/i,
  );
  if (missingFieldMatch) {
    const [, resource, providerKind, field] = missingFieldMatch;
    const providerName = providerDisplayName(locale, providerKind);
    return sectionCaption(
      locale,
      `Import failed: ${providerName} field ${field} for ${resource} was not found. Refresh the item fields and retry.`,
      `导入失败：${providerName} 中 ${resource} 的字段 ${field} 未找到。请刷新字段列表后重试。`,
    );
  }

  const verifyMatch = message.match(
    /failed to verify imported source for (.+): (.+)/i,
  );
  if (verifyMatch) {
    const [, resource, detail] = verifyMatch;
    return sectionCaption(
      locale,
      `Import failed while verifying ${resource}: ${detail}`,
      `校验导入资源 ${resource} 失败：${detail}`,
    );
  }

  return message;
}

function loadSavedResourceTemplates(): SavedResourceTemplate[] {
  const raw = getBrowserStorage().getItem(SAVED_RESOURCE_TEMPLATES_STORAGE_KEY);
  if (!raw) {
    return [];
  }

  try {
    const parsed = JSON.parse(raw) as unknown;
    if (!Array.isArray(parsed)) {
      return [];
    }

    return parsed.flatMap((entry) => {
      if (
        !entry ||
        typeof entry !== "object" ||
        !("id" in entry) ||
        !("name" in entry) ||
        !("providerKind" in entry) ||
        !("template" in entry) ||
        !("createdAt" in entry) ||
        !("updatedAt" in entry)
      ) {
        return [];
      }

      const template = entry as SavedResourceTemplate;
      if (
        typeof template.id !== "string" ||
        typeof template.name !== "string" ||
        typeof template.providerKind !== "string" ||
        typeof template.template !== "string" ||
        typeof template.createdAt !== "string" ||
        typeof template.updatedAt !== "string"
      ) {
        return [];
      }

      if (
        template.providerKind !== "1password_cli" &&
        template.providerKind !== "bitwarden_cli" &&
        template.providerKind !== "dotenv_file"
      ) {
        return [];
      }

      return [template];
    });
  } catch (error) {
    console.error("failed to load saved resource templates", error);
    return [];
  }
}

function persistSavedResourceTemplates(
  templates: SavedResourceTemplate[],
): void {
  getBrowserStorage().setItem(
    SAVED_RESOURCE_TEMPLATES_STORAGE_KEY,
    JSON.stringify(templates),
  );
}

function normalizeResourceSegment(value: string): string | null {
  let normalized = "";
  let previousWasDash = false;

  for (const character of value.trim()) {
    let next: string;
    if (
      (character >= "a" && character <= "z") ||
      (character >= "0" && character <= "9") ||
      character === "_" ||
      character === "."
    ) {
      next = character;
    } else if (character >= "A" && character <= "Z") {
      next = character.toLowerCase();
    } else {
      next = "-";
    }

    if (next === "-") {
      if (normalized.length === 0 || previousWasDash) {
        continue;
      }

      previousWasDash = true;
      normalized += next;
      continue;
    }

    previousWasDash = false;
    normalized += next;
  }

  const trimmed = normalized.replace(/^[-_.]+|[-_.]+$/g, "");
  return trimmed.length > 0 ? trimmed : null;
}

function normalizeResourcePath(value: string): string {
  return value
    .split("/")
    .map((segment) => normalizeResourceSegment(segment))
    .filter((segment): segment is string => segment !== null)
    .join("/");
}

function defaultResourceTemplateForProvider(
  providerKind: SecretImportProviderKind,
): string {
  if (providerKind === "1password_cli") {
    return "secret/{{ account }}/{{ vault }}/{{ item }}/{{ field }}";
  }

  if (providerKind === "bitwarden_cli") {
    return "secret/{{ container }}/{{ item }}/{{ field }}";
  }

  return "secret/{{ source_name }}/{{ key }}";
}

function availableTemplateTokens(
  providerKind: SecretImportProviderKind,
): string[] {
  if (providerKind === "1password_cli") {
    return [
      "provider_kind",
      "account",
      "account_id",
      "vault",
      "vault_id",
      "container",
      "item",
      "item_id",
      "field",
      "field_id",
    ];
  }

  if (providerKind === "bitwarden_cli") {
    return [
      "provider_kind",
      "account",
      "organization",
      "collection",
      "folder",
      "container",
      "item",
      "item_id",
      "field",
    ];
  }

  return [
    "provider_kind",
    "file_path",
    "file_name",
    "file_stem",
    "namespace",
    "prefix",
    "source_name",
    "key",
  ];
}

function pathFileName(value: string): string {
  return value.split(/[/\\]/).filter(Boolean).at(-1) ?? "dotenv";
}

function pathFileStem(value: string): string {
  const fileName = pathFileName(value);
  const lastDot = fileName.lastIndexOf(".");
  return lastDot > 0 ? fileName.slice(0, lastDot) : fileName;
}

function templateTokensForSourceLocator(
  locator: SecretSourceLocator,
): ResourceTemplateTokenMap {
  if (locator.provider_kind === "1password_cli") {
    const tokens: ResourceTemplateTokenMap = {
      provider_kind: locator.provider_kind,
      account: locator.account,
      vault: locator.vault,
      container: locator.vault,
      item: locator.item,
      field: locator.field,
    };

    if (locator.account_id) {
      tokens.account_id = locator.account_id;
    }
    if (locator.vault_id) {
      tokens.vault_id = locator.vault_id;
    }
    if (locator.item_id) {
      tokens.item_id = locator.item_id;
    }
    if (locator.field_id) {
      tokens.field_id = locator.field_id;
    }

    return tokens;
  }

  if (locator.provider_kind === "bitwarden_cli") {
    const container =
      locator.collection ??
      locator.folder ??
      locator.organization ??
      locator.account;
    const tokens: ResourceTemplateTokenMap = {
      provider_kind: locator.provider_kind,
      account: locator.account,
      container,
      item: locator.item,
      field: locator.field,
    };

    if (locator.organization) {
      tokens.organization = locator.organization;
    }
    if (locator.collection) {
      tokens.collection = locator.collection;
    }
    if (locator.folder) {
      tokens.folder = locator.folder;
    }
    if (locator.item_id) {
      tokens.item_id = locator.item_id;
    }

    return tokens;
  }

  if (locator.provider_kind === "keepassxc_cli") {
    return {
      provider_kind: locator.provider_kind,
      vault: pathFileStem(locator.database),
      source_name: pathFileStem(locator.database),
      item: locator.entry,
      field: locator.field,
    };
  }

  const tokens: ResourceTemplateTokenMap = {
    provider_kind: locator.provider_kind,
    file_path: locator.file_path,
    file_name: pathFileName(locator.file_path),
    file_stem: pathFileStem(locator.file_path),
    key: locator.key,
  };

  if (locator.namespace) {
    tokens.namespace = locator.namespace;
    tokens.source_name = locator.namespace;
  }
  if (locator.prefix) {
    tokens.prefix = locator.prefix;
    if (!tokens.source_name) {
      tokens.source_name = locator.prefix;
    }
  }
  if (!tokens.source_name) {
    tokens.source_name = tokens.file_stem || tokens.file_name;
  }

  return tokens;
}

function renderGeneratedResource(
  template: string,
  tokens: ResourceTemplateTokenMap,
): ResourcePreviewResult {
  const missingTokens = Array.from(
    new Set(
      Array.from(template.matchAll(/\{\{\s*([a-z0-9_]+)\s*\}\}/gi))
        .map((match) => match[1])
        .filter((token) => !(token in tokens)),
    ),
  );

  if (missingTokens.length > 0) {
    return {
      missingTokens,
      resource: null,
    };
  }

  const rendered = template.replace(
    /\{\{\s*([a-z0-9_]+)\s*\}\}/gi,
    (_, token: string) => tokens[token] ?? "",
  );
  const normalized = normalizeResourcePath(rendered);

  return {
    missingTokens: [],
    resource: normalized.length > 0 ? normalized : null,
  };
}

function previewResourceForImport(
  spec: SecretImportSpec,
  resourceTemplate: string | null,
): ResourcePreviewResult {
  const explicitResource = spec.resource.trim();
  if (explicitResource.length > 0) {
    return {
      missingTokens: [],
      resource: explicitResource,
    };
  }

  const template =
    resourceTemplate && resourceTemplate.trim().length > 0
      ? resourceTemplate
      : defaultResourceTemplateForProvider(spec.source_locator.provider_kind);

  return renderGeneratedResource(
    template,
    templateTokensForSourceLocator(spec.source_locator),
  );
}

function uniqueValues(values: string[]): string[] {
  return Array.from(new Set(values));
}

function batchResourceTemplateForSubmit(
  mode: ResourceTemplateMode,
  template: string,
  explicitResource: string,
): string | null {
  if (explicitResource.trim().length > 0 || mode !== "custom") {
    return null;
  }

  const trimmed = template.trim();
  return trimmed.length > 0 ? trimmed : null;
}

function importBatchPayload(
  resourceTemplate: string | null,
  imports: SecretImportSpec[],
): SecretImportBatchSpec {
  return {
    resource_template: resourceTemplate,
    imports,
  };
}

function optionById<T extends { id: string }>(
  options: T[],
  nextId: string | null,
): T | null {
  if (!nextId) {
    return null;
  }

  return options.find((option) => option.id === nextId) ?? null;
}

function hasCachedFieldOptions(
  cache: FieldOptionsByResourceId,
  resourceId: string,
): boolean {
  return Object.prototype.hasOwnProperty.call(cache, resourceId);
}

function toggleSelection(
  current: string[],
  id: string,
  selectionMode: "single" | "multi",
): string[] {
  if (selectionMode === "single") {
    return current[0] === id ? [] : [id];
  }

  if (current.includes(id)) {
    return current.filter((entry) => entry !== id);
  }

  return [...current, id];
}

function matchesQuery(
  option: PickerRenderableOption,
  query: string | undefined,
): boolean {
  if (!query) {
    return true;
  }

  const normalizedQuery = query.trim().toLowerCase();
  if (normalizedQuery.length === 0) {
    return true;
  }

  return [option.label, option.subtitle, option.id]
    .filter((value): value is string => Boolean(value))
    .some((value) => value.toLowerCase().includes(normalizedQuery));
}

function fieldOptionId(field: ImportFieldOption): string {
  return field.field_id ?? field.selector;
}

function dotenvKeySelectionId(option: DotenvKeyOption): string {
  return `${option.group_id}:${option.full_key}`;
}

function displayFileName(filePath: string): string {
  return filePath.split(/[\\/]/).filter(Boolean).at(-1) ?? filePath;
}

function searchPlaceholder(locale: Locale, label: string): string {
  return locale === "zh-CN" ? `搜索${label}` : `Search ${label.toLowerCase()}`;
}

function sectionCaption(
  locale: Locale,
  english: string,
  chinese: string,
): string {
  return locale === "zh-CN" ? chinese : english;
}

function friendlyImportProviderDescription(
  locale: Locale,
  kind: GuidedImportProviderKind,
): string {
  switch (kind) {
    case "1password_cli":
      return sectionCaption(
        locale,
        "Choose passwords from your 1Password account.",
        "从你的 1Password 账号中选择密码。",
      );
    case "bitwarden_cli":
      return sectionCaption(
        locale,
        "Choose passwords from your Bitwarden account.",
        "从你的 Bitwarden 账号中选择密码。",
      );
    case "dotenv_file":
      return sectionCaption(
        locale,
        "Choose keys from one or more .env files.",
        "从一个或多个 .env 文件中选择键。",
      );
  }
}

function friendlyImportProviderSteps(
  locale: Locale,
  kind: GuidedImportProviderKind,
): string {
  switch (kind) {
    case "1password_cli":
      return sectionCaption(
        locale,
        "Account → Vault → Passwords",
        "账号 → 保险库 → 密码",
      );
    case "bitwarden_cli":
      return sectionCaption(
        locale,
        "Account → Collection → Passwords",
        "账号 → 集合 → 密码",
      );
    case "dotenv_file":
      return sectionCaption(locale, "Files → Keys", "文件 → 键");
  }
}

function getImportedContainerLabel(
  reference: ImportedSecretReference,
): string | null {
  if (reference.provider_kind === "1password_cli") {
    return reference.vault;
  }

  if (reference.provider_kind === "bitwarden_cli") {
    return (
      reference.collection ??
      reference.folder ??
      reference.organization ??
      reference.account
    );
  }

  if (reference.provider_kind === "keepassxc_cli") {
    return reference.database;
  }

  return reference.namespace ?? reference.prefix ?? reference.file_path;
}

function getImportedFieldSelector(reference: ImportedSecretReference): string {
  if (reference.provider_kind === "dotenv_file") {
    return reference.key;
  }

  return reference.field;
}

function PickerSection(props: PickerSectionProps): JSX.Element {
  const visibleOptions = props.options.filter((option) =>
    matchesQuery(option, props.searchQuery),
  );

  return (
    <section
      aria-busy={props.loading}
      className="detail-section"
      data-testid={props.dataTestId}
    >
      <div className="detail-section-header">
        <h3>{props.title}</h3>
        {props.caption ? <span>{props.caption}</span> : null}
      </div>
      {props.onSearchQueryChange ? (
        <input
          className="settings-input picker-search"
          data-testid={`${props.dataTestId}-search`}
          onChange={(event) => {
            props.onSearchQueryChange?.(event.currentTarget.value);
          }}
          placeholder={props.searchPlaceholder}
          type="search"
          value={props.searchQuery ?? ""}
        />
      ) : null}
      <div
        className="queue-list picker-list"
        data-testid={`${props.dataTestId}-list`}
      >
        {props.loading ? (
          <p
            aria-live="polite"
            className="empty"
            data-testid={`${props.dataTestId}-loading`}
            role="status"
          >
            {props.emptyMessage}
          </p>
        ) : visibleOptions.length === 0 ? (
          <p className="empty" data-testid={`${props.dataTestId}-empty`}>
            {props.emptyMessage}
          </p>
        ) : (
          visibleOptions.map((option) => {
            const isActive = option.id === props.selectedId;
            return (
              <button
                aria-pressed={isActive ? "true" : "false"}
                className={`queue-item ${isActive ? "active" : ""}`}
                data-option-id={option.id}
                data-selected={isActive ? "true" : "false"}
                data-testid={`${props.dataTestId}-option`}
                key={option.id}
                onClick={() => {
                  props.onSelect(option.id);
                }}
                type="button"
              >
                <div className="queue-item-header">
                  <strong>{option.label}</strong>
                </div>
                {option.subtitle ? (
                  <div className="queue-item-meta">
                    <span>{option.subtitle}</span>
                  </div>
                ) : null}
              </button>
            );
          })
        )}
      </div>
    </section>
  );
}

function MultiPickerSection(props: MultiPickerSectionProps): JSX.Element {
  const visibleOptions = props.options.filter((option) =>
    matchesQuery(option, props.searchQuery),
  );
  const availableOptions = visibleOptions.filter(
    (option) => !props.selectedIds.includes(option.id),
  );
  const selectedOptions = visibleOptions.filter((option) =>
    props.selectedIds.includes(option.id),
  );

  function renderOption(
    option: PickerRenderableOption,
    selected: boolean,
  ): JSX.Element {
    return (
      <button
        aria-pressed={selected ? "true" : "false"}
        className={`queue-item transfer-option ${selected ? "active" : ""}`}
        data-option-id={option.id}
        data-selected={selected ? "true" : "false"}
        data-testid={`${props.dataTestId}-option`}
        key={option.id}
        onClick={() => props.onToggleSelect(option.id)}
        type="button"
      >
        <div className="queue-item-header">
          <strong>{option.label}</strong>
          <span aria-hidden="true">{selected ? "←" : "→"}</span>
        </div>
        {option.subtitle ? (
          <div className="queue-item-meta">
            <span>{option.subtitle}</span>
          </div>
        ) : null}
      </button>
    );
  }

  return (
    <section
      aria-busy={props.loading}
      className="detail-section"
      data-testid={props.dataTestId}
    >
      <div className="detail-section-header">
        <h3>{props.title}</h3>
        {props.caption ? <span>{props.caption}</span> : null}
      </div>
      {props.helper ? (
        <p className="section-copy" data-testid={`${props.dataTestId}-helper`}>
          {props.helper}
        </p>
      ) : null}
      {props.onSearchQueryChange ? (
        <input
          className="settings-input picker-search"
          data-testid={`${props.dataTestId}-search`}
          onChange={(event) => {
            props.onSearchQueryChange?.(event.currentTarget.value);
          }}
          placeholder={props.searchPlaceholder}
          type="search"
          value={props.searchQuery ?? ""}
        />
      ) : null}
      <div className="transfer-picker" data-testid={`${props.dataTestId}-list`}>
        <div className="transfer-column">
          <div className="transfer-column-heading">
            <strong>
              {sectionCaption(props.locale, "Available", "可选择")}
            </strong>
            <span>{availableOptions.length}</span>
          </div>
          <div className="queue-list picker-list transfer-list">
            {props.loading ? (
              <p
                aria-live="polite"
                className="empty"
                data-testid={`${props.dataTestId}-loading`}
                role="status"
              >
                {props.emptyMessage}
              </p>
            ) : availableOptions.length === 0 ? (
              <p className="empty" data-testid={`${props.dataTestId}-empty`}>
                {visibleOptions.length === 0
                  ? props.emptyMessage
                  : sectionCaption(
                      props.locale,
                      "Everything is selected",
                      "全部已选择",
                    )}
              </p>
            ) : (
              availableOptions.map((option) => renderOption(option, false))
            )}
          </div>
        </div>
        <div className="transfer-direction" aria-hidden="true">
          <span>→</span>
          <span>←</span>
        </div>
        <div className="transfer-column">
          <div className="transfer-column-heading">
            <strong>
              {sectionCaption(props.locale, "Selected", "已选择")}
            </strong>
            <span>{selectedOptions.length}</span>
          </div>
          <div className="queue-list picker-list transfer-list">
            {selectedOptions.length === 0 ? (
              <p className="empty">
                {sectionCaption(
                  props.locale,
                  "Choose from the left",
                  "从左侧选择",
                )}
              </p>
            ) : (
              selectedOptions.map((option) => renderOption(option, true))
            )}
          </div>
        </div>
      </div>
    </section>
  );
}

function LocatorField(props: LocatorFieldProps): JSX.Element {
  return (
    <label className="settings-field" data-testid={props.dataTestId}>
      <span className="field-label">
        {props.label}
        {props.optional ? (
          <span className="field-optional"> · {props.optionalLabel}</span>
        ) : null}
      </span>
      <input
        className="settings-input"
        disabled={props.disabled}
        onChange={(event) => {
          props.onChange(event.currentTarget.value);
        }}
        type="text"
        value={props.value}
      />
      {props.hint ? <span className="field-hint">{props.hint}</span> : null}
    </label>
  );
}

function OptionalDetails(props: {
  children: ReactNode;
  collapsed: boolean;
  summary: string;
}): JSX.Element {
  if (!props.collapsed) {
    return <>{props.children}</>;
  }

  return (
    <details className="password-import-optional-details">
      <summary>{props.summary}</summary>
      {props.children}
    </details>
  );
}

export function PasswordManagementView(
  props: PasswordManagementViewProps,
): JSX.Element {
  const surface = props.surface ?? "full";
  const [entryMode, setEntryMode] = useState<PasswordEntryMode>("manual");
  const [manualDraft, setManualDraft] = useState<ManualSecretDraft>(
    EMPTY_MANUAL_SECRET_DRAFT,
  );
  const [isSavingManual, setIsSavingManual] = useState(false);
  const [providerKind, setProviderKind] =
    useState<SecretImportProviderKind>("1password_cli");
  const [availableProviderKinds, setAvailableProviderKinds] = useState<
    SecretImportProviderKind[] | null
  >(
    surface === "import" ? null : PROVIDER_OPTIONS.map((option) => option.kind),
  );
  const [resourceTemplateMode, setResourceTemplateMode] =
    useState<ResourceTemplateMode>("default");
  const [resourceTemplate, setResourceTemplate] = useState("");
  const [resourceTemplateName, setResourceTemplateName] = useState("");
  const [selectedSavedResourceTemplateId, setSelectedSavedResourceTemplateId] =
    useState<string | null>(null);
  const [savedResourceTemplates, setSavedResourceTemplates] = useState<
    SavedResourceTemplate[]
  >(() => loadSavedResourceTemplates());
  const [commonDraft, setCommonDraft] =
    useState<CommonImportDraft>(EMPTY_COMMON_DRAFT);
  const [onePasswordDraft, setOnePasswordDraft] = useState<OnePasswordDraft>(
    EMPTY_ONEPASSWORD_DRAFT,
  );
  const [bitwardenDraft, setBitwardenDraft] = useState<BitwardenDraft>(
    EMPTY_BITWARDEN_DRAFT,
  );
  const [dotenvDraft, setDotenvDraft] =
    useState<DotenvDraft>(EMPTY_DOTENV_DRAFT);
  const [receipts, setReceipts] = useState<ImportedSecretReceipt[]>([]);
  const [importedCatalog, setImportedCatalog] =
    useState<LocalSecretCatalog | null>(null);
  const [browseErrorMessage, setBrowseErrorMessage] = useState<string | null>(
    null,
  );
  const [submitErrorMessage, setSubmitErrorMessage] = useState<string | null>(
    null,
  );
  const [noticeMessage, setNoticeMessage] = useState<string | null>(null);
  const [catalogErrorMessage, setCatalogErrorMessage] = useState<string | null>(
    null,
  );
  const [catalogNoticeMessage, setCatalogNoticeMessage] = useState<
    string | null
  >(null);
  const [isSubmitting, setIsSubmitting] = useState(false);
  const [isCatalogLoading, setIsCatalogLoading] = useState(false);

  const [onePasswordAccounts, setOnePasswordAccounts] = useState<
    ImportPickerOption[]
  >([]);
  const [onePasswordVaults, setOnePasswordVaults] = useState<
    ImportPickerOption[]
  >([]);
  const [onePasswordItems, setOnePasswordItems] = useState<
    ImportPickerOption[]
  >([]);
  const [onePasswordFieldsByItemId, setOnePasswordFieldsByItemId] =
    useState<FieldOptionsByResourceId>({});
  const [selectedOnePasswordAccountId, setSelectedOnePasswordAccountId] =
    useState<string | null>(null);
  const [selectedOnePasswordVaultId, setSelectedOnePasswordVaultId] = useState<
    string | null
  >(null);
  const [selectedOnePasswordItemId, setSelectedOnePasswordItemId] = useState<
    string | null
  >(null);
  const [selectedOnePasswordItemIds, setSelectedOnePasswordItemIds] = useState<
    string[]
  >([]);
  const [selectedOnePasswordFieldIds, setSelectedOnePasswordFieldIds] =
    useState<string[]>([]);
  const [onePasswordItemQuery, setOnePasswordItemQuery] = useState("");
  const [isOnePasswordAccountsLoading, setIsOnePasswordAccountsLoading] =
    useState(false);
  const [isOnePasswordVaultsLoading, setIsOnePasswordVaultsLoading] =
    useState(false);
  const [isOnePasswordItemsLoading, setIsOnePasswordItemsLoading] =
    useState(false);
  const [isOnePasswordFieldsLoading, setIsOnePasswordFieldsLoading] =
    useState(false);

  const [bitwardenAccounts, setBitwardenAccounts] = useState<
    ImportPickerOption[]
  >([]);
  const [bitwardenContainers, setBitwardenContainers] = useState<
    BitwardenContainerOption[]
  >([]);
  const [bitwardenItems, setBitwardenItems] = useState<ImportPickerOption[]>(
    [],
  );
  const [bitwardenFieldsByItemId, setBitwardenFieldsByItemId] =
    useState<FieldOptionsByResourceId>({});
  const [selectedBitwardenAccountId, setSelectedBitwardenAccountId] = useState<
    string | null
  >(null);
  const [selectedBitwardenContainerId, setSelectedBitwardenContainerId] =
    useState<string | null>("all");
  const [selectedBitwardenItemId, setSelectedBitwardenItemId] = useState<
    string | null
  >(null);
  const [selectedBitwardenItemIds, setSelectedBitwardenItemIds] = useState<
    string[]
  >([]);
  const [selectedBitwardenFieldIds, setSelectedBitwardenFieldIds] = useState<
    string[]
  >([]);
  const [bitwardenItemQuery, setBitwardenItemQuery] = useState("");
  const [isBitwardenAccountsLoading, setIsBitwardenAccountsLoading] =
    useState(false);
  const [isBitwardenContainersLoading, setIsBitwardenContainersLoading] =
    useState(false);
  const [isBitwardenItemsLoading, setIsBitwardenItemsLoading] = useState(false);
  const [isBitwardenFieldsLoading, setIsBitwardenFieldsLoading] =
    useState(false);

  const [dotenvFilePaths, setDotenvFilePaths] = useState<string[]>([]);
  const [dotenvInspections, setDotenvInspections] = useState<
    DotenvInspection[]
  >([]);
  const [selectedDotenvGroupId, setSelectedDotenvGroupId] = useState<
    string | null
  >("all");
  const [selectedDotenvKeys, setSelectedDotenvKeys] = useState<string[]>([]);
  const [dotenvKeyQuery, setDotenvKeyQuery] = useState("");
  const [isDotenvPicking, setIsDotenvPicking] = useState(false);
  const [isDotenvInspecting, setIsDotenvInspecting] = useState(false);

  const availableProviderOptions = PROVIDER_OPTIONS.filter((option) =>
    availableProviderKinds?.includes(option.kind),
  );
  const selectedProvider = availableProviderOptions.find(
    (option) => option.kind === providerKind,
  );
  const visibleSavedResourceTemplates = savedResourceTemplates.filter(
    (template) => template.providerKind === providerKind,
  );
  const selectedOnePasswordItem =
    onePasswordItems.find(
      (option) => option.id === selectedOnePasswordItemId,
    ) ?? null;
  const onePasswordFields: ImportFieldOption[] = selectedOnePasswordItemId
    ? (onePasswordFieldsByItemId[selectedOnePasswordItemId] ?? [])
    : [];
  const selectedBitwardenItem =
    bitwardenItems.find((option) => option.id === selectedBitwardenItemId) ??
    null;
  const bitwardenFields: ImportFieldOption[] = selectedBitwardenItemId
    ? (bitwardenFieldsByItemId[selectedBitwardenItemId] ?? [])
    : [];
  const primaryDotenvInspection = dotenvInspections.at(0) ?? null;
  const dotenvSelectableKeys: DotenvSelectableKey[] = dotenvInspections.flatMap(
    (inspection) => {
      const multipleFiles = dotenvInspections.length > 1;
      const requiredGroupId = multipleFiles
        ? "all"
        : (selectedDotenvGroupId ?? "all");
      const fileLabel = displayFileName(inspection.file_path);
      return inspection.keys
        .filter((option) => option.group_id === requiredGroupId)
        .map((option) => {
          const group =
            inspection.groups.find(
              (candidate) => candidate.id === option.group_id,
            ) ?? null;
          const baseId = dotenvKeySelectionId(option);
          return {
            id: multipleFiles
              ? `${encodeURIComponent(inspection.file_path)}::${baseId}`
              : baseId,
            filePath: inspection.file_path,
            fileLabel,
            group,
            option,
          };
        });
    },
  );
  const visibleDotenvKeys = dotenvSelectableKeys.filter((entry) =>
    matchesQuery(
      {
        id: entry.id,
        label: entry.option.label,
        subtitle: `${entry.fileLabel} ${entry.option.full_key}`,
      },
      dotenvKeyQuery,
    ),
  );
  const selectedOnePasswordItems = onePasswordItems.filter((option) =>
    selectedOnePasswordItemIds.includes(option.id),
  );
  const selectedOnePasswordFields = onePasswordFields.filter((field) =>
    selectedOnePasswordFieldIds.includes(fieldOptionId(field)),
  );
  const isOnePasswordMultiResourceMode = selectedOnePasswordItemIds.length > 1;
  const areOnePasswordSelectedFieldsReady =
    selectedOnePasswordItems.length > 0 &&
    selectedOnePasswordItems.every((item) =>
      hasCachedFieldOptions(onePasswordFieldsByItemId, item.id),
    );
  const selectedBitwardenItems = bitwardenItems.filter((option) =>
    selectedBitwardenItemIds.includes(option.id),
  );
  const selectedBitwardenFields = bitwardenFields.filter((field) =>
    selectedBitwardenFieldIds.includes(fieldOptionId(field)),
  );
  const isBitwardenMultiResourceMode = selectedBitwardenItemIds.length > 1;
  const areBitwardenSelectedFieldsReady =
    selectedBitwardenItems.length > 0 &&
    selectedBitwardenItems.every((item) =>
      hasCachedFieldOptions(bitwardenFieldsByItemId, item.id),
    );
  const selectedDotenvKeyOptions = dotenvSelectableKeys.filter((entry) =>
    selectedDotenvKeys.includes(entry.id),
  );
  const explicitResource = commonDraft.resource.trim();
  const sharedResourceTemplate = batchResourceTemplateForSubmit(
    resourceTemplateMode,
    resourceTemplate,
    explicitResource,
  );
  const metadataDraft = parseMetadataDraft(commonDraft.metadata);
  const manualItemSegment = normalizeResourceSegment(manualDraft.title);
  const manualFieldSegment = normalizeResourceSegment(manualDraft.fieldLabel);
  const generatedManualResource =
    manualItemSegment && manualFieldSegment
      ? `plankton://field/${manualItemSegment}/${manualFieldSegment}`
      : "";
  const resolvedManualResource =
    manualDraft.resource.trim() || generatedManualResource;
  const canSaveManual =
    manualDraft.title.trim().length > 0 &&
    manualDraft.value.length > 0 &&
    manualDraft.fieldLabel.trim().length > 0 &&
    resolvedManualResource.length > 0;

  let plannedSpecs: SecretImportSpec[] = [];
  let planBlockerMessage: string | null = null;

  if (providerKind === "1password_cli") {
    if (onePasswordDraft.account.trim().length === 0) {
      planBlockerMessage = sectionCaption(
        props.locale,
        "Select a 1Password account first.",
        "请先选择 1Password 账号。",
      );
    } else if (onePasswordDraft.vault.trim().length === 0) {
      planBlockerMessage = sectionCaption(
        props.locale,
        "Select a vault before importing.",
        "请先选择 vault 再导入。",
      );
    } else if (selectedOnePasswordItems.length === 0) {
      planBlockerMessage = sectionCaption(
        props.locale,
        "Select at least one item before importing.",
        "请至少选择一个条目再导入。",
      );
    } else if (
      isOnePasswordMultiResourceMode &&
      !areOnePasswordSelectedFieldsReady
    ) {
      planBlockerMessage = sectionCaption(
        props.locale,
        "Waiting for 1Password fields to finish loading for the selected items.",
        "正在等待所选 1Password 条目的字段加载完成。",
      );
    } else if (
      !isOnePasswordMultiResourceMode &&
      selectedOnePasswordFields.length === 0 &&
      onePasswordDraft.field.trim().length === 0
    ) {
      planBlockerMessage = sectionCaption(
        props.locale,
        "Select a field, or enter a fallback field selector before importing.",
        "导入前请先选择字段，或者手动填写一个兜底字段选择器。",
      );
    }
  } else if (providerKind === "bitwarden_cli") {
    if (bitwardenDraft.account.trim().length === 0) {
      planBlockerMessage = sectionCaption(
        props.locale,
        "Select a Bitwarden account first.",
        "请先选择 Bitwarden 账号。",
      );
    } else if (selectedBitwardenItems.length === 0) {
      planBlockerMessage = sectionCaption(
        props.locale,
        "Select at least one item before importing.",
        "请至少选择一个条目再导入。",
      );
    } else if (
      isBitwardenMultiResourceMode &&
      !areBitwardenSelectedFieldsReady
    ) {
      planBlockerMessage = sectionCaption(
        props.locale,
        "Waiting for Bitwarden fields to finish loading for the selected items.",
        "正在等待所选 Bitwarden 条目的字段加载完成。",
      );
    } else if (
      !isBitwardenMultiResourceMode &&
      selectedBitwardenFields.length === 0 &&
      bitwardenDraft.field.trim().length === 0
    ) {
      planBlockerMessage = sectionCaption(
        props.locale,
        "Select a field, or enter a fallback field selector before importing.",
        "导入前请先选择字段，或者手动填写一个兜底字段选择器。",
      );
    }
  } else if (dotenvFilePaths.length === 0) {
    planBlockerMessage = sectionCaption(
      props.locale,
      "Choose a dotenv file before importing.",
      "导入前请先选择 dotenv 文件。",
    );
  } else if (selectedDotenvKeyOptions.length === 0) {
    planBlockerMessage = sectionCaption(
      props.locale,
      "Select at least one dotenv key before importing.",
      "导入前请至少选择一个 dotenv key。",
    );
  }

  if (
    !planBlockerMessage &&
    resourceTemplateMode === "custom" &&
    commonDraft.resource.trim().length === 0 &&
    resourceTemplate.trim().length === 0
  ) {
    planBlockerMessage = sectionCaption(
      props.locale,
      "Custom resource templates cannot be empty.",
      "自定义资源模板不能为空。",
    );
  } else if (!planBlockerMessage && metadataDraft.invalidLines.length > 0) {
    planBlockerMessage = sectionCaption(
      props.locale,
      `Metadata must use KEY=VALUE lines: ${metadataDraft.invalidLines.join(", ")}`,
      `元信息必须使用 KEY=VALUE 格式：${metadataDraft.invalidLines.join("、")}`,
    );
  } else if (!planBlockerMessage && providerKind === "1password_cli") {
    if (
      onePasswordDraft.account.trim().length > 0 &&
      onePasswordDraft.vault.trim().length > 0 &&
      selectedOnePasswordItems.length > 0
    ) {
      const manualResource = commonDraft.resource.trim();
      const manualDisplayName = optionalValue(commonDraft.displayName);
      const description = optionalValue(commonDraft.description);
      const tags = parseTags(commonDraft.tags);
      const metadata = metadataDraft.metadata;
      const isSingleImport =
        selectedOnePasswordItems.length === 1 &&
        selectedOnePasswordFields.length === 1;

      if (isOnePasswordMultiResourceMode) {
        if (areOnePasswordSelectedFieldsReady) {
          plannedSpecs = selectedOnePasswordItems.flatMap((item) =>
            (onePasswordFieldsByItemId[item.id] ?? []).map((field) => {
              return {
                resource: "",
                display_name: null,
                description,
                tags,
                metadata,
                source_locator: {
                  provider_kind: "1password_cli",
                  account: onePasswordDraft.account.trim(),
                  account_id: optionalValue(onePasswordDraft.accountId),
                  vault: onePasswordDraft.vault.trim(),
                  vault_id: optionalValue(onePasswordDraft.vaultId),
                  item: item.label,
                  item_id: item.id,
                  field: field.selector,
                  field_id: optionalValue(field.field_id ?? ""),
                },
              } satisfies SecretImportSpec;
            }),
          );
        }
      } else if (selectedOnePasswordFields.length > 0) {
        plannedSpecs = selectedOnePasswordItems.flatMap((item) =>
          selectedOnePasswordFields.map((field) => {
            return {
              resource:
                manualResource.length > 0 && isSingleImport
                  ? manualResource
                  : "",
              display_name: isSingleImport ? manualDisplayName : null,
              description,
              tags,
              metadata,
              source_locator: {
                provider_kind: "1password_cli",
                account: onePasswordDraft.account.trim(),
                account_id: optionalValue(onePasswordDraft.accountId),
                vault: onePasswordDraft.vault.trim(),
                vault_id: optionalValue(onePasswordDraft.vaultId),
                item: item.label,
                item_id: item.id,
                field: field.selector,
                field_id: optionalValue(field.field_id ?? ""),
              },
            } satisfies SecretImportSpec;
          }),
        );
      }
    }
  } else if (!planBlockerMessage && providerKind === "bitwarden_cli") {
    if (
      bitwardenDraft.account.trim().length > 0 &&
      selectedBitwardenItems.length > 0
    ) {
      const manualResource = commonDraft.resource.trim();
      const manualDisplayName = optionalValue(commonDraft.displayName);
      const description = optionalValue(commonDraft.description);
      const tags = parseTags(commonDraft.tags);
      const metadata = metadataDraft.metadata;
      const isSingleImport =
        selectedBitwardenItems.length === 1 &&
        selectedBitwardenFields.length === 1;

      if (isBitwardenMultiResourceMode) {
        if (areBitwardenSelectedFieldsReady) {
          plannedSpecs = selectedBitwardenItems.flatMap((item) =>
            (bitwardenFieldsByItemId[item.id] ?? []).map((field) => {
              return {
                resource: "",
                display_name: null,
                description,
                tags,
                metadata,
                source_locator: {
                  provider_kind: "bitwarden_cli",
                  account: bitwardenDraft.account.trim(),
                  organization: optionalValue(bitwardenDraft.organization),
                  collection: optionalValue(bitwardenDraft.collection),
                  folder: optionalValue(bitwardenDraft.folder),
                  item: item.label,
                  item_id: item.id,
                  field: field.selector,
                },
              } satisfies SecretImportSpec;
            }),
          );
        }
      } else if (selectedBitwardenFields.length > 0) {
        plannedSpecs = selectedBitwardenItems.flatMap((item) =>
          selectedBitwardenFields.map((field) => {
            return {
              resource:
                manualResource.length > 0 && isSingleImport
                  ? manualResource
                  : "",
              display_name: isSingleImport ? manualDisplayName : null,
              description,
              tags,
              metadata,
              source_locator: {
                provider_kind: "bitwarden_cli",
                account: bitwardenDraft.account.trim(),
                organization: optionalValue(bitwardenDraft.organization),
                collection: optionalValue(bitwardenDraft.collection),
                folder: optionalValue(bitwardenDraft.folder),
                item: item.label,
                item_id: item.id,
                field: field.selector,
              },
            } satisfies SecretImportSpec;
          }),
        );
      }
    }
  } else if (
    !planBlockerMessage &&
    dotenvFilePaths.length > 0 &&
    selectedDotenvKeyOptions.length > 0
  ) {
    const manualResource = commonDraft.resource.trim();
    const manualDisplayName = optionalValue(commonDraft.displayName);
    const description = optionalValue(commonDraft.description);
    const tags = parseTags(commonDraft.tags);
    const metadata = metadataDraft.metadata;
    const isSingleImport = selectedDotenvKeyOptions.length === 1;

    plannedSpecs = selectedDotenvKeyOptions.map((entry) => {
      const { group, option } = entry;
      const resolvedKey =
        group?.prefix && option.group_id !== "all"
          ? option.label
          : option.full_key;

      return {
        resource:
          manualResource.length > 0 && isSingleImport ? manualResource : "",
        display_name: isSingleImport ? manualDisplayName : null,
        description,
        tags,
        metadata,
        source_locator: {
          provider_kind: "dotenv_file",
          file_path: entry.filePath,
          namespace: optionalValue(group?.namespace ?? ""),
          prefix: optionalValue(group?.prefix ?? ""),
          key: resolvedKey,
        },
      } satisfies SecretImportSpec;
    });
  }

  const plannedPreviewEntries = plannedSpecs.map((spec) => ({
    spec,
    ...previewResourceForImport(spec, sharedResourceTemplate),
  }));

  if (!planBlockerMessage) {
    const missingTokens = uniqueValues(
      plannedPreviewEntries.flatMap((entry) => entry.missingTokens),
    );

    if (missingTokens.length > 0) {
      planBlockerMessage = sectionCaption(
        props.locale,
        `Template uses unsupported placeholders: ${missingTokens.join(", ")}`,
        `模板包含不支持的占位符：${missingTokens.join("、")}`,
      );
    }
  }

  if (!planBlockerMessage) {
    const invalidPreview = plannedPreviewEntries.find(
      (entry) => entry.resource === null,
    );

    if (invalidPreview) {
      planBlockerMessage = sectionCaption(
        props.locale,
        "The current template does not produce a valid resource id.",
        "当前模板没有生成有效的资源标识。",
      );
    }
  }

  if (!planBlockerMessage) {
    const duplicates = plannedPreviewEntries.reduce<Record<string, number>>(
      (counts, entry) => {
        const resource = entry.resource ?? "";
        counts[resource] = (counts[resource] ?? 0) + 1;
        return counts;
      },
      {},
    );
    const duplicateResource = Object.keys(duplicates).find(
      (resource) => duplicates[resource] > 1,
    );

    if (duplicateResource) {
      planBlockerMessage = sectionCaption(
        props.locale,
        `Resource template produced duplicate ids: ${duplicateResource}`,
        `资源模板生成了重复资源标识：${duplicateResource}`,
      );
    }
  }

  const previewResources = plannedPreviewEntries
    .map((entry) => entry.resource)
    .filter((resource): resource is string => resource !== null);
  const previewEmptyMessage =
    providerKind === "1password_cli" && isOnePasswordMultiResourceMode
      ? isOnePasswordFieldsLoading || !areOnePasswordSelectedFieldsReady
        ? sectionCaption(
            props.locale,
            "Loading fields for the selected resources.",
            "正在加载所选资源的字段。",
          )
        : sectionCaption(
            props.locale,
            "No importable fields were found for the selected resources.",
            "所选资源没有可导入的字段。",
          )
      : providerKind === "bitwarden_cli" && isBitwardenMultiResourceMode
        ? isBitwardenFieldsLoading || !areBitwardenSelectedFieldsReady
          ? sectionCaption(
              props.locale,
              "Loading fields for the selected resources.",
              "正在加载所选资源的字段。",
            )
          : sectionCaption(
              props.locale,
              "No importable fields were found for the selected resources.",
              "所选资源没有可导入的字段。",
            )
        : sectionCaption(
            props.locale,
            "Select resources and fields to preview generated ids.",
            "先选择资源和字段，再预览生成后的资源标识。",
          );
  const isBatchMode =
    plannedSpecs.length > 1 ||
    selectedOnePasswordItemIds.length > 1 ||
    selectedOnePasswordFieldIds.length > 1 ||
    selectedBitwardenItemIds.length > 1 ||
    selectedBitwardenFieldIds.length > 1 ||
    selectedDotenvKeys.length > 1;
  const canSubmit = plannedSpecs.length > 0 && planBlockerMessage === null;
  const importedReceipts = receipts;

  function resetFeedback(): void {
    setBrowseErrorMessage(null);
    setSubmitErrorMessage(null);
    setNoticeMessage(null);
    setReceipts([]);
  }

  function suggestDisplayName(nextDisplayName: string): void {
    setCommonDraft((current) => {
      if (current.displayName.trim().length > 0) {
        return current;
      }

      return {
        ...current,
        displayName: nextDisplayName,
      };
    });
  }

  function saveCurrentResourceTemplate(): void {
    const trimmedTemplate = resourceTemplate.trim();
    const trimmedName = resourceTemplateName.trim();

    if (trimmedTemplate.length === 0 || trimmedName.length === 0) {
      setNoticeMessage(null);
      setSubmitErrorMessage(
        sectionCaption(
          props.locale,
          "Saved templates require both a name and a custom template value.",
          "保存模板时必须同时填写名称和自定义模板内容。",
        ),
      );
      return;
    }

    const now = new Date().toISOString();
    const nextTemplates = [...savedResourceTemplates];
    const existingIndex = nextTemplates.findIndex(
      (template) =>
        template.providerKind === providerKind && template.name === trimmedName,
    );

    const nextTemplate: SavedResourceTemplate = {
      id:
        existingIndex >= 0
          ? nextTemplates[existingIndex].id
          : `${providerKind}:${trimmedName}:${now}`,
      name: trimmedName,
      providerKind,
      template: trimmedTemplate,
      createdAt:
        existingIndex >= 0 ? nextTemplates[existingIndex].createdAt : now,
      updatedAt: now,
    };

    if (existingIndex >= 0) {
      nextTemplates[existingIndex] = nextTemplate;
    } else {
      nextTemplates.push(nextTemplate);
    }

    persistSavedResourceTemplates(nextTemplates);
    setSavedResourceTemplates(nextTemplates);
    setSelectedSavedResourceTemplateId(nextTemplate.id);
    setSubmitErrorMessage(null);
    setNoticeMessage(
      sectionCaption(
        props.locale,
        `Saved template ${trimmedName}`,
        `已保存模板 ${trimmedName}`,
      ),
    );
  }

  function applySavedResourceTemplate(templateId: string): void {
    const nextTemplate =
      savedResourceTemplates.find((template) => template.id === templateId) ??
      null;
    if (!nextTemplate) {
      return;
    }

    setResourceTemplateMode("custom");
    setResourceTemplate(nextTemplate.template);
    setResourceTemplateName(nextTemplate.name);
    setSelectedSavedResourceTemplateId(nextTemplate.id);
    setSubmitErrorMessage(null);
    setNoticeMessage(
      sectionCaption(
        props.locale,
        `Loaded template ${nextTemplate.name}`,
        `已加载模板 ${nextTemplate.name}`,
      ),
    );
  }

  function deleteSavedResourceTemplate(templateId: string): void {
    const nextTemplates = savedResourceTemplates.filter(
      (template) => template.id !== templateId,
    );
    persistSavedResourceTemplates(nextTemplates);
    setSavedResourceTemplates(nextTemplates);
    if (selectedSavedResourceTemplateId === templateId) {
      setSelectedSavedResourceTemplateId(null);
    }
    setNoticeMessage(
      sectionCaption(
        props.locale,
        "Deleted saved template",
        "已删除保存的模板",
      ),
    );
  }

  async function loadImportedCatalog(options?: {
    silent?: boolean;
  }): Promise<void> {
    if (!options?.silent) {
      setIsCatalogLoading(true);
    }
    setCatalogErrorMessage(null);

    try {
      const nextCatalog = await invoke<LocalSecretCatalog>(
        "list_secret_catalog_metadata",
      );
      setImportedCatalog(nextCatalog);
    } catch (error) {
      setCatalogErrorMessage(
        error instanceof Error ? error.message : String(error),
      );
    } finally {
      if (!options?.silent) {
        setIsCatalogLoading(false);
      }
    }
  }

  async function notifyCatalogChange(): Promise<void> {
    await props.onCatalogChange?.();
  }

  function handleBrowseError(error: unknown): void {
    setBrowseErrorMessage(formatUserFacingImportError(props.locale, error));
  }

  async function saveImportedSecret(
    update: ImportedSecretReferenceUpdate,
  ): Promise<void> {
    setCatalogErrorMessage(null);

    try {
      const receipt = await invoke<ImportedSecretReceipt>(
        "update_imported_secret_source",
        {
          update,
        },
      );
      setCatalogNoticeMessage(
        sectionCaption(
          props.locale,
          `Saved metadata for ${receipt.reference.resource}`,
          `已保存 ${receipt.reference.resource} 的元信息`,
        ),
      );
      await loadImportedCatalog({ silent: true });
      await notifyCatalogChange();
    } catch (error) {
      const message = error instanceof Error ? error.message : String(error);
      setCatalogErrorMessage(message);
      throw error;
    }
  }

  async function revealSecret(resource: string): Promise<string> {
    return invoke<string>("resolve_human_secret", { resource });
  }

  async function refreshImportedSecret(resource: string): Promise<void> {
    setCatalogErrorMessage(null);

    try {
      const receipt = await invoke<ImportedSecretReceipt>(
        "refresh_imported_secret_source",
        {
          resource,
        },
      );
      setCatalogNoticeMessage(
        sectionCaption(
          props.locale,
          `Updated ${receipt.reference.resource} from upstream source`,
          `已从上游更新 ${receipt.reference.resource}`,
        ),
      );
      await loadImportedCatalog({ silent: true });
      await notifyCatalogChange();
    } catch (error) {
      const message = error instanceof Error ? error.message : String(error);
      setCatalogErrorMessage(message);
      throw error;
    }
  }

  async function saveLocalSecret(
    entry: LocalSecretLiteralUpsert,
  ): Promise<void> {
    setCatalogErrorMessage(null);

    try {
      await invoke("upsert_local_secret_literal_command", {
        entry,
      });
      setCatalogNoticeMessage(
        sectionCaption(
          props.locale,
          `Saved local secret ${entry.resource}`,
          `已保存本地密钥 ${entry.resource}`,
        ),
      );
      await loadImportedCatalog({ silent: true });
      await notifyCatalogChange();
    } catch (error) {
      const message = error instanceof Error ? error.message : String(error);
      setCatalogErrorMessage(message);
      throw error;
    }
  }

  async function submitManualSecret(): Promise<void> {
    if (!canSaveManual) {
      return;
    }

    setIsSavingManual(true);
    setSubmitErrorMessage(null);
    setNoticeMessage(null);
    try {
      const draft = await invoke<{ draft_id: string }>(
        "create_password_draft_command",
        {
          input: {
            descriptor: {
              kind: "environment",
              names: [manualFieldSegment ?? "value"],
            },
            entries: [
              {
                key: manualFieldSegment ?? "value",
                value: manualDraft.value,
              },
            ],
            suggested_item_title: manualDraft.title.trim(),
            suggested_destination: null,
            suggested_layout: {
              description: optionalValue(manualDraft.description),
              tags: parseTags(manualDraft.tags),
              field_labels: {
                [manualFieldSegment ?? "value"]: manualDraft.fieldLabel.trim(),
              },
              field_resources: {
                [manualFieldSegment ?? "value"]: resolvedManualResource,
              },
            },
          },
        },
      );
      setManualDraft(EMPTY_MANUAL_SECRET_DRAFT);
      props.onDraftCreated?.(draft.draft_id);
      if (!props.onDraftCreated) {
        setNoticeMessage(
          sectionCaption(
            props.locale,
            "Password draft created. Open the password vault to choose its destination.",
            "密码草稿已创建。请打开密码库选择保存位置。",
          ),
        );
      }
    } catch {
      setSubmitErrorMessage(
        sectionCaption(
          props.locale,
          "This password could not be saved. Check Diagnostics and try again.",
          "无法保存此密码。请查看诊断信息后重试。",
        ),
      );
    } finally {
      setIsSavingManual(false);
    }
  }

  async function deleteImportedSecret(resource: string): Promise<void> {
    setCatalogErrorMessage(null);

    try {
      const deleted = await invoke<boolean>(
        "delete_local_secret_entry_command",
        {
          resource,
        },
      );
      if (!deleted) {
        throw new Error(
          sectionCaption(
            props.locale,
            `Secret entry was not found: ${resource}`,
            `未找到密钥条目：${resource}`,
          ),
        );
      }
      setCatalogNoticeMessage(
        sectionCaption(
          props.locale,
          `Deleted ${resource}`,
          `已删除 ${resource}`,
        ),
      );
      await loadImportedCatalog({ silent: true });
      await notifyCatalogChange();
    } catch (error) {
      const message = error instanceof Error ? error.message : String(error);
      setCatalogErrorMessage(message);
      throw error;
    }
  }

  async function renameSecret(
    resource: string,
    nextResource: string,
  ): Promise<void> {
    setCatalogErrorMessage(null);
    try {
      await invoke<boolean>("rename_local_secret_entry_command", {
        resource,
        nextResource,
      });
      setCatalogNoticeMessage(
        sectionCaption(
          props.locale,
          `Moved ${resource} to ${nextResource}`,
          `已将 ${resource} 移动到 ${nextResource}`,
        ),
      );
      await loadImportedCatalog({ silent: true });
      await notifyCatalogChange();
    } catch (error) {
      const message = error instanceof Error ? error.message : String(error);
      setCatalogErrorMessage(message);
      throw error;
    }
  }

  async function submitImport(): Promise<void> {
    setIsSubmitting(true);
    setSubmitErrorMessage(null);
    setNoticeMessage(null);

    console.info("[password-import] submit start", {
      providerKind,
      plannedSpecCount: plannedSpecs.length,
      resourceTemplate: sharedResourceTemplate,
      resources: plannedSpecs.map((spec) => spec.resource || "<generated>"),
    });

    try {
      const nextBatchReceipt = await invoke<ImportedSecretBatchReceipt>(
        "import_secret_sources",
        {
          spec: importBatchPayload(sharedResourceTemplate, plannedSpecs),
        },
      );
      const nextReceipts = nextBatchReceipt.receipts;

      setReceipts(nextReceipts);
      await loadImportedCatalog({ silent: true });
      await notifyCatalogChange();
      setNoticeMessage(
        nextReceipts.length === 1
          ? t(props.locale, "importSourceSuccess", {
              resource: nextReceipts[0].reference.resource,
            })
          : sectionCaption(
              props.locale,
              `Imported ${nextReceipts.length} resources`,
              `已导入 ${nextReceipts.length} 个资源`,
            ),
      );
      console.info("[password-import] submit success", {
        providerKind,
        importedCount: nextReceipts.length,
        resources: nextReceipts.map((entry) => entry.reference.resource),
      });
    } catch (error) {
      console.error("[password-import] submit failed", {
        providerKind,
        error,
      });
      setSubmitErrorMessage(formatUserFacingImportError(props.locale, error));
    } finally {
      setIsSubmitting(false);
    }
  }

  function selectOnePasswordAccount(nextAccountId: string): void {
    applyOnePasswordAccountSelection(nextAccountId, onePasswordAccounts);
  }

  function applyOnePasswordAccountSelection(
    nextAccountId: string,
    options: ImportPickerOption[],
  ): void {
    if (nextAccountId === selectedOnePasswordAccountId) {
      return;
    }

    const nextAccount = optionById(options, nextAccountId);
    setSelectedOnePasswordAccountId(nextAccountId);
    setSelectedOnePasswordVaultId(null);
    setSelectedOnePasswordItemId(null);
    setSelectedOnePasswordItemIds([]);
    setSelectedOnePasswordFieldIds([]);
    setOnePasswordVaults([]);
    setOnePasswordItems([]);
    setOnePasswordFieldsByItemId({});
    setOnePasswordItemQuery("");
    setOnePasswordDraft((current) => ({
      ...current,
      account: nextAccount?.label ?? current.account,
      accountId: nextAccount?.id ?? current.accountId,
      vault: "",
      item: "",
      field: "",
      vaultId: "",
      itemId: "",
      fieldId: "",
    }));
  }

  function selectOnePasswordVault(nextVaultId: string): void {
    applyOnePasswordVaultSelection(nextVaultId, onePasswordVaults);
  }

  function applyOnePasswordVaultSelection(
    nextVaultId: string,
    options: ImportPickerOption[],
  ): void {
    if (nextVaultId === selectedOnePasswordVaultId) {
      return;
    }

    const nextVault = optionById(options, nextVaultId);
    setSelectedOnePasswordVaultId(nextVaultId);
    setSelectedOnePasswordItemId(null);
    setSelectedOnePasswordItemIds([]);
    setSelectedOnePasswordFieldIds([]);
    setOnePasswordItems([]);
    setOnePasswordFieldsByItemId({});
    setOnePasswordItemQuery("");
    setOnePasswordDraft((current) => ({
      ...current,
      vault: nextVault?.label ?? current.vault,
      vaultId: nextVault?.id ?? current.vaultId,
      item: "",
      field: "",
      itemId: "",
      fieldId: "",
    }));
  }

  function toggleOnePasswordItem(nextItemId: string): void {
    applyOnePasswordItemSelection(nextItemId, onePasswordItems);
  }

  function applyOnePasswordItemSelection(
    nextItemId: string,
    options: ImportPickerOption[],
  ): void {
    const nextSelectedIds = toggleSelection(
      selectedOnePasswordItemIds,
      nextItemId,
      "multi",
    );
    const nextPrimaryId = nextSelectedIds.at(-1) ?? null;
    const nextItem = optionById(options, nextPrimaryId);

    setSelectedOnePasswordItemIds(nextSelectedIds);
    setSelectedOnePasswordItemId(nextPrimaryId);
    setSelectedOnePasswordFieldIds([]);
    setOnePasswordDraft((current) => ({
      ...current,
      item: nextItem?.label ?? "",
      itemId: nextItem?.id ?? "",
      field: "",
      fieldId: "",
    }));
    if (nextItem) {
      suggestDisplayName(nextItem.label);
    }
  }

  function toggleOnePasswordField(nextFieldId: string): void {
    applyOnePasswordFieldSelection(nextFieldId, onePasswordFields);
  }

  function applyOnePasswordFieldSelection(
    nextFieldId: string,
    options: ImportFieldOption[],
  ): void {
    const selectionMode =
      selectedOnePasswordItemIds.length > 1 ? "single" : "multi";
    const nextSelectedIds = toggleSelection(
      selectedOnePasswordFieldIds,
      nextFieldId,
      selectionMode,
    );
    const nextPrimaryId = nextSelectedIds.at(0) ?? null;
    const nextField =
      options.find((field) => fieldOptionId(field) === nextPrimaryId) ?? null;

    setSelectedOnePasswordFieldIds(nextSelectedIds);
    setOnePasswordDraft((current) => ({
      ...current,
      field: nextField?.selector ?? "",
      fieldId:
        selectedOnePasswordItemIds.length === 1
          ? (nextField?.field_id ?? "")
          : "",
    }));
    if (nextField && selectedOnePasswordItem) {
      suggestDisplayName(`${selectedOnePasswordItem.label}:${nextField.label}`);
    }
  }

  function selectBitwardenAccount(nextAccountId: string): void {
    applyBitwardenAccountSelection(nextAccountId, bitwardenAccounts);
  }

  function applyBitwardenAccountSelection(
    nextAccountId: string,
    options: ImportPickerOption[],
  ): void {
    const nextAccount = optionById(options, nextAccountId);
    setSelectedBitwardenAccountId(nextAccountId);
    setBitwardenDraft((current) => ({
      ...current,
      account: nextAccount?.label ?? current.account,
      organization: "",
      collection: "",
      folder: "",
      item: "",
      field: "",
      itemId: "",
    }));
    setSelectedBitwardenContainerId("all");
    setSelectedBitwardenItemId(null);
    setSelectedBitwardenItemIds([]);
    setSelectedBitwardenFieldIds([]);
    setBitwardenFieldsByItemId({});
  }

  function selectBitwardenContainer(nextContainerId: string): void {
    const nextContainer =
      bitwardenContainers.find((option) => option.id === nextContainerId) ??
      null;
    setSelectedBitwardenContainerId(nextContainerId);
    setSelectedBitwardenItemId(null);
    setSelectedBitwardenItemIds([]);
    setSelectedBitwardenFieldIds([]);
    setBitwardenFieldsByItemId({});
    setBitwardenDraft((current) => ({
      ...current,
      organization:
        nextContainer?.kind === "organization"
          ? nextContainer.label
          : (nextContainer?.organization_label ?? ""),
      collection:
        nextContainer?.kind === "collection" ? nextContainer.label : "",
      folder: nextContainer?.kind === "folder" ? nextContainer.label : "",
      item: "",
      field: "",
      itemId: "",
    }));
  }

  function toggleBitwardenItem(nextItemId: string): void {
    applyBitwardenItemSelection(nextItemId, bitwardenItems);
  }

  function applyBitwardenItemSelection(
    nextItemId: string,
    options: ImportPickerOption[],
  ): void {
    const nextSelectedIds = toggleSelection(
      selectedBitwardenItemIds,
      nextItemId,
      "multi",
    );
    const nextPrimaryId = nextSelectedIds.at(-1) ?? null;
    const nextItem = optionById(options, nextPrimaryId);

    setSelectedBitwardenItemIds(nextSelectedIds);
    setSelectedBitwardenItemId(nextPrimaryId);
    setSelectedBitwardenFieldIds([]);
    setBitwardenDraft((current) => ({
      ...current,
      item: nextItem?.label ?? "",
      itemId: nextItem?.id ?? "",
      field: "",
    }));
    if (nextItem) {
      suggestDisplayName(nextItem.label);
    }
  }

  function toggleBitwardenField(nextFieldId: string): void {
    const selectionMode =
      selectedBitwardenItemIds.length > 1 ? "single" : "multi";
    const nextSelectedIds = toggleSelection(
      selectedBitwardenFieldIds,
      nextFieldId,
      selectionMode,
    );
    const nextPrimaryId = nextSelectedIds.at(0) ?? null;
    const nextField =
      bitwardenFields.find((field) => fieldOptionId(field) === nextPrimaryId) ??
      null;

    setSelectedBitwardenFieldIds(nextSelectedIds);
    setBitwardenDraft((current) => ({
      ...current,
      field: nextField?.selector ?? "",
    }));
    if (nextField && selectedBitwardenItem) {
      suggestDisplayName(`${selectedBitwardenItem.label}:${nextField.label}`);
    }
  }

  function selectDotenvGroup(nextGroupId: string): void {
    const nextGroup =
      primaryDotenvInspection?.groups.find(
        (group) => group.id === nextGroupId,
      ) ?? null;
    setSelectedDotenvGroupId(nextGroupId);
    setSelectedDotenvKeys([]);
    setDotenvKeyQuery("");
    setDotenvDraft((current) => ({
      ...current,
      namespace: nextGroup?.namespace ?? "",
      prefix: nextGroup?.prefix ?? "",
      key: "",
    }));
  }

  function toggleDotenvKey(entry: DotenvSelectableKey): void {
    const nextSelectedIds = toggleSelection(
      selectedDotenvKeys,
      entry.id,
      "multi",
    );
    const nextPrimarySelectionId = nextSelectedIds.at(-1) ?? null;
    const nextPrimaryOption =
      visibleDotenvKeys.find(
        (candidate) => candidate.id === nextPrimarySelectionId,
      ) ?? null;
    const nextKey =
      nextPrimaryOption &&
      nextPrimaryOption.group?.prefix &&
      nextPrimaryOption.option.group_id !== "all"
        ? nextPrimaryOption.option.label
        : (nextPrimaryOption?.option.full_key ?? "");

    setSelectedDotenvKeys(nextSelectedIds);
    setDotenvDraft((current) => ({
      ...current,
      key: nextKey,
    }));
    if (nextKey) {
      suggestDisplayName(nextKey);
    }
  }

  async function chooseDotenvFile(): Promise<void> {
    setIsDotenvPicking(true);
    setBrowseErrorMessage(null);

    try {
      const selection = await invoke<string | string[] | null>(
        "pick_dotenv_file_command",
      );
      const filePaths = Array.isArray(selection)
        ? selection
        : selection
          ? [selection]
          : [];
      if (filePaths.length === 0) {
        return;
      }

      setDotenvDraft((current) => ({
        ...current,
        filePath: filePaths[0],
        namespace: "",
        prefix: "",
        key: "",
      }));
      setDotenvFilePaths(filePaths);
      setDotenvInspections([]);
      setSelectedDotenvGroupId("all");
      setSelectedDotenvKeys([]);
      setDotenvKeyQuery("");
    } catch (error) {
      handleBrowseError(error);
    } finally {
      setIsDotenvPicking(false);
    }
  }

  useEffect(() => {
    resetFeedback();
  }, [providerKind]);

  useEffect(() => {
    if (surface !== "import") {
      return;
    }
    let active = true;
    void invoke<BackendConnectionSummary[]>("list_backend_connections")
      .then((connections) => {
        if (!active) return;
        const enabledKinds: SecretImportProviderKind[] = [];
        if (
          connections.some(
            (connection) =>
              connection.enabled && connection.backend_kind === "one_password",
          )
        ) {
          enabledKinds.push("1password_cli");
        }
        if (
          connections.some(
            (connection) =>
              connection.enabled && connection.backend_kind === "bitwarden",
          )
        ) {
          enabledKinds.push("bitwarden_cli");
        }
        enabledKinds.push("dotenv_file");
        setAvailableProviderKinds(enabledKinds);
        setProviderKind((current) =>
          enabledKinds.includes(current) ? current : enabledKinds[0],
        );
      })
      .catch((error) => {
        if (!active) return;
        handleBrowseError(error);
        setAvailableProviderKinds(["dotenv_file"]);
        setProviderKind("dotenv_file");
      });
    return () => {
      active = false;
    };
  }, [surface]);

  useEffect(() => {
    if (surface !== "import") {
      void loadImportedCatalog();
    }
  }, [surface]);

  useEffect(() => {
    if (
      surface === "catalog" ||
      providerKind !== "1password_cli" ||
      !availableProviderKinds?.includes("1password_cli")
    ) {
      return;
    }

    let active = true;
    setIsOnePasswordAccountsLoading(true);
    void invoke<ImportPickerOption[]>("list_onepassword_accounts_command")
      .then((accounts) => {
        if (!active) {
          return;
        }
        setOnePasswordAccounts(accounts);
        if (accounts.length === 1) {
          applyOnePasswordAccountSelection(accounts[0].id, accounts);
        }
      })
      .catch((error) => {
        if (active) {
          handleBrowseError(error);
        }
      })
      .finally(() => {
        if (active) {
          setIsOnePasswordAccountsLoading(false);
        }
      });

    return () => {
      active = false;
    };
  }, [availableProviderKinds, providerKind, surface]);

  useEffect(() => {
    if (providerKind !== "1password_cli" || !selectedOnePasswordAccountId) {
      return;
    }

    let active = true;
    setIsOnePasswordVaultsLoading(true);
    void invoke<ImportPickerOption[]>("list_onepassword_vaults_command", {
      accountId: selectedOnePasswordAccountId,
    })
      .then((vaults) => {
        if (!active) {
          return;
        }
        setOnePasswordVaults(vaults);
        if (vaults.length === 1) {
          applyOnePasswordVaultSelection(vaults[0].id, vaults);
        }
      })
      .catch((error) => {
        if (active) {
          handleBrowseError(error);
        }
      })
      .finally(() => {
        if (active) {
          setIsOnePasswordVaultsLoading(false);
        }
      });

    return () => {
      active = false;
    };
  }, [providerKind, selectedOnePasswordAccountId]);

  useEffect(() => {
    if (
      providerKind !== "1password_cli" ||
      !selectedOnePasswordAccountId ||
      !selectedOnePasswordVaultId
    ) {
      return;
    }

    let active = true;
    setIsOnePasswordItemsLoading(true);
    void invoke<ImportPickerOption[]>("list_onepassword_items_command", {
      accountId: selectedOnePasswordAccountId,
      vaultId: selectedOnePasswordVaultId,
    })
      .then((items) => {
        if (!active) {
          return;
        }
        setOnePasswordItems(items);
        if (items.length === 1) {
          applyOnePasswordItemSelection(items[0].id, items);
        }
      })
      .catch((error) => {
        if (active) {
          handleBrowseError(error);
        }
      })
      .finally(() => {
        if (active) {
          setIsOnePasswordItemsLoading(false);
        }
      });

    return () => {
      active = false;
    };
  }, [providerKind, selectedOnePasswordAccountId, selectedOnePasswordVaultId]);

  useEffect(() => {
    if (
      providerKind !== "1password_cli" ||
      !selectedOnePasswordAccountId ||
      !selectedOnePasswordVaultId ||
      selectedOnePasswordItemIds.length === 0
    ) {
      return;
    }

    const missingItemIds = selectedOnePasswordItemIds.filter(
      (itemId) => !hasCachedFieldOptions(onePasswordFieldsByItemId, itemId),
    );
    if (missingItemIds.length === 0) {
      return;
    }

    let active = true;
    setIsOnePasswordFieldsLoading(true);
    void Promise.all(
      missingItemIds.map(async (itemId) => ({
        itemId,
        fields: await invoke<ImportFieldOption[]>(
          "list_onepassword_fields_command",
          {
            accountId: selectedOnePasswordAccountId,
            vaultId: selectedOnePasswordVaultId,
            itemId,
          },
        ),
      })),
    )
      .then((results) => {
        if (!active) {
          return;
        }
        setOnePasswordFieldsByItemId((current) => {
          const next = { ...current };
          for (const { itemId, fields } of results) {
            next[itemId] = fields;
          }
          return next;
        });
      })
      .catch((error) => {
        if (active) {
          handleBrowseError(error);
        }
      })
      .finally(() => {
        if (active) {
          setIsOnePasswordFieldsLoading(false);
        }
      });

    return () => {
      active = false;
    };
  }, [
    providerKind,
    selectedOnePasswordAccountId,
    selectedOnePasswordVaultId,
    selectedOnePasswordItemIds,
    onePasswordFieldsByItemId,
  ]);

  useEffect(() => {
    if (
      providerKind !== "1password_cli" ||
      !selectedOnePasswordItemId ||
      selectedOnePasswordItemIds.length !== 1 ||
      selectedOnePasswordFieldIds.length > 0
    ) {
      return;
    }

    const fields = onePasswordFieldsByItemId[selectedOnePasswordItemId] ?? [];
    if (fields.length === 1) {
      applyOnePasswordFieldSelection(fieldOptionId(fields[0]), fields);
    }
  }, [
    onePasswordFieldsByItemId,
    providerKind,
    selectedOnePasswordFieldIds.length,
    selectedOnePasswordItemId,
    selectedOnePasswordItemIds.length,
  ]);

  useEffect(() => {
    if (
      surface === "catalog" ||
      providerKind !== "bitwarden_cli" ||
      !availableProviderKinds?.includes("bitwarden_cli")
    ) {
      return;
    }

    let active = true;
    setIsBitwardenAccountsLoading(true);
    setIsBitwardenContainersLoading(true);

    void invoke<ImportPickerOption[]>("list_bitwarden_accounts_command")
      .then((accounts) => {
        if (!active) {
          return;
        }
        setBitwardenAccounts(accounts);
        if (accounts.length > 0) {
          applyBitwardenAccountSelection(accounts[0].id, accounts);
        }
      })
      .catch((error) => {
        if (active) {
          handleBrowseError(error);
        }
      })
      .finally(() => {
        if (active) {
          setIsBitwardenAccountsLoading(false);
        }
      });

    void invoke<BitwardenContainerOption[]>("list_bitwarden_containers_command")
      .then((containers) => {
        if (!active) {
          return;
        }
        setBitwardenContainers(containers);
      })
      .catch((error) => {
        if (active) {
          handleBrowseError(error);
        }
      })
      .finally(() => {
        if (active) {
          setIsBitwardenContainersLoading(false);
        }
      });

    return () => {
      active = false;
    };
  }, [availableProviderKinds, providerKind, surface]);

  useEffect(() => {
    if (providerKind !== "bitwarden_cli" || !selectedBitwardenAccountId) {
      return;
    }

    let active = true;
    setIsBitwardenItemsLoading(true);
    setSelectedBitwardenItemId(null);
    setSelectedBitwardenItemIds([]);
    setSelectedBitwardenFieldIds([]);
    setBitwardenFieldsByItemId({});

    const container = bitwardenContainers.find(
      (option) => option.id === selectedBitwardenContainerId,
    );

    void invoke<ImportPickerOption[]>("list_bitwarden_items_command", {
      containerKind:
        container?.kind === "all" ? null : (container?.kind ?? null),
      containerId: container?.kind === "all" ? null : (container?.id ?? null),
      organizationId: container?.organization_id ?? null,
    })
      .then((items) => {
        if (!active) {
          return;
        }
        setBitwardenItems(items);
        if (items.length === 1) {
          applyBitwardenItemSelection(items[0].id, items);
        }
      })
      .catch((error) => {
        if (active) {
          handleBrowseError(error);
        }
      })
      .finally(() => {
        if (active) {
          setIsBitwardenItemsLoading(false);
        }
      });

    return () => {
      active = false;
    };
  }, [
    providerKind,
    selectedBitwardenAccountId,
    selectedBitwardenContainerId,
    bitwardenContainers,
  ]);

  useEffect(() => {
    if (
      providerKind !== "bitwarden_cli" ||
      selectedBitwardenItemIds.length === 0
    ) {
      return;
    }

    const missingItemIds = selectedBitwardenItemIds.filter(
      (itemId) => !hasCachedFieldOptions(bitwardenFieldsByItemId, itemId),
    );
    if (missingItemIds.length === 0) {
      return;
    }

    let active = true;
    setIsBitwardenFieldsLoading(true);

    void Promise.all(
      missingItemIds.map(async (itemId) => ({
        itemId,
        fields: await invoke<ImportFieldOption[]>(
          "list_bitwarden_fields_command",
          {
            itemId,
          },
        ),
      })),
    )
      .then((results) => {
        if (!active) {
          return;
        }
        setBitwardenFieldsByItemId((current) => {
          const next = { ...current };
          for (const { itemId, fields } of results) {
            next[itemId] = fields;
          }
          return next;
        });
      })
      .catch((error) => {
        if (active) {
          handleBrowseError(error);
        }
      })
      .finally(() => {
        if (active) {
          setIsBitwardenFieldsLoading(false);
        }
      });

    return () => {
      active = false;
    };
  }, [bitwardenFieldsByItemId, providerKind, selectedBitwardenItemIds]);

  useEffect(() => {
    if (
      providerKind !== "bitwarden_cli" ||
      !selectedBitwardenItemId ||
      selectedBitwardenItemIds.length !== 1 ||
      selectedBitwardenFieldIds.length > 0
    ) {
      return;
    }

    const fields = bitwardenFieldsByItemId[selectedBitwardenItemId] ?? [];
    if (fields.length === 1) {
      toggleBitwardenField(fieldOptionId(fields[0]));
    }
  }, [
    bitwardenFieldsByItemId,
    providerKind,
    selectedBitwardenFieldIds.length,
    selectedBitwardenItemId,
    selectedBitwardenItemIds.length,
  ]);

  useEffect(() => {
    if (providerKind !== "dotenv_file" || dotenvFilePaths.length === 0) {
      return;
    }

    let active = true;
    setIsDotenvInspecting(true);

    void Promise.all(
      dotenvFilePaths.map((filePath) =>
        invoke<DotenvInspection>("inspect_dotenv_file_command", { filePath }),
      ),
    )
      .then((inspections) => {
        if (!active) {
          return;
        }
        setDotenvInspections(inspections);
        setSelectedDotenvGroupId("all");
      })
      .catch((error) => {
        if (active) {
          handleBrowseError(error);
        }
      })
      .finally(() => {
        if (active) {
          setIsDotenvInspecting(false);
        }
      });

    return () => {
      active = false;
    };
  }, [dotenvFilePaths, providerKind]);

  const onePasswordFieldOptions = onePasswordFields.map((field) => ({
    id: fieldOptionId(field),
    label: field.label,
    subtitle: field.subtitle,
  }));
  const bitwardenContainerOptions = bitwardenContainers.map((container) => ({
    id: container.id,
    label: container.label,
    subtitle: container.subtitle,
  }));
  const bitwardenFieldOptions = bitwardenFields.map((field) => ({
    id: fieldOptionId(field),
    label: field.label,
    subtitle: field.subtitle,
  }));
  const dotenvGroupOptions = (primaryDotenvInspection?.groups ?? []).map(
    (group) => ({
      id: group.id,
      label: group.label,
      subtitle: sectionCaption(
        props.locale,
        `${group.key_count} key(s)`,
        `${group.key_count} 个 key`,
      ),
    }),
  );
  const dotenvKeyOptions = visibleDotenvKeys.map((entry) => ({
    id: entry.id,
    label: entry.option.label,
    subtitle:
      dotenvInspections.length > 1
        ? `${entry.fileLabel} · ${entry.option.full_key}`
        : entry.option.full_key !== entry.option.label
          ? entry.option.full_key
          : null,
  }));

  return (
    <section
      className="panel password-panel password-management-view"
      data-surface={surface}
      data-testid="password-management-panel"
    >
      {submitErrorMessage || browseErrorMessage ? (
        <section
          className="alert"
          data-testid="password-import-error-banner"
          role="alert"
        >
          <p data-testid="password-import-error-message">
            {submitErrorMessage ?? browseErrorMessage}
          </p>
        </section>
      ) : null}

      {noticeMessage ? (
        <section
          className="alert"
          data-testid="password-import-notice-banner"
          role="status"
        >
          <p data-testid="password-import-notice-message">{noticeMessage}</p>
        </section>
      ) : null}

      {surface === "import" ? (
        <nav
          aria-label={sectionCaption(
            props.locale,
            "Choose how to add a password",
            "选择密码添加方式",
          )}
          className="password-entry-mode-tabs"
          data-testid="password-entry-mode-tabs"
        >
          <button
            aria-pressed={entryMode === "manual"}
            className={entryMode === "manual" ? "active" : ""}
            data-testid="password-entry-mode-manual"
            onClick={() => {
              resetFeedback();
              setEntryMode("manual");
            }}
            type="button"
          >
            <strong>
              {sectionCaption(props.locale, "Create manually", "手动添加")}
            </strong>
            <span>
              {sectionCaption(
                props.locale,
                "Review fields, then choose a backend and vault",
                "检查字段后选择后端与保险库",
              )}
            </span>
          </button>
          <button
            aria-pressed={entryMode === "import"}
            className={entryMode === "import" ? "active" : ""}
            data-testid="password-entry-mode-import"
            onClick={() => {
              resetFeedback();
              setEntryMode("import");
            }}
            type="button"
          >
            <strong>
              {sectionCaption(props.locale, "Import existing", "导入已有密码")}
            </strong>
            <span>
              {sectionCaption(
                props.locale,
                "Bring in selected entries from a connected source or file",
                "从已连接来源或文件选择条目导入",
              )}
            </span>
          </button>
        </nav>
      ) : null}

      {surface === "import" && entryMode === "manual" ? (
        <section
          className="password-management-primary-section manual-secret-section"
          data-password-primary-section="manual"
          data-testid="manual-secret-section"
        >
          <header className="password-management-primary-heading">
            <h2>
              {sectionCaption(props.locale, "Create a password", "添加密码")}
            </h2>
            <p>
              {sectionCaption(
                props.locale,
                "Give it a recognizable name and value. Technical identifiers are generated automatically.",
                "填写容易识别的名称和值；技术标识会自动生成。",
              )}
            </p>
          </header>
          <div className="manual-secret-form">
            <label className="settings-field">
              <span className="field-label">
                {sectionCaption(props.locale, "Name", "名称")}
              </span>
              <input
                autoFocus
                className="settings-input"
                data-testid="manual-secret-title"
                onChange={(event) => {
                  const title = event.currentTarget.value;
                  setManualDraft((current) => ({
                    ...current,
                    title,
                  }));
                }}
                placeholder={sectionCaption(
                  props.locale,
                  "GitHub production",
                  "GitHub 生产账号",
                )}
                value={manualDraft.title}
              />
            </label>
            <label className="settings-field manual-secret-value-field">
              <span className="field-label">
                {sectionCaption(props.locale, "Password or value", "密码或值")}
              </span>
              <SecretInput
                locale={props.locale}
                fieldName={sectionCaption(props.locale, "value", "值")}
                autoComplete="new-password"
                className="settings-input"
                data-testid="manual-secret-value"
                onChange={(event) => {
                  const value = event.currentTarget.value;
                  setManualDraft((current) => ({
                    ...current,
                    value,
                  }));
                }}
                value={manualDraft.value}
              />
            </label>
            <label className="settings-field">
              <span className="field-label">
                {sectionCaption(props.locale, "Field label", "字段名称")}
              </span>
              <input
                className="settings-input"
                data-testid="manual-secret-field-label"
                onChange={(event) => {
                  const fieldLabel = event.currentTarget.value;
                  setManualDraft((current) => ({
                    ...current,
                    fieldLabel,
                  }));
                }}
                value={manualDraft.fieldLabel}
              />
            </label>
            <label className="settings-field">
              <span className="field-label">
                {sectionCaption(props.locale, "Tags", "标签")}
                <span className="field-optional">
                  {" "}
                  · {t(props.locale, "optional")}
                </span>
              </span>
              <input
                className="settings-input"
                data-testid="manual-secret-tags"
                onChange={(event) => {
                  const tags = event.currentTarget.value;
                  setManualDraft((current) => ({
                    ...current,
                    tags,
                  }));
                }}
                placeholder={sectionCaption(
                  props.locale,
                  "production, shared",
                  "生产、共享",
                )}
                value={manualDraft.tags}
              />
            </label>
            <label className="settings-field settings-field-wide">
              <span className="field-label">
                {sectionCaption(props.locale, "Notes", "备注")}
                <span className="field-optional">
                  {" "}
                  · {t(props.locale, "optional")}
                </span>
              </span>
              <textarea
                className="settings-input note-field"
                data-testid="manual-secret-description"
                onChange={(event) => {
                  const description = event.currentTarget.value;
                  setManualDraft((current) => ({
                    ...current,
                    description,
                  }));
                }}
                rows={3}
                value={manualDraft.description}
              />
            </label>
          </div>
          <details className="manual-secret-advanced">
            <summary>
              {sectionCaption(
                props.locale,
                "Advanced identifier",
                "高级标识设置",
              )}
            </summary>
            <label className="settings-field">
              <span className="field-label">
                {t(props.locale, "resourceId")}
              </span>
              <input
                className="settings-input"
                data-testid="manual-secret-resource"
                onChange={(event) => {
                  const resource = event.currentTarget.value;
                  setManualDraft((current) => ({
                    ...current,
                    resource,
                  }));
                }}
                placeholder={generatedManualResource}
                value={manualDraft.resource}
              />
              <span className="field-hint">
                {resolvedManualResource ||
                  sectionCaption(
                    props.locale,
                    "Generated after a name is entered",
                    "填写名称后自动生成",
                  )}
              </span>
            </label>
          </details>
          <div className="password-actions manual-secret-actions">
            <button
              className="primary"
              data-testid="manual-secret-submit"
              disabled={!canSaveManual || isSavingManual}
              onClick={() => void submitManualSecret()}
              type="button"
            >
              {isSavingManual
                ? sectionCaption(props.locale, "Preparing…", "正在准备…")
                : sectionCaption(
                    props.locale,
                    "Choose vault and continue",
                    "选择保险库并继续",
                  )}
            </button>
          </div>
        </section>
      ) : null}

      {surface !== "import" ? (
        <section
          className="password-management-primary-section"
          data-password-primary-section="catalog"
        >
          <header className="password-management-primary-heading">
            <h2>{sectionCaption(props.locale, "Catalog", "目录")}</h2>
            <p>
              {sectionCaption(
                props.locale,
                "Review local entries and imported references without exposing stored values.",
                "审查本地条目与导入引用，不暴露已存储的值。",
              )}
            </p>
          </header>
          <ImportedSecretCatalogPanel
            catalog={importedCatalog}
            errorMessage={catalogErrorMessage}
            isLoading={isCatalogLoading}
            locale={props.locale}
            noticeMessage={catalogNoticeMessage}
            onDelete={deleteImportedSecret}
            onRefreshImported={refreshImportedSecret}
            onRename={renameSecret}
            onReload={loadImportedCatalog}
            onReveal={revealSecret}
            onSaveImported={saveImportedSecret}
            onSaveLiteral={saveLocalSecret}
          />
        </section>
      ) : null}

      {surface !== "catalog" &&
      (surface !== "import" || entryMode === "import") ? (
        <>
          <section
            className="password-management-primary-section"
            data-password-primary-section="add"
          >
            <header className="password-management-primary-heading">
              <h2>
                {surface === "import"
                  ? sectionCaption(
                      props.locale,
                      "Optional details",
                      "可选条目信息",
                    )
                  : sectionCaption(props.locale, "Add or Import", "添加或导入")}
              </h2>
              <p>
                {surface === "import"
                  ? sectionCaption(
                      props.locale,
                      "Names and identifiers are generated automatically. Open this section only when you want to customize them.",
                      "名称和标识默认自动生成；仅在需要自定义时展开此区域。",
                    )
                  : sectionCaption(
                      props.locale,
                      "Describe the catalog entry, choose its source, then review the exact generated resource identifiers before importing.",
                      "填写目录条目信息、选择来源，并在导入前检查生成的资源标识。",
                    )}
              </p>
            </header>
            <OptionalDetails
              collapsed={surface === "import"}
              summary={sectionCaption(
                props.locale,
                "Customize name, tags, and resource identifiers",
                "自定义名称、标签和资源标识",
              )}
            >
              <div
                className="password-layout"
                data-testid="password-management-layout"
              >
                <section
                  className="detail-section"
                  data-testid="password-common-section"
                >
                  <div className="detail-section-header">
                    <h3>{t(props.locale, "importDetailsTitle")}</h3>
                    <span>
                      {isBatchMode
                        ? sectionCaption(
                            props.locale,
                            `${plannedSpecs.length} imports planned`,
                            `计划导入 ${plannedSpecs.length} 条`,
                          )
                        : t(props.locale, "resourceId")}
                    </span>
                  </div>
                  <p className="section-copy">
                    {isBatchMode
                      ? sectionCaption(
                          props.locale,
                          "Resource id and display name stay optional. Batch mode uses generated resource ids for each import.",
                          "资源标识和显示名都可留空；批量模式会为每条导入生成默认资源标识。",
                        )
                      : t(props.locale, "importDetailsHelp")}
                  </p>
                  <div className="settings-form-grid">
                    <LocatorField
                      dataTestId="password-field-resource"
                      label={t(props.locale, "resourceId")}
                      disabled={isBatchMode}
                      hint={
                        isBatchMode
                          ? sectionCaption(
                              props.locale,
                              "Manual resource ids are only available for single imports. Use the template below for batch imports.",
                              "手填资源标识仅用于单条导入；批量导入请使用下方模板。",
                            )
                          : sectionCaption(
                              props.locale,
                              "Leave empty to generate the default resource id automatically.",
                              "留空时自动生成默认资源标识。",
                            )
                      }
                      onChange={(value) => {
                        setCommonDraft((current) => ({
                          ...current,
                          resource: value,
                        }));
                      }}
                      optional
                      optionalLabel={t(props.locale, "optional")}
                      value={commonDraft.resource}
                    />
                    <LocatorField
                      dataTestId="password-field-display-name"
                      label={t(props.locale, "displayName")}
                      disabled={isBatchMode}
                      hint={
                        isBatchMode
                          ? sectionCaption(
                              props.locale,
                              "Batch mode derives display names from each selected resource.",
                              "批量模式会按每个已选资源自动生成显示名。",
                            )
                          : undefined
                      }
                      onChange={(value) => {
                        setCommonDraft((current) => ({
                          ...current,
                          displayName: value,
                        }));
                      }}
                      optional
                      optionalLabel={t(props.locale, "optional")}
                      value={commonDraft.displayName}
                    />
                    <LocatorField
                      dataTestId="password-field-description"
                      label={t(props.locale, "description")}
                      onChange={(value) => {
                        setCommonDraft((current) => ({
                          ...current,
                          description: value,
                        }));
                      }}
                      optional
                      optionalLabel={t(props.locale, "optional")}
                      value={commonDraft.description}
                    />
                    <label
                      className="settings-field"
                      data-testid="password-field-tags"
                    >
                      <span className="field-label">
                        {t(props.locale, "tags")}
                        <span className="field-optional">
                          {" "}
                          · {t(props.locale, "optional")}
                        </span>
                      </span>
                      <input
                        className="settings-input"
                        onChange={(event) => {
                          const nextValue = event.currentTarget.value;
                          setCommonDraft((current) => ({
                            ...current,
                            tags: nextValue,
                          }));
                        }}
                        type="text"
                        value={commonDraft.tags}
                      />
                      <span className="field-hint">
                        {sectionCaption(
                          props.locale,
                          "Optional. Applied to every generated import in batch mode.",
                          "可留空；批量模式下会应用到每条生成的导入记录。",
                        )}
                      </span>
                    </label>
                    <label
                      className="settings-field settings-field-wide"
                      data-testid="password-field-metadata"
                    >
                      <span className="field-label">
                        {t(props.locale, "metadata")}
                        <span className="field-optional">
                          {" "}
                          · {t(props.locale, "optional")}
                        </span>
                      </span>
                      <textarea
                        className="settings-input note-field"
                        onChange={(event) => {
                          const nextValue = event.currentTarget.value;
                          setCommonDraft((current) => ({
                            ...current,
                            metadata: nextValue,
                          }));
                        }}
                        placeholder={sectionCaption(
                          props.locale,
                          "team=backend\nowner=alice",
                          "team=backend\nowner=alice",
                        )}
                        value={commonDraft.metadata}
                      />
                      <span className="field-hint">
                        {t(props.locale, "metadataFormatHelp")}
                      </span>
                    </label>
                  </div>
                </section>

                <details
                  className="password-advanced-templates"
                  data-testid="password-advanced-templates"
                >
                  <summary>
                    {sectionCaption(
                      props.locale,
                      "Advanced resource templates",
                      "高级资源模板",
                    )}
                  </summary>
                  <section
                    className="detail-section"
                    data-testid="password-template-section"
                  >
                    <div className="detail-section-header">
                      <h3>
                        {sectionCaption(
                          props.locale,
                          "Resource Template",
                          "资源模板",
                        )}
                      </h3>
                      <span>
                        {resourceTemplateMode === "default"
                          ? sectionCaption(props.locale, "Default", "默认")
                          : sectionCaption(props.locale, "Custom", "自定义")}
                      </span>
                    </div>
                    <p className="section-copy">
                      {sectionCaption(
                        props.locale,
                        "Default ids are generated from the current provider locator. Switch to a custom template only when you need a different path shape.",
                        "默认资源标识会按当前 provider locator 自动生成；只有在需要不同路径规则时再切到自定义模板。",
                      )}
                    </p>
                    <div
                      className="provider-option-list"
                      data-testid="password-template-mode-options"
                    >
                      <button
                        aria-pressed={
                          resourceTemplateMode === "default" ? "true" : "false"
                        }
                        className={`provider-option ${
                          resourceTemplateMode === "default" ? "active" : ""
                        }`}
                        data-testid="password-template-mode-default"
                        onClick={() => {
                          setResourceTemplateMode("default");
                          setSelectedSavedResourceTemplateId(null);
                        }}
                        type="button"
                      >
                        <strong>
                          {sectionCaption(
                            props.locale,
                            "Default Rule",
                            "默认规则",
                          )}
                        </strong>
                        <p>
                          {defaultResourceTemplateForProvider(providerKind)}
                        </p>
                      </button>
                      <button
                        aria-pressed={
                          resourceTemplateMode === "custom" ? "true" : "false"
                        }
                        className={`provider-option ${
                          resourceTemplateMode === "custom" ? "active" : ""
                        }`}
                        data-testid="password-template-mode-custom"
                        onClick={() => {
                          setResourceTemplateMode("custom");
                          setSelectedSavedResourceTemplateId(null);
                        }}
                        type="button"
                      >
                        <strong>
                          {sectionCaption(
                            props.locale,
                            "Custom Template",
                            "自定义模板",
                          )}
                        </strong>
                        <p>
                          {sectionCaption(
                            props.locale,
                            "Use placeholders such as {{ item }} or {{ field }}",
                            "使用 {{ item }} / {{ field }} 等占位符",
                          )}
                        </p>
                      </button>
                      {visibleSavedResourceTemplates.map((template) => (
                        <div
                          className={`provider-option provider-option-card ${
                            selectedSavedResourceTemplateId === template.id
                              ? "active"
                              : ""
                          }`}
                          data-testid="password-saved-template-item"
                          key={template.id}
                        >
                          <div className="saved-template-copy">
                            <strong>{template.name}</strong>
                            <p>{template.template}</p>
                          </div>
                          <div className="provider-option-actions">
                            <button
                              className="ghost"
                              data-testid="password-saved-template-apply"
                              onClick={() => {
                                applySavedResourceTemplate(template.id);
                              }}
                              type="button"
                            >
                              {sectionCaption(props.locale, "Apply", "应用")}
                            </button>
                            <button
                              className="ghost"
                              data-testid="password-saved-template-delete"
                              onClick={() => {
                                deleteSavedResourceTemplate(template.id);
                              }}
                              type="button"
                            >
                              {sectionCaption(props.locale, "Delete", "删除")}
                            </button>
                          </div>
                        </div>
                      ))}
                    </div>
                    {resourceTemplateMode === "custom" ? (
                      <>
                        <LocatorField
                          dataTestId="password-field-resource-template"
                          hint={sectionCaption(
                            props.locale,
                            `Supported placeholders: ${availableTemplateTokens(providerKind).join(", ")}`,
                            `支持的占位符：${availableTemplateTokens(providerKind).join("、")}`,
                          )}
                          label={sectionCaption(
                            props.locale,
                            "Template",
                            "模板",
                          )}
                          onChange={setResourceTemplate}
                          optionalLabel={t(props.locale, "optional")}
                          value={resourceTemplate}
                        />
                        <div className="template-save-row">
                          <LocatorField
                            dataTestId="password-field-resource-template-name"
                            hint={sectionCaption(
                              props.locale,
                              "Save reusable templates under a short name for this provider.",
                              "给当前 provider 的可复用模板保存一个简短名称。",
                            )}
                            label={sectionCaption(
                              props.locale,
                              "Template Name",
                              "模板名称",
                            )}
                            onChange={setResourceTemplateName}
                            optionalLabel={t(props.locale, "optional")}
                            value={resourceTemplateName}
                          />
                          <div className="password-actions">
                            <button
                              className="ghost"
                              data-testid="password-template-save"
                              onClick={() => {
                                saveCurrentResourceTemplate();
                              }}
                              type="button"
                            >
                              {sectionCaption(
                                props.locale,
                                "Save Template",
                                "保存模板",
                              )}
                            </button>
                          </div>
                        </div>
                      </>
                    ) : null}
                  </section>
                </details>
                <section
                  className="detail-section detail-section-low template-preview-panel"
                  data-testid="password-template-preview"
                >
                  <div className="detail-section-header">
                    <h3>
                      {sectionCaption(
                        props.locale,
                        "Import Preview",
                        "导入预览",
                      )}
                    </h3>
                    <span>
                      {sectionCaption(
                        props.locale,
                        `${plannedSpecs.length} target(s)`,
                        `${plannedSpecs.length} 个目标`,
                      )}
                    </span>
                  </div>
                  {planBlockerMessage ? (
                    <p
                      className="empty"
                      data-testid="password-template-preview-blocker"
                    >
                      {planBlockerMessage}
                    </p>
                  ) : plannedSpecs.length === 0 ? (
                    <p
                      className="empty"
                      data-testid="password-template-preview-empty"
                    >
                      {previewEmptyMessage}
                    </p>
                  ) : (
                    <ol
                      className="boundary-list"
                      data-testid="password-template-preview-list"
                    >
                      {previewResources.slice(0, 6).map((resource) => (
                        <li key={resource}>
                          <code>{resource}</code>
                        </li>
                      ))}
                    </ol>
                  )}
                </section>
              </div>
            </OptionalDetails>
          </section>

          <section
            className="password-management-primary-section"
            data-password-primary-section="sources"
          >
            <header className="password-management-primary-heading">
              <h2>{sectionCaption(props.locale, "Sources", "来源")}</h2>
              <p>
                {sectionCaption(
                  props.locale,
                  "Choose an enabled provider and the exact accounts, vaults, items, or files to import.",
                  "选择已启用的提供方，以及要导入的具体账号、保险库、条目或文件。",
                )}
              </p>
            </header>
            <div
              className="password-layout"
              data-testid="password-sources-layout"
            >
              <section
                className="detail-section"
                data-testid="password-provider-section"
              >
                <div className="detail-section-header">
                  <h3>{t(props.locale, "passwordProvidersTitle")}</h3>
                  <span>
                    {selectedProvider
                      ? surface === "import"
                        ? sectionCaption(
                            props.locale,
                            "Selected source",
                            "已选来源",
                          )
                        : t(props.locale, selectedProvider.scopeKey)
                      : ""}
                  </span>
                </div>
                <p className="section-copy">
                  {surface === "import"
                    ? sectionCaption(
                        props.locale,
                        "Choose where your passwords are coming from.",
                        "选择密码的来源。",
                      )
                    : t(props.locale, "passwordProvidersHelp")}
                </p>
                <div
                  className="provider-option-list"
                  data-testid="password-provider-options"
                >
                  {availableProviderKinds === null ? (
                    <p className="empty">
                      {sectionCaption(
                        props.locale,
                        "Loading enabled sources…",
                        "正在加载已启用来源…",
                      )}
                    </p>
                  ) : null}
                  {availableProviderOptions.map((option) => (
                    <button
                      aria-pressed={
                        providerKind === option.kind ? "true" : "false"
                      }
                      className={`provider-option ${
                        providerKind === option.kind ? "active" : ""
                      }`}
                      data-testid={`password-provider-option-${option.kind}`}
                      key={option.kind}
                      onClick={() => {
                        resetFeedback();
                        setProviderKind(option.kind);
                      }}
                      type="button"
                    >
                      <strong>
                        {translateCode(props.locale, option.kind)}
                      </strong>
                      <p>
                        {surface === "import"
                          ? friendlyImportProviderDescription(
                              props.locale,
                              option.kind,
                            )
                          : t(props.locale, option.descriptionKey)}
                      </p>
                      <span className="toolbar-count">
                        {surface === "import"
                          ? friendlyImportProviderSteps(
                              props.locale,
                              option.kind,
                            )
                          : t(props.locale, option.scopeKey)}
                      </span>
                    </button>
                  ))}
                </div>
                {surface === "import" && availableProviderKinds !== null ? (
                  <p className="field-hint">
                    {sectionCaption(
                      props.locale,
                      "Only enabled password connections appear here. Enable more under Connections.",
                      "这里只显示已启用的密码连接；可前往“连接”启用更多来源。",
                    )}
                  </p>
                ) : null}
              </section>

              {providerKind === "1password_cli" ? (
                <>
                  <PickerSection
                    caption={sectionCaption(
                      props.locale,
                      "Configured account",
                      "已配置账号",
                    )}
                    dataTestId="onepassword-account-picker"
                    emptyMessage={
                      isOnePasswordAccountsLoading
                        ? sectionCaption(
                            props.locale,
                            "Loading accounts",
                            "加载账号中",
                          )
                        : sectionCaption(
                            props.locale,
                            "No accounts available",
                            "没有可用账号",
                          )
                    }
                    loading={isOnePasswordAccountsLoading}
                    onSelect={selectOnePasswordAccount}
                    options={onePasswordAccounts}
                    selectedId={selectedOnePasswordAccountId}
                    title={t(props.locale, "account")}
                  />

                  <PickerSection
                    caption={sectionCaption(props.locale, "Required", "必选")}
                    dataTestId="onepassword-vault-picker"
                    emptyMessage={
                      selectedOnePasswordAccountId
                        ? isOnePasswordVaultsLoading
                          ? sectionCaption(
                              props.locale,
                              "Loading vaults",
                              "加载保险库中",
                            )
                          : sectionCaption(
                              props.locale,
                              "No vaults available",
                              "没有可用保险库",
                            )
                        : sectionCaption(
                            props.locale,
                            "Select an account first",
                            "先选择账号",
                          )
                    }
                    loading={isOnePasswordVaultsLoading}
                    onSelect={selectOnePasswordVault}
                    options={onePasswordVaults}
                    selectedId={selectedOnePasswordVaultId}
                    title={t(props.locale, "vault")}
                  />

                  <MultiPickerSection
                    caption={sectionCaption(
                      props.locale,
                      `${selectedOnePasswordItemIds.length} selected`,
                      `已选 ${selectedOnePasswordItemIds.length} 个`,
                    )}
                    dataTestId="onepassword-item-picker"
                    emptyMessage={
                      selectedOnePasswordVaultId
                        ? isOnePasswordItemsLoading
                          ? sectionCaption(
                              props.locale,
                              "Loading items",
                              "加载条目中",
                            )
                          : sectionCaption(
                              props.locale,
                              "No items found",
                              "没有找到条目",
                            )
                        : sectionCaption(
                            props.locale,
                            "Select a vault first",
                            "先选择保险库",
                          )
                    }
                    helper={sectionCaption(
                      props.locale,
                      isOnePasswordMultiResourceMode
                        ? "All fields from the selected resources will be imported."
                        : "Single-resource mode supports selecting specific fields from the current resource.",
                      isOnePasswordMultiResourceMode
                        ? "当前会导入所选资源的全部字段。"
                        : "单资源模式支持从当前资源中选择指定字段。",
                    )}
                    loading={isOnePasswordItemsLoading}
                    locale={props.locale}
                    onSearchQueryChange={setOnePasswordItemQuery}
                    onToggleSelect={toggleOnePasswordItem}
                    options={onePasswordItems}
                    searchPlaceholder={searchPlaceholder(
                      props.locale,
                      t(props.locale, "item"),
                    )}
                    searchQuery={onePasswordItemQuery}
                    selectedIds={selectedOnePasswordItemIds}
                    title={t(props.locale, "item")}
                  />

                  {!isOnePasswordMultiResourceMode ? (
                    <MultiPickerSection
                      caption={sectionCaption(
                        props.locale,
                        `${selectedOnePasswordFieldIds.length} selected`,
                        `已选 ${selectedOnePasswordFieldIds.length} 个`,
                      )}
                      dataTestId="onepassword-field-picker"
                      emptyMessage={
                        selectedOnePasswordItemId
                          ? isOnePasswordFieldsLoading
                            ? sectionCaption(
                                props.locale,
                                "Loading fields",
                                "加载字段中",
                              )
                            : sectionCaption(
                                props.locale,
                                "No fields available",
                                "没有可用字段",
                              )
                          : sectionCaption(
                              props.locale,
                              "Select an item first",
                              "先选择条目",
                            )
                      }
                      helper={sectionCaption(
                        props.locale,
                        "Single-resource mode supports selecting multiple fields from the same resource.",
                        "单资源模式支持对同一个资源多选字段。",
                      )}
                      loading={isOnePasswordFieldsLoading}
                      locale={props.locale}
                      onToggleSelect={toggleOnePasswordField}
                      options={onePasswordFieldOptions}
                      selectedIds={selectedOnePasswordFieldIds}
                      title={t(props.locale, "field")}
                    />
                  ) : null}

                  {!isOnePasswordMultiResourceMode &&
                  selectedOnePasswordItemId &&
                  onePasswordFieldOptions.length === 0 ? (
                    <section
                      className="detail-section detail-section-low"
                      data-testid="onepassword-field-fallback"
                    >
                      <div className="detail-section-header">
                        <h3>{t(props.locale, "field")}</h3>
                        <span>
                          {sectionCaption(
                            props.locale,
                            "Minimal fallback",
                            "最小兜底",
                          )}
                        </span>
                      </div>
                      <LocatorField
                        dataTestId="password-field-1password-field"
                        hint={sectionCaption(
                          props.locale,
                          "Only used when field enumeration is unavailable.",
                          "仅在字段枚举不可用时兜底使用。",
                        )}
                        label={t(props.locale, "field")}
                        onChange={(value) => {
                          setOnePasswordDraft((current) => ({
                            ...current,
                            field: value,
                          }));
                        }}
                        optionalLabel={t(props.locale, "optional")}
                        value={onePasswordDraft.field}
                      />
                    </section>
                  ) : null}
                </>
              ) : null}

              {providerKind === "bitwarden_cli" ? (
                <>
                  <PickerSection
                    caption={sectionCaption(
                      props.locale,
                      "Detected session",
                      "已检测到会话",
                    )}
                    dataTestId="bitwarden-account-picker"
                    emptyMessage={
                      isBitwardenAccountsLoading
                        ? sectionCaption(
                            props.locale,
                            "Loading account",
                            "加载账号中",
                          )
                        : sectionCaption(
                            props.locale,
                            "No Bitwarden session",
                            "没有 Bitwarden 会话",
                          )
                    }
                    loading={isBitwardenAccountsLoading}
                    onSelect={selectBitwardenAccount}
                    options={bitwardenAccounts}
                    selectedId={selectedBitwardenAccountId}
                    title={t(props.locale, "account")}
                  />

                  <PickerSection
                    caption={sectionCaption(
                      props.locale,
                      "Container filter",
                      "容器过滤",
                    )}
                    dataTestId="bitwarden-container-picker"
                    emptyMessage={
                      isBitwardenContainersLoading
                        ? sectionCaption(
                            props.locale,
                            "Loading containers",
                            "加载容器中",
                          )
                        : sectionCaption(
                            props.locale,
                            "No folders or collections available",
                            "没有可用容器",
                          )
                    }
                    loading={isBitwardenContainersLoading}
                    onSelect={selectBitwardenContainer}
                    options={bitwardenContainerOptions}
                    selectedId={selectedBitwardenContainerId}
                    title={sectionCaption(props.locale, "Container", "容器")}
                  />

                  <MultiPickerSection
                    caption={sectionCaption(
                      props.locale,
                      `${selectedBitwardenItemIds.length} selected`,
                      `已选 ${selectedBitwardenItemIds.length} 个`,
                    )}
                    dataTestId="bitwarden-item-picker"
                    emptyMessage={
                      isBitwardenItemsLoading
                        ? sectionCaption(
                            props.locale,
                            "Loading items",
                            "加载条目中",
                          )
                        : sectionCaption(
                            props.locale,
                            "No items found",
                            "没有找到条目",
                          )
                    }
                    helper={sectionCaption(
                      props.locale,
                      isBitwardenMultiResourceMode
                        ? "All fields from the selected resources will be imported."
                        : "Single-resource mode supports selecting specific fields from the current resource.",
                      isBitwardenMultiResourceMode
                        ? "当前会导入所选资源的全部字段。"
                        : "单资源模式支持从当前资源中选择指定字段。",
                    )}
                    loading={isBitwardenItemsLoading}
                    locale={props.locale}
                    onSearchQueryChange={setBitwardenItemQuery}
                    onToggleSelect={toggleBitwardenItem}
                    options={bitwardenItems}
                    searchPlaceholder={searchPlaceholder(
                      props.locale,
                      t(props.locale, "item"),
                    )}
                    searchQuery={bitwardenItemQuery}
                    selectedIds={selectedBitwardenItemIds}
                    title={t(props.locale, "item")}
                  />

                  {!isBitwardenMultiResourceMode ? (
                    <>
                      <MultiPickerSection
                        caption={sectionCaption(
                          props.locale,
                          `${selectedBitwardenFieldIds.length} selected`,
                          `已选 ${selectedBitwardenFieldIds.length} 个`,
                        )}
                        dataTestId="bitwarden-field-picker"
                        emptyMessage={
                          selectedBitwardenItemId
                            ? isBitwardenFieldsLoading
                              ? sectionCaption(
                                  props.locale,
                                  "Loading fields",
                                  "加载字段中",
                                )
                              : sectionCaption(
                                  props.locale,
                                  "No field suggestions",
                                  "没有可用字段",
                                )
                            : sectionCaption(
                                props.locale,
                                "Select an item first",
                                "先选择条目",
                              )
                        }
                        helper={sectionCaption(
                          props.locale,
                          "Single-resource mode supports selecting multiple fields from the same resource.",
                          "单资源模式支持对同一个资源多选字段。",
                        )}
                        loading={isBitwardenFieldsLoading}
                        locale={props.locale}
                        onToggleSelect={toggleBitwardenField}
                        options={bitwardenFieldOptions}
                        selectedIds={selectedBitwardenFieldIds}
                        title={t(props.locale, "field")}
                      />

                      {surface !== "import" ||
                      bitwardenFieldOptions.length === 0 ? (
                        <section
                          className="detail-section detail-section-low"
                          data-testid="bitwarden-field-fallback"
                        >
                          <div className="detail-section-header">
                            <h3>{t(props.locale, "field")}</h3>
                            <span>
                              {sectionCaption(
                                props.locale,
                                "Minimal fallback",
                                "最小兜底",
                              )}
                            </span>
                          </div>
                          <LocatorField
                            dataTestId="password-field-bitwarden-field"
                            hint={sectionCaption(
                              props.locale,
                              "Keep a manual field fallback for custom names not returned by the CLI picker.",
                              "对 CLI picker 没列出的自定义字段保留最小手填兜底。",
                            )}
                            label={t(props.locale, "field")}
                            onChange={(value) => {
                              setBitwardenDraft((current) => ({
                                ...current,
                                field: value,
                              }));
                            }}
                            optionalLabel={t(props.locale, "optional")}
                            value={bitwardenDraft.field}
                          />
                        </section>
                      ) : null}
                    </>
                  ) : null}
                </>
              ) : null}

              {providerKind === "dotenv_file" ? (
                <>
                  <section
                    className="detail-section"
                    data-testid="dotenv-file-picker"
                  >
                    <div className="detail-section-header">
                      <h3>{t(props.locale, "filePath")}</h3>
                      <span>
                        {sectionCaption(
                          props.locale,
                          "Native chooser",
                          "原生文件选择器",
                        )}
                      </span>
                    </div>
                    <div className="password-actions">
                      <button
                        className="ghost"
                        data-testid="dotenv-choose-file-button"
                        disabled={isDotenvPicking}
                        onClick={() => {
                          void chooseDotenvFile();
                        }}
                        type="button"
                      >
                        {sectionCaption(
                          props.locale,
                          "Choose .env files",
                          "选择 .env 文件（可多选）",
                        )}
                      </button>
                    </div>
                    {dotenvFilePaths.length > 0 ? (
                      <ul className="selected-file-list">
                        {dotenvFilePaths.map((filePath) => (
                          <li key={filePath} title={filePath}>
                            {displayFileName(filePath)}
                          </li>
                        ))}
                      </ul>
                    ) : (
                      <p className="section-copy">
                        {sectionCaption(
                          props.locale,
                          "Select one or more files. Values are never displayed in this step.",
                          "选择一个或多个文件；此步骤不会显示其中的值。",
                        )}
                      </p>
                    )}
                    {surface !== "import" ? (
                      <LocatorField
                        dataTestId="password-field-dotenv-file"
                        hint={sectionCaption(
                          props.locale,
                          "The chooser is the primary path. Keep file path as a minimal fallback only.",
                          "文件选择器是主路径；这里仅保留最小文件路径兜底。",
                        )}
                        label={t(props.locale, "filePath")}
                        onChange={(value) => {
                          setDotenvDraft((current) => ({
                            ...current,
                            filePath: value,
                          }));
                          setDotenvFilePaths(value.trim() ? [value] : []);
                        }}
                        optionalLabel={t(props.locale, "optional")}
                        value={dotenvDraft.filePath}
                      />
                    ) : null}
                  </section>

                  {dotenvFilePaths.length <= 1 ? (
                    <PickerSection
                      caption={sectionCaption(
                        props.locale,
                        surface === "import" ? "Optional" : "Prefix groups",
                        surface === "import" ? "可选" : "前缀分组",
                      )}
                      dataTestId="dotenv-group-picker"
                      emptyMessage={
                        dotenvFilePaths.length === 0
                          ? sectionCaption(
                              props.locale,
                              "Choose a file first",
                              "先选择文件",
                            )
                          : isDotenvInspecting
                            ? sectionCaption(
                                props.locale,
                                "Inspecting file",
                                "分析文件中",
                              )
                            : sectionCaption(
                                props.locale,
                                "No keys found",
                                "没有找到 key",
                              )
                      }
                      loading={isDotenvInspecting}
                      onSelect={selectDotenvGroup}
                      options={dotenvGroupOptions}
                      selectedId={selectedDotenvGroupId}
                      title={sectionCaption(
                        props.locale,
                        surface === "import" ? "Filter keys" : "Import Range",
                        surface === "import" ? "筛选键" : "导入范围",
                      )}
                    />
                  ) : null}

                  <MultiPickerSection
                    caption={sectionCaption(
                      props.locale,
                      `${selectedDotenvKeys.length} selected`,
                      `已选 ${selectedDotenvKeys.length} 个`,
                    )}
                    dataTestId="dotenv-key-picker"
                    emptyMessage={
                      selectedDotenvGroupId
                        ? isDotenvInspecting
                          ? sectionCaption(
                              props.locale,
                              "Loading keys",
                              "加载 key 中",
                            )
                          : sectionCaption(
                              props.locale,
                              "No keys found",
                              "没有找到 key",
                            )
                        : sectionCaption(
                            props.locale,
                            "Choose a group first",
                            "先选择分组",
                          )
                    }
                    helper={sectionCaption(
                      props.locale,
                      "Move keys from Available to Selected. Multiple files are combined here without rendering their values.",
                      "将 key 从“可选择”移到“已选择”；多个文件会合并展示，但不会显示 value。",
                    )}
                    loading={isDotenvInspecting}
                    locale={props.locale}
                    onSearchQueryChange={setDotenvKeyQuery}
                    onToggleSelect={(nextKey) => {
                      const option =
                        visibleDotenvKeys.find(
                          (entry) => entry.id === nextKey,
                        ) ?? null;
                      if (option) {
                        toggleDotenvKey(option);
                      }
                    }}
                    options={dotenvKeyOptions}
                    searchPlaceholder={searchPlaceholder(
                      props.locale,
                      t(props.locale, "key"),
                    )}
                    searchQuery={dotenvKeyQuery}
                    selectedIds={selectedDotenvKeys}
                    title={t(props.locale, "key")}
                  />
                </>
              ) : null}

              <OptionalDetails
                collapsed={surface === "import"}
                summary={sectionCaption(
                  props.locale,
                  "Privacy and refresh behavior",
                  "隐私与刷新方式",
                )}
              >
                <section
                  className="detail-section detail-section-low"
                  data-testid="password-boundaries-section"
                >
                  <div className="detail-section-header">
                    <h3>{t(props.locale, "passwordBoundariesTitle")}</h3>
                    <span>{translateCode(props.locale, providerKind)}</span>
                  </div>
                  <ul
                    className="boundary-list"
                    data-testid="password-boundaries-list"
                  >
                    <li>{t(props.locale, "passwordBoundaryNoLogin")}</li>
                    <li>{t(props.locale, "passwordBoundaryNoSnapshot")}</li>
                    <li>{t(props.locale, "passwordBoundaryNoRegression")}</li>
                  </ul>
                </section>
              </OptionalDetails>
            </div>

            <div className="import-review-summary" role="status">
              <div>
                <strong>
                  {plannedSpecs.length > 0
                    ? sectionCaption(
                        props.locale,
                        `${plannedSpecs.length} field${plannedSpecs.length === 1 ? "" : "s"} ready`,
                        `已准备 ${plannedSpecs.length} 个字段`,
                      )
                    : sectionCaption(
                        props.locale,
                        "Choose what to import",
                        "请选择要导入的内容",
                      )}
                </strong>
                <span>
                  {sectionCaption(
                    props.locale,
                    "Names and resource IDs will be generated automatically.",
                    "名称与资源标识将自动生成。",
                  )}
                </span>
              </div>
            </div>
            <div
              className="password-actions"
              data-testid="password-import-actions"
            >
              {planBlockerMessage ? (
                <p className="empty" data-testid="password-import-blocker">
                  {planBlockerMessage}
                </p>
              ) : null}
              <button
                className="primary"
                data-testid="password-import-submit"
                disabled={!canSubmit || isSubmitting}
                onClick={() => {
                  void submitImport();
                }}
                type="button"
              >
                {isSubmitting
                  ? t(props.locale, "importingSource")
                  : isBatchMode
                    ? sectionCaption(
                        props.locale,
                        "Import Selected",
                        "导入所选项",
                      )
                    : t(props.locale, "importSource")}
              </button>
            </div>

            {importedReceipts.length > 0 ? (
              <section
                className="detail-section detail-section-wide"
                data-testid="password-import-receipt"
              >
                <div className="detail-section-header">
                  <h3>{t(props.locale, "importReceiptTitle")}</h3>
                  <span>
                    {sectionCaption(
                      props.locale,
                      `${importedReceipts.length} receipt(s)`,
                      `${importedReceipts.length} 条回执`,
                    )}
                  </span>
                </div>
                {importedReceipts.length > 1 ? (
                  <ol
                    className="boundary-list"
                    data-testid="password-import-receipt-list"
                  >
                    {importedReceipts.map((entry) => (
                      <li key={entry.reference.resource}>
                        <code>{entry.reference.resource}</code>
                      </li>
                    ))}
                  </ol>
                ) : null}
                <dl className="facts">
                  <div data-testid="password-receipt-resource">
                    <dt>{t(props.locale, "importedResource")}</dt>
                    <dd>{importedReceipts[0].reference.resource}</dd>
                  </div>
                  <div data-testid="password-receipt-catalog-path">
                    <dt>{t(props.locale, "catalogPath")}</dt>
                    <dd>{importedReceipts[0].catalog_path}</dd>
                  </div>
                  <div data-testid="password-receipt-container">
                    <dt>{t(props.locale, "importedContainer")}</dt>
                    <dd>
                      {getImportedContainerLabel(
                        importedReceipts[0].reference,
                      ) ?? t(props.locale, "notAvailable")}
                    </dd>
                  </div>
                  <div data-testid="password-receipt-field">
                    <dt>{t(props.locale, "importedField")}</dt>
                    <dd>
                      {getImportedFieldSelector(importedReceipts[0].reference)}
                    </dd>
                  </div>
                  <div data-testid="password-receipt-imported-at">
                    <dt>{t(props.locale, "importedAt")}</dt>
                    <dd>
                      {formatTimestamp(
                        importedReceipts[0].reference.imported_at,
                        t(props.locale, "notAvailable"),
                        props.locale,
                      )}
                    </dd>
                  </div>
                </dl>
              </section>
            ) : null}
          </section>
        </>
      ) : null}
    </section>
  );
}
