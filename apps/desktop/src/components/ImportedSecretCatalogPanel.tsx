import { SecretInput } from "./SecretInput";
import { useEffect, useMemo, useRef, useState, type JSX } from "react";

import { formatTimestamp } from "../formatters";
import { translateCode, type Locale } from "../i18n";
import type {
  ImportedSecretReference,
  ImportedSecretReferenceUpdate,
  LocalSecretCatalog,
  LocalSecretLiteralEntry,
  LocalSecretLiteralUpsert,
} from "../types";

type ImportedSecretCatalogPanelProps = {
  catalog: LocalSecretCatalog | null;
  errorMessage: string | null;
  isLoading: boolean;
  locale: Locale;
  noticeMessage: string | null;
  onDelete: (resource: string) => Promise<void>;
  onRefreshImported: (resource: string) => Promise<void>;
  onRename: (resource: string, nextResource: string) => Promise<void>;
  onReload: (options?: { silent?: boolean }) => Promise<void>;
  onReveal: (resource: string) => Promise<string>;
  onSaveImported: (update: ImportedSecretReferenceUpdate) => Promise<void>;
  onSaveLiteral: (entry: LocalSecretLiteralUpsert) => Promise<void>;
};

type CatalogLeafEntry =
  | {
      kind: "literal";
      resource: string;
      literal: LocalSecretLiteralEntry;
    }
  | {
      kind: "imported";
      resource: string;
      reference: ImportedSecretReference;
    };

type CatalogTreeNode = {
  id: string;
  label: string;
  resource: string | null;
  entry: CatalogLeafEntry | null;
  leafCount: number;
  children: CatalogTreeNode[];
};

type CatalogTreeMode = "item" | "resource";

type ImportedEditorDraft = {
  resource: string;
  displayName: string;
  description: string;
  tags: string;
  metadata: string;
};

type LiteralEditorDraft = {
  resource: string;
  value: string;
  displayName: string;
  description: string;
  tags: string;
  metadata: string;
};

const EMPTY_IMPORTED_DRAFT: ImportedEditorDraft = {
  resource: "",
  displayName: "",
  description: "",
  tags: "",
  metadata: "",
};

const EMPTY_LITERAL_DRAFT: LiteralEditorDraft = {
  resource: "",
  value: "",
  displayName: "",
  description: "",
  tags: "",
  metadata: "",
};

