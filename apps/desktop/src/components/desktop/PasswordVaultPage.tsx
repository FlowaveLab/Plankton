import { SecretInput } from "../SecretInput";
import { ChoiceGroup } from "../ChoiceGroup";
import {
  useCallback,
  useEffect,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
  type JSX,
} from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

import type { Locale } from "../../i18n";
import { PASSWORD_CATALOG_CHANGED_EVENT } from "../../passwordCatalogEvents";
import { PasswordManagementView } from "../PasswordManagementView";
import {
  CollectionExposurePolicyEditor,
  FieldExposurePolicyEditor,
  ExposurePolicySummary,
  ExposureRadar,
  exposurePolicyNeedsNetworkAllowlist,
  normalizeExposurePolicy,
  type CredentialExposurePolicy,
  type ExposureSurface,
} from "../ExposurePolicy";
import { PasswordAddDialog } from "./PasswordAddDialog";
import { LocalVaultManagerDialog } from "./LocalVaultManagerDialog";
import {
  PasswordMigrationDialog,
  type MigratablePasswordItem,
} from "./PasswordMigrationDialog";
import { Copy, Ellipsis, ListFilter, Search, Trash2, KeyRound } from "./icons";
import { Plus, FolderKey } from "lucide-react";
import {
  Dialog,
  Drawer,
  EmptyState,
  ErrorState,
  PageHeader,
  Pagination,
} from "./PagePrimitives";
import {
  loadPasswordItems,
  passwordItemIdForResource,
  resolvePasswordValue,
} from "./passwordAdapter";
import "./password-vault.css";
import type {
  PasswordBackend,
  PasswordField,
  PasswordItem,
  PasswordOrigin,
} from "./workspaceTypes";

type PasswordVaultPageProps = {
  incomingDraftId: string | null;
  incomingEditItemId?: string | null;
  incomingMigration?: PasswordMigrationHandoff | null;
  incomingVaultManager?: boolean;
  locale: Locale;
  onDraftConsumed: () => void;
  onEditConsumed?: () => void;
  onMigrationConsumed?: () => void;
  onVaultManagerConsumed?: () => void;
};

export type PasswordMigrationHandoff = {
  item_id: string;
  backend: string;
  vault: string;
  mode: "copy" | "move";
};

type TagMode = "all" | "any";
type MatchKind = "field" | "notes" | "tag" | "title" | "vault";
type MatchResult = { item: PasswordItem; kind: MatchKind; value?: string };
type DialogDraft = { generation: number; id: string };
type RevealedItemValues = {
  itemId: string;
  values: Record<string, string>;
};

type PasswordFieldSummary = {
  resource_id: string;
  label: string;
  provider_kind: string;
  vault?: string | null;
  has_value: boolean;
  exposure_policy?: CredentialExposurePolicy;
  inherits_exposure_policy?: boolean;
};

type PasswordItemSummary = {
  record_id: string;
  item_id: string;
  title: string;
  description?: string | null;
  tags: string[];
  metadata: Record<string, string>;
  default_exposure_policy?: CredentialExposurePolicy;
  fields: PasswordFieldSummary[];
};

type PasswordCatalogMetadata = {
  revision: string;
  items: PasswordItemSummary[];
};

type PasswordItemSummaryWire = Omit<
  PasswordItemSummary,
  "metadata" | "tags"
> & {
  metadata?: Record<string, string>;
  tags?: string[];
};

type PasswordCatalogMetadataWire = {
  revision: string;
  items: PasswordItemSummaryWire[];
};

type PasswordChangeOperation =
  | {
      operation: "set_item_exposure_policy";
      item_id: string;
      policy: CredentialExposurePolicy;
    }
  | { operation: "inherit_field_exposure_policy"; resource_id: string }
  | {
      operation: "update_item";
      item_id: string;
      next_item_id?: string;
      title?: string;
      description?: string;
      clear_description?: boolean;
      tags?: string[];
    }
  | {
      operation: "rename_resource";
      resource_id: string;
      next_resource_id: string;
    }
  | {
      operation: "rename_field_label";
      resource_id: string;
      label: string;
    }
  | {
      operation: "update_field";
      resource_id: string;
      label?: string;
      exposure_policy?: CredentialExposurePolicy;
    }
  | {
      operation: "move_field";
      resource_id: string;
      target_item_id: string;
      target_title?: string;
    }
  | {
      operation: "merge_items";
      source_item_id: string;
      target_item_id: string;
    }
  | { operation: "delete_field"; resource_id: string }
  | {
      operation: "delete_duplicate_field";
      resource_id: string;
      canonical_resource_id: string;
    }
  | { operation: "refresh_item"; item_id: string }
  | { operation: "delete_item"; item_id: string };

type EditableField = {
  originalLabel: string;
  label: string;
  originalResourceId: string;
  resourceId: string;
  originalValue?: string;
  value?: string;
  removed: boolean;
  originalExposurePolicy: CredentialExposurePolicy;
  originalInheritsExposurePolicy: boolean;
  exposurePolicy: CredentialExposurePolicy | null;
};

type EditDraft = {
  original: PasswordItemSummary;
  defaultExposurePolicy: CredentialExposurePolicy;
  itemId: string;
  title: string;
  description: string;
  tags: string;
  fields: EditableField[];
};

type OrganizeMode = "split" | "merge" | "dedupe";

type OrganizeDraft = {
  source: PasswordItemSummary;
  mode: OrganizeMode;
  selectedResourceIds: string[];
  targetItemId: string;
  targetTitle: string;
  canonicalResourceId: string;
};

type ConfirmationEntry = {
  label: string;
  before?: string;
  after?: string;
  destructive?: boolean;
  exposureBefore?: CredentialExposurePolicy;
  exposureAfter?: CredentialExposurePolicy;
};

type PendingUserConfirmation = {
  title: string;
  description: string;
  operations: PasswordChangeOperation[];
  entries: ConfirmationEntry[];
  vaults: string[];
  destructive?: boolean;
  confirmLabel?: string;
  committingLabel?: string;
  valueUpdate?: {
    expectedRevision: string;
    sourceRecordId: string;
    values: Record<string, string>;
  };
};

type ItemContextMenu = {
  item: PasswordItem;
  left: number;
  top: number;
};

const ALL_PASSWORD_BACKENDS: PasswordBackend[] = [
  "plankton",
  "one_password",
  "bitwarden",
];

const PAGE_SIZE = 8;
const EXTERNAL_REFRESH_DEBOUNCE_MS = 80;

type CatalogLoadMode = "replace" | "refresh";

function caption(locale: Locale, english: string, chinese: string): string {
  return locale === "zh-CN" ? chinese : english;
}

function vaultLabels(item: PasswordItemSummary): string[] {
  return Array.from(
    new Set(
      item.fields
        .map((field) => field.vault?.trim())
        .filter((vault): vault is string => Boolean(vault)),
    ),
  );
}

function uniqueTags(tags: string[]): string[] {
  const unique = new Map<string, string>();
  for (const tag of tags) {
    const trimmed = tag.trim();
    const normalized = trimmed.toLocaleLowerCase();
    if (normalized && !unique.has(normalized)) {
      unique.set(normalized, trimmed);
    }
  }
  return Array.from(unique.values());
}

function matchingItem(
  item: PasswordItem,
  query: string,
  tagMode: TagMode,
  tags: string[],
): MatchResult | null {
  const term = query.trim().toLocaleLowerCase();
  const normalizedTags = item.tags.map((tag) => tag.toLocaleLowerCase());
  const tagPasses =
    tags.length === 0 ||
    (tagMode === "all"
      ? tags.every((tag) => normalizedTags.includes(tag.toLocaleLowerCase()))
      : tags.some((tag) => normalizedTags.includes(tag.toLocaleLowerCase())));
  if (!tagPasses) {
    return null;
  }

  if (!term) {
    return {
      item,
      kind: tags.length > 0 ? "tag" : "vault",
      value: tags.length > 0 ? tags.join(", ") : undefined,
    };
  }

  if (item.title.toLocaleLowerCase().includes(term)) {
    return { item, kind: "title" };
  }
  if (item.notes.toLocaleLowerCase().includes(term)) {
    return { item, kind: "notes" };
  }
  const field = item.fields.find((candidate) =>
    candidate.key.toLocaleLowerCase().includes(term),
  );
  if (field) {
    return { item, kind: "field", value: field.key };
  }
  const tag = item.tags.find((candidate) =>
    candidate.toLocaleLowerCase().includes(term),
  );
  return tag ? { item, kind: "tag", value: tag } : null;
}

function matchReason(locale: Locale, result: MatchResult): string {
  switch (result.kind) {
    case "title":
      return caption(locale, "Matched title", "匹配标题");
    case "notes":
      return caption(locale, "Matched notes", "匹配备注");
    case "field":
      return caption(
        locale,
        `Matched field key: ${result.value}`,
        `匹配字段 key：${result.value}`,
      );
    case "tag":
      return caption(
        locale,
        `Matched tag: ${result.value}`,
        `匹配标签：${result.value}`,
      );
    case "vault":
      return caption(locale, "Available in selected vault", "位于所选保险库");
  }
}

function fieldActionFailure(
  locale: Locale,
  action: "copy" | "reveal",
  fieldLabel: string,
): string {
  if (action === "reveal") {
    return caption(
      locale,
      `${fieldLabel} could not be revealed. Check Diagnostics and try again.`,
      `无法显示${fieldLabel}。请查看诊断信息后重试。`,
    );
  }
  return caption(
    locale,
    `${fieldLabel} could not be copied. Check Diagnostics and try again.`,
    `无法复制${fieldLabel}。请查看诊断信息后重试。`,
  );
}

type FilterSurfaceProps = {
  availableBackends: PasswordBackend[];
  availableTags: string[];
  backendFilter: PasswordBackend[];
  context: "drawer" | "sidebar";
  locale: Locale;
  onTagFilterChange: (value: string) => void;
  onTagModeChange: (mode: TagMode) => void;
  onToggleBackend: (backend: PasswordBackend) => void;
  onVaultFilterChange: (vault: string) => void;
  tagFilter: string;
  tagMode: TagMode;
  vaultFilter: string;
  vaults: string[];
};

function backendLabel(locale: Locale, backend: PasswordBackend): string {
  if (backend === "one_password") return "1Password";
  if (backend === "bitwarden") return "Bitwarden";
  return caption(locale, "Local (Plankton)", "本地（Plankton）");
}

function originLabel(locale: Locale, origin: PasswordOrigin): string {
  switch (origin) {
    case "dotenv":
      return caption(locale, "Imported from .env", "导入自 .env");
    case "one_password":
      return "1Password";
    case "bitwarden":
      return "Bitwarden";
    case "plankton_vault":
      return caption(locale, "Plankton vault", "Plankton 保险库");
    case "local":
      return caption(locale, "Local value", "本地值");
  }
}

function metadataForItem(
  catalog: PasswordCatalogMetadata | null,
  item: PasswordItem,
): PasswordItemSummary | null {
  const resourceIds = new Set(item.fields.map((field) => field.resourceId));
  return (
    catalog?.items.find((candidate) =>
      candidate.fields.some((field) => resourceIds.has(field.resource_id)),
    ) ?? null
  );
}

function normalizePasswordCatalogMetadata(
  catalog: PasswordCatalogMetadataWire,
): PasswordCatalogMetadata {
  return {
    revision: catalog.revision,
    items: catalog.items.map((item) => ({
      ...item,
      metadata: item.metadata ?? {},
      tags: item.tags ?? [],
    })),
  };
}

async function loadPasswordCatalogMetadata(): Promise<PasswordCatalogMetadata> {
  const catalog = await invoke<PasswordCatalogMetadataWire>(
    "list_password_catalog_metadata_command",
  );
  return normalizePasswordCatalogMetadata(catalog);
}

function FilterSurface(props: FilterSurfaceProps): JSX.Element {
  const selectedTags = uniqueTags(props.tagFilter.split(","));
  const selectedTagKeys = new Set(
    selectedTags.map((tag) => tag.toLocaleLowerCase()),
  );
  const suffix = props.context;
  return (
    <div className="password-filter-surface">
      {props.availableBackends.length > 0 ? (
        <fieldset className="password-backend-filter">
          <legend>{caption(props.locale, "Entry type", "条目类型")}</legend>
          <div className="password-backend-options">
            {props.availableBackends.map((backend) => (
              <button
                aria-pressed={props.backendFilter.includes(backend)}
                className={
                  props.backendFilter.includes(backend)
                    ? "backend-filter active"
                    : "backend-filter"
                }
                key={backend}
                onClick={() => props.onToggleBackend(backend)}
                type="button"
              >
                <span>{backendLabel(props.locale, backend)}</span>
              </button>
            ))}
          </div>
        </fieldset>
      ) : null}
      <label htmlFor={`password-vault-filter-${suffix}`}>
        <span>{caption(props.locale, "Vault", "保险库")}</span>
        <select
          aria-label={caption(props.locale, "Filter by vault", "按保险库筛选")}
          id={`password-vault-filter-${suffix}`}
          onChange={(event) =>
            props.onVaultFilterChange(event.currentTarget.value)
          }
          value={props.vaultFilter}
        >
          <option value="">
            {caption(props.locale, "All vaults", "全部保险库")}
          </option>
          {props.vaults.map((vault) => (
            <option key={vault} value={vault}>
              {vault}
            </option>
          ))}
        </select>
      </label>
      <label htmlFor={`password-tag-filter-${suffix}`}>
        <span>{caption(props.locale, "Tags", "标签")}</span>
        <input
          aria-label={caption(props.locale, "Filter by tags", "按标签筛选")}
          id={`password-tag-filter-${suffix}`}
          onChange={(event) =>
            props.onTagFilterChange(event.currentTarget.value)
          }
          placeholder={caption(
            props.locale,
            "production, shared",
            "生产、共享",
          )}
          value={props.tagFilter}
        />
      </label>
      <ChoiceGroup
        label={
          <>
            <span>{caption(props.locale, "Tag match", "标签匹配")}</span>
          </>
        }
        aria-label={caption(props.locale, "Tag match mode", "标签匹配模式")}
        id={`password-tag-mode-${suffix}`}
        onChange={(value) => props.onTagModeChange(value as TagMode)}
        value={props.tagMode}
        options={[
          {
            value: "any",
            label: <>{caption(props.locale, "Any tag", "任意标签")}</>,
          },
          {
            value: "all",
            label: <>{caption(props.locale, "All tags", "所有标签")}</>,
          },
        ]}
      />
      {props.availableTags.length > 0 ? (
        <div
          aria-label={caption(props.locale, "Available tags", "可用标签")}
          className="password-filter-tags"
        >
          {props.availableTags.map((tag) => (
            <button
              aria-pressed={selectedTagKeys.has(tag.toLocaleLowerCase())}
              className={
                selectedTagKeys.has(tag.toLocaleLowerCase())
                  ? "tag-filter active"
                  : "tag-filter"
              }
              key={tag}
              onClick={() => {
                const tagKey = tag.toLocaleLowerCase();
                const nextTags = selectedTagKeys.has(tagKey)
                  ? selectedTags.filter(
                      (selectedTag) =>
                        selectedTag.toLocaleLowerCase() !== tagKey,
                    )
                  : [...selectedTags, tag];
                props.onTagFilterChange(nextTags.join(", "));
              }}
              type="button"
            >
              #{tag}
            </button>
          ))}
        </div>
      ) : null}
    </div>
  );
}