function caption(locale: Locale, english: string, chinese: string): string {
  return locale === "zh-CN" ? chinese : english;
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

function formatMetadataDraft(
  metadata: Record<string, string> | undefined,
): string {
  return Object.entries(metadata ?? {})
    .sort(([left], [right]) => left.localeCompare(right))
    .map(([key, value]) => `${key}=${value}`)
    .join("\n");
}

function buildImportedEditorDraft(
  reference: ImportedSecretReference,
): ImportedEditorDraft {
  return {
    resource: reference.resource,
    displayName: reference.display_name,
    description: reference.description ?? "",
    tags: (reference.tags ?? []).join(", "),
    metadata: formatMetadataDraft(reference.metadata),
  };
}

function buildCatalogEntries(
  catalog: LocalSecretCatalog | null,
): CatalogLeafEntry[] {
  if (!catalog) {
    return [];
  }

  return [
    ...catalog.literals.map((literal) => ({
      kind: "literal" as const,
      resource: literal.resource,
      literal,
    })),
    ...catalog.imports.map((reference) => ({
      kind: "imported" as const,
      resource: reference.resource,
      reference,
    })),
  ];
}

function buildLiteralEditorDraft(
  literal: LocalSecretLiteralEntry,
): LiteralEditorDraft {
  return {
    resource: literal.resource,
    value: literal.value,
    displayName: literal.display_name ?? "",
    description: literal.description ?? "",
    tags: (literal.tags ?? []).join(", "),
    metadata: formatMetadataDraft(literal.metadata),
  };
}

function buildCatalogTreeFromSegments(
  entries: CatalogLeafEntry[],
  segmentsForEntry: (entry: CatalogLeafEntry) => string[],
): CatalogTreeNode[] {
  type MutableNode = {
    id: string;
    label: string;
    resource: string | null;
    entry: CatalogLeafEntry | null;
    leafCount: number;
    children: Map<string, MutableNode>;
  };

  const root = new Map<string, MutableNode>();

  for (const entry of entries) {
    const segments = segmentsForEntry(entry).filter(Boolean);
    let current = root;
    let currentPath = "";

    for (const [index, segment] of segments.entries()) {
      currentPath =
        currentPath.length > 0 ? `${currentPath}/${segment}` : segment;
      let node = current.get(segment);
      if (!node) {
        node = {
          id: currentPath,
          label: segment,
          resource: null,
          entry: null,
          leafCount: 0,
          children: new Map(),
        };
        current.set(segment, node);
      }

      node.leafCount += 1;
      if (index === segments.length - 1) {
        node.resource = entry.resource;
        node.entry = entry;
      } else {
        current = node.children;
      }
    }
  }

  function materialize(nodes: Map<string, MutableNode>): CatalogTreeNode[] {
    return Array.from(nodes.values())
      .sort((left, right) => left.label.localeCompare(right.label))
      .map((node) => ({
        id: node.id,
        label: node.label,
        resource: node.resource,
        entry: node.entry,
        leafCount: node.leafCount,
        children: materialize(node.children),
      }));
  }

  return materialize(root);
}

function buildResourceTree(entries: CatalogLeafEntry[]): CatalogTreeNode[] {
  return buildCatalogTreeFromSegments(entries, (entry) =>
    entry.resource.split("/").filter(Boolean),
  );
}

function buildItemTree(
  entries: CatalogLeafEntry[],
  locale: Locale,
): CatalogTreeNode[] {
  return buildCatalogTreeFromSegments(entries, (entry) => {
    if (entry.kind === "literal") {
      return [
        caption(locale, "Local values", "本地值"),
        entry.literal.display_name || entry.resource,
      ];
    }

    const reference = entry.reference;
    if (reference.provider_kind === "1password_cli") {
      return ["1Password", reference.vault, reference.item, reference.field];
    }
    if (reference.provider_kind === "bitwarden_cli") {
      return [
        "Bitwarden",
        reference.collection ??
          reference.folder ??
          reference.organization ??
          reference.account,
        reference.item,
        reference.field,
      ];
    }
    if (reference.provider_kind === "dotenv_file") {
      const pathSegments = reference.file_path.split(/[\\/]/).filter(Boolean);
      const fileName = pathSegments.at(-1) ?? ".env";
      const parentName = pathSegments.at(-2);
      const meaningfulFileName =
        fileName === ".env" && parentName
          ? caption(
              locale,
              `${parentName} environment`,
              `${parentName} 环境变量`,
            )
          : fileName;
      const sourceName =
        reference.metadata?.item_title ??
        reference.namespace ??
        reference.prefix ??
        meaningfulFileName;
      return [".env", sourceName, reference.key];
    }

    return [
      "Plankton",
      reference.metadata?.item_title ?? reference.entry,
      reference.metadata?.field_label ??
        reference.metadata?.field_key ??
        reference.resource.split("/").filter(Boolean).at(-1) ??
        reference.field,
    ];
  });
}

function importedSearchText(reference: ImportedSecretReference): string {
  const parts = [
    reference.resource,
    reference.display_name,
    reference.description ?? "",
    ...(reference.tags ?? []),
    ...Object.entries(reference.metadata ?? {}).flatMap(([key, value]) => [
      key,
      value,
      `${key}=${value}`,
    ]),
  ];

  if (reference.provider_kind === "1password_cli") {
    parts.push(
      reference.account,
      reference.vault,
      reference.item,
      reference.field,
      reference.account_id ?? "",
      reference.vault_id ?? "",
      reference.item_id ?? "",
      reference.field_id ?? "",
    );
  } else if (reference.provider_kind === "bitwarden_cli") {
    parts.push(
      reference.account,
      reference.organization ?? "",
      reference.collection ?? "",
      reference.folder ?? "",
      reference.item,
      reference.field,
      reference.item_id ?? "",
    );
  } else if (reference.provider_kind === "keepassxc_cli") {
    parts.push(reference.database, reference.entry, reference.field);
  } else {
    parts.push(
      reference.file_path,
      reference.namespace ?? "",
      reference.prefix ?? "",
      reference.key,
    );
  }

  return parts.join("\n").toLowerCase();
}

function matchesSearch(entry: CatalogLeafEntry, query: string): boolean {
  const normalizedQuery = query.trim().toLowerCase();
  if (normalizedQuery.length === 0) {
    return true;
  }

  if (entry.kind === "literal") {
    return [
      entry.resource,
      entry.literal.display_name ?? "",
      entry.literal.description ?? "",
      ...(entry.literal.tags ?? []),
      ...Object.entries(entry.literal.metadata ?? {}).flatMap(
        ([key, value]) => [key, value, `${key}=${value}`],
      ),
    ]
      .join("\n")
      .toLowerCase()
      .includes(normalizedQuery);
  }

  return importedSearchText(entry.reference).includes(normalizedQuery);
}

function importedLocatorEntries(
  locale: Locale,
  reference: ImportedSecretReference,
): Array<{ label: string; value: string }> {
  if (reference.provider_kind === "1password_cli") {
    return [
      { label: caption(locale, "Account", "账号"), value: reference.account },
      { label: caption(locale, "Vault", "保险库"), value: reference.vault },
      { label: caption(locale, "Item", "条目"), value: reference.item },
      { label: caption(locale, "Field", "字段"), value: reference.field },
      ...(reference.account_id
        ? [
            {
              label: caption(locale, "Account ID", "账号 ID"),
              value: reference.account_id,
            },
          ]
        : []),
      ...(reference.vault_id
        ? [
            {
              label: caption(locale, "Vault ID", "保险库 ID"),
              value: reference.vault_id,
            },
          ]
        : []),
      ...(reference.item_id
        ? [
            {
              label: caption(locale, "Item ID", "条目 ID"),
              value: reference.item_id,
            },
          ]
        : []),
      ...(reference.field_id
        ? [
            {
              label: caption(locale, "Field ID", "字段 ID"),
              value: reference.field_id,
            },
          ]
        : []),
    ];
  }

  if (reference.provider_kind === "bitwarden_cli") {
    return [
      { label: caption(locale, "Account", "账号"), value: reference.account },
      ...(reference.organization
        ? [
            {
              label: caption(locale, "Organization", "组织"),
              value: reference.organization,
            },
          ]
        : []),
      ...(reference.collection
        ? [
            {
              label: caption(locale, "Collection", "集合"),
              value: reference.collection,
            },
          ]
        : []),
      ...(reference.folder
        ? [
            {
              label: caption(locale, "Folder", "文件夹"),
              value: reference.folder,
            },
          ]
        : []),
      { label: caption(locale, "Item", "条目"), value: reference.item },
      { label: caption(locale, "Field", "字段"), value: reference.field },
      ...(reference.item_id
        ? [
            {
              label: caption(locale, "Item ID", "条目 ID"),
              value: reference.item_id,
            },
          ]
        : []),
    ];
  }

  if (reference.provider_kind === "keepassxc_cli") {
    return [
      {
        label: caption(locale, "Database", "数据库"),
        value: reference.database,
      },
      { label: caption(locale, "Item", "条目"), value: reference.entry },
      { label: caption(locale, "Field", "字段"), value: reference.field },
    ];
  }

  return [
    {
      label: caption(locale, "File Path", "文件路径"),
      value: reference.file_path,
    },
    ...(reference.namespace
      ? [
          {
            label: caption(locale, "Namespace", "命名空间"),
            value: reference.namespace,
          },
        ]
      : []),
    ...(reference.prefix
      ? [{ label: caption(locale, "Prefix", "前缀"), value: reference.prefix }]
      : []),
    { label: caption(locale, "Key", "键名"), value: reference.key },
  ];
}

function importedLeafSubtitle(reference: ImportedSecretReference): string {
  return reference.display_name;
}

function literalLeafSubtitle(literal: LocalSecretLiteralEntry): string {
  return literal.display_name ?? literal.description ?? "";
}

function importedTagCount(reference: ImportedSecretReference): number {
  return reference.tags?.length ?? 0;
}

function literalTagCount(literal: LocalSecretLiteralEntry): number {
  return literal.tags?.length ?? 0;
}

function importedDraftChanged(
  reference: ImportedSecretReference,
  draft: ImportedEditorDraft,
  metadata: Record<string, string>,
): boolean {
  const normalizedTags = parseTags(draft.tags);
  const currentTags = reference.tags ?? [];
  const currentMetadata = reference.metadata ?? {};

  if (reference.display_name !== draft.displayName.trim()) {
    return true;
  }
  if ((reference.description ?? "") !== draft.description.trim()) {
    return true;
  }
  if (normalizedTags.join("\n") !== currentTags.join("\n")) {
    return true;
  }

  const leftMetadata = JSON.stringify(
    Object.entries(metadata).sort(([left], [right]) =>
      left.localeCompare(right),
    ),
  );
  const rightMetadata = JSON.stringify(
    Object.entries(currentMetadata).sort(([left], [right]) =>
      left.localeCompare(right),
    ),
  );
  return leftMetadata !== rightMetadata;
}

function literalEditModeDescription(
  locale: Locale,
  selectedEntry: CatalogLeafEntry | null,
): string {
  if (selectedEntry?.kind === "imported") {
    return caption(
      locale,
      "Saving a local value for this resource will replace the imported locator entry. Use this when you want to edit the concrete secret value locally.",
      "为这个资源保存本地值后，会替换掉原来的导入 locator。需要直接编辑具体密钥值时，请使用这个入口。",
    );
  }

  return caption(
    locale,
    "Manual secrets store a local value directly in the secret catalog.",
    "手工密钥会把值直接保存在本地 secret catalog 中。",
  );
}

function literalDraftChanged(
  literal: LocalSecretLiteralEntry,
  draft: LiteralEditorDraft,
  metadata: Record<string, string>,
): boolean {
  const normalizedTags = parseTags(draft.tags);
  const currentTags = literal.tags ?? [];
  const currentMetadata = literal.metadata ?? {};

  if (literal.value !== draft.value) {
    return true;
  }
  if ((literal.display_name ?? "") !== draft.displayName.trim()) {
    return true;
  }
  if ((literal.description ?? "") !== draft.description.trim()) {
    return true;
  }
  if (normalizedTags.join("\n") !== currentTags.join("\n")) {
    return true;
  }

  const leftMetadata = JSON.stringify(
    Object.entries(metadata).sort(([left], [right]) =>
      left.localeCompare(right),
    ),
  );
  const rightMetadata = JSON.stringify(
    Object.entries(currentMetadata).sort(([left], [right]) =>
      left.localeCompare(right),
    ),
  );
  return leftMetadata !== rightMetadata;
}

function TreeBranch(props: {
  locale: Locale;
  nodes: CatalogTreeNode[];
  selectedResource: string | null;
  onSelect: (resource: string) => void;
}): JSX.Element {
  return (
    <ol className="imported-tree-list">
      {props.nodes.map((node) => (
        <li className="imported-tree-node" key={node.id}>
          {node.entry ? (
            <button
              className={`queue-item imported-tree-leaf ${
                props.selectedResource === node.entry.resource ? "active" : ""
              }`}
              data-resource={node.entry.resource}
              data-testid="imported-secret-tree-leaf"
              onClick={() => {
                props.onSelect(node.entry!.resource);
              }}
              type="button"
            >
              <div className="queue-item-header">
                <strong>{node.label}</strong>
                <span>
                  {node.entry.kind === "literal"
                    ? caption(props.locale, "Manual", "手工")
                    : translateCode(
                        props.locale,
                        node.entry.reference.provider_kind,
                      )}
                </span>
              </div>
              <div className="queue-item-meta">
                <span>
                  {node.entry.kind === "literal"
                    ? literalLeafSubtitle(node.entry.literal) ||
                      caption(props.locale, "Local secret value", "本地密钥值")
                    : importedLeafSubtitle(node.entry.reference)}
                </span>
                <span>
                  {node.entry.kind === "literal"
                    ? `${literalTagCount(node.entry.literal)} ${caption(props.locale, "tag(s)", "个标签")}`
                    : `${importedTagCount(node.entry.reference)} ${caption(props.locale, "tag(s)", "个标签")}`}
                </span>
              </div>
            </button>
          ) : (
            <>
              <div className="imported-tree-branch">
                <strong>{node.label}</strong>
                <span>{node.leafCount}</span>
              </div>
              {node.children.length > 0 ? (
                <TreeBranch
                  locale={props.locale}
                  nodes={node.children}
                  onSelect={props.onSelect}
                  selectedResource={props.selectedResource}
                />
              ) : null}
            </>
          )}
        </li>
      ))}
    </ol>
  );
}

export function ImportedSecretCatalogPanel(
  props: ImportedSecretCatalogPanelProps,
): JSX.Element {
  const [searchQuery, setSearchQuery] = useState("");
  const [treeMode, setTreeMode] = useState<CatalogTreeMode>("item");
  const [selectedResource, setSelectedResource] = useState<string | null>(null);
  const [isCreatingLiteral, setIsCreatingLiteral] = useState(false);
  const [isSaving, setIsSaving] = useState(false);
  const [isDeleting, setIsDeleting] = useState(false);
  const [isRefreshing, setIsRefreshing] = useState(false);
  const [isLocatorOpen, setIsLocatorOpen] = useState(false);
  const [revealEpoch, setRevealEpoch] = useState(0);
  const revealGeneration = useRef(0);
  const [revealedValue, setRevealedValue] = useState<string | null>(null);
  const [revealError, setRevealError] = useState<string | null>(null);
  const [importedDraft, setImportedDraft] =
    useState<ImportedEditorDraft>(EMPTY_IMPORTED_DRAFT);
  const [literalDraft, setLiteralDraft] =
    useState<LiteralEditorDraft>(EMPTY_LITERAL_DRAFT);

  const entries = useMemo(
    () => buildCatalogEntries(props.catalog),
    [props.catalog],
  );
  const filteredEntries = useMemo(
    () => entries.filter((entry) => matchesSearch(entry, searchQuery)),
    [entries, searchQuery],
  );
  const tree = useMemo(
    () =>
      treeMode === "item"
        ? buildItemTree(filteredEntries, props.locale)
        : buildResourceTree(filteredEntries),
    [filteredEntries, props.locale, treeMode],
  );
  const selectedEntry =
    entries.find((entry) => entry.resource === selectedResource) ?? null;
  const metadataDraft = parseMetadataDraft(importedDraft.metadata);
  const literalMetadataDraft = parseMetadataDraft(literalDraft.metadata);

  useEffect(() => {
    if (isCreatingLiteral) {
      return;
    }

    if (
      selectedResource &&
      entries.some((entry) => entry.resource === selectedResource)
    ) {
      return;
    }

    setSelectedResource(entries[0]?.resource ?? null);
  }, [entries, isCreatingLiteral, selectedResource]);

  useEffect(() => {
    revealGeneration.current += 1;
    setRevealEpoch((epoch) => epoch + 1);
    setIsLocatorOpen(false);
    setRevealedValue(null);
    setRevealError(null);
  }, [selectedResource, isCreatingLiteral]);

  useEffect(() => {
    if (isCreatingLiteral) {
      return;
    }

    if (!selectedEntry) {
      setImportedDraft(EMPTY_IMPORTED_DRAFT);
      setLiteralDraft(EMPTY_LITERAL_DRAFT);
      return;
    }

    if (selectedEntry.kind === "literal") {
      setLiteralDraft(buildLiteralEditorDraft(selectedEntry.literal));
      return;
    }

    setImportedDraft(buildImportedEditorDraft(selectedEntry.reference));
  }, [isCreatingLiteral, selectedEntry]);

  function selectEntry(resource: string): void {
    setIsCreatingLiteral(false);
    setSelectedResource(resource);
  }

  function startCreatingLiteral(resource?: string): void {
    const selectedImported =
      resource && selectedEntry?.kind === "imported"
        ? selectedEntry.reference
        : null;
    setIsCreatingLiteral(true);
    setSelectedResource(resource ?? null);
    setLiteralDraft({
      resource: resource ?? "",
      value: "",
      displayName: selectedImported?.display_name ?? "",
      description: selectedImported?.description ?? "",
      tags: (selectedImported?.tags ?? []).join(", "),
      metadata: formatMetadataDraft(selectedImported?.metadata),
    });
  }

  function cancelLiteralEdit(): void {
    setIsCreatingLiteral(false);
  }

  async function handleSaveImported(): Promise<void> {
    if (
      !selectedEntry ||
      selectedEntry.kind !== "imported" ||
      metadataDraft.invalidLines.length > 0
    ) {
      return;
    }

    setIsSaving(true);
    try {
      await props.onSaveImported({
        resource: selectedEntry.reference.resource,
        display_name: importedDraft.displayName.trim(),
        description: importedDraft.description.trim(),
        tags: parseTags(importedDraft.tags),
        metadata: metadataDraft.metadata,
      });
    } finally {
      setIsSaving(false);
    }
  }

  async function handleReveal(): Promise<boolean> {
    const resource = isCreatingLiteral
      ? literalDraft.resource.trim()
      : selectedEntry?.resource;
    if (!resource) return false;
    if (
      revealedValue !== null ||
      (literalDraft.value !== "" && selectedEntry?.kind === "literal")
    )
      return true;
    setRevealError(null);
    const generation = revealGeneration.current;
    try {
      const value = await props.onReveal(resource);
      if (generation !== revealGeneration.current) return false;
      setRevealedValue(value);
      if (selectedEntry?.kind === "literal") {
        setLiteralDraft((current) => ({ ...current, value }));
      }
      return true;
    } catch {
      if (generation !== revealGeneration.current) return false;
      setRevealError(
        caption(
          props.locale,
          "Secret value could not be revealed. Check Diagnostics and try again.",
          "无法显示密钥值。请查看诊断信息后重试。",
        ),
      );
      return false;
    }
  }

  async function handleSaveLiteral(): Promise<void> {
    const resource = isCreatingLiteral
      ? literalDraft.resource.trim()
      : (selectedEntry?.resource ?? "");
    if (
      resource.length === 0 ||
      literalDraft.value.length === 0 ||
      literalMetadataDraft.invalidLines.length > 0
    ) {
      return;
    }

    setIsSaving(true);
    try {
      await props.onSaveLiteral({
        resource,
        value: literalDraft.value,
        display_name: literalDraft.displayName.trim() || null,
        description: literalDraft.description.trim() || null,
        tags: parseTags(literalDraft.tags),
        metadata: literalMetadataDraft.metadata,
      });
      setIsCreatingLiteral(false);
      setSelectedResource(resource);
    } finally {
      setIsSaving(false);
    }
  }

  async function handleRename(): Promise<void> {
    if (!selectedEntry) return;
    const nextResource =
      selectedEntry.kind === "imported"
        ? importedDraft.resource.trim()
        : literalDraft.resource.trim();
    if (!nextResource || nextResource === selectedEntry.resource) return;

    const confirmed = window.confirm(
      caption(
        props.locale,
        `Move ${selectedEntry.resource} to ${nextResource}? Existing CLI and policy references to the old key will no longer resolve.`,
        `确认将 ${selectedEntry.resource} 移动到 ${nextResource}？现有 CLI 与策略中的旧 key 将无法继续解析。`,
      ),
    );
    if (!confirmed) return;

    setIsSaving(true);
    try {
      await props.onRename(selectedEntry.resource, nextResource);
      setSelectedResource(nextResource);
    } finally {
      setIsSaving(false);
    }
  }

  async function handleDelete(): Promise<void> {
    const resource = isCreatingLiteral
      ? literalDraft.resource.trim()
      : selectedEntry?.resource;
    if (!resource) {
      return;
    }

    const confirmed = window.confirm(
      caption(props.locale, `Delete ${resource}?`, `确认删除 ${resource}？`),
    );
    if (!confirmed) {
      return;
    }

    setIsDeleting(true);
    try {
      await props.onDelete(resource);
      if (selectedResource === resource) {
        setSelectedResource(null);
      }
      if (isCreatingLiteral) {
        setLiteralDraft(EMPTY_LITERAL_DRAFT);
      }
    } finally {
      setIsDeleting(false);
    }
  }

  async function handleRefreshImported(): Promise<void> {
    if (!selectedEntry || selectedEntry.kind !== "imported") {
      return;
    }

    setIsRefreshing(true);
    try {
      await props.onRefreshImported(selectedEntry.reference.resource);
      revealGeneration.current += 1;
      setRevealEpoch((epoch) => epoch + 1);
      setRevealedValue(null);
    } finally {
      setIsRefreshing(false);
    }
  }

  const canSaveImported =
    selectedEntry?.kind === "imported" &&
    metadataDraft.invalidLines.length === 0 &&
    importedDraftChanged(
      selectedEntry.reference,
      importedDraft,
      metadataDraft.metadata,
    ) &&
    !isSaving &&
    !isDeleting &&
    !isRefreshing;

  const canSaveLiteral = (() => {
    if (isSaving || isDeleting) {
      return false;
    }
    const resource = literalDraft.resource.trim();
    if (
      resource.length === 0 ||
      literalDraft.value.length === 0 ||
      literalMetadataDraft.invalidLines.length > 0
    ) {
      return false;
    }
    if (!selectedEntry || selectedEntry.kind !== "literal") {
      return true;
    }
    return literalDraftChanged(
      selectedEntry.literal,
      literalDraft,
      literalMetadataDraft.metadata,
    );
  })();
  const canRename =
    !isSaving &&
    !isDeleting &&
    !isRefreshing &&
    selectedEntry !== null &&
    (selectedEntry.kind === "imported"
      ? importedDraft.resource.trim()
      : literalDraft.resource.trim()) !== selectedEntry.resource &&
    (selectedEntry.kind === "imported"
      ? importedDraft.resource.trim()
      : literalDraft.resource.trim()
    ).length > 0;

  const totalCount = entries.length;
  const filteredCount = filteredEntries.length;

  return (
    <section
      className="detail-section detail-section-wide"
      data-testid="imported-secret-catalog-panel"
    >
      <div className="detail-section-header">
        <h3>{caption(props.locale, "Local Secret Catalog", "本地密钥目录")}</h3>
        <span>
          {props.catalog?.catalog_path ??
            caption(props.locale, "n/a", "不可用")}
        </span>
      </div>
      <p className="section-copy">
        {caption(
          props.locale,
          "Manage imported snapshots and manual local secret values in one tree. Imported entries keep a local value snapshot plus the upstream locator for refresh.",
          "通过一棵树统一管理导入快照和手工本地密钥值。导入项会保存本地值快照，并保留上游 locator 以便刷新。",
        )}
      </p>

      {props.errorMessage ? (
        <section
          className="alert"
          data-testid="imported-secret-catalog-error"
          role="alert"
        >
          <p>{props.errorMessage}</p>
        </section>
      ) : null}

      {props.noticeMessage ? (
        <section className="alert" data-testid="imported-secret-catalog-notice">
          <p>{props.noticeMessage}</p>
        </section>
      ) : null}

      {revealError ? (
        <section
          className="alert"
          data-testid="imported-secret-reveal-error"
          role="alert"
        >
          <p>{revealError}</p>
        </section>
      ) : null}

      <div className="catalog-toolbar">
        <input
          className="settings-input imported-search-input"
          data-testid="imported-secret-search"
          onChange={(event) => {
            setSearchQuery(event.currentTarget.value);
          }}
          placeholder={caption(
            props.locale,
            "Search resource path, display name, tag, metadata, or locator value",
            "搜索资源路径、显示名、标签、元信息或 locator 值",
          )}
          type="search"
          value={searchQuery}
        />
        <div className="catalog-toolbar-actions">
          <button
            className="ghost"
            disabled={props.isLoading}
            onClick={() => {
              void props.onReload();
            }}
            type="button"
          >
            {caption(props.locale, "Refresh", "刷新")}
          </button>
          <button
            className="ghost"
            data-testid="local-secret-create"
            onClick={() => {
              startCreatingLiteral();
            }}
            type="button"
          >
            {caption(props.locale, "Manual Add", "手工添加")}
          </button>
        </div>
      </div>

      <div className="imported-catalog-layout">
        <section
          className="detail-section"
          data-testid="imported-secret-tree-panel"
        >
          <div className="detail-section-header">
            <h3>
              {treeMode === "item"
                ? caption(props.locale, "Item Tree", "条目树")
                : caption(props.locale, "Resource Key Tree", "资源 KEY 树")}
            </h3>
            <div className="catalog-tree-heading-actions">
              <div
                aria-label={caption(
                  props.locale,
                  "Catalog tree view",
                  "目录树视图",
                )}
                className="catalog-tree-mode-switcher"
              >
                <button
                  aria-pressed={treeMode === "item"}
                  className={treeMode === "item" ? "active" : ""}
                  data-testid="catalog-tree-mode-item"
                  onClick={() => setTreeMode("item")}
                  type="button"
                >
                  {caption(props.locale, "By item", "按条目")}
                </button>
                <button
                  aria-pressed={treeMode === "resource"}
                  className={treeMode === "resource" ? "active" : ""}
                  data-testid="catalog-tree-mode-resource"
                  onClick={() => setTreeMode("resource")}
                  type="button"
                >
                  {caption(props.locale, "By resource key", "按资源 KEY")}
                </button>
              </div>
              <span>
                {props.isLoading
                  ? caption(props.locale, "Loading", "加载中")
                  : `${filteredCount}/${totalCount}`}
              </span>
            </div>
          </div>

          {props.isLoading ? (
            <p className="empty" data-testid="imported-secret-tree-loading">
              {caption(props.locale, "Loading", "加载中")}
            </p>
          ) : tree.length === 0 ? (
            <p className="empty" data-testid="imported-secret-tree-empty">
              {searchQuery.trim().length > 0
                ? caption(
                    props.locale,
                    "No secrets match the current search",
                    "当前搜索没有匹配的密钥",
                  )
                : caption(
                    props.locale,
                    "No local secrets yet",
                    "还没有本地密钥",
                  )}
            </p>
          ) : (
            <TreeBranch
              locale={props.locale}
              nodes={tree}
              onSelect={selectEntry}
              selectedResource={selectedResource}
            />
          )}
        </section>

        <section
          className="detail-section"
          data-testid="imported-secret-detail-panel"
        >
          <div className="detail-section-header">
            <h3>{caption(props.locale, "Secret Details", "密钥详情")}</h3>
            <span>
              {isCreatingLiteral
                ? caption(props.locale, "Manual", "手工")
                : selectedEntry?.kind === "literal"
                  ? caption(props.locale, "Manual", "手工")
                  : selectedEntry
                    ? translateCode(
                        props.locale,
                        selectedEntry.reference.provider_kind,
                      )
                    : caption(props.locale, "n/a", "不可用")}
            </span>
          </div>

          {!selectedEntry && !isCreatingLiteral ? (
            <p className="empty">
              {caption(
                props.locale,
                "Select an entry from the tree, or add a manual secret.",
                "请从左侧资源树选择一条记录，或手工新增一个密钥。",
              )}
            </p>
          ) : isCreatingLiteral || selectedEntry?.kind === "literal" ? (
            <div className="panel-stack">
              <p className="section-copy">
                {literalEditModeDescription(props.locale, selectedEntry)}
              </p>

              <div className="settings-form-grid">
                <label
                  className="settings-field"
                  data-testid="local-secret-resource"
                >
                  <span className="field-label">
                    {caption(props.locale, "Resource", "资源标识")}
                  </span>
                  <input
                    className="settings-input"
                    onChange={(event) => {
                      const nextValue = event.currentTarget.value;
                      setLiteralDraft((current) => ({
                        ...current,
                        resource: nextValue,
                      }));
                    }}
                    type="text"
                    value={literalDraft.resource}
                  />
                </label>

                <label
                  className="settings-field"
                  data-testid="local-secret-display-name"
                >
                  <span className="field-label">
                    {caption(props.locale, "Display Name", "显示名称")}
                  </span>
                  <input
                    className="settings-input"
                    onChange={(event) => {
                      const nextValue = event.currentTarget.value;
                      setLiteralDraft((current) => ({
                        ...current,
                        displayName: nextValue,
                      }));
                    }}
                    type="text"
                    value={literalDraft.displayName}
                  />
                </label>

                <label
                  className="settings-field"
                  data-testid="local-secret-description"
                >
                  <span className="field-label">
                    {caption(props.locale, "Description", "描述")}
                  </span>
                  <input
                    className="settings-input"
                    onChange={(event) => {
                      const nextValue = event.currentTarget.value;
                      setLiteralDraft((current) => ({
                        ...current,
                        description: nextValue,
                      }));
                    }}
                    type="text"
                    value={literalDraft.description}
                  />
                </label>

                <label
                  className="settings-field"
                  data-testid="local-secret-tags"
                >
                  <span className="field-label">
                    {caption(props.locale, "Tags", "标签")}
                  </span>
                  <input
                    className="settings-input"
                    onChange={(event) => {
                      const nextValue = event.currentTarget.value;
                      setLiteralDraft((current) => ({
                        ...current,
                        tags: nextValue,
                      }));
                    }}
                    type="text"
                    value={literalDraft.tags}
                  />
                </label>

                <label
                  className="settings-field settings-field-wide"
                  data-testid="local-secret-metadata"
                >
                  <span className="field-label">
                    {caption(props.locale, "Metadata", "元信息")}
                  </span>
                  <textarea
                    className="settings-input note-field"
                    onChange={(event) => {
                      const nextValue = event.currentTarget.value;
                      setLiteralDraft((current) => ({
                        ...current,
                        metadata: nextValue,
                      }));
                    }}
                    value={literalDraft.metadata}
                  />
                  <span className="field-hint">
                    {literalMetadataDraft.invalidLines.length > 0
                      ? `${caption(props.locale, "Use KEY=VALUE lines.", "请使用 KEY=VALUE 格式。")} (${literalMetadataDraft.invalidLines.join(", ")})`
                      : caption(
                          props.locale,
                          "Use one KEY=VALUE pair per line.",
                          "每行使用一个 KEY=VALUE。",
                        )}
                  </span>
                </label>

                <label
                  className="settings-field settings-field-wide"
                  data-testid="local-secret-value"
                >
                  <span className="field-label">
                    {caption(props.locale, "Secret Value", "密钥值")}
                  </span>
                  <SecretInput
                    key={isCreatingLiteral ? "new" : selectedEntry?.resource}
                    locale={props.locale}
                    fieldName={caption(props.locale, "secret value", "密钥值")}
                    resetKey={revealEpoch}
                    onReveal={isCreatingLiteral ? undefined : handleReveal}
                    className="settings-input"
                    onChange={(event) => {
                      const nextValue = event.currentTarget.value;
                      setLiteralDraft((current) => ({
                        ...current,
                        value: nextValue,
                      }));
                    }}
                    placeholder={caption(
                      props.locale,
                      "Reveal to edit or enter a replacement value",
                      "显示后编辑，或直接输入替换值",
                    )}
                    value={literalDraft.value}
                  />
                </label>
              </div>

              <div className="actions">
                {!isCreatingLiteral ? (
                  <button
                    className="ghost"
                    data-testid="local-secret-rename"
                    disabled={!canRename}
                    onClick={() => {
                      void handleRename();
                    }}
                    type="button"
                  >
                    {caption(props.locale, "Move Resource", "移动资源")}
                  </button>
                ) : null}
                <button
                  className="primary"
                  data-testid="local-secret-save"
                  disabled={!canSaveLiteral}
                  onClick={() => {
                    void handleSaveLiteral();
                  }}
                  type="button"
                >
                  {isSaving
                    ? caption(props.locale, "Saving...", "保存中...")
                    : caption(props.locale, "Save", "保存")}
                </button>
                {isCreatingLiteral ? (
                  <button
                    className="ghost"
                    data-testid="local-secret-cancel"
                    disabled={isSaving || isDeleting}
                    onClick={cancelLiteralEdit}
                    type="button"
                  >
                    {caption(props.locale, "Cancel", "取消")}
                  </button>
                ) : null}
                {!isCreatingLiteral ? (
                  <button
                    className="danger"
                    data-testid="local-secret-delete"
                    disabled={isSaving || isDeleting}
                    onClick={() => {
                      void handleDelete();
                    }}
                    type="button"
                  >
                    {isDeleting
                      ? caption(props.locale, "Deleting...", "删除中...")
                      : caption(props.locale, "Delete", "删除")}
                  </button>
                ) : null}
              </div>
            </div>
          ) : selectedEntry ? (
            <div className="panel-stack">
              <dl className="facts">
                <div>
                  <dt>{caption(props.locale, "Resource", "资源标识")}</dt>
                  <dd>{selectedEntry.reference.resource}</dd>
                </div>
                <div>
                  <dt>{caption(props.locale, "Display Name", "显示名称")}</dt>
                  <dd>{selectedEntry.reference.display_name}</dd>
                </div>
                <div>
                  <dt>{caption(props.locale, "Imported At", "导入时间")}</dt>
                  <dd>
                    {formatTimestamp(
                      selectedEntry.reference.imported_at,
                      caption(props.locale, "n/a", "不可用"),
                      props.locale,
                    )}
                  </dd>
                </div>
                <div>
                  <dt>
                    {caption(props.locale, "Last Verified", "最近校验时间")}
                  </dt>
                  <dd>
                    {formatTimestamp(
                      selectedEntry.reference.last_verified_at ?? null,
                      caption(props.locale, "n/a", "不可用"),
                      props.locale,
                    )}
                  </dd>
                </div>
              </dl>

              <p className="section-copy">
                {caption(
                  props.locale,
                  "Values stay concealed until you explicitly reveal one. Refresh revalidates the registered source.",
                  "值会保持隐藏，直到你明确点击显示。刷新会重新校验已登记的来源。",
                )}
              </p>

              <div className="settings-form-grid">
                <label
                  className="settings-field settings-field-wide"
                  data-testid="imported-secret-resource"
                >
                  <span className="field-label">
                    {caption(props.locale, "Resource Key", "资源 key")}
                  </span>
                  <input
                    className="settings-input"
                    onChange={(event) => {
                      const nextValue = event.currentTarget.value;
                      setImportedDraft((current) => ({
                        ...current,
                        resource: nextValue,
                      }));
                    }}
                    type="text"
                    value={importedDraft.resource}
                  />
                  <span className="field-hint">
                    {caption(
                      props.locale,
                      "Moving a resource changes the key used by CLI, policies, and agents. The refresh locator is preserved.",
                      "移动资源会改变 CLI、策略和智能体使用的 key；刷新 locator 会保持不变。",
                    )}
                  </span>
                </label>
                <label
                  className="settings-field settings-field-wide"
                  data-testid="imported-secret-value"
                >
                  <span className="field-label">
                    {caption(props.locale, "Secret Value", "密钥值")}
                  </span>
                  <SecretInput
                    key={selectedEntry?.resource}
                    locale={props.locale}
                    fieldName={caption(props.locale, "secret value", "密钥值")}
                    className="settings-input"
                    readOnly
                    placeholder="••••••••"
                    resetKey={revealEpoch}
                    onReveal={handleReveal}
                    value={revealedValue ?? ""}
                  />
                </label>
              </div>

              <div className="settings-form-grid">
                <label
                  className="settings-field"
                  data-testid="imported-secret-display-name"
                >
                  <span className="field-label">
                    {caption(props.locale, "Display Name", "显示名称")}
                  </span>
                  <input
                    className="settings-input"
                    onChange={(event) => {
                      const nextValue = event.currentTarget.value;
                      setImportedDraft((current) => ({
                        ...current,
                        displayName: nextValue,
                      }));
                    }}
                    type="text"
                    value={importedDraft.displayName}
                  />
                </label>

                <label
                  className="settings-field"
                  data-testid="imported-secret-description"
                >
                  <span className="field-label">
                    {caption(props.locale, "Description", "描述")}
                  </span>
                  <input
                    className="settings-input"
                    onChange={(event) => {
                      const nextValue = event.currentTarget.value;
                      setImportedDraft((current) => ({
                        ...current,
                        description: nextValue,
                      }));
                    }}
                    type="text"
                    value={importedDraft.description}
                  />
                </label>

                <label
                  className="settings-field"
                  data-testid="imported-secret-tags"
                >
                  <span className="field-label">
                    {caption(props.locale, "Tags", "标签")}
                  </span>
                  <input
                    className="settings-input"
                    onChange={(event) => {
                      const nextValue = event.currentTarget.value;
                      setImportedDraft((current) => ({
                        ...current,
                        tags: nextValue,
                      }));
                    }}
                    type="text"
                    value={importedDraft.tags}
                  />
                </label>

                <label
                  className="settings-field settings-field-wide"
                  data-testid="imported-secret-metadata"
                >
                  <span className="field-label">
                    {caption(props.locale, "Metadata", "元信息")}
                  </span>
                  <textarea
                    className="settings-input note-field"
                    onChange={(event) => {
                      const nextValue = event.currentTarget.value;
                      setImportedDraft((current) => ({
                        ...current,
                        metadata: nextValue,
                      }));
                    }}
                    value={importedDraft.metadata}
                  />
                  <span className="field-hint">
                    {metadataDraft.invalidLines.length > 0
                      ? `${caption(props.locale, "Use KEY=VALUE lines.", "请使用 KEY=VALUE 格式。")} (${metadataDraft.invalidLines.join(", ")})`
                      : caption(
                          props.locale,
                          "Use one KEY=VALUE pair per line.",
                          "每行使用一个 KEY=VALUE。",
                        )}
                  </span>
                </label>
              </div>

              <div className="catalog-source-section">
                <button
                  className="ghost catalog-toggle"
                  data-testid="catalog-locator-toggle"
                  onClick={() => {
                    setIsLocatorOpen((current) => !current);
                  }}
                  type="button"
                >
                  {isLocatorOpen
                    ? caption(props.locale, "Hide Source", "收起来源")
                    : caption(props.locale, "Show Source", "展开来源")}
                </button>
                {isLocatorOpen ? (
                  <div className="detail-section detail-section-low">
                    <div className="detail-section-header">
                      <h3>
                        {caption(props.locale, "Source Locator", "来源定位值")}
                      </h3>
                      <span>
                        {translateCode(
                          props.locale,
                          selectedEntry.reference.provider_kind,
                        )}
                      </span>
                    </div>
                    <p className="section-copy">
                      {caption(
                        props.locale,
                        "Locator values are read-only here. Re-import if the upstream path changes.",
                        "这里的 locator 值只读；如果外部路径变了，请重新导入。",
                      )}
                    </p>
                    <dl className="facts">
                      {importedLocatorEntries(
                        props.locale,
                        selectedEntry.reference,
                      ).map((entry) => (
                        <div key={entry.label}>
                          <dt>{entry.label}</dt>
                          <dd>{entry.value}</dd>
                        </div>
                      ))}
                    </dl>
                  </div>
                ) : null}
              </div>

              {(selectedEntry.reference.tags?.length ?? 0) > 0 ? (
                <div className="imported-tag-list">
                  {(selectedEntry.reference.tags ?? []).map((tag) => (
                    <span className="id-pill" key={tag}>
                      {tag}
                    </span>
                  ))}
                </div>
              ) : null}

              <div className="actions">
                <button
                  className="ghost"
                  data-testid="imported-secret-rename"
                  disabled={!canRename}
                  onClick={() => {
                    void handleRename();
                  }}
                  type="button"
                >
                  {caption(props.locale, "Move Resource", "移动资源")}
                </button>
                <button
                  className="ghost"
                  data-testid="imported-secret-refresh"
                  disabled={isSaving || isDeleting || isRefreshing}
                  onClick={() => {
                    void handleRefreshImported();
                  }}
                  type="button"
                >
                  {isRefreshing
                    ? caption(props.locale, "Refreshing...", "刷新中...")
                    : caption(
                        props.locale,
                        "Refresh From Source",
                        "从上游更新",
                      )}
                </button>
                <button
                  className="ghost"
                  data-testid="imported-secret-edit-local"
                  disabled={isSaving || isDeleting || isRefreshing}
                  onClick={() => {
                    startCreatingLiteral(selectedEntry.reference.resource);
                  }}
                  type="button"
                >
                  {caption(props.locale, "Edit Local Value", "改为本地值")}
                </button>
                <button
                  className="primary"
                  data-testid="imported-secret-save"
                  disabled={!canSaveImported}
                  onClick={() => {
                    void handleSaveImported();
                  }}
                  type="button"
                >
                  {isSaving
                    ? caption(props.locale, "Saving...", "保存中...")
                    : caption(props.locale, "Save", "保存")}
                </button>
                <button
                  className="danger"
                  data-testid="imported-secret-delete"
                  disabled={isSaving || isDeleting || isRefreshing}
                  onClick={() => {
                    void handleDelete();
                  }}
                  type="button"
                >
                  {isDeleting
                    ? caption(props.locale, "Deleting...", "删除中...")
                    : caption(props.locale, "Delete", "删除")}
                </button>
              </div>
            </div>
          ) : null}
        </section>
      </div>
    </section>
  );
}