export function PasswordVaultPage(props: PasswordVaultPageProps): JSX.Element {
  const [items, setItems] = useState<PasswordItem[]>([]);
  const [loadState, setLoadState] = useState<"loading" | "ready" | "error">(
    "loading",
  );
  const [query, setQuery] = useState("");
  const [tagFilter, setTagFilter] = useState("");
  const [tagMode, setTagMode] = useState<TagMode>("any");
  const [vaultFilter, setVaultFilter] = useState("");
  const [backendFilter, setBackendFilter] = useState<PasswordBackend[]>(
    ALL_PASSWORD_BACKENDS,
  );
  const [selectedId, setSelectedId] = useState("");
  const [page, setPage] = useState(1);
  const [dialogDraft, setDialogDraft] = useState<DialogDraft | null>(null);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [actionError, setActionError] = useState<string | null>(null);
  const [feedback, setFeedback] = useState<string | null>(null);
  const [visibilityEpoch, setVisibilityEpoch] = useState(0);
  const [revealedValues, setRevealedValues] =
    useState<RevealedItemValues | null>(null);
  const [workingFieldId, setWorkingFieldId] = useState<string | null>(null);
  const [catalogMetadata, setCatalogMetadata] =
    useState<PasswordCatalogMetadata | null>(null);
  const [editDraft, setEditDraft] = useState<EditDraft | null>(null);
  const editSessionRef = useRef(0);
  const [organizeDraft, setOrganizeDraft] = useState<OrganizeDraft | null>(
    null,
  );
  const [migrationDialog, setMigrationDialog] = useState<{
    item: MigratablePasswordItem;
    handoff?: PasswordMigrationHandoff;
  } | null>(null);
  const [pendingConfirmation, setPendingConfirmation] =
    useState<PendingUserConfirmation | null>(null);
  const [confirmationReason, setConfirmationReason] = useState("");
  const [isCommittingChange, setIsCommittingChange] = useState(false);
  const [importDialogOpen, setImportDialogOpen] = useState(false);
  const [vaultManagerOpen, setVaultManagerOpen] = useState(false);
  const [vaultRevision, setVaultRevision] = useState(0);
  const [filterDrawerOpen, setFilterDrawerOpen] = useState(false);
  const [itemMenuOpen, setItemMenuOpen] = useState(false);
  const [itemContextMenu, setItemContextMenu] =
    useState<ItemContextMenu | null>(null);
  const draftGenerationRef = useRef(0);
  const currentDraftRef = useRef<DialogDraft | null>(null);
  const revealGenerationRef = useRef(0);
  const selectedItemIdRef = useRef<string | null>(null);
  const itemMenuRef = useRef<HTMLDivElement | null>(null);
  const itemMenuTriggerRef = useRef<HTMLButtonElement | null>(null);
  const itemMenuFirstActionRef = useRef<HTMLButtonElement | null>(null);
  const itemContextMenuRef = useRef<HTMLDivElement | null>(null);
  const itemContextMenuActionRef = useRef<HTMLButtonElement | null>(null);

  useLayoutEffect(() => {
    if (itemMenuOpen) itemMenuFirstActionRef.current?.focus();
  }, [itemMenuOpen]);

  useLayoutEffect(() => {
    if (itemContextMenu) itemContextMenuActionRef.current?.focus();
  }, [itemContextMenu]);

  useEffect(() => {
    if (!itemMenuOpen) return;
    const closeOnOutsidePress = (event: PointerEvent): void => {
      if (!itemMenuRef.current?.contains(event.target as Node)) {
        setItemMenuOpen(false);
      }
    };
    const closeOnEscape = (event: KeyboardEvent): void => {
      if (event.key !== "Escape") return;
      setItemMenuOpen(false);
      itemMenuTriggerRef.current?.focus();
    };
    document.addEventListener("pointerdown", closeOnOutsidePress);
    document.addEventListener("keydown", closeOnEscape);
    return () => {
      document.removeEventListener("pointerdown", closeOnOutsidePress);
      document.removeEventListener("keydown", closeOnEscape);
    };
  }, [itemMenuOpen]);

  useEffect(() => {
    if (!itemContextMenu) return;
    const closeOnOutsidePress = (event: PointerEvent): void => {
      if (!itemContextMenuRef.current?.contains(event.target as Node)) {
        setItemContextMenu(null);
      }
    };
    const closeOnEscape = (event: KeyboardEvent): void => {
      if (event.key === "Escape") setItemContextMenu(null);
    };
    const closeOnViewportChange = (): void => setItemContextMenu(null);
    document.addEventListener("pointerdown", closeOnOutsidePress);
    document.addEventListener("keydown", closeOnEscape);
    document.addEventListener("scroll", closeOnViewportChange, true);
    window.addEventListener("resize", closeOnViewportChange);
    return () => {
      document.removeEventListener("pointerdown", closeOnOutsidePress);
      document.removeEventListener("keydown", closeOnEscape);
      document.removeEventListener("scroll", closeOnViewportChange, true);
      window.removeEventListener("resize", closeOnViewportChange);
    };
  }, [itemContextMenu]);

  const cancelRevealOwnership = useCallback((): void => {
    revealGenerationRef.current += 1;
    setVisibilityEpoch((epoch) => epoch + 1);
    setWorkingFieldId(null);
  }, []);

  const concealRevealedValues = useCallback((): void => {
    cancelRevealOwnership();
    setRevealedValues(null);
  }, [cancelRevealOwnership]);

  const loadItems = useCallback(
    async (mode: CatalogLoadMode = "replace"): Promise<void> => {
      if (mode === "replace") setLoadState("loading");
      setLoadError(null);
      concealRevealedValues();
      try {
        const result = await loadPasswordItems();
        if (result.kind === "live") {
          setItems(result.items);
          setSelectedId((current) =>
            mode === "refresh" &&
            result.items.some((item) => item.id === current)
              ? current
              : (result.items[0]?.id ?? ""),
          );
          if (mode === "replace") setFeedback(null);
          try {
            setCatalogMetadata(await loadPasswordCatalogMetadata());
          } catch {
            setCatalogMetadata(null);
          }
        } else {
          setItems([]);
          setFeedback(result.message);
        }
        setLoadState("ready");
      } catch (reason) {
        if (mode === "refresh") {
          setActionError(
            caption(
              props.locale,
              "The password catalog changed, but this page could not refresh it. Retry from this page.",
              "密码目录已发生变化，但此页面无法自动刷新。请在此页面重试。",
            ),
          );
        } else {
          setLoadError(
            reason instanceof Error ? reason.message : String(reason),
          );
          setLoadState("error");
        }
      }
    },
    [concealRevealedValues, props.locale],
  );

  useEffect(() => {
    void loadItems();
  }, [loadItems]);

  useEffect(() => {
    const handoff = props.incomingMigration;
    if (!handoff || !catalogMetadata) return;
    const item = catalogMetadata.items.find(
      (candidate) =>
        candidate.item_id === handoff.item_id ||
        candidate.record_id === handoff.item_id,
    );
    if (item) {
      setMigrationDialog({ item, handoff });
    } else {
      setActionError(
        caption(
          props.locale,
          `Password item ${handoff.item_id} was not found.`,
          `未找到密码条目 ${handoff.item_id}。`,
        ),
      );
    }
    props.onMigrationConsumed?.();
  }, [
    catalogMetadata,
    props.incomingMigration,
    props.locale,
    props.onMigrationConsumed,
  ]);

  useEffect(() => {
    if (!props.incomingVaultManager) return;
    setVaultManagerOpen(true);
    props.onVaultManagerConsumed?.();
  }, [props.incomingVaultManager, props.onVaultManagerConsumed]);

  useEffect(() => {
    let disposed = false;
    let unlisten: (() => void) | null = null;
    let refreshTimer: number | null = null;
    const scheduleRefresh = (): void => {
      if (refreshTimer !== null) window.clearTimeout(refreshTimer);
      refreshTimer = window.setTimeout(() => {
        refreshTimer = null;
        if (!disposed) void loadItems("refresh");
      }, EXTERNAL_REFRESH_DEBOUNCE_MS);
    };

    window.addEventListener("focus", scheduleRefresh);
    void listen(PASSWORD_CATALOG_CHANGED_EVENT, scheduleRefresh)
      .then((dispose) => {
        if (disposed) dispose();
        else unlisten = dispose;
      })
      .catch((error: unknown) => {
        console.error("Password catalog refresh listener failed.", error);
      });

    return () => {
      disposed = true;
      window.removeEventListener("focus", scheduleRefresh);
      if (refreshTimer !== null) window.clearTimeout(refreshTimer);
      unlisten?.();
    };
  }, [loadItems]);

  useLayoutEffect(() => {
    if (props.incomingDraftId) {
      concealRevealedValues();
      const nextDraft = {
        generation: draftGenerationRef.current + 1,
        id: props.incomingDraftId,
      };
      draftGenerationRef.current = nextDraft.generation;
      currentDraftRef.current = nextDraft;
      setDialogDraft(nextDraft);
    }
  }, [concealRevealedValues, props.incomingDraftId]);

  const selectedTags = useMemo(
    () => uniqueTags(tagFilter.split(",")),
    [tagFilter],
  );
  const vaults = useMemo(
    () => Array.from(new Set(items.map((item) => item.vault))).sort(),
    [items],
  );
  const availableTags = useMemo(
    () =>
      uniqueTags(items.flatMap((item) => item.tags)).sort((left, right) =>
        left.localeCompare(right, undefined, { sensitivity: "base" }),
      ),
    [items],
  );
  const availableBackends = useMemo(
    () =>
      ALL_PASSWORD_BACKENDS.filter((backend) =>
        items.some((item) => item.backend === backend),
      ),
    [items],
  );
  const visible = useMemo(
    () =>
      items
        .filter(
          (item) =>
            !item.archived &&
            backendFilter.includes(item.backend) &&
            (!vaultFilter || item.vault === vaultFilter),
        )
        .flatMap((item) => {
          const result = matchingItem(item, query, tagMode, selectedTags);
          return result ? [result] : [];
        }),
    [backendFilter, items, query, selectedTags, tagMode, vaultFilter],
  );
  const pageCount = Math.max(1, Math.ceil(visible.length / PAGE_SIZE));
  const currentPage = Math.min(page, pageCount);
  const paged = visible.slice(
    (currentPage - 1) * PAGE_SIZE,
    currentPage * PAGE_SIZE,
  );
  const selected =
    paged.find((entry) => entry.item.id === selectedId)?.item ??
    paged[0]?.item ??
    null;
  const selectedMetadata = selected
    ? metadataForItem(catalogMetadata, selected)
    : null;
  selectedItemIdRef.current = selected?.id ?? null;

  useEffect(() => {
    if (page !== currentPage) {
      setPage(currentPage);
    }
  }, [currentPage, page]);

  useEffect(() => {
    if (selectedId !== (selected?.id ?? "")) {
      setSelectedId(selected?.id ?? "");
    }
  }, [selected?.id, selectedId]);

  useLayoutEffect(() => {
    concealRevealedValues();
    setEditDraft(null);
  }, [concealRevealedValues, selected?.id]);

  useEffect(() => {
    const itemId = props.incomingEditItemId;
    if (!itemId || !catalogMetadata) return;
    const metadata = catalogMetadata.items.find(
      (candidate) =>
        candidate.item_id === itemId || candidate.record_id === itemId,
    );
    const resources = new Set(
      metadata?.fields.map((field) => field.resource_id) ?? [],
    );
    const item = items.find((candidate) =>
      candidate.fields.some((field) => resources.has(field.resourceId)),
    );
    if (!metadata || !item) {
      setActionError(
        caption(
          props.locale,
          `Password item ${itemId} was not found.`,
          `未找到密码条目 ${itemId}。`,
        ),
      );
      props.onEditConsumed?.();
      return;
    }
    if (selected?.id !== item.id) {
      setQuery("");
      setTagFilter("");
      setVaultFilter("");
      setBackendFilter(ALL_PASSWORD_BACKENDS);
      setPage(1);
      setSelectedId(item.id);
      return;
    }
    void startEditing(item);
    props.onEditConsumed?.();
  }, [
    catalogMetadata,
    items,
    props.incomingEditItemId,
    props.locale,
    props.onEditConsumed,
    selected?.id,
  ]);

  function resetPage(): void {
    setPage(1);
  }

  function clearFilters(): void {
    concealRevealedValues();
    setQuery("");
    setTagFilter("");
    setVaultFilter("");
    setBackendFilter(ALL_PASSWORD_BACKENDS);
    setTagMode("any");
    resetPage();
  }

  async function revealField(
    itemId: string,
    field: PasswordField,
  ): Promise<boolean> {
    const currentValues =
      revealedValues?.itemId === itemId ? revealedValues.values : {};
    if (currentValues[field.resourceId] !== undefined) {
      return true;
    }
    const ownerToken = revealGenerationRef.current;
    setWorkingFieldId(field.resourceId);
    setActionError(null);
    setFeedback(null);
    try {
      const value = await resolvePasswordValue(field.resourceId);
      if (
        revealGenerationRef.current !== ownerToken ||
        selectedItemIdRef.current !== itemId
      ) {
        return false;
      }
      setRevealedValues((current) => ({
        itemId,
        values: {
          ...(current?.itemId === itemId ? current.values : {}),
          [field.resourceId]: value,
        },
      }));
      return true;
    } catch {
      if (revealGenerationRef.current === ownerToken) {
        setActionError(fieldActionFailure(props.locale, "reveal", field.label));
      }
      return false;
    } finally {
      if (revealGenerationRef.current === ownerToken) {
        setWorkingFieldId(null);
      }
    }
  }

  async function copyField(field: PasswordField): Promise<void> {
    setWorkingFieldId(field.resourceId);
    setActionError(null);
    setFeedback(null);
    try {
      const value = field.secret
        ? await resolvePasswordValue(field.resourceId)
        : field.value;
      await navigator.clipboard.writeText(value);
      setFeedback(
        caption(
          props.locale,
          `${field.label} copied.`,
          `已复制${field.label}。`,
        ),
      );
    } catch {
      setActionError(fieldActionFailure(props.locale, "copy", field.label));
    } finally {
      setWorkingFieldId(null);
    }
  }

  async function copyItemId(itemId: string): Promise<void> {
    setActionError(null);
    try {
      await navigator.clipboard.writeText(itemId);
      setFeedback(caption(props.locale, "Item ID copied.", "已复制条目 ID。"));
    } catch {
      setActionError(
        caption(
          props.locale,
          "Item ID could not be copied. Check Diagnostics and try again.",
          "无法复制条目 ID。请查看诊断信息后重试。",
        ),
      );
    }
  }

  function openItemContextMenu(
    item: PasswordItem,
    requestedLeft: number,
    requestedTop: number,
  ): void {
    const menuWidth = 188;
    const menuHeight = 52;
    const viewportInset = 8;
    concealRevealedValues();
    setItemMenuOpen(false);
    setSelectedId(item.id);
    setItemContextMenu({
      item,
      left: Math.max(
        viewportInset,
        Math.min(requestedLeft, window.innerWidth - menuWidth - viewportInset),
      ),
      top: Math.max(
        viewportInset,
        Math.min(requestedTop, window.innerHeight - menuHeight - viewportInset),
      ),
    });
  }

  async function startEditing(item: PasswordItem): Promise<void> {
    editSessionRef.current += 1;
    concealRevealedValues();
    setActionError(null);
    try {
      let metadata = metadataForItem(catalogMetadata, item);
      if (!metadata) {
        const catalog = await loadPasswordCatalogMetadata();
        setCatalogMetadata(catalog);
        metadata = metadataForItem(catalog, item);
      }
      if (!metadata) {
        setActionError(
          caption(
            props.locale,
            "This entry cannot be matched to an editable catalog record.",
            "无法将此条目匹配到可编辑的目录记录。",
          ),
        );
        return;
      }
      setEditDraft({
        original: metadata,
        defaultExposurePolicy: normalizeExposurePolicy(
          metadata.default_exposure_policy,
        ),
        itemId: metadata.item_id,
        title: metadata.title,
        description: metadata.description ?? "",
        tags: metadata.tags.join(", "),
        fields: metadata.fields.map((field) => ({
          originalLabel: field.label,
          label: field.label,
          originalResourceId: field.resource_id,
          resourceId: field.resource_id,
          removed: false,
          originalExposurePolicy: normalizeExposurePolicy(
            field.exposure_policy,
          ),
          originalInheritsExposurePolicy:
            field.inherits_exposure_policy ?? false,
          exposurePolicy: field.inherits_exposure_policy
            ? null
            : normalizeExposurePolicy(field.exposure_policy),
        })),
      });
    } catch (error) {
      setActionError(
        caption(
          props.locale,
          `Entry could not be opened for editing: ${String(error)}`,
          `无法打开条目进行编辑：${String(error)}`,
        ),
      );
    }
  }

  async function loadEditValue(resourceId: string): Promise<boolean> {
    if (!editDraft) return false;
    if (
      editDraft.fields.find((field) => field.originalResourceId === resourceId)
        ?.value !== undefined
    )
      return true;
    const session = editSessionRef.current;
    setActionError(null);
    try {
      const value = await resolvePasswordValue(resourceId);
      if (session !== editSessionRef.current) return false;
      setEditDraft((current) =>
        current
          ? {
              ...current,
              fields: current.fields.map((field) =>
                field.originalResourceId === resourceId
                  ? {
                      ...field,
                      originalValue: value,
                      value: field.value ?? value,
                    }
                  : field,
              ),
            }
          : null,
      );
      return true;
    } catch {
      if (session === editSessionRef.current)
        setActionError(
          caption(
            props.locale,
            "Password value could not be loaded. Check Diagnostics and try again.",
            "无法载入密码值。请查看诊断信息后重试。",
          ),
        );
      return false;
    }
  }

  async function startOrganizing(item: PasswordItem): Promise<void> {
    concealRevealedValues();
    setActionError(null);
    try {
      let metadata = metadataForItem(catalogMetadata, item);
      let catalog = catalogMetadata;
      if (!metadata || !catalog) {
        catalog = await loadPasswordCatalogMetadata();
        setCatalogMetadata(catalog);
        metadata = metadataForItem(catalog, item);
      }
      if (!metadata) {
        setActionError(
          caption(
            props.locale,
            "This entry cannot be organized because its catalog metadata is unavailable.",
            "无法整理此条目，因为目录元信息不可用。",
          ),
        );
        return;
      }
      const firstCanonical = catalog.items
        .filter((candidate) => candidate.record_id !== metadata.record_id)
        .flatMap((candidate) => candidate.fields)[0];
      setOrganizeDraft({
        source: metadata,
        mode: "split",
        selectedResourceIds: metadata.fields.map((field) => field.resource_id),
        targetItemId: "",
        targetTitle: "",
        canonicalResourceId: firstCanonical?.resource_id ?? "",
      });
    } catch (error) {
      setActionError(
        caption(
          props.locale,
          `Entry organization could not be opened: ${String(error)}`,
          `无法打开字段整理：${String(error)}`,
        ),
      );
    }
  }

  function requestOrganizeConfirmation(): void {
    if (!organizeDraft) return;
    const selectedFields = organizeDraft.source.fields.filter((field) =>
      organizeDraft.selectedResourceIds.includes(field.resource_id),
    );
    const operations: PasswordChangeOperation[] = [];
    const entries: ConfirmationEntry[] = [];

    if (organizeDraft.mode === "dedupe") {
      const duplicate = selectedFields[0];
      if (!duplicate || !organizeDraft.canonicalResourceId) {
        setActionError(
          caption(
            props.locale,
            "Choose the duplicate field and the field to keep.",
            "请选择重复字段和要保留的字段。",
          ),
        );
        return;
      }
      operations.push({
        operation: "delete_duplicate_field",
        resource_id: duplicate.resource_id,
        canonical_resource_id: organizeDraft.canonicalResourceId,
      });
      entries.push({
        label: caption(props.locale, "Remove duplicate field", "删除重复字段"),
        before: `${organizeDraft.source.title} / ${duplicate.label}`,
        after: organizeDraft.canonicalResourceId,
        destructive: true,
      });
    } else {
      if (selectedFields.length === 0) {
        setActionError(
          caption(
            props.locale,
            "Choose at least one field to move.",
            "请至少选择一个要移动的字段。",
          ),
        );
        return;
      }
      const targetItemId = organizeDraft.targetItemId.trim();
      if (!targetItemId) {
        setActionError(
          caption(props.locale, "Choose a target item.", "请选择目标条目。"),
        );
        return;
      }
      if (organizeDraft.mode === "split") {
        const targetTitle = organizeDraft.targetTitle.trim();
        if (!targetTitle) {
          setActionError(
            caption(
              props.locale,
              "A title is required for the new item.",
              "新条目的标题不能为空。",
            ),
          );
          return;
        }
        for (const field of selectedFields) {
          operations.push({
            operation: "move_field",
            resource_id: field.resource_id,
            target_item_id: targetItemId,
            target_title: targetTitle,
          });
        }
      } else if (selectedFields.length === organizeDraft.source.fields.length) {
        operations.push({
          operation: "merge_items",
          source_item_id: organizeDraft.source.item_id,
          target_item_id: targetItemId,
        });
      } else {
        for (const field of selectedFields) {
          operations.push({
            operation: "move_field",
            resource_id: field.resource_id,
            target_item_id: targetItemId,
          });
        }
      }
      const targetTitle =
        organizeDraft.mode === "split"
          ? organizeDraft.targetTitle.trim()
          : (catalogMetadata?.items.find(
              (item) => item.item_id === targetItemId,
            )?.title ?? targetItemId);
      for (const field of selectedFields) {
        entries.push({
          label: field.label,
          before: organizeDraft.source.title,
          after: targetTitle,
        });
      }
    }

    setActionError(null);
    setConfirmationReason("");
    setOrganizeDraft(null);
    setPendingConfirmation({
      title: caption(
        props.locale,
        "Confirm field organization",
        "确认字段整理",
      ),
      description:
        organizeDraft.mode === "dedupe"
          ? caption(
              props.locale,
              "The duplicate is removed only if both stored values match locally.",
              "仅当两个本地存储值一致时才会删除重复字段。",
            )
          : caption(
              props.locale,
              "Resource keys stay unchanged while fields move between password items.",
              "字段会在密码条目之间移动，资源 key 保持不变。",
            ),
      operations,
      entries,
      vaults: vaultLabels(organizeDraft.source),
    });
  }

  function requestEditConfirmation(): void {
    if (!editDraft) return;
    const original = editDraft.original;
    const removedFields = editDraft.fields.filter((field) => field.removed);
    const removesAllFields = removedFields.length === editDraft.fields.length;
    const nextItemId = editDraft.itemId.trim();
    const nextTitle = editDraft.title.trim();
    const nextDescription = editDraft.description.trim();
    const nextTags = uniqueTags(editDraft.tags.split(","));
    if (!removesAllFields && (!nextItemId || !nextTitle)) {
      setActionError(
        caption(
          props.locale,
          "Item ID and title are required.",
          "条目 ID 和标题不能为空。",
        ),
      );
      return;
    }
    if (
      !removesAllFields &&
      exposurePolicyNeedsNetworkAllowlist(editDraft.defaultExposurePolicy)
    ) {
      setActionError(
        caption(
          props.locale,
          "Default profile: controlled network exposure requires an allowlist.",
          "默认配置：受控网络暴露必须填写白名单。",
        ),
      );
      return;
    }
    const incompleteExposureField = editDraft.fields.find(
      (field) =>
        !field.removed &&
        exposurePolicyNeedsNetworkAllowlist(
          field.exposurePolicy ?? editDraft.defaultExposurePolicy,
        ),
    );
    if (incompleteExposureField) {
      setActionError(
        caption(
          props.locale,
          `${incompleteExposureField.label}: controlled network exposure requires at least one allowlist destination.`,
          `${incompleteExposureField.label}：受控网络暴露必须至少填写一个白名单目标。`,
        ),
      );
      return;
    }

    const update: Extract<
      PasswordChangeOperation,
      { operation: "update_item" }
    > = {
      operation: "update_item",
      item_id: original.item_id,
    };
    const entries: ConfirmationEntry[] = [];
    if (!removesAllFields && nextItemId !== original.item_id) {
      update.next_item_id = nextItemId;
      entries.push({
        label: caption(props.locale, "Item ID", "条目 ID"),
        before: original.item_id,
        after: nextItemId,
      });
    }
    if (!removesAllFields && nextTitle !== original.title) {
      update.title = nextTitle;
      entries.push({
        label: caption(props.locale, "Title", "标题"),
        before: original.title,
        after: nextTitle,
      });
    }
    if (!removesAllFields && nextDescription !== (original.description ?? "")) {
      if (nextDescription) update.description = nextDescription;
      else update.clear_description = true;
      entries.push({
        label: caption(props.locale, "Notes", "备注"),
        before: original.description ?? "",
        after: nextDescription,
      });
    }
    if (!removesAllFields && nextTags.join("\n") !== original.tags.join("\n")) {
      update.tags = nextTags;
      entries.push({
        label: caption(props.locale, "Tags", "标签"),
        before: original.tags.join(", "),
        after: nextTags.join(", "),
      });
    }

    const operations: PasswordChangeOperation[] = [];
    const originalDefault = normalizeExposurePolicy(
      original.default_exposure_policy,
    );
    if (
      !removesAllFields &&
      JSON.stringify(editDraft.defaultExposurePolicy) !==
        JSON.stringify(originalDefault)
    ) {
      operations.push({
        operation: "set_item_exposure_policy",
        item_id: original.record_id,
        policy: editDraft.defaultExposurePolicy,
      });
      entries.push({
        label: caption(
          props.locale,
          "Default exposure profile",
          "默认暴露面配置",
        ),
        exposureBefore: originalDefault,
        exposureAfter: editDraft.defaultExposurePolicy,
      });
    }

    if (Object.keys(update).length > 2) operations.push(update);
    for (const field of editDraft.fields) {
      if (field.removed) {
        operations.push({
          operation: "delete_field",
          resource_id: field.originalResourceId,
        });
        entries.push({
          label: caption(
            props.locale,
            `Delete field: ${field.originalLabel}`,
            `删除字段：${field.originalLabel}`,
          ),
          before: field.originalResourceId,
          after: caption(props.locale, "Deleted", "已删除"),
          destructive: true,
        });
        continue;
      }
      const nextResourceId = field.resourceId.trim();
      const nextLabel = field.label.trim();
      if (!nextResourceId || !nextLabel) {
        setActionError(
          caption(
            props.locale,
            "Field labels and resource keys cannot be empty.",
            "字段名称和资源 key 不能为空。",
          ),
        );
        return;
      }
      const effectivePolicy =
        field.exposurePolicy ?? editDraft.defaultExposurePolicy;
      const exposureChanged =
        JSON.stringify(effectivePolicy) !==
        JSON.stringify(field.originalExposurePolicy);
      const inherits = field.exposurePolicy === null;
      const sourceChanged = inherits !== field.originalInheritsExposurePolicy;
      if (inherits && sourceChanged)
        operations.push({
          operation: "inherit_field_exposure_policy",
          resource_id: field.originalResourceId,
        });
      const customChanged = !inherits && (exposureChanged || sourceChanged);
      if (nextLabel !== field.originalLabel || customChanged) {
        operations.push({
          operation: "update_field",
          resource_id: field.originalResourceId,
          label: nextLabel !== field.originalLabel ? nextLabel : undefined,
          exposure_policy: customChanged ? effectivePolicy : undefined,
        });
      }
      if (nextLabel !== field.originalLabel)
        entries.push({
          label: caption(props.locale, "Field label", "字段名称"),
          before: field.originalLabel,
          after: nextLabel,
        });
      if (sourceChanged)
        entries.push({
          label: caption(
            props.locale,
            `${field.label} exposure source`,
            `${field.label} 配置来源`,
          ),
          before: field.originalInheritsExposurePolicy
            ? caption(props.locale, "Inherit defaults", "继承默认")
            : caption(props.locale, "Custom", "自定义"),
          after: inherits
            ? caption(props.locale, "Inherit defaults", "继承默认")
            : caption(props.locale, "Custom", "自定义"),
        });
      if (exposureChanged)
        entries.push({
          label: caption(
            props.locale,
            `${field.originalLabel} exposure controls`,
            `${field.originalLabel} 暴露面控制`,
          ),
          exposureBefore: field.originalExposurePolicy,
          exposureAfter: effectivePolicy,
        });
      if (nextResourceId !== field.originalResourceId) {
        operations.push({
          operation: "rename_resource",
          resource_id: field.originalResourceId,
          next_resource_id: nextResourceId,
        });
        entries.push({
          label: caption(
            props.locale,
            `${field.label} resource key`,
            `${field.label} 资源 key`,
          ),
          before: field.originalResourceId,
          after: nextResourceId,
        });
      }
    }
    const changedValues = Object.fromEntries(
      editDraft.fields.flatMap((field) =>
        !field.removed &&
        field.value !== undefined &&
        field.value !== field.originalValue
          ? [[field.originalResourceId, field.value]]
          : [],
      ),
    );
    if (Object.values(changedValues).some((value) => value.length === 0)) {
      setActionError(
        caption(
          props.locale,
          "Password values cannot be empty.",
          "密码值不能为空。",
        ),
      );
      return;
    }
    if (Object.keys(changedValues).length > 0) {
      entries.push({
        label: caption(props.locale, "Password values", "密码值"),
        before: caption(props.locale, "Stored values", "已保存的值"),
        after: caption(
          props.locale,
          `${Object.keys(changedValues).length} changed`,
          `已修改 ${Object.keys(changedValues).length} 项`,
        ),
      });
    }
    if (operations.length === 0 && Object.keys(changedValues).length === 0) {
      setActionError(
        caption(props.locale, "Nothing has changed.", "没有需要保存的更改。"),
      );
      return;
    }
    setConfirmationReason("");
    setPendingConfirmation({
      title: caption(props.locale, "Confirm entry changes", "确认条目修改"),
      description: removesAllFields
        ? caption(
            props.locale,
            "Every field will be deleted, so this entry will also be removed from Plankton. Upstream password-manager entries and source files are not deleted.",
            "所有字段都会被删除，因此 Plankton 中的整个条目也会消失。密码管理器或源文件中的上游条目不会被删除。",
          )
        : removedFields.length > 0
          ? caption(
              props.locale,
              removedFields.length === 1
                ? "1 field and its stored value will be deleted. Other changes will be saved at the same time."
                : `${removedFields.length} fields and their stored values will be deleted. Other changes will be saved at the same time.`,
              `将删除 ${removedFields.length} 个字段及其保存的值，同时保存其他修改。`,
            )
          : caption(
              props.locale,
              Object.keys(changedValues).length > 0
                ? "Confirm these changes before saving. Password values are intentionally hidden here."
                : "Confirm these metadata changes before saving.",
              Object.keys(changedValues).length > 0
                ? "保存前请确认这些修改。密码值在此处会有意隐藏。"
                : "保存前请确认这些元信息修改。",
            ),
      operations,
      entries,
      vaults: vaultLabels(original),
      destructive: removedFields.length > 0,
      confirmLabel:
        removedFields.length > 0
          ? caption(props.locale, "Delete and save", "删除并保存")
          : undefined,
      committingLabel:
        removedFields.length > 0
          ? caption(props.locale, "Deleting and saving…", "正在删除并保存…")
          : undefined,
      valueUpdate:
        Object.keys(changedValues).length > 0
          ? {
              expectedRevision: catalogMetadata?.revision ?? "",
              sourceRecordId: original.record_id,
              values: changedValues,
            }
          : undefined,
    });
  }

  function requestSingleOperationConfirmation(
    kind: "delete" | "refresh",
    metadata: PasswordItemSummary,
  ): void {
    setConfirmationReason("");
    setPendingConfirmation({
      title:
        kind === "delete"
          ? caption(props.locale, "Confirm deletion", "确认删除条目")
          : caption(
              props.locale,
              "Confirm locator refresh",
              "确认刷新 locator",
            ),
      description:
        kind === "delete"
          ? caption(
              props.locale,
              "This removes the entry and stored snapshot from Plankton. Connected password-manager entries and source files are not deleted upstream.",
              "这会删除 Plankton 中的条目及已保存快照，但不会删除密码管理器或源文件中的上游条目。",
            )
          : caption(
              props.locale,
              "The saved snapshot will be refreshed from its retained locator.",
              "将通过保留的 locator 刷新已保存的值快照。",
            ),
      operations: [
        kind === "delete"
          ? { operation: "delete_item", item_id: metadata.item_id }
          : { operation: "refresh_item", item_id: metadata.item_id },
      ],
      entries: [
        {
          label:
            kind === "delete"
              ? caption(props.locale, "Delete entry", "删除条目")
              : caption(props.locale, "Refresh locator", "刷新 locator"),
          before: metadata.item_id,
          after:
            kind === "delete"
              ? caption(props.locale, "Deleted", "已删除")
              : caption(
                  props.locale,
                  "Latest upstream snapshot",
                  "最新上游快照",
                ),
          destructive: kind === "delete",
        },
      ],
      vaults: vaultLabels(metadata),
      destructive: kind === "delete",
      confirmLabel:
        kind === "delete"
          ? caption(props.locale, "Delete entry", "删除条目")
          : undefined,
      committingLabel:
        kind === "delete"
          ? caption(props.locale, "Deleting…", "正在删除…")
          : undefined,
    });
  }

  async function requestDeleteConfirmation(item: PasswordItem): Promise<void> {
    setActionError(null);
    let metadata = metadataForItem(catalogMetadata, item);
    if (!metadata) {
      try {
        const catalog = await loadPasswordCatalogMetadata();
        setCatalogMetadata(catalog);
        metadata = metadataForItem(catalog, item);
      } catch {
        setActionError(
          caption(
            props.locale,
            "This entry could not be prepared for deletion. Refresh the catalog and try again.",
            "无法准备删除此条目。请刷新目录后重试。",
          ),
        );
        return;
      }
    }
    if (!metadata) {
      setActionError(
        caption(
          props.locale,
          "This entry cannot be matched to a deletable catalog record.",
          "无法将此条目匹配到可删除的目录记录。",
        ),
      );
      return;
    }
    requestSingleOperationConfirmation("delete", metadata);
  }

  async function confirmUserChange(): Promise<void> {
    if (!pendingConfirmation || isCommittingChange) return;
    const reason = confirmationReason.trim();
    setIsCommittingChange(true);
    setActionError(null);
    try {
      if (pendingConfirmation.valueUpdate) {
        await invoke("update_local_password_values", {
          request: {
            source_record_id: pendingConfirmation.valueUpdate.sourceRecordId,
            expected_revision: pendingConfirmation.valueUpdate.expectedRevision,
            values: pendingConfirmation.valueUpdate.values,
          },
        });
      }
      if (pendingConfirmation.operations.length > 0) {
        await invoke("submit_desktop_password_change", {
          operations: pendingConfirmation.operations,
          reason,
        });
      }
      setPendingConfirmation(null);
      setEditDraft(null);
      setOrganizeDraft(null);
      setFeedback(caption(props.locale, "Changes saved.", "修改已保存。"));
      await loadItems("refresh");
    } catch (error) {
      setActionError(
        caption(
          props.locale,
          `Changes could not be saved: ${String(error)}`,
          `无法保存修改：${String(error)}`,
        ),
      );
    } finally {
      setIsCommittingChange(false);
    }
  }

  function isCurrentDraft(expected: DialogDraft, draftId: string): boolean {
    const current = currentDraftRef.current;
    return (
      current?.generation === expected.generation &&
      current.id === expected.id &&
      current.id === draftId
    );
  }

  function consumeDraft(expected: DialogDraft): void {
    if (!isCurrentDraft(expected, expected.id)) {
      return;
    }
    currentDraftRef.current = null;
    concealRevealedValues();
    setDialogDraft(null);
    props.onDraftConsumed();
  }

  async function refreshAfterCommit(
    resourceIds: string[],
    reportFailure = true,
  ): Promise<void> {
    concealRevealedValues();
    try {
      const result = await loadPasswordItems();
      if (result.kind !== "live") {
        if (reportFailure) {
          setActionError(
            caption(
              props.locale,
              "Saved, but the password catalog could not be refreshed. Retry from this page.",
              "已保存，但无法刷新密码目录。请在此页面重试。",
            ),
          );
        }
        return;
      }
      setItems(result.items);
      setSelectedId(
        resourceIds[0] ? passwordItemIdForResource(resourceIds[0]) : "",
      );
    } catch {
      if (reportFailure) {
        setActionError(
          caption(
            props.locale,
            "Saved, but the password catalog could not be refreshed. Retry from this page.",
            "已保存，但无法刷新密码目录。请在此页面重试。",
          ),
        );
      }
    }
  }

  const filterProps = {
    availableBackends,
    availableTags,
    backendFilter,
    locale: props.locale,
    onTagFilterChange: (value: string) => {
      concealRevealedValues();
      setTagFilter(value);
      resetPage();
    },
    onTagModeChange: (mode: TagMode) => {
      concealRevealedValues();
      setTagMode(mode);
      resetPage();
    },
    onToggleBackend: (backend: PasswordBackend) => {
      concealRevealedValues();
      setBackendFilter((current) =>
        current.includes(backend)
          ? current.filter((entry) => entry !== backend)
          : [...current, backend],
      );
      resetPage();
    },
    onVaultFilterChange: (vault: string) => {
      concealRevealedValues();
      setVaultFilter(vault);
      resetPage();
    },
    tagFilter,
    tagMode,
    vaultFilter,
    vaults,
  };

  const draftDialog = dialogDraft ? (
    <PasswordAddDialog
      draftId={dialogDraft.id}
      key={dialogDraft.generation}
      locale={props.locale}
      onClose={() => consumeDraft(dialogDraft)}
      onManageVaults={() => setVaultManagerOpen(true)}
      vaultRevision={vaultRevision}
      onCommitted={(committedDraftId, receipt) => {
        if (
          receipt.draft_id !== committedDraftId ||
          !isCurrentDraft(dialogDraft, committedDraftId)
        ) {
          void refreshAfterCommit(receipt.resource_ids, false);
          return;
        }
        consumeDraft(dialogDraft);
        setFeedback(
          caption(
            props.locale,
            `Saved ${receipt.resource_ids.length} fields to ${receipt.destination}.`,
            `已向 ${receipt.destination} 写入 ${receipt.resource_ids.length} 个字段。`,
          ),
        );
        void refreshAfterCommit(receipt.resource_ids);
      }}
    />
  ) : null;

  const hasActiveFilters =
    Boolean(query.trim()) ||
    Boolean(tagFilter.trim()) ||
    Boolean(vaultFilter) ||
    availableBackends.some((backend) => !backendFilter.includes(backend));

  return (
    <>
      <section
        aria-label={caption(props.locale, "Password vault", "密码库")}
        className="workspace-page password-page password-vault-shell"
      >
        <PageHeader
          icon={KeyRound}
          description={caption(
            props.locale,
            "Search, safely use, and edit credential metadata in one place.",
            "在一个页面中搜索、安全使用并编辑凭据元信息。",
          )}
          eyebrow={caption(props.locale, "LOCAL-FIRST VAULT", "本地优先保险库")}
          primaryAction={
            <div className="password-header-actions">
              <button onClick={() => setVaultManagerOpen(true)} type="button">
                <FolderKey aria-hidden="true" size={16} />
                {caption(props.locale, "Manage vaults", "管理保险库")}
              </button>
              <button
                className="primary"
                onClick={() => setImportDialogOpen(true)}
                type="button"
              >
                <Plus aria-hidden="true" size={16} />
                {caption(props.locale, "Add or import", "添加或导入")}
              </button>
            </div>
          }
          title={caption(props.locale, "Passwords", "密码库")}
        />

        {actionError ? (
          <p className="workspace-alert" role="alert">
            {actionError}
            <button
              aria-label={caption(props.locale, "Dismiss error", "关闭错误")}
              onClick={() => setActionError(null)}
              type="button"
            >
              {caption(props.locale, "Dismiss", "关闭")}
            </button>
          </p>
        ) : null}
        {feedback ? (
          <p className="workspace-notice" role="status">
            {feedback}
          </p>
        ) : null}

        {loadState === "error" ? (
          <ErrorState
            action={
              <button onClick={() => void loadItems()} type="button">
                {caption(props.locale, "Retry", "重试")}
              </button>
            }
            description={
              loadError ??
              caption(
                props.locale,
                "The password catalog could not be loaded.",
                "无法加载密码目录。",
              )
            }
            eyebrow={caption(props.locale, "CATALOG ERROR", "目录错误")}
            title={caption(
              props.locale,
              "Passwords are unavailable",
              "密码暂不可用",
            )}
          />
        ) : loadState === "loading" ? (
          <section
            aria-live="polite"
            className="password-vault-loading"
            data-testid="password-vault-loading"
            role="status"
          >
            <p>
              {caption(props.locale, "Loading passwords…", "正在加载密码…")}
            </p>
          </section>
        ) : loadState === "ready" &&
          visible.length === 0 &&
          !hasActiveFilters ? (
          <EmptyState
            action={
              <div className="password-empty-actions">
                <button
                  className="primary"
                  onClick={() => setImportDialogOpen(true)}
                  type="button"
                >
                  {caption(props.locale, "Add or import", "添加或导入")}
                </button>
              </div>
            }
            description={caption(
              props.locale,
              "Add a local entry, import from an enabled manager, or create a one-time draft with the CLI.",
              "添加本地条目、从已启用的管理器导入，或通过 CLI 创建一次性草稿。",
            )}
            eyebrow={caption(props.locale, "PASSWORD CATALOG", "密码目录")}
            title={caption(props.locale, "No passwords yet", "还没有密码")}
          />
        ) : (
          <div
            className={
              editDraft ? "vault-layout is-editing-entry" : "vault-layout"
            }
          >
            <aside
              aria-label={caption(props.locale, "Password filters", "密码筛选")}
              className="vault-sidebar"
            >
              <h2>{caption(props.locale, "Browse", "浏览")}</h2>
              <FilterSurface {...filterProps} context="sidebar" />
            </aside>

            <section
              aria-label={caption(props.locale, "Password list", "密码列表")}
              className="item-list-pane"
            >
              <div className="search-toolbar">
                <label className="password-search-field">
                  <span>{caption(props.locale, "Search", "搜索")}</span>
                  <span className="password-search-control">
                    <Search
                      aria-hidden="true"
                      focusable="false"
                      size={16}
                      strokeWidth={1.75}
                    />
                    <input
                      aria-label={caption(
                        props.locale,
                        "Search password items",
                        "搜索密码条目",
                      )}
                      onChange={(event) => {
                        concealRevealedValues();
                        setQuery(event.currentTarget.value);
                        resetPage();
                      }}
                      placeholder={caption(
                        props.locale,
                        "Title, notes, field key, or tag",
                        "标题、备注、字段 key 或标签",
                      )}
                      value={query}
                    />
                  </span>
                </label>
                <button
                  aria-label={caption(
                    props.locale,
                    "Open password filters",
                    "打开密码筛选",
                  )}
                  className="password-filter-trigger"
                  onClick={() => setFilterDrawerOpen(true)}
                  type="button"
                >
                  <ListFilter
                    aria-hidden="true"
                    focusable="false"
                    size={16}
                    strokeWidth={1.75}
                  />
                  {caption(props.locale, "Filters", "筛选")}
                </button>
              </div>
              <div className="result-meta">
                <span>
                  {visible.length} {caption(props.locale, "results", "个结果")}
                </span>
                <span>
                  {caption(
                    props.locale,
                    "Match reasons shown",
                    "已显示匹配原因",
                  )}
                </span>
              </div>
              <div className="password-list-scroll">
                {visible.length === 0 ? (
                  <div
                    className="password-list-empty"
                    data-testid="password-list-empty"
                    role="status"
                  >
                    <strong>
                      {caption(
                        props.locale,
                        "No matching passwords",
                        "没有匹配的密码",
                      )}
                    </strong>
                    <p>
                      {caption(
                        props.locale,
                        "Try another search or clear the current filters.",
                        "请尝试其他搜索词，或清除当前筛选。",
                      )}
                    </p>
                    <button onClick={clearFilters} type="button">
                      {caption(props.locale, "Clear filters", "清除筛选")}
                    </button>
                  </div>
                ) : (
                  <div className="password-list">
                    {paged.map((result) => (
                      <button
                        aria-pressed={selected?.id === result.item.id}
                        className={
                          selected?.id === result.item.id
                            ? "password-row active"
                            : "password-row"
                        }
                        key={result.item.id}
                        onClick={() => {
                          concealRevealedValues();
                          setItemMenuOpen(false);
                          setItemContextMenu(null);
                          setSelectedId(result.item.id);
                        }}
                        onContextMenu={(event) => {
                          event.preventDefault();
                          openItemContextMenu(
                            result.item,
                            event.clientX,
                            event.clientY,
                          );
                        }}
                        onKeyDown={(event) => {
                          if (
                            event.key !== "ContextMenu" &&
                            !(event.shiftKey && event.key === "F10")
                          ) {
                            return;
                          }
                          event.preventDefault();
                          const bounds =
                            event.currentTarget.getBoundingClientRect();
                          openItemContextMenu(
                            result.item,
                            bounds.left + 18,
                            bounds.top + 18,
                          );
                        }}
                        type="button"
                      >
                        <strong>{result.item.title}</strong>
                        <span>
                          {result.item.username || result.item.group} ·{" "}
                          {originLabel(props.locale, result.item.origin)}
                        </span>
                        <small>{matchReason(props.locale, result)}</small>
                      </button>
                    ))}
                  </div>
                )}
              </div>
              <Pagination
                label={caption(props.locale, "Password pages", "密码列表分页")}
                nextLabel={caption(props.locale, "Next page", "下一页")}
                onPageChange={(nextPage) => {
                  concealRevealedValues();
                  setPage(nextPage);
                }}
                page={currentPage}
                pageCount={pageCount}
                previousLabel={caption(props.locale, "Previous page", "上一页")}
              />
            </section>

            <section
              aria-label={caption(props.locale, "Password detail", "密码详情")}
              className="item-detail-pane"
            >
              {selected ? (
                <>
                  <div className="item-detail-scroll">
                    {editDraft ? (
                      <form
                        className="password-inline-editor"
                        onSubmit={(event) => {
                          event.preventDefault();
                          requestEditConfirmation();
                        }}
                      >
                        <div className="detail-title">
                          <div>
                            <p className="eyebrow">
                              {caption(props.locale, "EDIT ENTRY", "编辑条目")}
                            </p>
                            <h2>{editDraft.original.title}</h2>
                          </div>
                        </div>
                        <div className="password-edit-context">
                          <span>{caption(props.locale, "Source", "来源")}</span>
                          <strong>
                            {originLabel(props.locale, selected.origin)} ·{" "}
                            {selected.vault}
                          </strong>
                          <code>{editDraft.original.item_id}</code>
                        </div>
                        <label>
                          <span>{caption(props.locale, "Title", "标题")}</span>
                          <input
                            onChange={(event) => {
                              const value = event.currentTarget.value;
                              setEditDraft((current) =>
                                current ? { ...current, title: value } : null,
                              );
                            }}
                            value={editDraft.title}
                          />
                        </label>
                        <label>
                          <span>
                            {caption(props.locale, "Item ID", "条目 ID")}
                          </span>
                          <input
                            onChange={(event) => {
                              const value = event.currentTarget.value;
                              setEditDraft((current) =>
                                current ? { ...current, itemId: value } : null,
                              );
                            }}
                            value={editDraft.itemId}
                          />
                          <small>
                            {caption(
                              props.locale,
                              "Changing it may affect callers that use this ID.",
                              "修改后可能影响使用此 ID 的调用方。",
                            )}
                          </small>
                        </label>
                        <label>
                          <span>{caption(props.locale, "Notes", "备注")}</span>
                          <textarea
                            onChange={(event) => {
                              const value = event.currentTarget.value;
                              setEditDraft((current) =>
                                current
                                  ? {
                                      ...current,
                                      description: value,
                                    }
                                  : null,
                              );
                            }}
                            rows={3}
                            value={editDraft.description}
                          />
                        </label>
                        <label>
                          <span>{caption(props.locale, "Tags", "标签")}</span>
                          <input
                            onChange={(event) => {
                              const value = event.currentTarget.value;
                              setEditDraft((current) =>
                                current ? { ...current, tags: value } : null,
                              );
                            }}
                            placeholder={caption(
                              props.locale,
                              "production, shared",
                              "生产、共享",
                            )}
                            value={editDraft.tags}
                          />
                        </label>
                        <CollectionExposurePolicyEditor
                          locale={props.locale}
                          value={editDraft.defaultExposurePolicy}
                          onChange={(defaultExposurePolicy) =>
                            setEditDraft((current) =>
                              current
                                ? { ...current, defaultExposurePolicy }
                                : null,
                            )
                          }
                        />
                        <fieldset className="password-edit-fields">
                          <legend>
                            {caption(
                              props.locale,
                              "Fields and values",
                              "字段和值",
                            )}
                          </legend>
                          {editDraft.fields.map((field, index) => (
                            <div
                              className={
                                field.removed
                                  ? "password-edit-field removed"
                                  : "password-edit-field"
                              }
                              key={field.originalResourceId}
                            >
                              {field.removed ? (
                                <div className="password-edit-field-removed">
                                  <span>
                                    <strong>{field.originalLabel}</strong>
                                    <small>
                                      {caption(
                                        props.locale,
                                        "This field and its stored value will be deleted when you save.",
                                        "保存时将删除此字段及其已保存的值。",
                                      )}
                                    </small>
                                  </span>
                                  <button
                                    onClick={() =>
                                      setEditDraft((current) =>
                                        current
                                          ? {
                                              ...current,
                                              fields: current.fields.map(
                                                (candidate, candidateIndex) =>
                                                  candidateIndex === index
                                                    ? {
                                                        ...candidate,
                                                        removed: false,
                                                      }
                                                    : candidate,
                                              ),
                                            }
                                          : null,
                                      )
                                    }
                                    type="button"
                                  >
                                    {caption(props.locale, "Undo", "撤销")}
                                  </button>
                                </div>
                              ) : (
                                <>
                                  <div className="password-edit-field-header">
                                    <strong>{field.originalLabel}</strong>
                                    <button
                                      aria-label={caption(
                                        props.locale,
                                        `Delete ${field.originalLabel} field`,
                                        `删除${field.originalLabel}字段`,
                                      )}
                                      className="password-edit-field-delete"
                                      onClick={() =>
                                        setEditDraft((current) =>
                                          current
                                            ? {
                                                ...current,
                                                fields: current.fields.map(
                                                  (
                                                    candidate,
                                                    candidateIndex,
                                                  ) =>
                                                    candidateIndex === index
                                                      ? {
                                                          ...candidate,
                                                          removed: true,
                                                        }
                                                      : candidate,
                                                ),
                                              }
                                            : null,
                                        )
                                      }
                                      type="button"
                                    >
                                      <Trash2
                                        aria-hidden="true"
                                        focusable="false"
                                        size={15}
                                        strokeWidth={1.75}
                                      />
                                      {caption(
                                        props.locale,
                                        "Delete field",
                                        "删除字段",
                                      )}
                                    </button>
                                  </div>
                                  <label>
                                    <span>
                                      {caption(
                                        props.locale,
                                        "Field label",
                                        "字段名称",
                                      )}
                                    </span>
                                    <input
                                      onChange={(event) => {
                                        const label = event.currentTarget.value;
                                        setEditDraft((current) =>
                                          current
                                            ? {
                                                ...current,
                                                fields: current.fields.map(
                                                  (
                                                    candidate,
                                                    candidateIndex,
                                                  ) =>
                                                    candidateIndex === index
                                                      ? { ...candidate, label }
                                                      : candidate,
                                                ),
                                              }
                                            : null,
                                        );
                                      }}
                                      value={field.label}
                                    />
                                  </label>
                                  <label>
                                    <span>
                                      {caption(
                                        props.locale,
                                        "Resource key",
                                        "资源 key",
                                      )}
                                    </span>
                                    <input
                                      onChange={(event) => {
                                        const value = event.currentTarget.value;
                                        setEditDraft((current) =>
                                          current
                                            ? {
                                                ...current,
                                                fields: current.fields.map(
                                                  (
                                                    candidate,
                                                    candidateIndex,
                                                  ) =>
                                                    candidateIndex === index
                                                      ? {
                                                          ...candidate,
                                                          resourceId: value,
                                                        }
                                                      : candidate,
                                                ),
                                              }
                                            : null,
                                        );
                                      }}
                                      value={field.resourceId}
                                    />
                                  </label>
                                  <label>
                                    <span>
                                      {caption(
                                        props.locale,
                                        `${field.label} value`,
                                        `${field.label} 的值`,
                                      )}
                                    </span>
                                    <SecretInput
                                      locale={props.locale}
                                      fieldName={field.label}
                                      autoReveal={
                                        (
                                          field.exposurePolicy ??
                                          editDraft.defaultExposurePolicy
                                        ).access_mode === "direct"
                                      }
                                      onReveal={() =>
                                        loadEditValue(field.originalResourceId)
                                      }
                                      placeholder="••••••••"
                                      aria-label={caption(
                                        props.locale,
                                        `${field.label} password value`,
                                        `${field.label} 密码值`,
                                      )}
                                      autoComplete="new-password"
                                      onChange={(event) => {
                                        const value = event.currentTarget.value;
                                        setEditDraft((current) =>
                                          current
                                            ? {
                                                ...current,
                                                fields: current.fields.map(
                                                  (
                                                    candidate,
                                                    candidateIndex,
                                                  ) =>
                                                    candidateIndex === index
                                                      ? {
                                                          ...candidate,
                                                          value,
                                                        }
                                                      : candidate,
                                                ),
                                              }
                                            : null,
                                        );
                                      }}
                                      value={field.value ?? ""}
                                    />
                                  </label>
                                  <details className="password-edit-exposure">
                                    <summary>
                                      {caption(
                                        props.locale,
                                        "Exposure controls",
                                        "暴露面控制",
                                      )}
                                    </summary>
                                    <FieldExposurePolicyEditor
                                      defaultPolicy={
                                        editDraft.defaultExposurePolicy
                                      }
                                      locale={props.locale}
                                      onChange={(exposurePolicy) =>
                                        setEditDraft((current) =>
                                          current
                                            ? {
                                                ...current,
                                                fields: current.fields.map(
                                                  (
                                                    candidate,
                                                    candidateIndex,
                                                  ) =>
                                                    candidateIndex === index
                                                      ? {
                                                          ...candidate,
                                                          exposurePolicy,
                                                        }
                                                      : candidate,
                                                ),
                                              }
                                            : null,
                                        )
                                      }
                                      customPolicy={field.exposurePolicy}
                                    />
                                  </details>
                                </>
                              )}
                            </div>
                          ))}
                          {editDraft.fields.every((field) => field.removed) ? (
                            <p className="password-edit-delete-all-warning">
                              {caption(
                                props.locale,
                                "Saving now will delete the entire entry because no fields remain.",
                                "当前保存会删除整个条目，因为已没有保留的字段。",
                              )}
                            </p>
                          ) : null}
                        </fieldset>
                      </form>
                    ) : (
                      <>
                        <div className="detail-title">
                          <div>
                            <p className="eyebrow">
                              {selected.vault} / {selected.group}
                            </p>
                            <h2>{selected.title}</h2>
                            <span className="password-origin-badge">
                              {originLabel(props.locale, selected.origin)}
                            </span>
                          </div>
                          <div className="detail-title-actions">
                            <button
                              className="detail-edit-button"
                              onClick={() => {
                                setItemMenuOpen(false);
                                void startEditing(selected);
                              }}
                              type="button"
                            >
                              {caption(props.locale, "Edit entry", "编辑条目")}
                            </button>
                            <div
                              className="password-item-menu"
                              ref={itemMenuRef}
                            >
                              <button
                                aria-expanded={itemMenuOpen}
                                aria-haspopup="menu"
                                aria-label={caption(
                                  props.locale,
                                  "More entry actions",
                                  "更多条目操作",
                                )}
                                className="password-item-menu-trigger"
                                onClick={() =>
                                  setItemMenuOpen((current) => !current)
                                }
                                ref={itemMenuTriggerRef}
                                type="button"
                              >
                                <Ellipsis
                                  aria-hidden="true"
                                  focusable="false"
                                  size={20}
                                  strokeWidth={1.75}
                                />
                              </button>
                              {itemMenuOpen ? (
                                <div
                                  aria-label={caption(
                                    props.locale,
                                    "Entry actions",
                                    "条目操作",
                                  )}
                                  className="password-item-menu-popover"
                                  role="menu"
                                >
                                  <button
                                    className="danger"
                                    onClick={() => {
                                      setItemMenuOpen(false);
                                      void requestDeleteConfirmation(selected);
                                    }}
                                    ref={itemMenuFirstActionRef}
                                    role="menuitem"
                                    type="button"
                                  >
                                    <Trash2
                                      aria-hidden="true"
                                      focusable="false"
                                      size={17}
                                      strokeWidth={1.75}
                                    />
                                    {caption(
                                      props.locale,
                                      "Delete entry",
                                      "删除条目",
                                    )}
                                  </button>
                                </div>
                              ) : null}
                            </div>
                          </div>
                        </div>
                        {selected.notes ? <p>{selected.notes}</p> : null}
                        <div className="tag-list">
                          {selected.tags.map((tag) => (
                            <span key={tag}>#{tag}</span>
                          ))}
                        </div>
                        <details
                          className="collection-exposure-profile collection-exposure-summary"
                          key={selected.id}
                        >
                          <summary>
                            {caption(
                              props.locale,
                              "Default exposure profile",
                              "默认暴露面配置",
                            )}
                          </summary>
                          <ExposurePolicySummary
                            locale={props.locale}
                            value={normalizeExposurePolicy(
                              selectedMetadata?.default_exposure_policy,
                            )}
                            onEdit={() => void startEditing(selected)}
                          />
                        </details>
                        <dl className="field-list">
                          {selected.fields.map((field) => {
                            const exposurePolicy = normalizeExposurePolicy(
                              selectedMetadata?.fields.find(
                                (candidate) =>
                                  candidate.resource_id === field.resourceId,
                              )?.exposure_policy ??
                                selectedMetadata?.default_exposure_policy,
                            );
                            const visibleRevealedValues =
                              revealedValues?.itemId === selected.id
                                ? revealedValues.values
                                : {};
                            const isWorking =
                              workingFieldId === field.resourceId;
                            return (
                              <div
                                className="password-field-row"
                                key={field.resourceId}
                              >
                                <dt className="field-identity">
                                  <strong>{field.label}</strong>
                                  {field.key !== field.label ? (
                                    <small>{field.key}</small>
                                  ) : null}
                                  <span className="password-field-access-mode">
                                    {exposurePolicy.access_mode === "direct"
                                      ? caption(
                                          props.locale,
                                          "Direct · no approval",
                                          "直接可见 · 无需审批",
                                        )
                                      : caption(
                                          props.locale,
                                          "Protected · exposure review",
                                          "受保护 · 暴露面审批",
                                        )}
                                  </span>
                                  <details className="field-reference">
                                    <summary>
                                      {caption(
                                        props.locale,
                                        "Field reference",
                                        "字段标识",
                                      )}
                                    </summary>
                                    <code className="field-resource-id">
                                      {field.resourceId}
                                    </code>
                                  </details>
                                </dt>
                                <dd className="field-value">
                                  <SecretInput
                                    key={`${selected.id}:${field.resourceId}`}
                                    resetKey={visibilityEpoch}
                                    locale={props.locale}
                                    fieldName={field.label}
                                    aria-label={field.label}
                                    readOnly
                                    placeholder="••••••••"
                                    autoReveal={
                                      !field.secret ||
                                      exposurePolicy.access_mode === "direct"
                                    }
                                    onReveal={() =>
                                      field.secret
                                        ? revealField(selected.id, field)
                                        : Promise.resolve(true)
                                    }
                                    onConceal={() =>
                                      setRevealedValues((current) => {
                                        if (current?.itemId !== selected.id)
                                          return current;
                                        const values = { ...current.values };
                                        delete values[field.resourceId];
                                        return {
                                          itemId: current.itemId,
                                          values,
                                        };
                                      })
                                    }
                                    value={
                                      !field.secret
                                        ? field.value
                                        : (visibleRevealedValues[
                                            field.resourceId
                                          ] ?? "")
                                    }
                                  />
                                </dd>
                                <dd className="field-actions">
                                  <button
                                    aria-label={caption(
                                      props.locale,
                                      `Copy ${field.label}`,
                                      `复制${field.label}`,
                                    )}
                                    disabled={isWorking}
                                    onClick={() => void copyField(field)}
                                    type="button"
                                  >
                                    <Copy
                                      aria-hidden="true"
                                      focusable="false"
                                      size={18}
                                      strokeWidth={1.75}
                                    />
                                  </button>
                                </dd>
                                <dd className="password-field-exposure-summary">
                                  {selectedMetadata?.fields.find(
                                    (candidate) =>
                                      candidate.resource_id ===
                                      field.resourceId,
                                  )?.inherits_exposure_policy ? (
                                    <p className="field-exposure-inherited-hint">
                                      {caption(
                                        props.locale,
                                        "Inherits defaults",
                                        "继承默认配置",
                                      )}
                                    </p>
                                  ) : (
                                    <ExposurePolicySummary
                                      collapsible
                                      locale={props.locale}
                                      onEdit={() => void startEditing(selected)}
                                      value={exposurePolicy}
                                    />
                                  )}
                                </dd>
                              </div>
                            );
                          })}
                        </dl>
                      </>
                    )}
                  </div>
                  <footer className="item-detail-actions">
                    {editDraft ? (
                      <>
                        <button
                          onClick={() => setEditDraft(null)}
                          type="button"
                        >
                          {caption(props.locale, "Cancel", "取消")}
                        </button>
                        <button
                          className="primary"
                          onClick={requestEditConfirmation}
                          type="button"
                        >
                          {caption(props.locale, "Save", "保存")}
                        </button>
                      </>
                    ) : (
                      <>
                        <button
                          onClick={() =>
                            void copyItemId(
                              selectedMetadata?.item_id ?? selected.id,
                            )
                          }
                          type="button"
                        >
                          <Copy
                            aria-hidden="true"
                            focusable="false"
                            size={16}
                            strokeWidth={1.75}
                          />
                          {caption(props.locale, "Copy item ID", "复制条目 ID")}
                        </button>
                        {selectedMetadata?.fields.length ? (
                          <button
                            onClick={() =>
                              setMigrationDialog({ item: selectedMetadata })
                            }
                            type="button"
                          >
                            {caption(
                              props.locale,
                              "Move to vault",
                              "迁移到保险库",
                            )}
                          </button>
                        ) : null}
                        {selectedMetadata?.fields.length ? (
                          <button
                            onClick={() => void startOrganizing(selected)}
                            type="button"
                          >
                            {caption(
                              props.locale,
                              "Organize fields",
                              "整理字段",
                            )}
                          </button>
                        ) : null}
                        {selectedMetadata ? (
                          <button
                            onClick={() =>
                              requestSingleOperationConfirmation(
                                "refresh",
                                selectedMetadata,
                              )
                            }
                            type="button"
                          >
                            {caption(
                              props.locale,
                              "Refresh locator",
                              "刷新 locator",
                            )}
                          </button>
                        ) : null}
                      </>
                    )}
                  </footer>
                </>
              ) : (
                <div className="empty-detail">
                  {caption(
                    props.locale,
                    "Select a password item",
                    "选择一个密码条目",
                  )}
                </div>
              )}
            </section>
          </div>
        )}

        <Drawer
          closeLabel={caption(
            props.locale,
            "Close password filters drawer",
            "关闭密码筛选抽屉",
          )}
          description={caption(
            props.locale,
            "Filter by entry type, vault, and one or more tags.",
            "按条目类型、保险库与一个或多个标签筛选。",
          )}
          footer={
            <button
              onClick={() => {
                clearFilters();
                setFilterDrawerOpen(false);
              }}
              type="button"
            >
              {caption(props.locale, "Clear filters", "清除筛选")}
            </button>
          }
          onClose={() => setFilterDrawerOpen(false)}
          open={filterDrawerOpen}
          title={caption(props.locale, "Password filters", "密码筛选")}
        >
          <FilterSurface {...filterProps} context="drawer" />
        </Drawer>
        {migrationDialog && catalogMetadata ? (
          <PasswordMigrationDialog
            catalogRevision={catalogMetadata.revision}
            initialBackend={migrationDialog.handoff?.backend}
            initialMode={migrationDialog.handoff?.mode}
            initialVault={migrationDialog.handoff?.vault}
            item={migrationDialog.item}
            locale={props.locale}
            onClose={() => setMigrationDialog(null)}
            onManageVaults={() => setVaultManagerOpen(true)}
            vaultRevision={vaultRevision}
            onCompleted={(receipt) => {
              setMigrationDialog(null);
              setFeedback(
                caption(
                  props.locale,
                  `${receipt.mode === "move" ? "Moved" : "Copied"} ${receipt.resource_ids.length} fields to ${receipt.destination}.`,
                  `已${receipt.mode === "move" ? "迁移" : "复制"} ${receipt.resource_ids.length} 个字段到 ${receipt.destination}。`,
                ),
              );
              void loadItems("refresh");
            }}
          />
        ) : null}
        {vaultManagerOpen ? (
          <LocalVaultManagerDialog
            locale={props.locale}
            onChanged={() => {
              setVaultRevision((current) => current + 1);
              void loadItems("refresh");
            }}
            onClose={() => setVaultManagerOpen(false)}
          />
        ) : null}

        <Dialog
          closeLabel={caption(
            props.locale,
            "Close add or import dialog",
            "关闭添加或导入对话框",
          )}
          description={caption(
            props.locale,
            "Create a password directly, or import selected entries from a connected source or one or more .env files.",
            "直接添加密码，或从已连接来源、一个或多个 .env 文件中选择条目导入。",
          )}
          onClose={() => setImportDialogOpen(false)}
          open={importDialogOpen}
          title={caption(props.locale, "Add or import", "添加或导入")}
        >
          <PasswordManagementView
            locale={props.locale}
            onCatalogChange={loadItems}
            onDraftCreated={(draftId) => {
              setImportDialogOpen(false);
              const nextDraft = {
                generation: draftGenerationRef.current + 1,
                id: draftId,
              };
              draftGenerationRef.current = nextDraft.generation;
              currentDraftRef.current = nextDraft;
              setDialogDraft(nextDraft);
            }}
            surface="import"
          />
        </Dialog>

        <Dialog
          closeLabel={caption(
            props.locale,
            "Close field organization",
            "关闭字段整理",
          )}
          description={caption(
            props.locale,
            "Split fields into a new password, merge them into an existing password, or remove a locally verified duplicate.",
            "将字段拆分为新密码、合并到已有密码，或删除经过本地验证的重复字段。",
          )}
          footer={
            organizeDraft ? (
              <div className="password-confirmation-actions">
                <button onClick={() => setOrganizeDraft(null)} type="button">
                  {caption(props.locale, "Cancel", "取消")}
                </button>
                <button
                  className="primary"
                  onClick={requestOrganizeConfirmation}
                  type="button"
                >
                  {caption(props.locale, "Review changes", "检查更改")}
                </button>
              </div>
            ) : null
          }
          onClose={() => setOrganizeDraft(null)}
          open={organizeDraft !== null}
          title={caption(
            props.locale,
            "Organize password fields",
            "整理密码字段",
          )}
        >
          {organizeDraft ? (
            <form
              className="password-organize-form"
              onSubmit={(event) => {
                event.preventDefault();
                requestOrganizeConfirmation();
              }}
            >
              <ChoiceGroup
                label={
                  <>
                    <span>{caption(props.locale, "Action", "操作")}</span>
                  </>
                }
                onChange={(value) => {
                  const mode = value as OrganizeMode;
                  const firstTarget = catalogMetadata?.items.find(
                    (item) => item.record_id !== organizeDraft.source.record_id,
                  );
                  const firstCanonical = catalogMetadata?.items
                    .filter(
                      (item) =>
                        item.record_id !== organizeDraft.source.record_id,
                    )
                    .flatMap((item) => item.fields)[0];
                  setOrganizeDraft((current) =>
                    current
                      ? {
                          ...current,
                          mode,
                          selectedResourceIds:
                            mode === "dedupe"
                              ? current.source.fields
                                  .slice(0, 1)
                                  .map((field) => field.resource_id)
                              : current.source.fields.map(
                                  (field) => field.resource_id,
                                ),
                          targetItemId:
                            mode === "merge"
                              ? (firstTarget?.item_id ?? "")
                              : "",
                          targetTitle: "",
                          canonicalResourceId:
                            firstCanonical?.resource_id ?? "",
                        }
                      : null,
                  );
                }}
                value={organizeDraft.mode}
                options={[
                  {
                    value: "split",
                    label: (
                      <>
                        {caption(
                          props.locale,
                          "Split into a new item",
                          "拆分为新条目",
                        )}
                      </>
                    ),
                  },
                  {
                    value: "merge",
                    label: (
                      <>
                        {caption(
                          props.locale,
                          "Move or merge into an existing item",
                          "移动或合并到已有条目",
                        )}
                      </>
                    ),
                  },
                  {
                    value: "dedupe",
                    label: (
                      <>
                        {caption(
                          props.locale,
                          "Remove a verified duplicate",
                          "删除已验证的重复字段",
                        )}
                      </>
                    ),
                  },
                ]}
              />

              {organizeDraft.mode === "dedupe" ? (
                <>
                  <label>
                    <span>
                      {caption(props.locale, "Duplicate field", "重复字段")}
                    </span>
                    <select
                      onChange={(event) => {
                        const value = event.currentTarget.value;
                        setOrganizeDraft((current) =>
                          current
                            ? {
                                ...current,
                                selectedResourceIds: [value],
                              }
                            : null,
                        );
                      }}
                      value={organizeDraft.selectedResourceIds[0] ?? ""}
                    >
                      {organizeDraft.source.fields.map((field) => (
                        <option
                          key={field.resource_id}
                          value={field.resource_id}
                        >
                          {field.label}
                        </option>
                      ))}
                    </select>
                  </label>
                  <label>
                    <span>
                      {caption(props.locale, "Keep field", "保留字段")}
                    </span>
                    <select
                      onChange={(event) => {
                        const value = event.currentTarget.value;
                        setOrganizeDraft((current) =>
                          current
                            ? {
                                ...current,
                                canonicalResourceId: value,
                              }
                            : null,
                        );
                      }}
                      value={organizeDraft.canonicalResourceId}
                    >
                      {(catalogMetadata?.items ?? [])
                        .filter(
                          (item) =>
                            item.record_id !== organizeDraft.source.record_id,
                        )
                        .flatMap((item) =>
                          item.fields.map((field) => (
                            <option
                              key={field.resource_id}
                              value={field.resource_id}
                            >
                              {item.title} / {field.label}
                            </option>
                          )),
                        )}
                    </select>
                    <small>
                      {caption(
                        props.locale,
                        "Plankton compares the two stored values locally and refuses deletion if they differ.",
                        "Plankton 仅在本地比较两个已存储值；值不一致时会拒绝删除。",
                      )}
                    </small>
                  </label>
                </>
              ) : (
                <>
                  <fieldset>
                    <legend>
                      {caption(props.locale, "Fields to move", "要移动的字段")}
                    </legend>
                    {organizeDraft.source.fields.map((field) => (
                      <label
                        className="password-organize-field"
                        key={field.resource_id}
                      >
                        <input
                          checked={organizeDraft.selectedResourceIds.includes(
                            field.resource_id,
                          )}
                          onChange={(event) => {
                            const checked = event.currentTarget.checked;
                            setOrganizeDraft((current) =>
                              current
                                ? {
                                    ...current,
                                    selectedResourceIds: checked
                                      ? [
                                          ...current.selectedResourceIds,
                                          field.resource_id,
                                        ]
                                      : current.selectedResourceIds.filter(
                                          (resource) =>
                                            resource !== field.resource_id,
                                        ),
                                  }
                                : null,
                            );
                          }}
                          type="checkbox"
                        />
                        <span>
                          <strong>{field.label}</strong>
                          <code>{field.resource_id}</code>
                        </span>
                      </label>
                    ))}
                  </fieldset>
                  {organizeDraft.mode === "split" ? (
                    <div className="password-organize-target-grid">
                      <label>
                        <span>
                          {caption(props.locale, "New title", "新标题")}
                        </span>
                        <input
                          onChange={(event) => {
                            const value = event.currentTarget.value;
                            setOrganizeDraft((current) =>
                              current
                                ? { ...current, targetTitle: value }
                                : null,
                            );
                          }}
                          value={organizeDraft.targetTitle}
                        />
                      </label>
                      <label>
                        <span>
                          {caption(props.locale, "New item ID", "新条目 ID")}
                        </span>
                        <input
                          onChange={(event) => {
                            const value = event.currentTarget.value;
                            setOrganizeDraft((current) =>
                              current
                                ? { ...current, targetItemId: value }
                                : null,
                            );
                          }}
                          value={organizeDraft.targetItemId}
                        />
                      </label>
                    </div>
                  ) : (
                    <label>
                      <span>
                        {caption(props.locale, "Target item", "目标条目")}
                      </span>
                      <select
                        onChange={(event) => {
                          const value = event.currentTarget.value;
                          setOrganizeDraft((current) =>
                            current
                              ? { ...current, targetItemId: value }
                              : null,
                          );
                        }}
                        value={organizeDraft.targetItemId}
                      >
                        {(catalogMetadata?.items ?? [])
                          .filter(
                            (item) =>
                              item.record_id !== organizeDraft.source.record_id,
                          )
                          .map((item) => (
                            <option key={item.record_id} value={item.item_id}>
                              {item.title}
                            </option>
                          ))}
                      </select>
                    </label>
                  )}
                </>
              )}
            </form>
          ) : null}
        </Dialog>

        <Dialog
          closeDisabled={isCommittingChange}
          closeLabel={caption(
            props.locale,
            "Close change confirmation",
            "关闭修改确认",
          )}
          description={pendingConfirmation?.description}
          footer={
            <div className="password-confirmation-actions">
              <button
                disabled={isCommittingChange}
                onClick={() => setPendingConfirmation(null)}
                type="button"
              >
                {caption(props.locale, "Cancel", "取消")}
              </button>
              <button
                className={
                  pendingConfirmation?.destructive ? "danger" : "primary"
                }
                disabled={isCommittingChange}
                onClick={() => void confirmUserChange()}
                type="button"
              >
                {isCommittingChange
                  ? (pendingConfirmation?.committingLabel ??
                    caption(props.locale, "Saving…", "正在保存…"))
                  : (pendingConfirmation?.confirmLabel ??
                    caption(props.locale, "Confirm", "确认"))}
              </button>
            </div>
          }
          onClose={() => {
            if (!isCommittingChange) setPendingConfirmation(null);
          }}
          open={pendingConfirmation !== null}
          title={pendingConfirmation?.title ?? ""}
        >
          {pendingConfirmation ? (
            <div className="password-user-confirmation">
              <div className="password-confirmation-vaults">
                <span>{caption(props.locale, "Vault", "保险库")}</span>
                <strong>
                  {pendingConfirmation.vaults.length > 0
                    ? pendingConfirmation.vaults.join(", ")
                    : caption(props.locale, "Not recorded", "未记录")}
                </strong>
              </div>
              <dl>
                {pendingConfirmation.entries.map((entry, index) => (
                  <div
                    className={entry.destructive ? "destructive" : undefined}
                    key={`${entry.label}-${index}`}
                  >
                    <dt>{entry.label}</dt>
                    <dd>
                      {entry.exposureAfter ? (
                        <ExposureRadar
                          attentionLabel={caption(
                            props.locale,
                            "Red area: newly exposed range",
                            "红色色块：新增暴露范围",
                          )}
                          breachedSurfaces={entry.exposureAfter.surfaces
                            .filter(
                              (surface) =>
                                surface.max_level >
                                (entry.exposureBefore?.surfaces.find(
                                  (before) =>
                                    before.surface === surface.surface,
                                )?.max_level ?? 0),
                            )
                            .map(
                              (surface) => surface.surface as ExposureSurface,
                            )}
                          locale={props.locale}
                          primary={entry.exposureAfter}
                          primaryLabel={caption(
                            props.locale,
                            "After",
                            "修改后",
                          )}
                          secondary={entry.exposureBefore}
                          secondaryLabel={caption(
                            props.locale,
                            "Before",
                            "修改前",
                          )}
                        />
                      ) : null}
                      {!entry.exposureAfter && entry.before !== undefined ? (
                        <del>
                          {entry.before || caption(props.locale, "Empty", "空")}
                        </del>
                      ) : null}
                      {!entry.exposureAfter && entry.after !== undefined ? (
                        <ins>
                          {entry.after || caption(props.locale, "Empty", "空")}
                        </ins>
                      ) : null}
                    </dd>
                  </div>
                ))}
              </dl>
              <label>
                <span>
                  {caption(
                    props.locale,
                    "Reason (optional)",
                    "修改原因（可选）",
                  )}
                </span>
                <textarea
                  data-dialog-initial-focus
                  onChange={(event) =>
                    setConfirmationReason(event.currentTarget.value)
                  }
                  placeholder={caption(
                    props.locale,
                    "Why is this change needed?",
                    "为什么需要这次修改？",
                  )}
                  rows={3}
                  value={confirmationReason}
                />
              </label>
            </div>
          ) : null}
        </Dialog>
      </section>
      {itemContextMenu ? (
        <div
          aria-label={caption(
            props.locale,
            `Actions for ${itemContextMenu.item.title}`,
            `${itemContextMenu.item.title}的操作`,
          )}
          className="password-row-context-menu"
          ref={itemContextMenuRef}
          role="menu"
          style={{ left: itemContextMenu.left, top: itemContextMenu.top }}
        >
          <button
            className="danger"
            onClick={() => {
              const item = itemContextMenu.item;
              setItemContextMenu(null);
              void requestDeleteConfirmation(item);
            }}
            ref={itemContextMenuActionRef}
            role="menuitem"
            type="button"
          >
            <Trash2
              aria-hidden="true"
              focusable="false"
              size={17}
              strokeWidth={1.75}
            />
            {caption(props.locale, "Delete entry", "删除条目")}
          </button>
        </div>
      ) : null}
      {draftDialog}
    </>
  );
}
