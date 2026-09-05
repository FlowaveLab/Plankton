import { AcpSessionOptions } from "../AcpSessionOptions";
import { SecretInput } from "../SecretInput";
import { ChoiceGroup } from "../ChoiceGroup";
import { invoke } from "@tauri-apps/api/core";
import {
  Activity,
  Bot,
  Cable,
  CircleCheck,
  Code2,
  GitBranch,
  Globe,
  History,
  Inbox,
  ListChecks,
  Network,
  Pencil,
  Pin,
  RefreshCw,
  Save,
  ScrollText,
  Settings2,
  ShieldCheck,
  ShieldX,
  Sparkles,
  Terminal,
  TextCursorInput,
  Trash2,
  UserRoundCheck,
  type LucideIcon,
  ChevronDown,
  FileCode2,
  FolderOpen,
  Plus,
  Search,
  SlidersHorizontal,
} from "lucide-react";
import {
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
  type JSX,
  type ReactNode,
} from "react";

import { buildAcpProgramSummary } from "../../acpSettings";
import { codeAgentBoundaryIndex } from "../../callChainPresentation";
import { getPreviewHighlightResult } from "../../codePreview";
import {
  auditResultFor,
  buildAuditApprovalGroups,
  type AuditDecisionPath,
  type AuditPhase,
} from "../../dashboardModel";
import { t, translateCode, type Locale } from "../../i18n";
import { preferredRequestGroup, requestGroup } from "../../requestPriority";
import type {
  AccessRequest,
  AcpProfile,
  AcpProbeCheck,
  AcpProbeResult,
  CredentialExposureReport,
  DashboardData,
  DecisionCommand,
  DesktopSettings,
  ExposureEvidenceAnnotation,
  ExposureEvidenceTarget,
  ProviderTrace,
  StructuredCallChainNode,
} from "../../types";
import { ApprovalChat } from "../ApprovalChat";
import { ApprovalMarkdown } from "../ApprovalMarkdown";
import {
  ExposureRadar,
  defaultExposurePolicy,
  normalizeExposurePolicy,
  parseExposurePolicy,
  type CredentialExposurePolicy,
  type ExposureSurface,
} from "../ExposurePolicy";
import {
  Drawer,
  EmptyState,
  ErrorState,
  PageHeader,
  Pagination,
  SplitPane,
} from "./PagePrimitives";
import type { WorkspaceView } from "./workspaceTypes";

export type SettingsPageController = {
  settings: DesktopSettings | null;
  settingsDraft: DesktopSettings | null;
  isLoading: boolean;
  isSaving: boolean;
  errorMessage: string | null;
  noticeMessage: string | null;
  hasUnsavedChanges: boolean;
  canSave: boolean;
  validationMessage: string | null;
  onSave: () => void;
  onReload: () => void;
  onPolicyModeChange: (value: string) => void;
  onProviderKindChange: (value: string) => void;
  onAcpProfileChange: (profile: AcpProfile) => void;
  onFieldChange: (field: keyof DesktopSettings, value: string) => void;
};

type OperationsPageProps = {
  locale: Locale;
  dashboard?: DashboardData | null;
  focusedRequestId: string | null;
  isSubmitting: boolean;
  noteDraft: string;
  onDecision: (requestId: string, decision: DecisionCommand) => Promise<void>;
  view: Exclude<WorkspaceView, "passwords">;
  onNavigate: (view: WorkspaceView) => void;
  onNoteChange: (note: string) => void;
  settingsController: SettingsPageController;
};
type RequestRow = {
  id: string;
  resource: string;
  title: string;
  collection: string;
  field: string;
  actor: string;
  status: string;
  statusTone: "pending" | "approved" | "rejected" | "evaluating" | "failed";
  statusDetail: string | null;
  reason: string;
  time: string;
  request: AccessRequest;
};

function RequestStatusIcon({
  tone,
}: {
  tone: RequestRow["statusTone"];
}): JSX.Element {
  const Icon = {
    pending: UserRoundCheck,
    approved: CircleCheck,
    rejected: ShieldX,
    evaluating: Bot,
    failed: ShieldX,
  }[tone];
  return (
    <Icon aria-hidden="true" focusable="false" size={13} strokeWidth={1.75} />
  );
}

type RequestPage = {
  items: AccessRequest[];
  total: number;
  page: number;
  page_size: number;
};

function RequestToolbar(props: {
  hideSearch?: boolean;
  query: string;
  statusFilter: string;
  awaitingCount: number;
  failedCount?: number;
  evaluatingCount: number;
  zh: boolean;
  onQueryChange: (query: string) => void;
  onStatusChange: (status: string) => void;
}): JSX.Element {
  return (
    <div
      className="operations-toolbar request-toolbar"
      onClick={(event) => {
        const target = event.target;
        if (
          target instanceof HTMLInputElement &&
          target.type === "radio" &&
          target.value === props.statusFilter
        ) {
          props.onStatusChange(target.value);
        }
      }}
    >
      <ChoiceGroup
        label={props.zh ? "请求状态" : "Request status"}
        aria-label="Request status"
        onChange={props.onStatusChange}
        value={props.statusFilter}
        options={[
          {
            value: "all",
            icon: Inbox,
            label: props.zh ? "活动请求" : "Active requests",
          },
          {
            value: "awaiting",
            icon: UserRoundCheck,
            label: `${props.zh ? "待人工审批" : "Needs review"} · ${props.awaitingCount}${props.failedCount ? `（${props.zh ? "失败" : "failed"} ${props.failedCount}）` : ""}`,
          },
          {
            value: "evaluating",
            icon: Bot,
            label: `${props.zh ? "进行中" : "In progress"} · ${props.evaluatingCount}`,
          },
          {
            value: "completed",
            icon: History,
            label: props.zh ? "历史记录" : "History",
          },
        ]}
      />
      {props.hideSearch ? null : (
        <label className="toolbar-search">
          <Search
            aria-hidden="true"
            focusable="false"
            size={16}
            strokeWidth={1.75}
          />
          <input
            aria-label="Search requests"
            onChange={(event) => props.onQueryChange(event.currentTarget.value)}
            placeholder={
              props.zh
                ? "搜索资源、请求者或原因"
                : "Search resource, requester or reason"
            }
            value={props.query}
          />
        </label>
      )}
    </div>
  );
}

const sensitivePayloadMarkers = [
  "secret",
  "token",
  "password",
  "passwd",
  "api_key",
  "apikey",
  "authorization",
  "cookie",
  "session",
  "credential",
  "private",
];
const omittedPayloadValue = Symbol("omitted-payload-value");

function isSensitivePayloadKey(key: string): boolean {
  const normalized = key.toLowerCase();
  if (
    [
      "session_id",
      "secret_exposure_risk",
      "credential_exposure_policy_v1",
    ].includes(normalized)
  ) {
    return false;
  }
  return sensitivePayloadMarkers.some((marker) => normalized.includes(marker));
}

function looksSensitiveDisplayValue(value: string): boolean {
  const normalized = value.trim();
  const lowered = normalized.toLowerCase();
  if (
    lowered.startsWith("bearer ") ||
    lowered.startsWith("authorization:") ||
    lowered.includes("-----begin")
  ) {
    return true;
  }
  if (/\s/.test(normalized)) return false;
  const assignment = normalized.match(/^([^=]+)=(.+)$/);
  if (!assignment) return false;
  const [, name, assignedValue] = assignment;
  const sensitiveName = isSensitivePayloadKey(name);
  const variableReference = assignedValue
    .replace(/^['"]|['"]$/g, "")
    .startsWith("$");
  return sensitiveName && !variableReference;
}

function sanitizePayloadForDisplay(
  value: unknown,
  key = "",
): unknown | typeof omittedPayloadValue {
  if (key && isSensitivePayloadKey(key)) {
    return omittedPayloadValue;
  }
  if (typeof value === "string") {
    return looksSensitiveDisplayValue(value) ? omittedPayloadValue : value;
  }
  if (Array.isArray(value)) {
    return value
      .map((entry) => sanitizePayloadForDisplay(entry))
      .filter((entry) => entry !== omittedPayloadValue);
  }
  if (value && typeof value === "object") {
    return Object.fromEntries(
      Object.entries(value).flatMap(([entryKey, entryValue]) => {
        const sanitized = sanitizePayloadForDisplay(entryValue, entryKey);
        return sanitized === omittedPayloadValue ? [] : [[entryKey, sanitized]];
      }),
    );
  }
  return value;
}

function requestScriptPathForDisplay(request: AccessRequest): string | null {
  return request.context.script_path?.trim() || null;
}

function requestCallChainForDisplay(
  request: AccessRequest,
): AccessRequest["context"]["call_chain"] {
  return request.context.call_chain;
}

function callChainPath(node: StructuredCallChainNode): string | null {
  return (
    node.resolved_file_path?.trim() ||
    node.executable_path?.trim() ||
    node.process_name?.trim() ||
    null
  );
}

function previewStatusLabel(status: string | null, zh: boolean): string {
  const labels: Record<string, [string, string]> = {
    binary_file: ["Binary file", "二进制文件"],
    file_missing: ["File missing", "文件已不存在"],
    io_error: ["Read failed", "读取失败"],
    not_previewable: ["Not previewable", "不可预览"],
    path_only: ["Path recognized", "已识别路径"],
    preview_ready: ["Preview ready", "可预览"],
    too_large: ["File too large", "文件过大"],
    unsupported_encoding: ["Unsupported encoding", "编码不受支持"],
  };
  const label = labels[status ?? "not_previewable"] ?? [
    status ?? "Unknown",
    status ?? "未知",
  ];
  return label[zh ? 1 : 0];
}

export type DisplayEvidenceAnnotation = Omit<
  ExposureEvidenceAnnotation,
  "target"
> & {
  target: Exclude<ExposureEvidenceTarget, { kind: "resource" }>;
  surface: string;
};

function commandArguments(node?: StructuredCallChainNode): string[] {
  if (node?.argv && node.argv.length > 0) return node.argv;
  const fallback = callChainPath(node ?? {});
  return fallback ? [fallback] : [];
}

function commandText(node?: StructuredCallChainNode): string {
  return commandArguments(node).join(" ") || "—";
}

function codePointOffset(text: string, utf16Offset: number): number {
  return Array.from(text.slice(0, utf16Offset)).length;
}

function quoteCharacterRange(
  text: string,
  quote: string,
  occurrence: number,
): { start: number; end: number } | null {
  if (!quote) return null;
  let utf16Start = -1;
  let cursor = 0;
  for (let index = 0; index <= occurrence; index += 1) {
    utf16Start = text.indexOf(quote, cursor);
    if (utf16Start < 0) return null;
    cursor = utf16Start + quote.length;
  }
  const start = codePointOffset(text, utf16Start);
  return { start, end: start + Array.from(quote).length };
}

export function displayAnnotationsByNode(
  callChain: AccessRequest["context"]["call_chain"],
  report?: CredentialExposureReport | null,
): Map<number, DisplayEvidenceAnnotation[]> {
  const byNode = new Map<number, DisplayEvidenceAnnotation[]>();
  for (const surface of report?.surfaces ?? []) {
    for (const rawAnnotation of surface.annotations) {
      if (rawAnnotation.target.kind === "resource") continue;
      const annotation: DisplayEvidenceAnnotation = {
        ...rawAnnotation,
        target: rawAnnotation.target,
        surface: surface.surface,
      };
      const nodeIndex = annotation.target.node_index;
      if (nodeIndex < 0 || nodeIndex >= callChain.length) continue;
      const current = byNode.get(nodeIndex) ?? [];
      current.push(annotation);
      byNode.set(nodeIndex, current);
    }
  }
  return byNode;
}

type AggregatedEvidenceAnnotation = Omit<
  DisplayEvidenceAnnotation,
  "reason" | "surface"
> & {
  reasons: string[];
  surfaces: string[];
};

type NumberedEvidenceAnnotation = AggregatedEvidenceAnnotation & {
  reference: number;
};

type EvidenceSpan = {
  start: number;
  end: number;
  references: number[];
  badgeReferences?: number[];
};

type EvidenceOriginGroup = {
  key: string;
  kind: "command" | "source" | "node";
  sourceId?: string;
  annotations: NumberedEvidenceAnnotation[];
};

function layoutEvidenceSpans(spans: EvidenceSpan[]): EvidenceSpan[] {
  const valid = spans.filter((span) => span.end > span.start);
  const boundaries = Array.from(
    new Set(valid.flatMap((span) => [span.start, span.end])),
  ).sort((left, right) => left - right);
  return boundaries.slice(0, -1).flatMap((start, index): EvidenceSpan[] => {
    const end = boundaries[index + 1];
    if (end === undefined) return [];
    const covering = valid.filter(
      (span) => span.start < end && span.end > start,
    );
    if (covering.length === 0) return [];
    const references = Array.from(
      new Set(covering.flatMap((span) => span.references)),
    ).sort((left, right) => left - right);
    const badgeReferences = Array.from(
      new Set(
        covering
          .filter((span) => span.end === end)
          .flatMap((span) => span.badgeReferences ?? span.references),
      ),
    ).sort((left, right) => left - right);
    return [{ start, end, references, badgeReferences }];
  });
}

function commandEvidenceSpans(
  argument: string,
  argumentIndex: number,
  annotations: NumberedEvidenceAnnotation[],
): EvidenceSpan[] {
  const length = Array.from(argument).length;
  const spans = annotations.flatMap((annotation): EvidenceSpan[] => {
    const target = annotation.target;
    if (target.kind === "argument_quote") {
      if (target.argument_index !== argumentIndex) return [];
      const range = quoteCharacterRange(
        argument,
        target.quote,
        target.occurrence,
      );
      if (!range) return [];
      return [{ ...range, references: [annotation.reference] }];
    }
    if (
      target.kind !== "argument_span" ||
      argumentIndex < target.start.argument_index ||
      argumentIndex > target.end.argument_index
    ) {
      return [];
    }
    const startRange =
      argumentIndex === target.start.argument_index
        ? quoteCharacterRange(
            argument,
            target.start.quote,
            target.start.occurrence,
          )
        : null;
    const endRange =
      argumentIndex === target.end.argument_index
        ? quoteCharacterRange(argument, target.end.quote, target.end.occurrence)
        : null;
    if (
      (argumentIndex === target.start.argument_index && !startRange) ||
      (argumentIndex === target.end.argument_index && !endRange)
    ) {
      return [];
    }
    const start = startRange?.start ?? 0;
    const end = endRange?.end ?? length;
    return [
      {
        start,
        end,
        references: [annotation.reference],
      },
    ];
  });
  return layoutEvidenceSpans(spans);
}

function evidenceLocationKey(annotation: DisplayEvidenceAnnotation): string {
  const target = annotation.target;
  if (target.kind === "node") return "node";
  if (target.kind === "source_file") return `file:${target.source_id}`;
  if (target.kind === "source_quote") {
    return [
      "source",
      target.source_id,
      target.start_line,
      target.end_line,
      target.quote,
      target.occurrence,
    ].join(":");
  }
  if (target.kind === "argument_span") {
    return [
      "command-span",
      target.start.argument_index,
      target.start.quote,
      target.start.occurrence,
      target.end.argument_index,
      target.end.quote,
      target.end.occurrence,
    ].join(":");
  }
  return `command:${target.argument_index}:${target.quote}:${target.occurrence}`;
}

function aggregateEvidenceAnnotations(
  annotations: DisplayEvidenceAnnotation[],
): AggregatedEvidenceAnnotation[] {
  const aggregated = new Map<string, AggregatedEvidenceAnnotation>();
  for (const annotation of annotations) {
    const key = evidenceLocationKey(annotation);
    const current = aggregated.get(key);
    if (!current) {
      aggregated.set(key, {
        target: annotation.target,
        reasons: [annotation.reason],
        surfaces: [annotation.surface],
      });
      continue;
    }
    if (!current.reasons.includes(annotation.reason)) {
      current.reasons.push(annotation.reason);
    }
    if (!current.surfaces.includes(annotation.surface)) {
      current.surfaces.push(annotation.surface);
    }
  }
  return Array.from(aggregated.values());
}

function mergedEvidenceMessage(reasons: string[], zh: boolean): string {
  const normalized = reasons.map((reason) =>
    reason.trim().replace(/[。；;.!?！？]+$/u, ""),
  );
  return `${normalized.join(zh ? "；" : "; ")}${zh ? "。" : "."}`;
}

function EvidenceReference({
  activeReference,
  reference,
  setActiveReference,
}: {
  activeReference: number | null;
  reference: number;
  setActiveReference: (reference: number | null) => void;
}): JSX.Element {
  return (
    <button
      aria-label={`Evidence ${reference}`}
      className="request-evidence-reference"
      data-active={activeReference === reference ? "true" : undefined}
      data-reference-id={reference}
      onBlur={() => setActiveReference(null)}
      onFocus={() => setActiveReference(reference)}
      onMouseEnter={() => setActiveReference(reference)}
      onMouseLeave={() => setActiveReference(null)}
      type="button"
    >
      [{reference}]
    </button>
  );
}

function scrollEvidenceReferenceIntoView(
  trigger: HTMLElement,
  reference: number,
): void {
  const row = trigger.closest(".request-evidence-workbench__row");
  const targets = Array.from(
    row?.querySelectorAll<HTMLButtonElement>(
      `.request-evidence-reference[data-reference-id="${reference}"]`,
    ) ?? [],
  )
    .map((button) => button.closest<HTMLElement>("mark"))
    .filter((target): target is HTMLElement => target !== null);
  if (targets.length === 0) return;
  const previousIndex = Number.parseInt(
    trigger.dataset.evidenceTargetIndex ?? "-1",
    10,
  );
  const nextIndex = (previousIndex + 1) % targets.length;
  trigger.dataset.evidenceTargetIndex = String(nextIndex);
  const target = targets[nextIndex];
  const scroller = target?.closest<HTMLElement>(
    ".request-evidence-workbench__command, .request-evidence-workbench__origin pre",
  );
  if (!target || !scroller) return;
  const targetRect = target.getBoundingClientRect();
  const scrollerRect = scroller.getBoundingClientRect();
  if (
    targetRect.top >= scrollerRect.top &&
    targetRect.bottom <= scrollerRect.bottom
  ) {
    return;
  }
  const centeredTop =
    scroller.scrollTop +
    targetRect.top -
    scrollerRect.top -
    (scroller.clientHeight - targetRect.height) / 2;
  scroller.scrollTo({
    behavior: "smooth",
    top: Math.max(
      0,
      Math.min(centeredTop, scroller.scrollHeight - scroller.clientHeight),
    ),
  });
}

function AnnotatedEvidenceText({
  activeReference,
  setActiveReference,
  spans,
  text,
}: {
  activeReference: number | null;
  setActiveReference: (reference: number | null) => void;
  spans: EvidenceSpan[];
  text: string;
}): JSX.Element {
  const characters = Array.from(text);
  const content: ReactNode[] = [];
  let cursor = 0;
  for (const span of spans) {
    if (span.start > cursor) {
      content.push(characters.slice(cursor, span.start).join(""));
    }
    content.push(
      <mark
        data-active={
          activeReference !== null && span.references.includes(activeReference)
            ? "true"
            : undefined
        }
        data-reference-ids={span.references.join(" ")}
        key={`${span.start}:${span.end}:${span.references.join("-")}`}
      >
        {characters.slice(span.start, span.end).join("")}
        {(span.badgeReferences ?? span.references).length === 0 ? null : (
          <sup>
            {(span.badgeReferences ?? span.references).map((reference) => (
              <EvidenceReference
                activeReference={activeReference}
                key={reference}
                reference={reference}
                setActiveReference={setActiveReference}
              />
            ))}
          </sup>
        )}
      </mark>,
    );
    cursor = span.end;
  }
  if (cursor < characters.length) {
    content.push(characters.slice(cursor).join(""));
  }
  return <>{content}</>;
}

function HighlightedAnnotatedEvidenceText({
  activeReference,
  path,
  setActiveReference,
  spans,
  text,
}: {
  activeReference: number | null;
  path: string | null;
  setActiveReference: (reference: number | null) => void;
  spans: EvidenceSpan[];
  text: string;
}): JSX.Element {
  const highlighted = getPreviewHighlightResult(path, text);
  if (!highlighted.highlighted || typeof DOMParser === "undefined") {
    return (
      <AnnotatedEvidenceText
        activeReference={activeReference}
        setActiveReference={setActiveReference}
        spans={spans}
        text={text}
      />
    );
  }

  const document = new DOMParser().parseFromString(
    `<code>${highlighted.html}</code>`,
    "text/html",
  );
  let offset = 0;
  let nodeKey = 0;
  const renderNode = (node: Node): ReactNode => {
    const key = nodeKey++;
    if (node.nodeType === Node.TEXT_NODE) {
      const value = node.textContent ?? "";
      const start = offset;
      const end = start + Array.from(value).length;
      offset = end;
      const localSpans = spans.flatMap((span): EvidenceSpan[] => {
        const overlapStart = Math.max(start, span.start);
        const overlapEnd = Math.min(end, span.end);
        if (overlapEnd <= overlapStart) return [];
        return [
          {
            start: overlapStart - start,
            end: overlapEnd - start,
            references: span.references,
            badgeReferences:
              overlapEnd === span.end
                ? (span.badgeReferences ?? span.references)
                : [],
          },
        ];
      });
      return (
        <AnnotatedEvidenceText
          activeReference={activeReference}
          key={key}
          setActiveReference={setActiveReference}
          spans={localSpans}
          text={value}
        />
      );
    }
    if (!(node instanceof Element)) return null;
    return (
      <span className={node.className} key={key}>
        {Array.from(node.childNodes).map(renderNode)}
      </span>
    );
  };

  return (
    <span className="payload-code" data-language={highlighted.label}>
      {Array.from(document.body.firstElementChild?.childNodes ?? []).map(
        renderNode,
      )}
    </span>
  );
}

function completeCommandEvidenceSpans(
  node: StructuredCallChainNode,
  annotations: NumberedEvidenceAnnotation[],
): EvidenceSpan[] {
  let offset = 0;
  const spans: EvidenceSpan[] = [];
  for (const [argumentIndex, argument] of commandArguments(node).entries()) {
    spans.push(
      ...commandEvidenceSpans(argument, argumentIndex, annotations).map(
        (span) => ({
          ...span,
          start: span.start + offset,
          end: span.end + offset,
        }),
      ),
    );
    offset += Array.from(argument).length + 1;
  }
  return layoutEvidenceSpans(spans);
}

function evidenceOriginGroups(
  annotations: NumberedEvidenceAnnotation[],
): EvidenceOriginGroup[] {
  const groups = new Map<string, EvidenceOriginGroup>();
  for (const annotation of annotations) {
    const target = annotation.target;
    const kind =
      target.kind === "source_quote" || target.kind === "source_file"
        ? "source"
        : target.kind === "node"
          ? "node"
          : "command";
    const key =
      target.kind === "source_quote" || target.kind === "source_file"
        ? `source:${target.source_id}`
        : kind;
    const initial: EvidenceOriginGroup = {
      key,
      kind,
      sourceId:
        target.kind === "source_quote" || target.kind === "source_file"
          ? target.source_id
          : undefined,
      annotations: [],
    };
    const current = groups.get(key) ?? initial;
    current.annotations.push(annotation);
    groups.set(key, current);
  }
  return Array.from(groups.values());
}

function SourceEvidenceOrigin({
  activeReference,
  annotations,
  sourceId,
  sourceText,
  setActiveReference,
}: {
  activeReference: number | null;
  annotations: NumberedEvidenceAnnotation[];
  sourceId: string;
  sourceText: string | null;
  setActiveReference: (reference: number | null) => void;
}): JSX.Element {
  const targets = annotations.flatMap((annotation) =>
    annotation.target.kind === "source_quote"
      ? [{ ...annotation.target, reference: annotation.reference }]
      : [],
  );
  const files = annotations.filter(
    (annotation) => annotation.target.kind === "source_file",
  );
  const filePath = sourceId.startsWith("file:") ? sourceId.slice(5) : sourceId;
  const fileHeading = files.length ? (
    <code className="request-evidence-workbench__file">
      <HighlightedAnnotatedEvidenceText
        activeReference={activeReference}
        path="/tmp/plankton-evidence.txt"
        setActiveReference={setActiveReference}
        spans={[
          {
            start: 0,
            end: Array.from(filePath).length,
            references: files.map((annotation) => annotation.reference),
          },
        ]}
        text={filePath}
      />
    </code>
  ) : null;
  if (!targets.length) return <>{fileHeading}</>;
  const startLine = Math.min(...targets.map((target) => target.start_line));
  const endLine = Math.max(...targets.map((target) => target.end_line));
  const lines = sourceText?.split(/\r?\n/) ?? [];
  const selectedLines = lines.slice(startLine - 1, endLine);
  if (selectedLines.length === 0) {
    return (
      <>
        {fileHeading}
        {targets.map((target) => (
          <div key={target.reference}>
            <small>
              {sourceId}:{target.start_line}–{target.end_line}
            </small>
            <pre>
              <code>
                <HighlightedAnnotatedEvidenceText
                  activeReference={activeReference}
                  path={filePath}
                  setActiveReference={setActiveReference}
                  spans={[
                    {
                      start: 0,
                      end: Array.from(target.quote).length,
                      references: [target.reference],
                    },
                  ]}
                  text={target.quote}
                />
              </code>
            </pre>
          </div>
        ))}
      </>
    );
  }
  const selectedText = selectedLines.join("\n");
  const lineOffsets: number[] = [];
  let offset = 0;
  for (const line of selectedLines) {
    lineOffsets.push(offset);
    offset += Array.from(line).length + 1;
  }
  const spans = layoutEvidenceSpans(
    targets.flatMap((target): EvidenceSpan[] => {
      const relativeStartLine = target.start_line - startLine;
      const relativeEndLine = target.end_line - startLine;
      const targetText = selectedLines
        .slice(relativeStartLine, relativeEndLine + 1)
        .join("\n");
      const range = quoteCharacterRange(
        targetText,
        target.quote,
        target.occurrence,
      );
      if (!range) return [];
      const baseOffset = lineOffsets[relativeStartLine] ?? 0;
      return [
        {
          start: baseOffset + range.start,
          end: baseOffset + range.end,
          references: [target.reference],
        },
      ];
    }),
  );
  return (
    <>
      {fileHeading}
      <pre>
        <code>
          <HighlightedAnnotatedEvidenceText
            activeReference={activeReference}
            path={
              sourceId.startsWith("file:") ? sourceId.slice(5) : "inline.py"
            }
            setActiveReference={setActiveReference}
            spans={spans}
            text={selectedText}
          />
        </code>
      </pre>
    </>
  );
}

export function PreciseEvidencePanel({
  annotations,
  inlineSources,
  node,
  nodeIndex,
  zh,
}: {
  annotations: DisplayEvidenceAnnotation[];
  inlineSources: NonNullable<
    AccessRequest["provider_input"]
  >["sanitized_context"]["inline_sources"];
  node: StructuredCallChainNode;
  nodeIndex: number;
  zh: boolean;
}): JSX.Element {
  const [activeReference, setActiveReference] = useState<number | null>(null);
  const numbered = aggregateEvidenceAnnotations(annotations).map(
    (annotation, index) => ({
      ...annotation,
      reference: index + 1,
    }),
  );
  const annotatedGroups = evidenceOriginGroups(numbered);
  const commandGroup = annotatedGroups.find(
    (group) => group.kind === "command",
  ) ?? {
    key: "command",
    kind: "command" as const,
    annotations: [],
  };
  const groups = [
    commandGroup,
    ...annotatedGroups.filter((group) => group.kind !== "command"),
  ];
  return (
    <section
      aria-label={zh ? "节点精确证据" : "Precise node evidence"}
      className="request-evidence-workbench"
    >
      {groups.map((group) => (
        <div className="request-evidence-workbench__row" key={group.key}>
          <div className="request-evidence-workbench__origin">
            <small>
              {group.kind === "source"
                ? group.sourceId
                : group.kind === "node"
                  ? zh
                    ? `调用链节点 ${nodeIndex + 1}`
                    : `Call-chain node ${nodeIndex + 1}`
                  : zh
                    ? "原始完整命令"
                    : "Original full command"}
            </small>
            {group.kind === "source" ? (
              <SourceEvidenceOrigin
                activeReference={activeReference}
                annotations={group.annotations}
                sourceId={group.sourceId ?? ""}
                sourceText={
                  inlineSources
                    .find((source) => source.source_id === group.sourceId)
                    ?.lines.map((line) => line.text)
                    .join("\n") ??
                  (group.sourceId === `file:${node.resolved_file_path}`
                    ? (node.preview_text ?? null)
                    : null)
                }
                setActiveReference={setActiveReference}
              />
            ) : group.kind === "node" ? (
              <code className="request-evidence-workbench__node">
                {callChainPath(node) ?? node.process_name ?? "—"}
              </code>
            ) : (
              <code className="request-evidence-workbench__command">
                <HighlightedAnnotatedEvidenceText
                  activeReference={activeReference}
                  path="/tmp/plankton-evidence.sh"
                  setActiveReference={setActiveReference}
                  spans={completeCommandEvidenceSpans(node, group.annotations)}
                  text={commandText(node)}
                />
              </code>
            )}
          </div>
          <span
            aria-hidden="true"
            className="request-evidence-workbench__axis"
          />
          <ol className="request-evidence-workbench__notes">
            {group.annotations.map((annotation) => (
              <li
                data-active={
                  activeReference === annotation.reference ? "true" : undefined
                }
                key={annotation.reference}
              >
                <button
                  aria-label={
                    zh
                      ? `定位证据 ${annotation.reference}`
                      : `Locate evidence ${annotation.reference}`
                  }
                  className="request-evidence-workbench__note"
                  onBlur={() => setActiveReference(null)}
                  onClick={(event) => {
                    setActiveReference(annotation.reference);
                    scrollEvidenceReferenceIntoView(
                      event.currentTarget,
                      annotation.reference,
                    );
                  }}
                  onFocus={() => setActiveReference(annotation.reference)}
                  onMouseEnter={() => setActiveReference(annotation.reference)}
                  onMouseLeave={() => setActiveReference(null)}
                  type="button"
                >
                  <span>[{annotation.reference}]</span>
                  <div>
                    <code>
                      {annotation.surfaces
                        .map((surface) => operationCodeLabel(surface, zh))
                        .join(" · ")}
                    </code>
                    <ApprovalMarkdown>
                      {mergedEvidenceMessage(annotation.reasons, zh)}
                    </ApprovalMarkdown>
                  </div>
                </button>
              </li>
            ))}
          </ol>
        </div>
      ))}
    </section>
  );
}

function StructuredCallChainEntry({
  agentScoped,
  agentStart,
  index,
  node,
  zh,
  assessment,
  annotations,
  inlineSources,
}: {
  agentScoped: boolean;
  agentStart: boolean;
  index: number;
  node: StructuredCallChainNode;
  zh: boolean;
  assessment?: CredentialExposureReport["node_assessments"][number];
  annotations: DisplayEvidenceAnnotation[];
  inlineSources: NonNullable<
    AccessRequest["provider_input"]
  >["sanitized_context"]["inline_sources"];
}): JSX.Element {
  const path = callChainPath(node);
  const previewText = node.preview_text ?? null;
  const preview = previewText
    ? getPreviewHighlightResult(node.resolved_file_path ?? path, previewText)
    : null;
  const argumentsList = node.argv ?? [];
  const previewStatus = node.preview_status ?? "not_previewable";
  const canOpenPreview = Boolean(previewText || node.preview_error);

  return (
    <li
      className={`request-call-chain-entry${agentScoped ? " request-call-chain-entry-agent-scope" : ""}`}
      data-agent-start={agentStart}
      data-node-annotated={
        annotations.some((annotation) => annotation.target.kind === "node")
          ? "true"
          : undefined
      }
    >
      <header className="request-call-chain-heading">
        <span aria-hidden="true">{index + 1}</span>
        <div className="request-call-chain-identity">
          <strong>
            {node.process_name?.trim() || (zh ? "未知进程" : "Unknown process")}
          </strong>
          <code>{path ?? (zh ? "没有识别到路径" : "No path recognized")}</code>
        </div>
        {assessment ? (
          <aside className="request-call-chain-llm-note">
            <strong>{zh ? "节点说明" : "Node note"}</strong>
            <ApprovalMarkdown>{assessment.summary}</ApprovalMarkdown>
            {assessment.capabilities.length > 0 ? (
              <small>{assessment.capabilities.join(" · ")}</small>
            ) : null}
          </aside>
        ) : null}
      </header>
      {annotations.length > 0 ? (
        <PreciseEvidencePanel
          annotations={annotations}
          inlineSources={inlineSources}
          node={node}
          nodeIndex={index}
          zh={zh}
        />
      ) : (
        <section className="request-call-chain-command">
          <small>{zh ? "原始完整命令" : "Original full command"}</small>
          <code>{commandText(node)}</code>
        </section>
      )}
      <details className="request-call-chain-technical">
        <summary>
          {zh
            ? `进程与参数明细 · ${argumentsList.length} 项 argv`
            : `Process and argument details · ${argumentsList.length} argv items`}
        </summary>
        <dl className="request-call-chain-facts">
          <div>
            <dt>PID / PPID</dt>
            <dd>{`${node.pid ?? "—"} / ${node.ppid ?? "—"}`}</dd>
          </div>
          <div>
            <dt>{zh ? "可执行文件" : "Executable"}</dt>
            <dd>
              <code>{node.executable_path?.trim() || "—"}</code>
            </dd>
          </div>
          <div>
            <dt>{zh ? "识别来源" : "Detected by"}</dt>
            <dd>
              {node.source === "os_probe"
                ? zh
                  ? "系统进程探测"
                  : "OS process probe"
                : zh
                  ? "请求方提供 / 尽力识别"
                  : "Requester / best-effort detection"}
            </dd>
          </div>
          <div>
            <dt>{zh ? "文件预览" : "File preview"}</dt>
            <dd>{previewStatusLabel(previewStatus, zh)}</dd>
          </div>
        </dl>
        <section
          className="request-call-chain-arguments"
          aria-label={
            zh
              ? `调用链第 ${index + 1} 层参数`
              : `Arguments for call-chain step ${index + 1}`
          }
        >
          {argumentsList.length > 0 ? (
            <ol>
              {argumentsList.map((argument, argumentIndex) => (
                <HighlightedCallChainArgument
                  argument={argument}
                  argumentIndex={argumentIndex}
                  annotations={annotations.filter(
                    (annotation) =>
                      (annotation.target.kind === "argument_quote" &&
                        annotation.target.argument_index === argumentIndex) ||
                      (annotation.target.kind === "argument_span" &&
                        argumentIndex >=
                          annotation.target.start.argument_index &&
                        argumentIndex <= annotation.target.end.argument_index),
                  )}
                  key={`${index}:arg:${argumentIndex}`}
                />
              ))}
            </ol>
          ) : (
            <p>{zh ? "没有采集到参数。" : "No arguments were captured."}</p>
          )}
        </section>
      </details>
      {node.resolved_file_path ? (
        <details
          className="request-call-chain-preview"
          data-preview-ready={Boolean(previewText)}
        >
          <summary aria-disabled={!canOpenPreview}>
            <FileCode2
              aria-hidden="true"
              focusable="false"
              size={16}
              strokeWidth={1.75}
            />
            <span>{node.resolved_file_path}</span>
            <strong>{previewStatusLabel(previewStatus, zh)}</strong>
          </summary>
          {preview ? (
            <div className="request-call-chain-preview-body">
              <div>
                <span>{zh ? "本地源码预览" : "Local source preview"}</span>
                <code>{preview.label}</code>
              </div>
              <pre>
                <code
                  className="payload-code"
                  dangerouslySetInnerHTML={{ __html: preview.html }}
                />
              </pre>
            </div>
          ) : (
            <p className="request-call-chain-preview-error">
              {node.preview_error ??
                (zh
                  ? "当前请求数据中没有可预览的源码。"
                  : "No source preview is available in this request data.")}
            </p>
          )}
        </details>
      ) : null}
    </li>
  );
}

function HighlightedCallChainArgument({
  argument,
  argumentIndex,
  annotations,
}: {
  argument: string;
  argumentIndex: number;
  annotations: DisplayEvidenceAnnotation[];
}): JSX.Element {
  const highlighted = getPreviewHighlightResult(null, argument);
  return (
    <li data-annotated={annotations.length > 0 ? "true" : undefined}>
      <span>{argumentIndex}</span>
      {highlighted.highlighted ? (
        <code
          className="payload-code"
          data-language={highlighted.label}
          dangerouslySetInnerHTML={{ __html: highlighted.html }}
        />
      ) : (
        <code>{argument}</code>
      )}
    </li>
  );
}

function RequestCallChain({
  callChain,
  inlineSources = [],
  zh,
  report,
}: {
  callChain: AccessRequest["context"]["call_chain"];
  inlineSources?: NonNullable<
    AccessRequest["provider_input"]
  >["sanitized_context"]["inline_sources"];
  zh: boolean;
  report?: CredentialExposureReport | null;
}): JSX.Element {
  if (callChain.length === 0) {
    return <p>—</p>;
  }

  const agentBoundary = codeAgentBoundaryIndex(callChain);
  const annotationsByNode = displayAnnotationsByNode(callChain, report);

  return (
    <ol className="request-call-chain-list">
      {callChain.map((node, index) => (
        <StructuredCallChainEntry
          agentScoped={agentBoundary >= 0 && index >= agentBoundary}
          agentStart={index === agentBoundary}
          index={index}
          key={`structured:${index}:${callChainPath(node) ?? "unknown"}`}
          node={node}
          assessment={report?.node_assessments.find(
            (assessment) => assessment.node_index === index,
          )}
          annotations={annotationsByNode.get(index) ?? []}
          inlineSources={inlineSources}
          zh={zh}
        />
      ))}
    </ol>
  );
}

function operationCodeLabel(code: string, zh: boolean): string {
  const labels: Record<string, [string, string]> = {
    approval_recorded: ["Approval recorded", "审批已记录"],
    request_submitted: ["Request submitted", "请求已提交"],
    request_approved: ["Request approved", "请求已批准"],
    request_rejected: ["Request rejected", "请求已拒绝"],
    llm_suggestion_generated: ["AI advice generated", "AI 建议已生成"],
    llm_suggestion_failed: ["AI advice failed", "AI 建议失败"],
    llm_review_details_updated: [
      "AI review details updated",
      "AI 审批细节已更新",
    ],
    automatic_decision_recorded: ["Automatic decision", "自动决策"],
    automatic_escalated_to_human: ["Escalated to human", "已升级为人工处理"],
    llm_context: ["LLM context", "LLM 回显"],
    network: ["Network", "网络发送"],
    local_persistence: ["Local persistence", "本地持久化"],
    terminal_log: ["Terminal / logs", "终端 / 日志"],
    process_propagation: ["Process propagation", "进程传递"],
    local_rule: ["Local rule", "本地规则"],
    llm_suggestion: ["LLM suggestion", "LLM 建议"],
    combined_guardrail: ["Combined guardrail", "组合护栏"],
    batch_ticket: ["Batch ticket", "批量票据"],
    strict: ["Strict", "严格解析"],
    conservative: ["Conservative repair", "保守修复"],
    human_decision_overrode_llm: [
      "Human decision overrode AI",
      "人工决策覆盖 AI",
    ],
    memory_evaluated: ["Legacy note evaluation", "旧版备注评估"],
    status_viewed: ["Status viewed", "状态已查看"],
    approved: ["Approved", "已批准"],
    rejected: ["Rejected", "已拒绝"],
    failed: ["Failed", "失败"],
    generated: ["Generated", "已生成"],
    updated: ["Updated", "已更新"],
    allow: ["Allow", "允许"],
    deny: ["Deny", "拒绝"],
    escalate: ["Human review requested", "转人工复核"],
    observed: ["Observed", "已观察到"],
    not_observed: ["Not observed", "未观察到"],
    unknown: ["Unknown", "证据不足"],
    recorded: ["Recorded", "已记录"],
    pending: ["Pending", "待处理"],
    error: ["Error", "错误"],
    warning: ["Warning", "警告"],
    info: ["Info", "信息"],
    critical: ["Critical", "严重"],
    fatal: ["Fatal", "致命"],
    invalid_request: ["Invalid request", "请求无效"],
    protocol_mismatch: ["Protocol mismatch", "协议不匹配"],
    not_found: ["Not found", "未找到"],
    approval_required: ["Approval required", "需要审批"],
    approval_denied: ["Approval denied", "审批已拒绝"],
    backend_unavailable: ["Backend unavailable", "后端不可用"],
    timeout: ["Timeout", "超时"],
    backend_failed: ["Backend failed", "后端失败"],
    daemon_unavailable: ["Daemon unavailable", "Daemon 不可用"],
    cancelled: ["Cancelled", "已取消"],
    storage_failed: ["Storage failed", "存储失败"],
    configuration_failed: ["Configuration failed", "配置失败"],
    internal: ["Internal error", "内部错误"],
    ready: ["Ready", "正常"],
    degraded: ["Degraded", "降级"],
    manual_only: ["Manual only", "仅人工审批"],
    assisted: ["Assisted", "AI 辅助"],
    llm_automatic: ["Automatic", "自动审批"],
    queued: ["Queued", "排队中"],
    running: ["Running", "运行中"],
    completed: ["Completed", "已完成"],
    interrupted: ["Interrupted", "已中断"],
    not_required: ["Not required", "无需评估"],
    superseded: ["Superseded", "已被取代"],
    local_folder: ["Local folder", "本地文件夹"],
    git: ["Git", "Git"],
    webdav: ["WebDAV", "WebDAV"],
    custom_http: ["Custom HTTP", "自定义 HTTP"],
    idle: ["Idle", "空闲"],
    syncing: ["Syncing", "同步中"],
    pulling: ["Pulling", "正在拉取"],
    pushing: ["Pushing", "正在推送"],
    conflict: ["Conflict", "冲突"],
    latest: ["Latest", "最新"],
    pinned: ["Pinned", "固定版本"],
    custom: ["Custom", "自定义"],
    passed: ["Passed", "通过"],
    not_run: ["Not run", "未运行"],
    skipped: ["Skipped", "已跳过"],
    protocol: ["Protocol", "协议"],
    transport: ["Transport", "传输"],
    openai_compatible: ["OpenAI compatible", "OpenAI 兼容"],
    claude: ["Claude", "Claude"],
    backend: ["Backend", "后端"],
    daemon: ["Daemon", "Daemon"],
    store: ["Store", "存储"],
    desktop: ["Desktop", "桌面端"],
    cli: ["CLI", "CLI"],
    search: ["Search", "搜索"],
    read: ["Read", "读取"],
    create: ["Create", "创建"],
    acp: ["ACP", "ACP"],
    sync: ["Sync", "同步"],
  };
  return labels[code]?.[zh ? 1 : 0] ?? code.replaceAll("_", " ");
}

function requestEvaluationStatus(
  request: AccessRequest,
  zh: boolean,
): Pick<RequestRow, "status" | "statusTone" | "statusDetail"> {
  if (request.approval_status !== "pending") {
    return {
      status: operationCodeLabel(request.approval_status, zh),
      statusTone:
        request.approval_status === "approved" ? "approved" : "rejected",
      statusDetail: request.llm_suggestion?.error
        ? `${zh ? "此前自动评估失败：" : "Earlier automatic evaluation failed: "}${request.llm_suggestion.error}`
        : null,
    };
  }
  if (
    request.evaluation_state === "queued" ||
    request.evaluation_state === "running"
  ) {
    if (request.policy_mode === "llm_automatic") {
      return {
        status: zh ? "自动审批中" : "Automatic approval in progress",
        statusTone: "evaluating",
        statusDetail: zh
          ? "LLM 正在检查请求上下文；可立即人工批准或拒绝，人工决定优先"
          : "The LLM is checking the request context; you can approve or reject now. Your decision takes priority",
      };
    }
    return {
      status: zh ? "正在生成 AI 建议" : "Generating AI advice",
      statusTone: "evaluating",
      statusDetail: zh
        ? "LLM 正在生成供人工审批参考的建议；可立即人工批准或拒绝，人工决定优先"
        : "The LLM is preparing advice; you can approve or reject now. Your decision takes priority",
    };
  }
  if (request.evaluation_state === "failed") {
    if (
      request.llm_suggestion?.provider_trace?.stop_reason ===
      "decision_validation_failed"
    ) {
      return {
        status: zh
          ? "模型输出校验失败，等待人工处理"
          : "Model output validation failed; awaiting human review",
        statusTone: "failed",
        statusDetail: request.llm_suggestion.error,
      };
    }
    return {
      status: zh
        ? "自动评估失败，等待人工处理"
        : "Automatic evaluation failed; awaiting human review",
      statusTone: "failed",
      statusDetail: request.llm_suggestion?.error ?? null,
    };
  }
  if (request.evaluation_state === "interrupted") {
    return {
      status: zh
        ? "自动评估已中断，等待人工处理"
        : "Automatic evaluation was interrupted; awaiting human review",
      statusTone: "failed",
      statusDetail: null,
    };
  }
  if (
    request.evaluation_state === "completed" &&
    request.approval_status === "pending"
  ) {
    const automatic = request.policy_mode === "llm_automatic";
    return {
      status: automatic
        ? zh
          ? "模型建议人工复核"
          : "Model requested human review"
        : zh
          ? "AI 建议已生成，等待人工审批"
          : "AI advice ready; awaiting human approval",
      statusTone: "pending",
      statusDetail: zh
        ? "请查看评估结果并作出最终决定"
        : "Review the evaluation and make the final decision",
    };
  }
  return {
    status: operationCodeLabel(request.approval_status, zh),
    statusTone: "pending",
    statusDetail: null,
  };
}

function evaluationStage(request: AccessRequest, zh: boolean): string {
  switch (request.evaluation_state) {
    case "queued":
      return zh ? "等待评估" : "Evaluation queued";
    case "running":
      return zh ? "模型评估中" : "Model evaluating";
    case "completed":
      return zh ? "模型评估完成" : "Model evaluation complete";
    case "failed":
      return zh ? "自动评估失败" : "Automatic evaluation failed";
    case "interrupted":
      return zh ? "评估已中断" : "Evaluation interrupted";
    case "superseded":
      return zh ? "人工已接管" : "Human took over";
    default:
      return request.approval_status === "pending"
        ? zh
          ? "待人工处理"
          : "Awaiting human review"
        : zh
          ? "人工处理完成"
          : "Human review complete";
  }
}

function requestTime(value: string, zh: boolean): string {
  const date = new Date(value);
  return Number.isNaN(date.getTime())
    ? value
    : new Intl.DateTimeFormat(zh ? "zh-CN" : "en", {
        month: "2-digit",
        day: "2-digit",
        hour: "2-digit",
        minute: "2-digit",
        hour12: false,
      }).format(date);
}

function requestRows(
  dashboard: DashboardData | null | undefined,
  zh: boolean,
): RequestRow[] {
  if (!dashboard?.pending_requests.length) {
    return [];
  }

  return dashboard.pending_requests.map((request) => {
    const evaluation = requestEvaluationStatus(request, zh);
    const metadata = request.context.resource_metadata ?? {};
    const field =
      metadata.field_label?.trim() || metadata.field_key?.trim() || "";
    const title =
      metadata.item_title?.trim() ||
      metadata.display_name?.trim() ||
      field ||
      (zh ? "未命名项目" : "Unnamed item");
    const collection =
      [
        metadata.vault?.trim(),
        metadata.collection?.trim() ||
          metadata.group?.trim() ||
          metadata.section?.trim(),
      ]
        .filter(Boolean)
        .join(" / ") || (zh ? "未记录集合" : "Collection not recorded");
    return {
      id: request.id,
      resource: request.context.resource,
      title,
      collection,
      field,
      actor: request.context.requested_by,
      status: evaluation.status,
      statusTone: evaluation.statusTone,
      statusDetail: evaluation.statusDetail,
      reason: request.context.reason,
      time: request.created_at,
      request,
    };
  });
}

const approvalBatchCorrelationWindowMs = 5 * 60 * 1000;
const fieldSpecificMetadataKeys = new Set([
  "field_key",
  "field_label",
  "record_id",
]);

function semanticRequestCorrelationKey(entry: RequestRow): string | null {
  const callChain = requestCallChainForDisplay(entry.request);
  const metadata = entry.request.context.resource_metadata ?? {};
  if (callChain.length === 0 || !metadata.item_id?.trim()) return null;

  const semanticCallChain = callChain.map((node) => ({
    process_name: node.process_name ?? null,
    executable_path: node.executable_path ?? null,
    argv: node.argv ?? [],
    resolved_file_path: node.resolved_file_path ?? null,
    source: node.source ?? null,
  }));
  const sharedMetadata = Object.fromEntries(
    Object.entries(metadata)
      .filter(([key]) => !fieldSpecificMetadataKeys.has(key))
      .sort(([left], [right]) => left.localeCompare(right)),
  );
  return JSON.stringify({
    semanticCallChain,
    requestedBy: entry.actor,
    reason: entry.reason,
    sharedMetadata,
  });
}

function relatedRequestsForDetail(
  selected: RequestRow | null,
  candidates: RequestRow[],
): RequestRow[] {
  if (!selected) return [];

  const sourceRequestId =
    selected.request.automatic_decision?.batch_source_request_id ?? selected.id;
  const correlationKey = semanticRequestCorrelationKey(selected);
  const selectedAt = Date.parse(selected.request.created_at);
  return candidates
    .filter((candidate) => {
      const candidateSourceId =
        candidate.request.automatic_decision?.batch_source_request_id;
      if (
        candidate.id === sourceRequestId ||
        candidateSourceId === sourceRequestId
      ) {
        return true;
      }
      if (
        correlationKey === null ||
        semanticRequestCorrelationKey(candidate) !== correlationKey
      ) {
        return false;
      }
      const candidateAt = Date.parse(candidate.request.created_at);
      return (
        Number.isFinite(selectedAt) &&
        Number.isFinite(candidateAt) &&
        Math.abs(candidateAt - selectedAt) <= approvalBatchCorrelationWindowMs
      );
    })
    .sort((left, right) => left.time.localeCompare(right.time));
}

function optionalLabel(value: string | number | null | undefined): string {
  return value === null || value === undefined || value === ""
    ? "—"
    : String(value);
}

function actualExposurePolicy(
  report: NonNullable<AccessRequest["llm_suggestion"]>["exposure_report"],
): CredentialExposurePolicy {
  const policy = defaultExposurePolicy();
  if (!report) return policy;
  return {
    ...policy,
    surfaces: policy.surfaces.map((entry) => ({
      ...entry,
      max_level:
        report.surfaces.find((surface) => surface.surface === entry.surface)
          ?.actual_level ?? 2,
    })),
  };
}

function ReviewProgressRail({
  edge = false,
  progress,
  zh,
}: {
  edge?: boolean;
  progress: NonNullable<ProviderTrace["review_progress"]>;
  zh: boolean;
}): JSX.Element | null {
  if (progress.state === "complete") return null;
  const showMessage = progress.state !== "running" || Boolean(progress.error);
  const label =
    progress.state === "running"
      ? progress.error
        ? zh
          ? `正在自动修复详细解释 · ${progress.completed_units}/${progress.total_units}`
          : `Repairing detailed explanations · ${progress.completed_units}/${progress.total_units}`
        : zh
          ? `正在补充详细解释 · ${progress.completed_units}/${progress.total_units}`
          : `Adding detailed explanations · ${progress.completed_units}/${progress.total_units}`
      : progress.state === "partial"
        ? zh
          ? `详细解释部分完成 · ${progress.completed_units}/${progress.total_units}`
          : `Detailed explanations partially complete · ${progress.completed_units}/${progress.total_units}`
        : zh
          ? "详细解释生成失败"
          : "Detailed explanation failed";
  return (
    <section
      aria-label={zh ? "调用链注解进度" : "Call-chain annotation progress"}
      className={`request-review-progress${edge ? " request-review-progress--edge" : ""}`}
      data-has-message={showMessage ? "true" : "false"}
      data-state={progress.state}
      title={progress.error ?? undefined}
    >
      {showMessage ? (
        <div className="request-review-progress__label">
          <span>{label}</span>
          {progress.state === "running" && progress.error ? (
            <small>
              {zh
                ? "校验错误已反馈给 Agent，当前进度会保留"
                : "The validation error was sent back to the agent; current progress is preserved"}
            </small>
          ) : progress.error ? (
            <small>
              {zh
                ? "已停止生成；现有标记仍可查看"
                : "Generation stopped; existing marks remain available"}
            </small>
          ) : null}
        </div>
      ) : null}
      <div
        aria-label={zh ? "正在补充审批证据" : "Adding approval evidence"}
        aria-valuemax={progress.total_units}
        aria-valuemin={0}
        aria-valuenow={progress.completed_units}
        className="request-review-progress__track"
        role="progressbar"
      >
        <span
          aria-hidden="true"
          style={{
            width: `${Math.min(
              100,
              (progress.completed_units / Math.max(1, progress.total_units)) *
                100,
            )}%`,
          }}
        />
      </div>
    </section>
  );
}

function readRawDecisionSummary(
  raw: string,
): { decision?: string; risk?: number; rationale?: string } | null {
  let value: Record<string, unknown>;
  try {
    value = JSON.parse(raw) as Record<string, unknown>;
  } catch {
    return null;
  }
  if (!value || typeof value !== "object") return null;
  return {
    decision:
      typeof value.suggested_decision === "string" &&
      ["allow", "deny", "escalate"].includes(value.suggested_decision)
        ? value.suggested_decision
        : undefined,
    risk: typeof value.risk_score === "number" ? value.risk_score : undefined,
    rationale:
      typeof value.rationale_summary === "string"
        ? value.rationale_summary
        : undefined,
  };
}

function LlmApprovalEvidence({
  request,
  zh,
  overviewOnly = false,
  hideOverview = false,
}: {
  request: AccessRequest;
  zh: boolean;
  overviewOnly?: boolean;
  hideOverview?: boolean;
}): JSX.Element {
  const suggestion = request.llm_suggestion;
  const automatic = request.automatic_decision;
  const providerInput = request.provider_input;
  if (!suggestion && !automatic && !providerInput) {
    return (
      <p className="request-llm-empty">
        {zh
          ? "此请求没有记录 LLM 或自动审批证据。"
          : "No LLM or automatic-approval evidence was recorded for this request."}
      </p>
    );
  }

  const providerTrace = suggestion?.provider_trace;
  const usage = suggestion?.usage;
  const batchDecisions = suggestion?.batch_decisions ?? [];
  const prompt =
    providerTrace?.rendered_prompt ||
    providerInput?.prompt ||
    request.rendered_prompt;
  const rawAttempt = providerTrace?.decision_attempts?.at(-1);
  const rawSummary = rawAttempt
    ? readRawDecisionSummary(rawAttempt.raw_response)
    : null;
  const modelDecision = suggestion?.error
    ? rawSummary?.decision
    : (suggestion?.suggested_decision ?? automatic?.suggested_decision);
  const modelRisk = suggestion?.error
    ? rawSummary?.risk
    : suggestion?.risk_score;
  const modelRationale = suggestion?.error
    ? rawSummary?.rationale
    : suggestion?.rationale_summary;
  const exposureReport = suggestion?.exposure_report;
  const configuredExposurePolicy = normalizeExposurePolicy(
    parseExposurePolicy(
      request.context.resource_metadata?.credential_exposure_policy_v1,
    ),
  );
  const actualPolicy = exposureReport
    ? actualExposurePolicy(exposureReport)
    : null;
  const breachedSurfaces = exposureReport
    ? exposureReport.surfaces
        .filter(
          (surface) =>
            surface.actual_level >
            (configuredExposurePolicy.surfaces.find(
              (entry) => entry.surface === surface.surface,
            )?.max_level ?? 0),
        )
        .map((surface) => surface.surface as ExposureSurface)
    : [];
  const visibleExposureSurfaces =
    exposureReport?.surfaces.filter(
      (surface) =>
        surface.evidence_state === "unknown" ||
        breachedSurfaces.includes(surface.surface),
    ) ?? [];

  const overview = (
    <section
      className="request-decision-overview"
      aria-label={zh ? "审批摘要" : "Approval summary"}
    >
      <div className="request-decision-overview__text">
        <div className="request-model-summary">
          <span>
            {suggestion?.error
              ? zh
                ? "原始建议（未通过校验）"
                : "Raw suggestion (unvalidated)"
              : zh
                ? "模型建议"
                : "Model suggestion"}
            <strong>
              {modelDecision
                ? operationCodeLabel(modelDecision, zh)
                : zh
                  ? "暂无有效建议"
                  : "No validated suggestion"}
            </strong>
          </span>
          <span>
            {zh ? "风险分（参考）" : "Risk score (reference)"}
            <strong>{optionalLabel(modelRisk)}</strong>
          </span>
          <span>
            {zh ? "评估提供方" : "Review provider"}
            <strong>
              {[
                suggestion?.provider_kind ?? request.provider_kind,
                suggestion?.provider_model,
              ]
                .filter(Boolean)
                .join(" / ") || "—"}
            </strong>
          </span>
        </div>
        {modelRationale ? (
          <div className="request-llm-rationale">
            <strong>
              {suggestion?.error
                ? zh
                  ? "原始模型理由（未通过校验）"
                  : "Raw rationale (unvalidated)"
                : zh
                  ? "模型理由"
                  : "Model rationale"}
            </strong>
            <ApprovalMarkdown>{modelRationale}</ApprovalMarkdown>
          </div>
        ) : null}
        {automatic?.auto_rationale_summary ? (
          <details className="request-llm-rationale">
            <summary>{zh ? "自动审批理由" : "Automatic rationale"}</summary>
            <ApprovalMarkdown>
              {automatic.auto_rationale_summary}
            </ApprovalMarkdown>
          </details>
        ) : null}
        {visibleExposureSurfaces.length > 0 ? (
          <div className="request-exposure-annotations">
            {visibleExposureSurfaces.map((surface) => (
              <details
                className={
                  breachedSurfaces.includes(surface.surface)
                    ? "is-breached"
                    : undefined
                }
                key={surface.surface}
                open={
                  breachedSurfaces.includes(surface.surface) ||
                  surface.evidence_state === "unknown"
                }
              >
                <summary>
                  <strong>{operationCodeLabel(surface.surface, zh)}</strong>
                  <span>
                    {surface.actual_level} ·{" "}
                    {operationCodeLabel(surface.evidence_state, zh)}
                  </span>
                </summary>
                <ApprovalMarkdown>{surface.summary}</ApprovalMarkdown>
              </details>
            ))}
          </div>
        ) : null}
      </div>
      {exposureReport && actualPolicy ? (
        <div className="request-decision-overview__radar">
          <ExposureRadar
            compact
            breachedSurfaces={breachedSurfaces}
            locale={zh ? "zh-CN" : "en-US"}
            primary={actualPolicy}
            primaryLabel={zh ? "LLM 判定实际暴露" : "LLM-observed exposure"}
            secondary={configuredExposurePolicy}
            secondaryLabel={zh ? "字段允许上限" : "Allowed limit"}
          />
        </div>
      ) : null}
    </section>
  );
  if (overviewOnly) return overview;

  return (
    <div className="request-llm-evidence">
      {!hideOverview ? overview : null}
      <details className="request-technical-details">
        <summary>
          {zh ? "技术与来源信息" : "Technical details and provenance"}
        </summary>
        {exposureReport ? (
          <details className="request-decision-attempts">
            <summary>
              {zh
                ? "完整暴露面报告（原始数据）"
                : "Complete exposure report (raw data)"}
            </summary>
            <pre>{JSON.stringify(exposureReport, null, 2)}</pre>
          </details>
        ) : null}
        <dl className="request-llm-facts">
          <div>
            <dt>
              {suggestion?.error
                ? zh
                  ? "原始建议（未通过校验）"
                  : "Raw suggestion (unvalidated)"
                : zh
                  ? "模型建议"
                  : "Model suggestion"}
            </dt>
            <dd>
              {modelDecision ? operationCodeLabel(modelDecision, zh) : "—"}
            </dd>
          </div>
          <div>
            <dt>{zh ? "风险分" : "Risk score"}</dt>
            <dd>
              {optionalLabel(
                modelRisk ?? (suggestion ? null : automatic?.risk_score),
              )}
            </dd>
          </div>
          <div>
            <dt>{zh ? "自动决策来源" : "Decision source"}</dt>
            <dd>
              {automatic?.decision_source
                ? operationCodeLabel(automatic.decision_source, zh)
                : "—"}
            </dd>
          </div>
          <div>
            <dt>{zh ? "调用模型" : "Provider called"}</dt>
            <dd>
              {automatic
                ? automatic.provider_called
                  ? zh
                    ? "是"
                    : "Yes"
                  : zh
                    ? "否"
                    : "No"
                : suggestion
                  ? zh
                    ? "是"
                    : "Yes"
                  : "—"}
            </dd>
          </div>
          <div>
            <dt>{zh ? "提供方 / 模型" : "Provider / model"}</dt>
            <dd>
              {[
                suggestion?.provider_kind ?? automatic?.provider_kind,
                suggestion?.provider_model ?? automatic?.provider_model,
              ]
                .filter(Boolean)
                .join(" / ") || "—"}
            </dd>
          </div>
          <div>
            <dt>{zh ? "批量来源请求" : "Batch source request"}</dt>
            <dd>
              <code>{optionalLabel(automatic?.batch_source_request_id)}</code>
            </dd>
          </div>
          <div>
            <dt>{zh ? "模板 / 协议" : "Template / contract"}</dt>
            <dd>
              {[
                suggestion?.template_id ?? providerInput?.template_id,
                suggestion?.template_version ?? providerInput?.template_version,
                suggestion?.prompt_contract_version ??
                  providerInput?.prompt_contract_version,
              ]
                .filter(Boolean)
                .join(" / ") || "—"}
            </dd>
          </div>
          <div>
            <dt>{zh ? "JSON 解析" : "JSON parsing"}</dt>
            <dd>
              {suggestion?.json_repair_strategy
                ? operationCodeLabel(suggestion.json_repair_strategy, zh)
                : "—"}
            </dd>
          </div>
          <div>
            <dt>{zh ? "Prompt 摘要" : "Prompt digest"}</dt>
            <dd>
              <code>
                {optionalLabel(
                  suggestion?.prompt_sha256 || providerInput?.prompt_sha256,
                )}
              </code>
            </dd>
          </div>
          <div>
            <dt>{zh ? "响应标识" : "Response identifiers"}</dt>
            <dd>
              {[suggestion?.provider_response_id, suggestion?.x_request_id]
                .filter(Boolean)
                .join(" / ") || "—"}
            </dd>
          </div>
          <div>
            <dt>{zh ? "Token 用量" : "Token usage"}</dt>
            <dd>
              {usage
                ? `${usage.prompt_tokens} + ${usage.completion_tokens} = ${usage.total_tokens}`
                : "—"}
            </dd>
          </div>
          <div>
            <dt>{zh ? "记录时间" : "Recorded at"}</dt>
            <dd>
              {optionalLabel(
                suggestion?.generated_at ?? automatic?.evaluated_at,
              )}
            </dd>
          </div>
        </dl>
      </details>

      {suggestion?.error ? (
        <details className="request-technical-details">
          <summary>{zh ? "评估错误原文" : "Original evaluation error"}</summary>
          <p>{suggestion.error}</p>
        </details>
      ) : null}
      {providerTrace?.audit_events?.length ? (
        <details className="request-decision-attempts">
          <summary>
            {zh
              ? "审计阶段原始消息与工具记录"
              : "Raw audit messages and tool records"}{" "}
            · {providerTrace.audit_events.length}
          </summary>
          <pre>{JSON.stringify(providerTrace.audit_events, null, 2)}</pre>
        </details>
      ) : null}
      {providerTrace?.decision_attempts?.length ? (
        <details className="request-decision-attempts">
          <summary>
            {zh ? "原始响应与校验记录" : "Raw responses and validation"} ·{" "}
            {providerTrace.decision_attempts.length}
          </summary>
          <p>Session: {providerTrace.session_id ?? "—"}</p>
          <pre>
            {JSON.stringify(providerTrace.session_configuration, null, 2)}
          </pre>
          {providerTrace.decision_attempts.map((attempt, index) => (
            <details key={`${index}-${attempt.started_at}`}>
              <summary>
                {zh ? "第" : "Attempt "}
                {index + 1}
                {zh ? "次" : ""} ·{" "}
                {attempt.validation_error
                  ? zh
                    ? "校验失败"
                    : "Validation failed"
                  : (attempt.normalization ?? "—")}
              </summary>
              <p>
                {attempt.started_at} → {attempt.finished_at}
              </p>
              {attempt.validation_error ? (
                <p role="alert">{attempt.validation_error}</p>
              ) : null}
              <h5>{zh ? "原始回答" : "Original response"}</h5>
              <pre>{attempt.raw_response}</pre>
              <h5>{zh ? "实际提示词" : "Actual prompt"}</h5>
              <pre>{attempt.prompt}</pre>
              <h5>
                {zh ? "工具调用与结果" : "Tool calls and results"} ·{" "}
                {attempt.tool_events.length}
              </h5>
              <pre>{JSON.stringify(attempt.tool_events, null, 2)}</pre>
              <p>{attempt.evidence_path}</p>
            </details>
          ))}
        </details>
      ) : null}

      {batchDecisions.length > 0 ? (
        <div className="request-batch-decisions">
          <h5>
            {zh
              ? "同次模型调用返回的资源决策"
              : "Resource decisions from this model call"}
          </h5>
          <ol>
            {batchDecisions.map((decision) => (
              <li key={decision.resource_selector}>
                <header>
                  <code>{decision.resource_selector}</code>
                  <strong>
                    {operationCodeLabel(decision.suggested_decision, zh)} ·{" "}
                    {zh ? "风险" : "risk"} {decision.risk_score}
                  </strong>
                </header>
                <ApprovalMarkdown>
                  {decision.rationale_summary}
                </ApprovalMarkdown>
              </li>
            ))}
          </ol>
        </div>
      ) : null}

      <details className="request-llm-technical">
        <summary>
          {zh ? "模型输入与追踪明细" : "Model input and trace details"}
        </summary>
        <dl>
          <div>
            <dt>{zh ? "允许读取的文件" : "Allowed read files"}</dt>
            <dd>
              {providerInput?.allowed_read_files?.length
                ? providerInput.allowed_read_files.join("\n")
                : "—"}
            </dd>
          </div>
          <div>
            <dt>{zh ? "命中规则" : "Matched rules"}</dt>
            <dd>{automatic?.matched_rule_ids.join(", ") || "—"}</dd>
          </div>
          <div>
            <dt>{zh ? "传输追踪" : "Transport trace"}</dt>
            <dd>
              {providerTrace
                ? JSON.stringify(
                    sanitizePayloadForDisplay({
                      transport: providerTrace.transport,
                      protocol: providerTrace.protocol,
                      api_version: providerTrace.api_version,
                      output_format: providerTrace.output_format,
                      stop_reason: providerTrace.stop_reason,
                      package_name: providerTrace.package_name,
                      package_version: providerTrace.package_version,
                      session_id: providerTrace.session_id,
                      client_request_id: providerTrace.client_request_id,
                      agent_name: providerTrace.agent_name,
                      agent_version: providerTrace.agent_version,
                      beta_headers: providerTrace.beta_headers,
                    }),
                    null,
                    2,
                  )
                : "—"}
            </dd>
          </div>
        </dl>
        {prompt ? (
          <div>
            <h5>{zh ? "保存的模型输入" : "Stored model input"}</h5>
            <pre>{prompt}</pre>
          </div>
        ) : null}
      </details>
    </div>
  );
}

export function OperationsPage(props: OperationsPageProps): JSX.Element {
  const zh = props.locale === "zh-CN";
  if (props.view === "requests" || props.view === "audit") {
    return (
      <RequestsPage
        dashboard={props.dashboard}
        focusedRequestId={props.focusedRequestId}
        isSubmitting={props.isSubmitting}
        noteDraft={props.noteDraft}
        onDecision={props.onDecision}
        initialHistory={props.view === "audit" ? "grouped" : undefined}
        onNoteChange={props.onNoteChange}
        zh={zh}
      />
    );
  }
  if (props.view === "connections") return <ConnectionsPage zh={zh} />;
  if (props.view === "agents") {
    return (
      <AgentsPage controller={props.settingsController} locale={props.locale} />
    );
  }
  if (props.view === "policies") {
    return (
      <PoliciesPage
        controller={props.settingsController}
        locale={props.locale}
        onNavigate={props.onNavigate}
      />
    );
  }
  if (props.view === "diagnostics") {
    return <DiagnosticsPage controller={props.settingsController} zh={zh} />;
  }
  const unsupportedView: never = props.view;
  throw new Error(`Unsupported workspace view: ${unsupportedView}`);
}

function RequestsPage({
  dashboard,
  focusedRequestId,
  isSubmitting,
  noteDraft,
  onDecision,
  initialHistory,
  onNoteChange,
  zh,
}: {
  dashboard?: DashboardData | null;
  focusedRequestId: string | null;
  isSubmitting: boolean;
  noteDraft: string;
  onDecision: (requestId: string, decision: DecisionCommand) => Promise<void>;
  initialHistory?: "grouped";
  onNoteChange: (note: string) => void;
  zh: boolean;
}): JSX.Element {
  const requests = useMemo(
    () =>
      requestRows(dashboard, zh).sort((a, b) => b.time.localeCompare(a.time)),
    [dashboard, zh],
  );
  const [query, setQuery] = useState("");
  const [chosenStatus, setStatusFilter] = useState<string | null>(
    initialHistory ? "completed" : null,
  );
  const [historyView, setHistoryView] = useState<
    "requests" | "grouped" | "raw"
  >(initialHistory ?? "requests");
  function onOpenAudit(): void {
    setStatusFilter("completed");
    setHistoryView("grouped");
    setPage(1);
  }
  const statusFilter =
    chosenStatus ?? preferredRequestGroup(dashboard?.pending_requests ?? []);
  const awaitingCount = requests.filter(
    (entry) => requestGroup(entry.request) === "awaiting",
  ).length;
  const evaluatingCount = requests.filter(
    (entry) => requestGroup(entry.request) === "evaluating",
  ).length;
  const [retryRevision, setRetryRevision] = useState(0);
  const [selected, setSelected] = useState(
    focusedRequestId ?? requests[0]?.id ?? "",
  );
  const [page, setPage] = useState(1);
  const [resolvedPage, setResolvedPage] = useState<RequestPage | null>(null);
  const [resolvedLoading, setResolvedLoading] = useState(false);
  const [resolvedError, setResolvedError] = useState<string | null>(null);
  const resolvedGenerationRef = useRef(0);
  const appliedFocusedRequestRef = useRef<string | null>(null);
  const previousStatusRef = useRef(statusFilter);
  const desktopRuntime = Boolean(
    (window as Window & { __TAURI_INTERNALS__?: object }).__TAURI_INTERNALS__,
  );
  const resolvedReviewRevision = (dashboard?.recent_audit_records ?? [])
    .map((record) => `${record.id}:${record.created_at}`)
    .join("|");

  const pendingVisible = useMemo(
    () =>
      requests.filter((entry) => {
        const matchesQuery =
          `${entry.title} ${entry.collection} ${entry.field} ${entry.resource} ${entry.id} ${entry.actor} ${entry.reason}`
            .toLowerCase()
            .includes(query.trim().toLowerCase());
        return matchesQuery && requestGroup(entry.request) === statusFilter;
      }),
    [query, requests, statusFilter],
  );
  const allPendingVisible = useMemo(
    () =>
      requests.filter((entry) =>
        `${entry.title} ${entry.collection} ${entry.field} ${entry.resource} ${entry.id} ${entry.actor} ${entry.reason}`
          .toLowerCase()
          .includes(query.trim().toLowerCase()),
      ),
    [query, requests],
  );
  const resolvedRows = useMemo(
    () =>
      requestRows(
        resolvedPage
          ? {
              pending_requests: resolvedPage.items,
              recent_audit_records: [],
            }
          : null,
        zh,
      ),
    [resolvedPage, zh],
  );
  const visible =
    statusFilter === "completed"
      ? resolvedRows
      : statusFilter === "all"
        ? allPendingVisible
        : pendingVisible;
  const pages =
    statusFilter === "completed"
      ? Math.max(1, Math.ceil((resolvedPage?.total ?? 0) / 8))
      : Math.max(1, Math.ceil(visible.length / 8));
  const pageRows =
    statusFilter === "completed"
      ? visible
      : visible.slice((page - 1) * 8, page * 8);

  useEffect(() => {
    if (
      !dashboard ||
      statusFilter !== "completed" ||
      historyView !== "requests"
    )
      return;
    const generation = resolvedGenerationRef.current + 1;
    resolvedGenerationRef.current = generation;
    setResolvedLoading(true);
    setResolvedError(null);
    if (!desktopRuntime) {
      setResolvedLoading(false);
      setResolvedError(
        zh
          ? "已完成请求历史仅在桌面运行时可用。"
          : "Completed request history is available in the desktop runtime.",
      );
      return;
    }
    void invoke<RequestPage>("list_resolved_requests", {
      page,
      pageSize: 8,
      query: query.trim(),
    })
      .then((result) => {
        if (resolvedGenerationRef.current !== generation) return;
        setResolvedPage(result);
      })
      .catch((reason: unknown) => {
        if (resolvedGenerationRef.current !== generation) return;
        setResolvedError(
          reason instanceof Error ? reason.message : String(reason),
        );
      })
      .finally(() => {
        if (resolvedGenerationRef.current === generation) {
          setResolvedLoading(false);
        }
      });
    return () => {
      if (resolvedGenerationRef.current === generation) {
        resolvedGenerationRef.current += 1;
      }
    };
  }, [
    Boolean(dashboard),
    desktopRuntime,
    historyView,
    page,
    query,
    resolvedReviewRevision,
    retryRevision,
    statusFilter,
    zh,
  ]);

  useEffect(() => {
    if (previousStatusRef.current !== statusFilter && chosenStatus === null) {
      setPage(1);
    }
    previousStatusRef.current = statusFilter;
  }, [chosenStatus, statusFilter]);
  useEffect(() => {
    if (
      chosenStatus === "all" &&
      focusedRequestId &&
      !requests.some((entry) => entry.id === focusedRequestId)
    ) {
      setStatusFilter(null);
      setPage(1);
    }
  }, [chosenStatus, focusedRequestId, requests]);
  useEffect(() => {
    if (statusFilter !== "completed" || !resolvedLoading) {
      setPage((current) => Math.min(current, pages));
    }
  }, [pages, resolvedLoading, statusFilter]);
  useEffect(() => {
    if (!focusedRequestId) {
      appliedFocusedRequestRef.current = null;
      return;
    }
    if (appliedFocusedRequestRef.current === focusedRequestId) return;
    const index = requests.findIndex(
      (request) => request.id === focusedRequestId,
    );
    if (index >= 0) {
      appliedFocusedRequestRef.current = focusedRequestId;
      setStatusFilter("all");
      setQuery("");
      setSelected(focusedRequestId);
      setPage(Math.floor(index / 8) + 1);
      onNoteChange("");
    }
  }, [focusedRequestId, onNoteChange, requests]);
  useEffect(() => {
    if (resolvedLoading || visible.some((request) => request.id === selected))
      return;
    const nextSelected = visible[0]?.id ?? "";
    if (selected !== nextSelected) {
      setSelected(nextSelected);
      onNoteChange("");
    }
  }, [onNoteChange, resolvedLoading, selected, visible]);
  const detail =
    visible.find((entry) => entry.id === selected) ?? pageRows[0] ?? null;
  const detailId = detail?.id;
  const [relatedPage, setRelatedPage] = useState<{
    id: string;
    items: AccessRequest[];
  } | null>(null);
  const [relatedError, setRelatedError] = useState<string | null>(null);
  const pendingRevision = requests
    .map((entry) => `${entry.id}:${entry.request.updated_at}`)
    .join("|");
  useEffect(() => {
    setRelatedError(null);
    if (!desktopRuntime || !detailId) return;
    let active = true;
    void invoke<AccessRequest[]>("list_related_requests", {
      requestId: detailId,
    })
      .then((items) => {
        if (
          !Array.isArray(items) ||
          !items.some((entry) => entry.id === detailId)
        ) {
          throw new Error(
            zh ? "响应缺少当前请求" : "Response omitted the selected request",
          );
        }
        if (active) setRelatedPage({ id: detailId, items });
      })
      .catch((error: unknown) => {
        if (active)
          setRelatedError(
            error instanceof Error ? error.message : String(error),
          );
      });
    return () => {
      active = false;
    };
  }, [
    desktopRuntime,
    detailId,
    pendingRevision,
    resolvedReviewRevision,
    retryRevision,
    zh,
  ]);
  const relatedRequests = useMemo(() => {
    if (relatedPage && relatedPage.id === detailId) {
      return requestRows(
        { pending_requests: relatedPage.items, recent_audit_records: [] },
        zh,
      );
    }
    const candidates = [
      ...new Map(
        [...requests, ...resolvedRows].map((entry) => [entry.id, entry]),
      ).values(),
    ];
    return relatedRequestsForDetail(detail, candidates);
  }, [detail, detailId, relatedPage, requests, resolvedRows, zh]);
  const humanReviewable =
    detail !== null && detail.request.approval_status === "pending";

  function resetFilters(): void {
    setQuery("");
    setStatusFilter(null);
    setPage(1);
    setSelected("");
  }

  async function decideSelected(decision: DecisionCommand): Promise<void> {
    if (!detail) return;
    const keepVisible =
      detail.request.llm_suggestion?.provider_trace?.review_progress?.state ===
      "running";
    await onDecision(detail.id, decision);
    if (keepVisible) {
      setQuery("");
      setStatusFilter("completed");
      setHistoryView("requests");
      setPage(1);
      setSelected(detail.id);
    }
  }

  const navigation = (
    <>
      <PageHeader
        icon={Inbox}
        title={zh ? "请求" : "Requests"}
        primaryAction={
          <div className="request-header-tools">
            <button
              aria-pressed={chosenStatus === null}
              onClick={() => {
                setStatusFilter(null);
                setPage(1);
                setQuery("");
              }}
              type="button"
            >
              <Sparkles aria-hidden="true" size={15} />
              {zh ? "自动选择" : "Auto select"}
            </button>
          </div>
        }
      />
      <RequestToolbar
        hideSearch={statusFilter === "completed" && historyView !== "requests"}
        awaitingCount={awaitingCount}
        failedCount={
          requests.filter(
            (entry) =>
              entry.request.approval_status === "pending" &&
              entry.request.evaluation_state === "failed",
          ).length
        }
        evaluatingCount={evaluatingCount}
        onQueryChange={(value) => {
          setStatusFilter(statusFilter);
          setQuery(value);
          setPage(1);
        }}
        onStatusChange={(value) => {
          setStatusFilter(value);
          setPage(1);
        }}
        query={query}
        statusFilter={statusFilter}
        zh={zh}
      />
      {statusFilter === "completed" ? (
        <div
          className="request-history-views"
          role="group"
          aria-label={zh ? "历史视图" : "History view"}
        >
          {(
            [
              ["requests", zh ? "请求记录" : "Requests", ListChecks],
              ["grouped", zh ? "审批流程" : "Approval flows", GitBranch],
              ["raw", zh ? "原始事件" : "Raw events", ScrollText],
            ] as const
          ).map(([value, label, Icon]) => (
            <button
              key={value}
              type="button"
              aria-pressed={historyView === value}
              onClick={() => {
                setStatusFilter("completed");
                setHistoryView(value);
              }}
            >
              <Icon aria-hidden="true" size={15} />
              {label}
            </button>
          ))}
        </div>
      ) : null}
      <p className="request-queue-summary">
        {statusFilter === "completed"
          ? zh
            ? "查看已结束的请求、完整审批流程或原始事件。"
            : "Review past requests, complete approval flows, or original events."
          : statusFilter === "evaluating"
            ? zh
              ? "正在自动审批或生成 AI 建议，完成后实时更新。"
              : "Automatic approval and AI advice in progress. Results update live."
            : statusFilter === "all"
              ? zh
                ? "所有未完成请求，包括待人工审批和进行中的请求。"
                : "All unfinished requests, including those needing review and those in progress."
              : zh
                ? "需要你决定的请求，自动评估失败的请求也会在这里。"
                : "Requests needing your decision, including failed evaluations."}
      </p>
    </>
  );

  if (dashboard && statusFilter === "completed" && historyView !== "requests") {
    return (
      <section className="workspace-page operations-fill-page requests-page requests-page-audit">
        {navigation}
        <AuditPage
          dashboard={dashboard}
          zh={zh}
          embedded
          displayMode={historyView}
        />
      </section>
    );
  }

  if (
    !dashboard ||
    (statusFilter === "completed" && resolvedLoading && !resolvedPage)
  ) {
    return (
      <section className="workspace-page operations-fill-page requests-page">
        {navigation}
        <p className="operations-loading" role="status">
          {!dashboard && historyView !== "requests"
            ? zh
              ? "正在加载审计事件…"
              : "Loading audit events…"
            : !dashboard
              ? zh
                ? "正在加载请求…"
                : "Loading requests…"
              : zh
                ? "正在加载历史记录…"
                : "Loading history…"}
        </p>
      </section>
    );
  }

  if (statusFilter === "completed" && resolvedError) {
    return (
      <section className="workspace-page operations-fill-page requests-page">
        {navigation}
        <ErrorState
          action={
            <button
              onClick={() => setRetryRevision((value) => value + 1)}
              type="button"
            >
              {zh ? "重试" : "Retry"}
            </button>
          }
          description={resolvedError}
          title={zh ? "无法加载历史记录" : "History unavailable"}
        />
      </section>
    );
  }

  if (visible.length === 0) {
    const noHistory = statusFilter === "completed" && !query.trim();
    return (
      <section className="workspace-page operations-fill-page requests-page">
        {navigation}
        <EmptyState
          icon={noHistory ? History : Search}
          action={
            noHistory ? (
              <button onClick={onOpenAudit} type="button">
                <GitBranch aria-hidden="true" size={16} />
                {zh ? "查看审批流程" : "Open audit"}
              </button>
            ) : (
              <button onClick={resetFilters} type="button">
                {zh ? "返回优先请求" : "Reset filters"}
              </button>
            )
          }
          description={
            noHistory
              ? zh
                ? "请求处理完成后，审批结果会显示在这里。"
                : "Completed requests and their decisions will appear here."
              : zh
                ? "可以清除搜索条件，或切换到其他请求状态。"
                : "Clear the search or switch to another request status."
          }
          title={
            noHistory
              ? zh
                ? "暂无历史请求"
                : "No request history yet"
              : zh
                ? "没有匹配的请求"
                : "No requests match"
          }
        />
      </section>
    );
  }

  const displayedCallChain = detail
    ? requestCallChainForDisplay(detail.request)
    : [];
  const detailReviewProgress =
    detail?.request.llm_suggestion?.provider_trace?.review_progress;

  return (
    <section className="workspace-page operations-fill-page requests-page">
      {navigation}
      {statusFilter === "completed" && resolvedLoading ? (
        <span className="request-refresh-status" role="status">
          {zh ? "正在更新…" : "Updating…"}
        </span>
      ) : null}
      {visible.length === 1 ? (
        <Pagination
          label="Request pagination"
          nextLabel="Next request page"
          onPageChange={setPage}
          page={page}
          pageCount={pages}
          previousLabel="Previous request page"
        />
      ) : null}
      <SplitPane
        detail={
          detail ? (
            <div className="request-detail-frame">
              {detailReviewProgress ? (
                <ReviewProgressRail
                  edge
                  progress={detailReviewProgress}
                  zh={zh}
                />
              ) : null}
              <div className="request-detail-pane request-detail-scroll">
                <header className="request-detail-heading">
                  <p className="eyebrow">
                    {zh ? "凭据访问" : "CREDENTIAL ACCESS"} ·{" "}
                    {relatedRequests.length} {zh ? "项请求" : "requests"}
                  </p>
                  <p className="request-resource-collection">
                    {detail.collection}
                  </p>
                  <h2>{detail.title}</h2>
                  {detail.field ? (
                    <p className="request-resource-field">{detail.field}</p>
                  ) : null}
                  <p className="request-intent">{detail.reason}</p>
                  <p className="request-heading-meta">
                    <span>{detail.actor}</span>
                    <time title={detail.time} dateTime={detail.time}>
                      {requestTime(detail.time, zh)}
                    </time>
                    <span>
                      {operationCodeLabel(detail.request.policy_mode, zh)}
                    </span>
                  </p>
                </header>
                <LlmApprovalEvidence
                  request={detail.request}
                  zh={zh}
                  overviewOnly
                />
                <section
                  aria-label={zh ? "审批状态" : "Approval state"}
                  className="approval-state-rail"
                >
                  <div data-complete="true">
                    <span>
                      <Inbox aria-hidden="true" size={17} />
                    </span>
                    <strong>{zh ? "请求已接收" : "Request received"}</strong>
                  </div>
                  <div
                    data-complete={
                      detail.request.evaluation_state !== "queued" &&
                      detail.request.evaluation_state !== "running" &&
                      !(
                        detail.request.evaluation_state === "not_required" &&
                        detail.request.approval_status === "pending"
                      )
                    }
                  >
                    <span>
                      <Bot aria-hidden="true" size={17} />
                    </span>
                    <strong>{evaluationStage(detail.request, zh)}</strong>
                  </div>
                  <div
                    data-complete={detail.request.approval_status !== "pending"}
                  >
                    <span>
                      <ShieldCheck aria-hidden="true" size={17} />
                    </span>
                    <strong>
                      {detail.request.approval_status === "pending"
                        ? zh
                          ? "等待最终决定"
                          : "Final decision pending"
                        : `${zh ? "决定：" : "Decision: "}${operationCodeLabel(
                            detail.request.approval_status,
                            zh,
                          )}`}
                    </strong>
                  </div>
                </section>
                {detail.statusDetail ? (
                  <section
                    className="request-context request-evaluation"
                    role={detail.statusTone === "failed" ? "alert" : "status"}
                  >
                    <h3>
                      {detail.request.approval_status !== "pending"
                        ? zh
                          ? "此前自动评估未完成"
                          : "Earlier evaluation did not complete"
                        : detail.status}
                    </h3>
                    {detail.request.llm_suggestion?.error ? (
                      <>
                        <p>
                          {detail.request.approval_status !== "pending"
                            ? zh
                              ? "最终决定已记录；保留此前错误供审计查看。"
                              : "The final decision is recorded; the earlier error remains available for audit."
                            : zh
                              ? "本次评估未产生有效决定，请人工处理或查看错误详情。"
                              : "No valid decision was produced; review manually or inspect the error."}
                        </p>
                        <details>
                          <summary>
                            {zh ? "查看错误详情" : "View error details"}
                          </summary>
                          <pre>{detail.statusDetail}</pre>
                        </details>
                      </>
                    ) : (
                      <p>{detail.statusDetail}</p>
                    )}
                  </section>
                ) : null}
                <section className="request-context request-detail-section">
                  <div className="request-call-chain-title">
                    <div>
                      <h3>{zh ? "调用证据" : "Execution evidence"}</h3>
                      <p>
                        {zh
                          ? "按实际进程顺序核对调用链、完整参数与识别到的脚本。"
                          : "Review the actual process order, full arguments, and recognized scripts."}
                      </p>
                    </div>
                    <span>{`${displayedCallChain.length} ${zh ? "层" : displayedCallChain.length === 1 ? "step" : "steps"}`}</span>
                  </div>
                  <RequestCallChain
                    callChain={displayedCallChain}
                    inlineSources={
                      detail.request.provider_input?.sanitized_context
                        .inline_sources ?? []
                    }
                    report={detail.request.llm_suggestion?.exposure_report}
                    zh={zh}
                  />
                  <h4>{zh ? "请求元数据" : "Request metadata"}</h4>
                  <pre>
                    {JSON.stringify(
                      {
                        ...detail.request.context.metadata,
                        ...detail.request.context.resource_metadata,
                      },
                      null,
                      2,
                    )}
                  </pre>
                </section>
                <section className="request-chain-dossier request-detail-section">
                  <div className="request-call-chain-title">
                    <div>
                      <h3>
                        {zh
                          ? "关联请求与 LLM 决策"
                          : "Related requests and LLM decisions"}
                      </h3>
                      <p>
                        {zh
                          ? "按语义调用链、请求者、原因和共享资源元数据聚合；同一档案中的每项审批证据分别保留。"
                          : "Grouped by semantic call chain, requester, reason, and shared resource metadata; approval evidence remains per request."}
                      </p>
                    </div>
                    <span>{relatedRequests.length}</span>
                  </div>
                  <div className="request-dossier-list">
                    {relatedError ? (
                      <p role="alert">
                        {zh
                          ? "关联请求加载失败，当前仅显示已加载的记录："
                          : "Related requests could not be loaded; showing available records: "}
                        {relatedError}
                        <button
                          type="button"
                          onClick={() => setRetryRevision((value) => value + 1)}
                        >
                          {zh ? "重试" : "Retry"}
                        </button>
                      </p>
                    ) : null}
                    {relatedRequests.map((entry, index) => (
                      <article
                        className="request-dossier-entry"
                        data-selected={entry.id === detail.id}
                        key={entry.id}
                      >
                        <header>
                          <div>
                            <span>{String(index + 1).padStart(2, "0")}</span>
                            <div>
                              <span className="request-resource-collection">
                                {entry.collection}
                              </span>
                              <strong>
                                {entry.title}
                                {entry.field ? ` · ${entry.field}` : ""}
                              </strong>
                              <details className="request-identifiers">
                                <summary>
                                  {zh
                                    ? "资源与请求 ID"
                                    : "Resource and request IDs"}
                                </summary>
                                <code>{entry.resource}</code>
                                <code>{entry.id}</code>
                              </details>
                            </div>
                          </div>
                          <small className={`status-${entry.statusTone}`}>
                            {entry.status}
                          </small>
                        </header>
                        <details className="request-entry-context">
                          <summary>
                            {zh ? "请求信息" : "Request information"} ·{" "}
                            {entry.actor} ·{" "}
                            {operationCodeLabel(entry.request.policy_mode, zh)}
                          </summary>
                          <dl className="request-dossier-summary">
                            <div>
                              <dt>{zh ? "请求者" : "Requested by"}</dt>
                              <dd>{entry.actor}</dd>
                            </div>
                            <div>
                              <dt>{zh ? "原因" : "Reason"}</dt>
                              <dd>{entry.reason}</dd>
                            </div>
                            <div>
                              <dt>{zh ? "审批策略" : "Policy"}</dt>
                              <dd>
                                {operationCodeLabel(
                                  entry.request.policy_mode,
                                  zh,
                                )}
                              </dd>
                            </div>
                            <div>
                              <dt>{zh ? "最终决定" : "Final decision"}</dt>
                              <dd>
                                {entry.request.final_decision
                                  ? operationCodeLabel(
                                      entry.request.final_decision,
                                      zh,
                                    )
                                  : operationCodeLabel(
                                      entry.request.approval_status,
                                      zh,
                                    )}
                              </dd>
                            </div>
                            <div>
                              <dt>{zh ? "创建时间" : "Created at"}</dt>
                              <dd>{entry.request.created_at}</dd>
                            </div>
                            <div>
                              <dt>{zh ? "脚本" : "Script"}</dt>
                              <dd>
                                {requestScriptPathForDisplay(entry.request) ??
                                  "—"}
                              </dd>
                            </div>
                          </dl>
                        </details>
                        <LlmApprovalEvidence
                          request={entry.request}
                          zh={zh}
                          hideOverview={entry.id === detail.id}
                        />
                      </article>
                    ))}
                  </div>
                </section>
                {!humanReviewable ? (
                  <p className="request-passive-status" role="status">
                    {zh
                      ? "此请求当前不需要人工操作。"
                      : "This request does not currently need a human action."}
                  </p>
                ) : null}
                <ApprovalChat requestId={detail.id} zh={zh} />
                <button className="ghost" onClick={onOpenAudit} type="button">
                  {zh ? "打开完整审计记录" : "Open full audit record"}
                </button>
              </div>
              {humanReviewable ? (
                <div className="request-review-controls">
                  <label>
                    <span>
                      {zh ? "审批备注（可选）" : "Review note (optional)"}
                    </span>
                    <textarea
                      disabled={isSubmitting}
                      onChange={(event) =>
                        onNoteChange(event.currentTarget.value)
                      }
                      value={noteDraft}
                    />
                  </label>
                  <div className="detail-actions">
                    <button
                      className="primary"
                      disabled={isSubmitting}
                      onClick={() => void decideSelected("approve_request")}
                      type="button"
                    >
                      {isSubmitting
                        ? zh
                          ? "提交中…"
                          : "Submitting…"
                        : zh
                          ? "批准"
                          : "Approve"}
                    </button>
                    <button
                      disabled={isSubmitting}
                      onClick={() => void decideSelected("reject_request")}
                      type="button"
                    >
                      {zh ? "拒绝" : "Reject"}
                    </button>
                  </div>
                </div>
              ) : null}
            </div>
          ) : (
            <div className="request-detail-pane">
              <EmptyState
                action={
                  <button
                    onClick={() => setSelected(pageRows[0]?.id ?? "")}
                    type="button"
                  >
                    {zh ? "选择第一项" : "Select first request"}
                  </button>
                }
                description={
                  zh
                    ? "从左侧列表选择一项，查看同链请求、LLM 决策证据和审批状态。"
                    : "Choose an item to inspect related requests, LLM decision evidence, and approval state."
                }
                eyebrow={zh ? "详情" : "DETAIL"}
                title={zh ? "选择一个请求" : "Select a request"}
              />
            </div>
          )
        }
        detailLabel={zh ? "请求详情" : "Request details"}
        listVisible={visible.length > 1}
        list={
          <div className="request-list-column">
            <div className="request-list-scroll">
              {pageRows.map((entry) => (
                <button
                  aria-pressed={selected === entry.id}
                  className={
                    selected === entry.id ? "request-row active" : "request-row"
                  }
                  key={entry.id}
                  onClick={() => {
                    setSelected(entry.id);
                    onNoteChange("");
                  }}
                  type="button"
                >
                  <span className="request-resource-collection">
                    {entry.collection}
                  </span>
                  <strong>{entry.title}</strong>
                  {entry.field ? (
                    <span className="request-resource-field">
                      {entry.field}
                    </span>
                  ) : null}
                  <p className="request-intent" title={entry.reason}>
                    {entry.reason}
                  </p>
                  <span className="request-row-meta">
                    {entry.actor} ·{" "}
                    <time title={entry.time} dateTime={entry.time}>
                      {requestTime(entry.time, zh)}
                    </time>
                  </span>
                  <small className={`status-${entry.statusTone}`}>
                    <RequestStatusIcon tone={entry.statusTone} />
                    {entry.status}
                  </small>
                </button>
              ))}
            </div>
            <div className="request-list-footer">
              <Pagination
                label="Request pagination"
                nextLabel="Next request page"
                onPageChange={setPage}
                page={page}
                pageCount={pages}
                previousLabel="Previous request page"
              />
            </div>
          </div>
        }
        listLabel={zh ? "请求列表" : "Request list"}
        resizable
        storageKey="plankton.approval-list-width"
      />
    </section>
  );
}

function ConnectionsPage({ zh }: { zh: boolean }): JSX.Element {
  type BackendConnection = {
    id: string;
    backend_kind: "local" | "one_password" | "bitwarden" | "custom";
    display_name: string;
    enabled: boolean;
    capabilities: string[];
    setup_status: "built_in" | "configured" | "setup_required";
    health: "ready" | "not_checked" | "failed";
    detail: string;
  };
  type SyncConnection = {
    vault_id: string;
    adapter_id: string;
    remote_revision?: string | null;
    last_attempt_at?: string | null;
    last_success_at?: string | null;
    status: string;
    error_id?: string | null;
    config: Record<string, unknown>;
  };
  type SyncCompletion = "uploaded" | "downloaded" | "merged" | "up_to_date";
  type SyncRunReceipt = {
    connection: SyncConnection;
    completion: SyncCompletion;
  };
  type CredentialResource = {
    resource: string;
    display_name: string;
  };
  type LocalVaultOption = {
    id: string;
    file_name: string;
    unlock_file_name: string;
    label: string;
    subtitle: string;
    exists: boolean;
    unlock_file_exists: boolean;
  };
  type PreparedGitRepository = {
    directory: string;
    branch: string;
  };
  const [connections, setConnections] = useState<BackendConnection[]>([]);
  const [syncConnections, setSyncConnections] = useState<SyncConnection[]>([]);
  const [localVaults, setLocalVaults] = useState<LocalVaultOption[]>([]);
  const [credentialResources, setCredentialResources] = useState<
    CredentialResource[]
  >([]);
  const [loading, setLoading] = useState(true);
  const [busyId, setBusyId] = useState<string | null>(null);
  const [loadErrors, setLoadErrors] = useState<string[]>([]);
  const [backendErrors, setBackendErrors] = useState<Record<string, string>>(
    {},
  );
  const [syncErrors, setSyncErrors] = useState<Record<string, string>>({});
  const [syncNotices, setSyncNotices] = useState<Record<string, string>>({});
  const [syncFormError, setSyncFormError] = useState<string | null>(null);
  const [syncKind, setSyncKind] = useState("local_folder");
  const [syncAdapterId, setSyncAdapterId] = useState("primary");
  const [syncVaultId, setSyncVaultId] = useState("default");
  const [syncVaultIds, setSyncVaultIds] = useState<string[]>([]);
  const [syncLocation, setSyncLocation] = useState("");
  const [syncGitUrl, setSyncGitUrl] = useState("");
  const [syncGitDirectory, setSyncGitDirectory] = useState("");
  const [syncBranch, setSyncBranch] = useState("");
  const [syncCreateBranch, setSyncCreateBranch] = useState(true);
  const [syncCredentialResource, setSyncCredentialResource] = useState("");
  const [syncDrawerOpen, setSyncDrawerOpen] = useState(false);

  useEffect(() => {
    if (
      !(window as Window & { __TAURI_INTERNALS__?: object }).__TAURI_INTERNALS__
    ) {
      setLoading(false);
      return;
    }
    void Promise.allSettled([
      invoke<BackendConnection[]>("list_backend_connections"),
      invoke<SyncConnection[]>("list_sync_connections"),
      invoke<CredentialResource[]>("list_sync_credential_resources"),
      invoke<LocalVaultOption[]>("list_local_vaults"),
    ])
      .then(([backends, syncs, credentials, vaults]) => {
        const errors: string[] = [];
        if (backends.status === "fulfilled") {
          setConnections(backends.value);
        } else {
          errors.push(
            `${zh ? "后端连接加载失败" : "Backend connections failed to load"}: ${errorMessage(backends.reason)}`,
          );
        }
        if (syncs.status === "fulfilled") {
          setSyncConnections(syncs.value);
        } else {
          errors.push(
            `${zh ? "同步连接加载失败" : "Sync connections failed to load"}: ${errorMessage(syncs.reason)}`,
          );
        }
        if (credentials.status === "fulfilled") {
          setCredentialResources(credentials.value);
        } else {
          errors.push(
            `${zh ? "凭据资源加载失败" : "Credential resources failed to load"}: ${errorMessage(credentials.reason)}`,
          );
        }
        if (vaults.status === "fulfilled") {
          setLocalVaults(vaults.value);
          setSyncVaultIds((current) =>
            current.length > 0
              ? current
              : vaults.value.slice(0, 1).map((vault) => vault.id),
          );
        } else {
          errors.push(
            `${zh ? "本地保险库加载失败" : "Local vaults failed to load"}: ${errorMessage(vaults.reason)}`,
          );
        }
        setLoadErrors(errors);
      })
      .finally(() => setLoading(false));
  }, [zh]);

  function errorMessage(reason: unknown): string {
    return reason instanceof Error ? reason.message : String(reason);
  }

  function syncKey(connection: SyncConnection): string {
    return `${connection.vault_id}:${connection.adapter_id}`;
  }

  function configText(connection: SyncConnection, key: string): string | null {
    const value = connection.config[key];
    return typeof value === "string" && value.length > 0 ? value : null;
  }

  function syncDestinationDetail(connection: SyncConnection): string | null {
    const kind = configText(connection, "kind");
    if (kind === "git") {
      const repository =
        configText(connection, "repository_url") ??
        configText(connection, "repository");
      const branch = configText(connection, "branch");
      const blobPath = configText(connection, "blob_path");
      return [repository, branch && `@ ${branch}`, blobPath]
        .filter((part): part is string => part !== null)
        .join(" · ");
    }
    if (kind === "local_folder") {
      return configText(connection, "directory");
    }
    return configText(connection, "endpoint");
  }

  function syncCompletionMessage(completion: SyncCompletion): string {
    const messages: Record<SyncCompletion, [string, string]> = {
      uploaded: [
        "Local changes were uploaded securely.",
        "已安全上传本机更改。",
      ],
      downloaded: [
        "The latest remote vault was downloaded.",
        "已获取远端最新保险库。",
      ],
      merged: [
        "Local and remote changes were merged and synchronized. Both original copies were backed up.",
        "已自动合并并同步两边的更改，双方原始副本均已备份。",
      ],
      up_to_date: ["This vault is already up to date.", "保险库已是最新版本。"],
    };
    return messages[completion][zh ? 1 : 0];
  }

  function toggleSyncVault(vaultId: string): void {
    setSyncVaultIds((current) =>
      current.includes(vaultId)
        ? current.filter((id) => id !== vaultId)
        : [...current, vaultId],
    );
  }

  function updateLocalVault(updated: LocalVaultOption): void {
    setLocalVaults((current) => [
      updated,
      ...current.filter((vault) => vault.id !== updated.id),
    ]);
  }

  async function chooseVaultUnlockFile(
    connection: SyncConnection,
  ): Promise<void> {
    const key = syncKey(connection);
    setBusyId(`unlock:${key}`);
    setSyncErrors((current) => {
      const next = { ...current };
      delete next[key];
      return next;
    });
    try {
      const updated = await invoke<LocalVaultOption | null>(
        "pick_local_vault_unlock_file",
        { vaultId: connection.vault_id },
      );
      if (updated) updateLocalVault(updated);
    } catch (reason) {
      setSyncErrors((current) => ({
        ...current,
        [key]: errorMessage(reason),
      }));
    } finally {
      setBusyId(null);
    }
  }

  async function revealVaultUnlockFile(
    connection: SyncConnection,
  ): Promise<void> {
    const key = syncKey(connection);
    setBusyId(`unlock:${key}`);
    setSyncErrors((current) => {
      const next = { ...current };
      delete next[key];
      return next;
    });
    try {
      await invoke("reveal_local_vault_unlock_file", {
        vaultId: connection.vault_id,
      });
    } catch (reason) {
      setSyncErrors((current) => ({
        ...current,
        [key]: errorMessage(reason),
      }));
    } finally {
      setBusyId(null);
    }
  }

  async function chooseSyncDirectory(target: "folder" | "git"): Promise<void> {
    setBusyId(`directory:${target}`);
    setSyncFormError(null);
    try {
      const directory = await invoke<string | null>("pick_sync_directory");
      if (!directory) return;
      if (target === "git") setSyncGitDirectory(directory);
      else setSyncLocation(directory);
    } catch (reason) {
      setSyncFormError(errorMessage(reason));
    } finally {
      setBusyId(null);
    }
  }

  function setupLabel(status: BackendConnection["setup_status"]): string {
    if (status === "built_in") return zh ? "内置" : "Built in";
    if (status === "configured") return zh ? "已配置" : "Configured";
    return zh ? "需要设置" : "Setup required";
  }

  function healthLabel(status: BackendConnection["health"]): string {
    if (status === "ready") return zh ? "正常" : "Ready";
    if (status === "failed") return zh ? "健康检查失败" : "Health check failed";
    return zh ? "尚未检查" : "Not checked";
  }

  async function toggle(connection: BackendConnection): Promise<void> {
    setBusyId(connection.id);
    setBackendErrors((current) => {
      const next = { ...current };
      delete next[connection.id];
      return next;
    });
    try {
      const updated = await invoke<BackendConnection>(
        "set_backend_connection_enabled",
        {
          bindingId: connection.id,
          enabled: !connection.enabled,
        },
      );
      setConnections((current) =>
        current.map((entry) => (entry.id === updated.id ? updated : entry)),
      );
    } catch (reason) {
      setBackendErrors((current) => ({
        ...current,
        [connection.id]: errorMessage(reason),
      }));
    } finally {
      setBusyId(null);
    }
  }

  async function checkHealth(connection: BackendConnection): Promise<void> {
    setBusyId(`health:${connection.id}`);
    setBackendErrors((current) => {
      const next = { ...current };
      delete next[connection.id];
      return next;
    });
    try {
      const updated = await invoke<BackendConnection>(
        "check_backend_connection_health",
        { bindingId: connection.id },
      );
      setConnections((current) =>
        current.map((entry) => (entry.id === updated.id ? updated : entry)),
      );
    } catch (reason) {
      setConnections((current) =>
        current.map((entry) =>
          entry.id === connection.id ? { ...entry, health: "failed" } : entry,
        ),
      );
      setBackendErrors((current) => ({
        ...current,
        [connection.id]: errorMessage(reason),
      }));
    } finally {
      setBusyId(null);
    }
  }

  async function saveSync(): Promise<void> {
    setBusyId(`sync:${syncAdapterId}`);
    setSyncFormError(null);
    const vaultIds = syncKind === "git" ? syncVaultIds : [syncVaultId];
    if (vaultIds.length === 0) {
      setSyncFormError(
        zh
          ? "至少选择一个要同步到 Git 的保险库。"
          : "Select at least one vault to sync with Git.",
      );
      setBusyId(null);
      return;
    }
    try {
      const preparedGit =
        syncKind === "git"
          ? await invoke<PreparedGitRepository>("prepare_git_sync_repository", {
              repositoryUrl: syncGitUrl.trim(),
              directory: syncGitDirectory || null,
              branch: syncBranch.trim() || null,
              createBranchIfMissing: syncCreateBranch,
            })
          : null;
      const results = await Promise.allSettled(
        vaultIds.map((vaultId) => {
          const config =
            syncKind === "local_folder"
              ? { kind: syncKind, directory: syncLocation }
              : syncKind === "git"
                ? {
                    kind: syncKind,
                    repository: preparedGit?.directory ?? "",
                    repository_url: syncGitUrl.trim(),
                    blob_path: `${vaultId}.kdbx`,
                    remote: "origin",
                    branch: preparedGit?.branch ?? "main",
                  }
                : {
                    kind: syncKind,
                    endpoint: syncLocation,
                    ...(syncCredentialResource
                      ? { bearer_token_resource: syncCredentialResource }
                      : {}),
                  };
          return invoke<SyncConnection>("save_sync_connection", {
            vaultId,
            adapterId: syncAdapterId,
            enabled: true,
            config,
          });
        }),
      );
      const saved = results.flatMap((result) =>
        result.status === "fulfilled" ? [result.value] : [],
      );
      setSyncConnections((current) => {
        const savedKeys = new Set(saved.map(syncKey));
        return [
          ...saved,
          ...current.filter((entry) => !savedKeys.has(syncKey(entry))),
        ];
      });
      const failures = results.flatMap((result) =>
        result.status === "rejected" ? [errorMessage(result.reason)] : [],
      );
      if (failures.length > 0) {
        setSyncFormError(
          `${zh ? "部分保存失败" : "Some vaults could not be saved"} (${saved.length}/${vaultIds.length}): ${failures.join("; ")}`,
        );
        return;
      }
      setSyncKind("local_folder");
      setSyncAdapterId("primary");
      setSyncVaultId("default");
      setSyncVaultIds(localVaults.slice(0, 1).map((vault) => vault.id));
      setSyncLocation("");
      setSyncGitUrl("");
      setSyncGitDirectory("");
      setSyncBranch("");
      setSyncCreateBranch(true);
      setSyncCredentialResource("");
      setSyncDrawerOpen(false);
    } catch (reason) {
      setSyncFormError(errorMessage(reason));
    } finally {
      setBusyId(null);
    }
  }

  async function runSync(connection: SyncConnection): Promise<void> {
    const key = syncKey(connection);
    const operationId = `sync:${key}`;
    setBusyId(operationId);
    setSyncErrors((current) => {
      const next = { ...current };
      delete next[key];
      return next;
    });
    setSyncNotices((current) => {
      const next = { ...current };
      delete next[key];
      return next;
    });
    try {
      const receipt = await invoke<SyncRunReceipt>("run_sync_connection", {
        vaultId: connection.vault_id,
        adapterId: connection.adapter_id,
        direction: "sync",
      });
      setSyncConnections((current) =>
        current.map((entry) =>
          entry.vault_id === receipt.connection.vault_id &&
          entry.adapter_id === receipt.connection.adapter_id
            ? receipt.connection
            : entry,
        ),
      );
      setSyncNotices((current) => ({
        ...current,
        [key]: syncCompletionMessage(receipt.completion),
      }));
      try {
        setLocalVaults(await invoke<LocalVaultOption[]>("list_local_vaults"));
      } catch (reason) {
        setSyncErrors((current) => ({
          ...current,
          [key]: `${zh ? "同步成功，但本地保险库状态刷新失败" : "Sync succeeded, but local vault status could not be refreshed"}: ${errorMessage(reason)}`,
        }));
      }
    } catch (reason) {
      const message = errorMessage(reason);
      try {
        const refreshed = await invoke<SyncConnection[]>(
          "list_sync_connections",
        );
        setSyncConnections(refreshed);
        setSyncErrors((current) => ({ ...current, [key]: message }));
      } catch (refreshReason) {
        setSyncErrors((current) => ({
          ...current,
          [key]: `${message}; ${errorMessage(refreshReason)}`,
        }));
      }
    } finally {
      setBusyId(null);
    }
  }

  return (
    <section className="workspace-page operations-fill-page connections-page">
      <PageHeader
        icon={Cable}
        primaryAction={
          <button
            className="primary"
            onClick={() => {
              setSyncFormError(null);
              setSyncDrawerOpen(true);
            }}
            type="button"
          >
            <Plus
              aria-hidden="true"
              focusable="false"
              size={16}
              strokeWidth={1.75}
            />
            {zh ? "新增同步目的地" : "Add sync destination"}
          </button>
        }
        description={
          zh
            ? "控制哪些后端可以提供密码和资源。"
            : "Control which backends may provide secrets and resources."
        }
        eyebrow="SOURCE CONTROL"
        title={zh ? "连接" : "Connections"}
      />
      {loadErrors.map((message) => (
        <p className="workspace-alert" key={message} role="alert">
          {message}
        </p>
      ))}
      <section
        aria-labelledby="backend-group-title"
        className="connection-group"
      >
        <header className="connection-group-heading">
          <div>
            <p className="eyebrow">PASSWORD BACKENDS</p>
            <h2 id="backend-group-title">
              {zh ? "密码后端" : "Password backends"}
            </h2>
          </div>
          <span>{connections.length}</span>
        </header>
        <div className="connection-list">
          {loading ? (
            <p className="connection-group-status" role="status">
              {zh ? "正在加载密码后端…" : "Loading password backends…"}
            </p>
          ) : null}
          {!loading && connections.length === 0 && !loadErrors.length ? (
            <p className="connection-group-status">
              {zh
                ? "没有可用的密码后端。"
                : "No password backends are available."}
            </p>
          ) : null}
          {connections.map((connection) => (
            <article className="connection-row" key={connection.id}>
              <div>
                <h2>{connection.display_name}</h2>
                <p>
                  {connection.backend_kind === "local"
                    ? zh
                      ? "本地 KeePassXC / KDBX4 加密保险库"
                      : "Local KeePassXC / KDBX4 encrypted vault"
                    : zh
                      ? "可选 CLI 桥接；AI 只看到统一资源"
                      : "Optional CLI bridge; AI sees provider-neutral resources"}
                </p>
                <dl className="connection-status-grid">
                  <div>
                    <dt>{zh ? "设置" : "Setup"}</dt>
                    <dd>{setupLabel(connection.setup_status)}</dd>
                  </div>
                  <div>
                    <dt>{zh ? "健康" : "Health"}</dt>
                    <dd
                      className={
                        connection.health === "ready"
                          ? "status-approved"
                          : connection.health === "failed"
                            ? "status-rejected"
                            : "status-pending"
                      }
                    >
                      {healthLabel(connection.health)}
                    </dd>
                  </div>
                  <div>
                    <dt>{zh ? "启用" : "Enabled"}</dt>
                    <dd>
                      {connection.enabled
                        ? zh
                          ? "是"
                          : "Yes"
                        : zh
                          ? "否"
                          : "No"}
                    </dd>
                  </div>
                </dl>
                <p className="connection-detail">{connection.detail}</p>
                <small>
                  {connection.capabilities
                    .map((capability) => operationCodeLabel(capability, zh))
                    .join(" · ")}
                </small>
                {backendErrors[connection.id] ? (
                  <p className="connection-inline-alert" role="alert">
                    {backendErrors[connection.id]}
                  </p>
                ) : null}
              </div>
              <div className="connection-row-actions">
                <button
                  disabled={busyId !== null}
                  onClick={() => void checkHealth(connection)}
                  type="button"
                >
                  {busyId === `health:${connection.id}`
                    ? zh
                      ? "检查中…"
                      : "Checking…"
                    : zh
                      ? "检查健康状态"
                      : "Check health"}
                </button>
                <label className="switch">
                  <input
                    checked={connection.enabled}
                    disabled={busyId !== null || connection.id === "plankton"}
                    onChange={() => void toggle(connection)}
                    type="checkbox"
                  />
                  <span>
                    {connection.enabled
                      ? zh
                        ? "已启用"
                        : "Enabled"
                      : zh
                        ? "已关闭"
                        : "Off"}
                  </span>
                </label>
              </div>
            </article>
          ))}
        </div>
      </section>
      <section aria-labelledby="sync-group-title" className="connection-group">
        <header className="connection-group-heading">
          <div>
            <p className="eyebrow">ENCRYPTED BLOB SYNC</p>
            <h2 id="sync-group-title">
              {zh ? "加密同步目的地" : "Encrypted sync destinations"}
            </h2>
            <p>
              {zh
                ? "只传输加密 KDBX，不上传 unlock 文件；Plankton 会自动选择上传、下载或安全合并。"
                : "Transfers encrypted KDBX only, never the unlock file. Plankton automatically uploads, downloads, or safely merges changes."}
            </p>
          </div>
          <span>{syncConnections.length}</span>
        </header>
        <div className="connection-list">
          {!loading && syncConnections.length === 0 && !loadErrors.length ? (
            <p className="connection-group-status">
              {zh
                ? "尚未配置加密同步目的地。"
                : "No encrypted sync destinations configured."}
            </p>
          ) : null}
          {syncConnections.map((connection) => {
            const key = syncKey(connection);
            const localVault = localVaults.find(
              (vault) => vault.id === connection.vault_id,
            );
            const unlockReady = localVault?.unlock_file_exists === true;
            const unlockFileName =
              localVault?.unlock_file_name ?? `.${connection.vault_id}.unlock`;
            const destinationDetail = syncDestinationDetail(connection);
            const retry =
              connection.status === "error" ||
              connection.config.credential_migration_required === true;
            return (
              <article className="connection-row" key={key}>
                <div>
                  <h2>
                    {connection.vault_id} · {connection.adapter_id}
                  </h2>
                  <p>
                    {operationCodeLabel(String(connection.config.kind), zh)} ·{" "}
                    <strong
                      className={
                        connection.status === "idle"
                          ? "status-approved"
                          : connection.status === "error"
                            ? "status-rejected"
                            : "status-pending"
                      }
                    >
                      {operationCodeLabel(connection.status, zh)}
                    </strong>
                  </p>
                  {destinationDetail ? (
                    <p className="connection-detail sync-destination-detail">
                      {destinationDetail}
                    </p>
                  ) : null}
                  <dl className="connection-status-grid sync-status-grid">
                    <div>
                      <dt>{zh ? "上次尝试" : "Last attempt"}</dt>
                      <dd>
                        {connection.last_attempt_at ??
                          (zh ? "尚未尝试" : "Not attempted")}
                      </dd>
                    </div>
                    <div>
                      <dt>{zh ? "上次成功" : "Last success"}</dt>
                      <dd>
                        {connection.last_success_at ??
                          (zh ? "尚未同步" : "Not synchronized")}
                      </dd>
                    </div>
                  </dl>
                  {connection.remote_revision || connection.error_id ? (
                    <details className="sync-technical-details">
                      <summary>{zh ? "技术详情" : "Technical details"}</summary>
                      <dl>
                        {connection.remote_revision ? (
                          <div>
                            <dt>{zh ? "远端版本" : "Remote revision"}</dt>
                            <dd>{connection.remote_revision}</dd>
                          </div>
                        ) : null}
                        {connection.error_id ? (
                          <div>
                            <dt>{zh ? "错误 ID" : "Error ID"}</dt>
                            <dd>{connection.error_id}</dd>
                          </div>
                        ) : null}
                      </dl>
                    </details>
                  ) : null}
                  <div
                    className={`sync-unlock-status ${unlockReady ? "ready" : "missing"}`}
                  >
                    <div>
                      <strong>
                        {unlockReady
                          ? zh
                            ? "unlock 文件已就绪"
                            : "Unlock file ready"
                          : zh
                            ? "缺少 unlock 文件"
                            : "Unlock file required"}
                      </strong>
                      <p>
                        {unlockReady
                          ? zh
                            ? `同步只传输 KDBX，不会传输 ${unlockFileName}。如需在另一台电脑使用，请通过安全渠道单独传输。`
                            : `Sync transfers only KDBX, never ${unlockFileName}. Transfer it separately through a secure channel to use this vault on another computer.`
                          : zh
                            ? `此电脑需要匹配的 ${unlockFileName} 才能打开保险库。请从原电脑通过安全渠道传输；不要提交到 Git，也不要通过聊天或普通邮件发送。`
                            : `This computer needs the matching ${unlockFileName} to open the vault. Transfer it securely from the original computer; never commit it to Git or send it through chat or ordinary email.`}
                      </p>
                    </div>
                    <button
                      disabled={busyId !== null}
                      onClick={() =>
                        void (unlockReady
                          ? revealVaultUnlockFile(connection)
                          : chooseVaultUnlockFile(connection))
                      }
                      type="button"
                    >
                      {busyId === `unlock:${key}`
                        ? zh
                          ? "处理中…"
                          : "Working…"
                        : unlockReady
                          ? zh
                            ? "在文件管理器中显示"
                            : "Show in file manager"
                          : zh
                            ? "选择 unlock 文件"
                            : "Choose unlock file"}
                    </button>
                  </div>
                  {connection.config.credential_migration_required === true ? (
                    <p className="connection-inline-alert" role="alert">
                      {zh
                        ? "此旧连接包含已移除的原始 bearer token。请重新保存并选择 credential resource。"
                        : "This legacy connection contained a removed raw bearer token. Save it again with a credential resource."}
                    </p>
                  ) : null}
                  {retry ? (
                    <small className="connection-retry">
                      {zh ? "可重试" : "Retry available"}
                    </small>
                  ) : null}
                  {syncErrors[key] ? (
                    <p className="connection-inline-alert" role="alert">
                      {syncErrors[key]}
                    </p>
                  ) : null}
                  {syncNotices[key] ? (
                    <p className="sync-completion-notice" role="status">
                      {syncNotices[key]}
                    </p>
                  ) : null}
                </div>
                <div className="connection-row-actions">
                  <button
                    className="primary"
                    disabled={busyId !== null || !unlockReady}
                    onClick={() => void runSync(connection)}
                    type="button"
                  >
                    {busyId === `sync:${key}`
                      ? zh
                        ? "正在同步…"
                        : "Synchronizing…"
                      : retry
                        ? zh
                          ? "重新同步"
                          : "Sync again"
                        : zh
                          ? "立即同步"
                          : "Sync now"}
                  </button>
                </div>
              </article>
            );
          })}
        </div>
      </section>
      <Drawer
        closeLabel={zh ? "关闭同步目的地抽屉" : "Close sync destination drawer"}
        description={
          zh
            ? "配置一个只传输加密保险库内容的目的地。"
            : "Configure a destination that receives encrypted vault data only."
        }
        footer={
          <>
            <button onClick={() => setSyncDrawerOpen(false)} type="button">
              {zh ? "取消" : "Cancel"}
            </button>
            <button
              className="primary"
              disabled={
                (syncKind === "git"
                  ? !syncGitUrl.trim()
                  : !syncLocation.trim()) ||
                (syncKind === "git" && syncVaultIds.length === 0) ||
                busyId !== null
              }
              onClick={() => void saveSync()}
              type="button"
            >
              {busyId?.startsWith("sync:")
                ? zh
                  ? "保存中…"
                  : "Saving…"
                : zh
                  ? syncKind === "git" && syncVaultIds.length > 1
                    ? `保存 ${syncVaultIds.length} 个保险库`
                    : "保存同步连接"
                  : syncKind === "git" && syncVaultIds.length > 1
                    ? `Save ${syncVaultIds.length} vaults`
                    : "Save sync connection"}
            </button>
          </>
        }
        onClose={() => setSyncDrawerOpen(false)}
        open={syncDrawerOpen}
        title={zh ? "新增同步目的地" : "Add sync destination"}
      >
        <div className="sync-form">
          <ChoiceGroup
            label={<>{zh ? "类型" : "Type"}</>}
            aria-label="Sync adapter type"
            initialFocus
            onChange={(value) => {
              const kind = value;
              setSyncKind(kind);
              setSyncLocation("");
              setSyncFormError(null);
              if (kind === "git" && syncVaultIds.length === 0) {
                setSyncVaultIds(
                  localVaults.length > 0
                    ? localVaults.slice(0, 1).map((vault) => vault.id)
                    : [syncVaultId.trim() || "default"],
                );
              }
            }}
            value={syncKind}
            options={[
              {
                value: "local_folder",
                icon: FolderOpen,
                label: <>{zh ? "本地/云盘文件夹" : "Local/cloud folder"}</>,
              },
              { value: "git", icon: GitBranch, label: <>Git</> },
              { value: "webdav", icon: Globe, label: <>WebDAV</> },
              { value: "custom_http", icon: Network, label: <>Custom HTTP</> },
            ]}
          />
          {syncKind === "git" ? (
            <fieldset className="sync-vault-picker">
              <legend>{zh ? "选择保险库" : "Choose vaults"}</legend>
              <div className="sync-vault-picker-heading">
                <p>
                  {zh
                    ? "每个保险库会作为独立的加密 KDBX 文件提交到同一仓库。"
                    : "Each vault is committed to the same repository as its own encrypted KDBX file."}
                </p>
                {localVaults.length > 1 ? (
                  <button
                    className="text-action"
                    onClick={() =>
                      setSyncVaultIds(
                        syncVaultIds.length === localVaults.length
                          ? []
                          : localVaults.map((vault) => vault.id),
                      )
                    }
                    type="button"
                  >
                    {syncVaultIds.length === localVaults.length
                      ? zh
                        ? "清除"
                        : "Clear"
                      : zh
                        ? "全选"
                        : "Select all"}
                  </button>
                ) : null}
              </div>
              {localVaults.length === 0 ? (
                <div className="sync-vault-bootstrap">
                  <label>
                    {zh ? "远端保险库 ID" : "Remote vault ID"}
                    <input
                      aria-label="Remote vault ID"
                      onChange={(event) => {
                        const value = event.currentTarget.value;
                        setSyncVaultId(value);
                        setSyncVaultIds(value.trim() ? [value.trim()] : []);
                      }}
                      placeholder="default"
                      value={syncVaultId}
                    />
                  </label>
                  <p>
                    {zh
                      ? "首次在这台电脑同步时，输入远端 KDBX 文件名（不含 .kdbx）。保存后请选择从原电脑安全传输来的 unlock 文件，再点击立即同步。"
                      : "For the first sync on this computer, enter the remote KDBX filename without .kdbx. After saving, choose the unlock file transferred securely from the original computer, then select Sync now."}
                  </p>
                </div>
              ) : (
                <div className="sync-vault-options">
                  {localVaults.map((vault) => {
                    const selected = syncVaultIds.includes(vault.id);
                    return (
                      <label
                        className={`sync-vault-option ${selected ? "selected" : ""}`}
                        key={vault.id}
                      >
                        <input
                          checked={selected}
                          onChange={() => toggleSyncVault(vault.id)}
                          type="checkbox"
                          value={vault.id}
                        />
                        <span>
                          <strong>{vault.id}</strong>
                          <small>{vault.file_name}</small>
                        </span>
                        <em>{selected ? (zh ? "已装载" : "Included") : "—"}</em>
                      </label>
                    );
                  })}
                </div>
              )}
              <p className="sync-vault-selection-summary">
                {localVaults.length === 0
                  ? zh
                    ? `首次同步：${syncVaultIds.length} 个保险库`
                    : `First sync: ${syncVaultIds.length} ${syncVaultIds.length === 1 ? "vault" : "vaults"} selected`
                  : zh
                    ? `已选择 ${syncVaultIds.length} / ${localVaults.length}`
                    : `${syncVaultIds.length} of ${localVaults.length} selected`}
              </p>
            </fieldset>
          ) : (
            <label>
              {zh ? "保险库 ID" : "Vault ID"}
              <input
                aria-label="Vault ID"
                onChange={(event) => setSyncVaultId(event.currentTarget.value)}
                value={syncVaultId}
              />
            </label>
          )}
          <label>
            {zh ? "连接 ID" : "Connection ID"}
            <input
              aria-label="Connection ID"
              onChange={(event) => setSyncAdapterId(event.currentTarget.value)}
              value={syncAdapterId}
            />
          </label>
          {syncKind === "git" ? (
            <>
              <label>
                {zh ? "Git 仓库 URL" : "Git repository URL"}
                <input
                  aria-label="Git repository URL"
                  autoCapitalize="none"
                  autoCorrect="off"
                  onChange={(event) => setSyncGitUrl(event.currentTarget.value)}
                  placeholder="https://github.com/you/encrypted-vaults.git"
                  spellCheck={false}
                  type="url"
                  value={syncGitUrl}
                />
                <small className="sync-field-help">
                  {zh
                    ? "粘贴 URL 即可；使用系统 Git 凭据或 SSH Agent，不要把令牌写进 URL。"
                    : "Paste a URL to start. Authentication uses your Git credential helper or SSH agent; do not embed tokens."}
                </small>
              </label>
              <label>
                {zh ? "本地工作副本" : "Local working copy"}
                <span className="sync-path-picker">
                  <input
                    aria-label="Git repository directory"
                    placeholder={
                      zh
                        ? "由 Plankton 自动管理"
                        : "Managed automatically by Plankton"
                    }
                    readOnly
                    value={syncGitDirectory}
                  />
                  <button
                    disabled={busyId !== null}
                    onClick={() => void chooseSyncDirectory("git")}
                    type="button"
                  >
                    <FolderOpen
                      aria-hidden="true"
                      size={15}
                      strokeWidth={1.75}
                    />
                    {busyId === "directory:git"
                      ? zh
                        ? "选择中…"
                        : "Choosing…"
                      : zh
                        ? "选择位置"
                        : "Choose location"}
                  </button>
                  {syncGitDirectory ? (
                    <button
                      className="text-action"
                      onClick={() => setSyncGitDirectory("")}
                      type="button"
                    >
                      {zh ? "恢复自动" : "Use automatic"}
                    </button>
                  ) : null}
                </span>
              </label>
              <div className="sync-branch-field">
                <label>
                  {zh ? "分支（可选）" : "Branch (optional)"}
                  <input
                    aria-label="Git branch"
                    onChange={(event) =>
                      setSyncBranch(event.currentTarget.value)
                    }
                    placeholder={
                      zh ? "自动识别默认分支" : "Detect the default branch"
                    }
                    value={syncBranch}
                  />
                </label>
                <label className="sync-branch-create-option">
                  <input
                    checked={syncCreateBranch}
                    onChange={(event) =>
                      setSyncCreateBranch(event.currentTarget.checked)
                    }
                    type="checkbox"
                  />
                  <span>
                    {zh
                      ? "分支不存在时自动新建"
                      : "Create the branch when it does not exist"}
                  </span>
                </label>
              </div>
            </>
          ) : syncKind === "local_folder" ? (
            <label>
              {zh ? "同步文件夹" : "Sync folder"}
              <span className="sync-path-picker">
                <input
                  aria-label="Sync path"
                  placeholder={zh ? "请选择文件夹" : "Choose a folder"}
                  readOnly
                  value={syncLocation}
                />
                <button
                  disabled={busyId !== null}
                  onClick={() => void chooseSyncDirectory("folder")}
                  type="button"
                >
                  <FolderOpen aria-hidden="true" size={15} strokeWidth={1.75} />
                  {busyId === "directory:folder"
                    ? zh
                      ? "选择中…"
                      : "Choosing…"
                    : zh
                      ? "选择文件夹"
                      : "Choose folder"}
                </button>
              </span>
            </label>
          ) : syncKind === "webdav" || syncKind === "custom_http" ? (
            <>
              <label>
                Endpoint
                <input
                  aria-label="Sync endpoint"
                  onChange={(event) =>
                    setSyncLocation(event.currentTarget.value)
                  }
                  placeholder="https://sync.example.com/vault.kdbx"
                  type="url"
                  value={syncLocation}
                />
              </label>
              <label>
                {zh ? "选择凭据资源" : "Choose credential resource"}
                <select
                  aria-label="Available credential resources"
                  onChange={(event) =>
                    setSyncCredentialResource(event.currentTarget.value)
                  }
                  value={syncCredentialResource}
                >
                  <option value="">
                    {zh ? "不使用凭据" : "No credential"}
                  </option>
                  {credentialResources.map((credential) => (
                    <option
                      key={credential.resource}
                      value={credential.resource}
                    >
                      {credential.display_name} · {credential.resource}
                    </option>
                  ))}
                </select>
              </label>
            </>
          ) : null}
          {syncFormError ? (
            <p className="connection-inline-alert" role="alert">
              {syncFormError}
            </p>
          ) : null}
        </div>
      </Drawer>
    </section>
  );
}

function AgentRuntimeSettings(props: {
  controller: SettingsPageController;
  locale: Locale;
  settings: DesktopSettings;
}): JSX.Element {
  const { controller, locale, settings } = props;
  const disabled = controller.isLoading || controller.isSaving;
  const choices = [
    ["openai_compatible", "OpenAI Compatible", "providerOpenAiDesc"],
    ["claude", "Anthropic Claude", "providerClaudeDesc"],
    ["acp", "Local Agent (ACP)", "providerAcpDesc"],
  ] as const;

  return (
    <>
      <SettingsSection
        description={
          locale === "zh-CN"
            ? "选择直接调用模型 API，或通过本地 ACP 智能体执行。"
            : "Call a model API directly, or run through a local ACP agent."
        }
        testId="agents-provider-section"
        title={locale === "zh-CN" ? "执行协议" : "Execution protocol"}
      >
        <div className="settings-option-list agents-provider-options">
          {choices.map(([value, label, description]) => (
            <SettingsOption
              checked={settings.provider_kind === value}
              description={t(locale, description)}
              disabled={disabled}
              key={value}
              label={label}
              name="agentProviderKind"
              onChange={() => controller.onProviderKindChange(value)}
              value={value}
            />
          ))}
        </div>
      </SettingsSection>

      {settings.provider_kind === "openai_compatible" ? (
        <SettingsSection
          description={
            locale === "zh-CN"
              ? "兼容 OpenAI Chat Completions 的服务均可使用；填写自己的 endpoint、token 与模型名。"
              : "Use any OpenAI Chat Completions-compatible service with your own endpoint, token, and model name."
          }
          testId="agents-openai-section"
          title={t(locale, "settingsOpenAiTitle")}
        >
          <div className="settings-form-grid">
            <SettingsInput
              controller={controller}
              field="openai_api_base"
              label={t(locale, "openAiBase")}
              value={settings.openai_api_base}
            />
            <SecretSettingsInput
              controller={controller}
              field="openai_api_key"
              label={t(locale, "openAiApiKey")}
              locale={locale}
              providerLabel="OpenAI-compatible"
              value={settings.openai_api_key}
            />
            <SettingsInput
              controller={controller}
              field="openai_model"
              label={t(locale, "openAiModel")}
              value={settings.openai_model}
            />
            <SettingsInput
              controller={controller}
              field="openai_temperature"
              label={t(locale, "openAiTemperature")}
              min={0}
              step={0.1}
              type="number"
              value={settings.openai_temperature}
            />
          </div>
        </SettingsSection>
      ) : settings.provider_kind === "claude" ? (
        <SettingsSection
          description={t(locale, "providerClaudeDesc")}
          testId="agents-claude-section"
          title={t(locale, "settingsClaudeTitle")}
        >
          <div className="settings-form-grid">
            <SettingsInput
              controller={controller}
              field="claude_api_base"
              label={t(locale, "claudeBase")}
              value={settings.claude_api_base}
            />
            <SecretSettingsInput
              controller={controller}
              field="claude_api_key"
              label={t(locale, "claudeApiKey")}
              locale={locale}
              providerLabel="Claude"
              value={settings.claude_api_key}
            />
            <SettingsInput
              controller={controller}
              field="claude_model"
              label={t(locale, "claudeModel")}
              value={settings.claude_model}
            />
            <details className="agents-advanced settings-field-wide">
              <summary>{locale === "zh-CN" ? "高级" : "Advanced"}</summary>
              <div className="settings-form-grid">
                <SettingsInput
                  controller={controller}
                  field="claude_anthropic_version"
                  label={t(locale, "claudeApiVersion")}
                  value={settings.claude_anthropic_version}
                />
                <SettingsInput
                  controller={controller}
                  field="claude_max_tokens"
                  label={t(locale, "claudeMaxTokens")}
                  min={1}
                  step={1}
                  type="number"
                  value={settings.claude_max_tokens}
                />
                <SettingsInput
                  controller={controller}
                  field="claude_temperature"
                  label={t(locale, "claudeTemperature")}
                  min={0}
                  step={0.1}
                  type="number"
                  value={settings.claude_temperature}
                />
                <SettingsInput
                  controller={controller}
                  field="claude_timeout_secs"
                  label={t(locale, "claudeTimeout")}
                  min={1}
                  step={1}
                  type="number"
                  value={settings.claude_timeout_secs}
                />
              </div>
            </details>
          </div>
        </SettingsSection>
      ) : (
        <AcpSettings
          customInAdvanced
          controller={controller}
          locale={locale}
          settings={settings}
        />
      )}
    </>
  );
}

function AgentsPage(props: {
  controller: SettingsPageController;
  locale: Locale;
}): JSX.Element {
  const { controller, locale } = props;
  const settings = controller.settingsDraft ?? controller.settings;
  return (
    <section className="workspace-page agents-page">
      <PageHeader
        icon={Bot}
        primaryAction={
          <div className="agents-save-action">
            <button
              className="primary"
              data-testid="agents-save-settings-button"
              disabled={!controller.canSave}
              onClick={controller.onSave}
              type="button"
            >
              <Save aria-hidden="true" size={16} />
              {controller.isSaving ? t(locale, "saving") : t(locale, "save")}
            </button>
          </div>
        }
        description={
          locale === "zh-CN"
            ? "为审批与自动化选择执行器。"
            : "Choose the executor for approvals and automation."
        }
        eyebrow="RUNTIME ROUTING"
        status={settingsStatusText(controller, locale)}
        title={locale === "zh-CN" ? "智能体与模型" : "Agents & Models"}
      />
      {!settings && controller.errorMessage ? (
        <ErrorState
          action={
            <button onClick={controller.onReload} type="button">
              {t(locale, "settingsRetry")}
            </button>
          }
          description={controller.errorMessage}
          title={t(locale, "settingsLoadErrorTitle")}
        />
      ) : !settings ? (
        <p className="settings-loading" role="status">
          {t(locale, "settingsLoading")}
        </p>
      ) : (
        <>
          {controller.errorMessage ? (
            <p className="workspace-alert" role="alert">
              {controller.errorMessage}
            </p>
          ) : null}
          {controller.noticeMessage ? (
            <p className="settings-notice" role="status">
              {controller.noticeMessage}
            </p>
          ) : null}
          <div className="agents-controller" data-testid="agents-controller">
            <article className="agents-config-panel">
              <AgentRuntimeSettings
                controller={controller}
                locale={locale}
                settings={settings}
              />
            </article>
            <aside
              className="agents-runtime-panel"
              data-testid="agents-runtime-status"
            >
              <p className="eyebrow">RUNTIME STATUS</p>
              <h2>{locale === "zh-CN" ? "运行时状态" : "Runtime status"}</h2>
              <dl className="field-list">
                <div>
                  <dt>{locale === "zh-CN" ? "设置来源" : "Settings source"}</dt>
                  <dd>
                    {controller.settings
                      ? locale === "zh-CN"
                        ? "共享桌面设置"
                        : "Shared desktop settings"
                      : locale === "zh-CN"
                        ? "不可用"
                        : "Unavailable"}
                  </dd>
                </div>
                <div>
                  <dt>{locale === "zh-CN" ? "草稿" : "Draft"}</dt>
                  <dd>
                    {controller.hasUnsavedChanges
                      ? locale === "zh-CN"
                        ? "有未保存更改"
                        : "Unsaved changes"
                      : locale === "zh-CN"
                        ? "已同步"
                        : "In sync"}
                  </dd>
                </div>
                <div>
                  <dt>{locale === "zh-CN" ? "协议" : "Protocol"}</dt>
                  <dd>{translateCode(locale, settings.provider_kind)}</dd>
                </div>
              </dl>
              <p>
                {locale === "zh-CN"
                  ? "基础连接确认适配器可启动；模型就绪会建立会话并发送最小提示。"
                  : "Basic connection confirms the adapter starts. Model readiness creates a session and sends a minimal prompt."}
              </p>
            </aside>
          </div>
        </>
      )}
    </section>
  );
}

function probeStatusClass(check: AcpProbeCheck): string {
  if (check.status === "passed") return "status-approved";
  if (check.status === "failed") return "status-rejected";
  return "status-pending";
}

function AcpProbeCheckDetails({
  check,
  label,
  zh,
}: {
  check: AcpProbeCheck;
  label: string;
  zh: boolean;
}): JSX.Element {
  const error = check.error;
  return (
    <div>
      <dt>{label}</dt>
      <dd>
        <strong className={probeStatusClass(check)}>
          {operationCodeLabel(check.status, zh)}
        </strong>
        {error ? (
          <div>
            <span>
              {operationCodeLabel(error.kind, zh)}
              {error.code == null ? "" : ` · ${error.code}`}
            </span>
            <p>{error.message}</p>
            {error.data === undefined ? null : (
              <code>{JSON.stringify(error.data)}</code>
            )}
          </div>
        ) : null}
      </dd>
    </div>
  );
}

function AcpProbeDetails({
  probe,
  zh,
}: {
  probe: AcpProbeResult;
  zh: boolean;
}): JSX.Element {
  const command = [probe.program, ...probe.args].filter(Boolean).join(" ");
  const runtime = [probe.agent_name, probe.agent_version]
    .filter(Boolean)
    .join("@");
  return (
    <dl className="field-list">
      <div>
        <dt>{zh ? "实际配置命令" : "Configured command"}</dt>
        <dd>
          <code>{command}</code>
        </dd>
      </div>
      <div>
        <dt>{zh ? "适配器运行时" : "Adapter runtime"}</dt>
        <dd>{runtime || (zh ? "不可用" : "Unavailable")}</dd>
      </div>
      <div>
        <dt>{zh ? "ACP 协议版本" : "ACP protocol version"}</dt>
        <dd>{probe.protocol_version ?? (zh ? "不可用" : "Unavailable")}</dd>
      </div>
      <AcpProbeCheckDetails
        check={probe.basic}
        label={zh ? "基础连接" : "Basic connection"}
        zh={zh}
      />
      <AcpProbeCheckDetails
        check={probe.readiness}
        label={zh ? "模型就绪" : "Model readiness"}
        zh={zh}
      />
    </dl>
  );
}

function auditDecisionPathLabel(path: AuditDecisionPath, zh: boolean): string {
  const labels: Record<AuditDecisionPath, [string, string]> = {
    automatic: ["自动完成", "Automatic"],
    human: ["人工完成", "Human review"],
    overridden: ["人工覆盖", "Human override"],
    failed: ["评估失败", "Evaluation failed"],
    pending: ["等待决定", "Pending"],
  };
  return labels[path][zh ? 0 : 1];
}

function auditPhaseLabel(kind: AuditPhase["kind"], zh: boolean): string {
  const labels: Record<AuditPhase["kind"], [string, string]> = {
    request: ["请求提交", "Request"],
    model: ["模型评估", "Model"],
    policy: ["自动策略", "Policy"],
    human: ["人工决定", "Human"],
    final: ["最终决定", "Final"],
    activity: ["后续活动", "Activity"],
  };
  return labels[kind][zh ? 0 : 1];
}

function auditPhaseSummary(phase: AuditPhase, zh: boolean): string {
  const last = phase.events[phase.events.length - 1];
  const result = operationCodeLabel(auditResultFor(last), zh);
  const risk =
    typeof last.payload.risk_score === "number"
      ? `${zh ? "风险" : "Risk"} ${last.payload.risk_score}`
      : null;
  const actor = last.actor || (zh ? "系统" : "System");

  if (phase.kind === "request") return actor;
  if (phase.kind === "model") {
    return [actor, result, risk].filter(Boolean).join(" · ");
  }
  if (phase.kind === "policy" || phase.kind === "final") return result;
  return `${actor} · ${result}`;
}

function auditDurationLabel(durationMs: number, zh: boolean): string {
  if (durationMs < 1_000) return `${durationMs} ms`;
  const seconds = durationMs / 1_000;
  if (seconds < 60) return `${seconds.toFixed(seconds < 10 ? 1 : 0)} s`;
  const minutes = Math.floor(seconds / 60);
  const remainingSeconds = Math.round(seconds % 60);
  return zh
    ? `${minutes} 分 ${remainingSeconds} 秒`
    : `${minutes}m ${remainingSeconds}s`;
}

function auditTimestampLabel(timestamp: string, zh: boolean): string {
  const parsed = new Date(timestamp);
  if (Number.isNaN(parsed.getTime())) return timestamp;
  return new Intl.DateTimeFormat(zh ? "zh-CN" : "en-GB", {
    year: "numeric",
    month: "2-digit",
    day: "2-digit",
    hour: "2-digit",
    minute: "2-digit",
    second: "2-digit",
    hour12: false,
  }).format(parsed);
}

function exposureReportFromAudit(
  events: DashboardData["recent_audit_records"],
): CredentialExposureReport | null {
  const value = [...events]
    .reverse()
    .find(
      (event) =>
        event.action === "llm_review_details_updated" ||
        event.action === "llm_suggestion_generated" ||
        event.action === "llm_suggestion_failed",
    )?.payload.exposure_report;
  if (
    !value ||
    typeof value !== "object" ||
    !Array.isArray((value as { surfaces?: unknown }).surfaces)
  ) {
    return null;
  }
  return value as CredentialExposureReport;
}

function reviewProgressFromAudit(
  events: DashboardData["recent_audit_records"],
): NonNullable<ProviderTrace["review_progress"]> | null {
  const providerTrace = [...events]
    .reverse()
    .find(
      (event) =>
        event.action === "llm_review_details_updated" ||
        event.action === "llm_suggestion_generated",
    )?.payload.provider_trace;
  if (!providerTrace || typeof providerTrace !== "object") return null;
  const progress = (providerTrace as Record<string, unknown>).review_progress;
  if (!progress || typeof progress !== "object") return null;
  const value = progress as Record<string, unknown>;
  if (
    !["running", "complete", "partial", "failed"].includes(
      String(value.state),
    ) ||
    typeof value.completed_units !== "number" ||
    typeof value.total_units !== "number" ||
    typeof value.updated_at !== "string"
  ) {
    return null;
  }
  return {
    state: value.state as NonNullable<
      ProviderTrace["review_progress"]
    >["state"],
    completed_units: value.completed_units,
    total_units: value.total_units,
    error: typeof value.error === "string" ? value.error : null,
    updated_at: value.updated_at,
  };
}

function exposurePolicyFromAudit(
  events: DashboardData["recent_audit_records"],
): CredentialExposurePolicy {
  const metadata = events.find((event) => event.action === "request_submitted")
    ?.payload.resource_metadata;
  if (!metadata || typeof metadata !== "object") {
    return defaultExposurePolicy();
  }
  const encoded = (metadata as Record<string, unknown>)[
    "credential_exposure_policy_v1"
  ];
  return normalizeExposurePolicy(
    parseExposurePolicy(typeof encoded === "string" ? encoded : null),
  );
}

type CondensedAuditEvent = {
  record: DashboardData["recent_audit_records"][number];
  compactedCount: number;
};

function condensedAuditEvents(
  events: DashboardData["recent_audit_records"],
): CondensedAuditEvent[] {
  const detailUpdates = events.filter(
    (event) => event.action === "llm_review_details_updated",
  );
  const latestDetailUpdate = detailUpdates.at(-1);
  return events.flatMap((record) => {
    if (record.action !== "llm_review_details_updated") {
      return [{ record, compactedCount: 1 }];
    }
    return record === latestDetailUpdate
      ? [{ record, compactedCount: detailUpdates.length }]
      : [];
  });
}

function AuditPage({
  dashboard,
  zh,
  embedded = false,
  displayMode,
}: {
  dashboard?: DashboardData | null;
  zh: boolean;
  embedded?: boolean;
  displayMode?: "grouped" | "raw";
}): JSX.Element {
  type AuditRecord = DashboardData["recent_audit_records"][number];
  const records = dashboard?.recent_audit_records ?? [];
  const groups = useMemo(() => buildAuditApprovalGroups(records), [records]);
  const [viewMode, setViewMode] = useState<"grouped" | "raw">("grouped");
  useEffect(() => {
    if (displayMode) {
      setViewMode(displayMode);
      setPage(1);
      setResultFilter("all");
    }
  }, [displayMode]);
  const [query, setQuery] = useState("");
  const [actorFilter, setActorFilter] = useState("all");
  const [actionFilter, setActionFilter] = useState("all");
  const [resultFilter, setResultFilter] = useState("all");
  const [timeFilter, setTimeFilter] = useState("all");
  const [page, setPage] = useState(1);
  const [expandedRequestId, setExpandedRequestId] = useState<string | null>(
    null,
  );
  const [selected, setSelected] = useState<AuditRecord | null>(null);
  const [requestEvidenceById, setRequestEvidenceById] = useState<
    Record<string, AccessRequest>
  >({});
  const [requestEvidenceLoadingId, setRequestEvidenceLoadingId] = useState<
    string | null
  >(null);
  const [requestEvidenceError, setRequestEvidenceError] = useState<
    string | null
  >(null);
  const requestEvidenceGenerationRef = useRef(0);
  const desktopRuntime = Boolean(
    (window as Window & { __TAURI_INTERNALS__?: object }).__TAURI_INTERNALS__,
  );
  const expandedGroup = groups.find(
    (group) => group.request_id === expandedRequestId,
  );
  const expandedReviewProgress = expandedGroup
    ? reviewProgressFromAudit(expandedGroup.events)
    : null;
  const evidenceRefreshKey =
    expandedReviewProgress?.updated_at ?? expandedGroup?.finished_at ?? null;

  useEffect(() => {
    if (!desktopRuntime || !expandedRequestId || !evidenceRefreshKey) return;
    const generation = requestEvidenceGenerationRef.current + 1;
    requestEvidenceGenerationRef.current = generation;
    setRequestEvidenceLoadingId(expandedRequestId);
    setRequestEvidenceError(null);
    void invoke<AccessRequest>("request_evidence", {
      requestId: expandedRequestId,
    })
      .then((request) => {
        if (requestEvidenceGenerationRef.current !== generation) return;
        setRequestEvidenceById((current) => ({
          ...current,
          [expandedRequestId]: request,
        }));
      })
      .catch((reason: unknown) => {
        if (requestEvidenceGenerationRef.current !== generation) return;
        setRequestEvidenceError(
          reason instanceof Error ? reason.message : String(reason),
        );
      })
      .finally(() => {
        if (requestEvidenceGenerationRef.current === generation) {
          setRequestEvidenceLoadingId(null);
        }
      });
    return () => {
      if (requestEvidenceGenerationRef.current === generation) {
        requestEvidenceGenerationRef.current += 1;
      }
    };
  }, [desktopRuntime, evidenceRefreshKey, expandedRequestId]);

  function resultFor(record: AuditRecord): string {
    return auditResultFor(record);
  }

  const actors = useMemo(
    () => Array.from(new Set(records.map((record) => record.actor))).sort(),
    [records],
  );
  const actions = useMemo(
    () => Array.from(new Set(records.map((record) => record.action))).sort(),
    [records],
  );
  const results = useMemo(() => {
    const values =
      viewMode === "grouped"
        ? groups.map((group) => group.result)
        : records.map(resultFor);
    return Array.from(new Set(values)).sort();
  }, [groups, records, viewMode]);
  const duration =
    timeFilter === "24h"
      ? 24 * 60 * 60 * 1000
      : timeFilter === "7d"
        ? 7 * 24 * 60 * 60 * 1000
        : timeFilter === "30d"
          ? 30 * 24 * 60 * 60 * 1000
          : null;
  const normalizedQuery = query.trim().toLowerCase();
  const filteredRecords = useMemo(() => {
    const now = Date.now();
    return records.filter((record) => {
      const inTime =
        duration === null || now - Date.parse(record.created_at) <= duration;
      const matchesQuery =
        normalizedQuery.length === 0 ||
        [
          record.request_id,
          record.action,
          record.actor,
          record.note ?? "",
        ].some((value) => value.toLowerCase().includes(normalizedQuery));
      return (
        (actorFilter === "all" || record.actor === actorFilter) &&
        (actionFilter === "all" || record.action === actionFilter) &&
        (resultFilter === "all" || resultFor(record) === resultFilter) &&
        inTime &&
        matchesQuery
      );
    });
  }, [
    actionFilter,
    actorFilter,
    duration,
    normalizedQuery,
    records,
    resultFilter,
  ]);
  const filteredGroups = useMemo(() => {
    const now = Date.now();
    return groups.filter((group) => {
      const inTime =
        duration === null || now - Date.parse(group.finished_at) <= duration;
      const matchesActor =
        actorFilter === "all" ||
        group.events.some((event) => event.actor === actorFilter);
      const matchesAction =
        actionFilter === "all" ||
        group.events.some((event) => event.action === actionFilter);
      const matchesQuery =
        normalizedQuery.length === 0 ||
        [
          group.request_id,
          group.resource ?? "",
          group.requested_by ?? "",
          group.reason ?? "",
          group.summary_note ?? "",
        ].some((value) => value.toLowerCase().includes(normalizedQuery));
      return (
        matchesActor &&
        matchesAction &&
        (resultFilter === "all" || group.result === resultFilter) &&
        inTime &&
        matchesQuery
      );
    });
  }, [
    actionFilter,
    actorFilter,
    duration,
    groups,
    normalizedQuery,
    resultFilter,
  ]);
  const filteredCount =
    viewMode === "grouped" ? filteredGroups.length : filteredRecords.length;
  const pageCount = Math.max(1, Math.ceil(filteredCount / 20));
  const pageRecords = filteredRecords.slice((page - 1) * 20, page * 20);
  const pageGroups = filteredGroups.slice((page - 1) * 20, page * 20);
  useEffect(() => {
    setPage((current) => Math.min(current, pageCount));
  }, [pageCount]);

  function resetAuditFilters(): void {
    setQuery("");
    setActorFilter("all");
    setActionFilter("all");
    setResultFilter("all");
    setTimeFilter("all");
    setPage(1);
  }

  if (!dashboard) {
    return (
      <section
        className={
          embedded
            ? "request-audit-content"
            : "workspace-page operations-fill-page"
        }
      >
        {!embedded ? (
          <PageHeader
            description={
              zh
                ? "按审批查看完整决策路径，并下钻到不可变原始事件。"
                : "Review complete decision paths and drill into immutable events."
            }
            eyebrow="IMMUTABLE TRAIL"
            title={zh ? "审计" : "Audit"}
          />
        ) : null}
        <p className="operations-loading" role="status">
          {zh ? "正在加载审计事件…" : "Loading audit events…"}
        </p>
      </section>
    );
  }

  return (
    <section
      className={
        embedded
          ? "request-audit-content"
          : "workspace-page operations-fill-page"
      }
    >
      {!embedded ? (
        <PageHeader
          description={
            zh
              ? "按审批查看完整决策路径，并下钻到不可变原始事件。"
              : "Review complete decision paths and drill into immutable events."
          }
          eyebrow="IMMUTABLE TRAIL"
          title={zh ? "审计" : "Audit"}
        />
      ) : null}
      {records.length === 0 ? (
        <EmptyState
          action={<span>{zh ? "无需操作" : "No action needed"}</span>}
          description={
            zh
              ? "审批、资源访问和系统操作发生后会显示在这里。"
              : "Approvals, resource access, and system actions will appear here."
          }
          eyebrow={zh ? "审计轨迹" : "AUDIT TRAIL"}
          title={zh ? "尚无审计记录" : "No audit records yet"}
        />
      ) : (
        <div className="audit-workspace">
          {!embedded ? (
            <div
              aria-label={zh ? "审计视图" : "Audit view"}
              className="audit-view-switch"
              role="group"
            >
              <button
                aria-pressed={viewMode === "grouped"}
                onClick={() => {
                  setViewMode("grouped");
                  setResultFilter("all");
                  setPage(1);
                }}
                type="button"
              >
                {zh ? "按审批分组" : "Approval flows"}
              </button>
              <button
                aria-pressed={viewMode === "raw"}
                onClick={() => {
                  setViewMode("raw");
                  setResultFilter("all");
                  setPage(1);
                }}
                type="button"
              >
                {zh ? "原始事件" : "Raw events"}
              </button>
            </div>
          ) : null}
          <div className="operations-toolbar audit-toolbar">
            <label className="toolbar-search audit-search">
              <span>{zh ? "搜索" : "Search"}</span>
              <Search
                aria-hidden="true"
                focusable="false"
                size={16}
                strokeWidth={1.75}
              />
              <input
                aria-label="Search audit"
                onChange={(event) => {
                  setQuery(event.currentTarget.value);
                  setPage(1);
                }}
                placeholder={
                  zh ? "资源、请求 ID、原因" : "Resource, request ID, reason"
                }
                value={query}
              />
            </label>
            <label>
              <span>{zh ? "操作者" : "Actor"}</span>
              <select
                aria-label="Audit actor"
                onChange={(event) => {
                  setActorFilter(event.currentTarget.value);
                  setPage(1);
                }}
                value={actorFilter}
              >
                <option value="all">{zh ? "全部" : "All actors"}</option>
                {actors.map((actor) => (
                  <option key={actor} value={actor}>
                    {actor}
                  </option>
                ))}
              </select>
            </label>
            <label>
              <span>{zh ? "动作" : "Action"}</span>
              <select
                aria-label="Audit action"
                onChange={(event) => {
                  setActionFilter(event.currentTarget.value);
                  setPage(1);
                }}
                value={actionFilter}
              >
                <option value="all">{zh ? "全部" : "All actions"}</option>
                {actions.map((action) => (
                  <option key={action} value={action}>
                    {operationCodeLabel(action, zh)}
                  </option>
                ))}
              </select>
            </label>
            <label>
              <span>{zh ? "结果" : "Result"}</span>
              <select
                aria-label="Audit result"
                onChange={(event) => {
                  setResultFilter(event.currentTarget.value);
                  setPage(1);
                }}
                value={resultFilter}
              >
                <option value="all">{zh ? "全部" : "All results"}</option>
                {results.map((result) => (
                  <option key={result} value={result}>
                    {operationCodeLabel(result, zh)}
                  </option>
                ))}
              </select>
            </label>
            <ChoiceGroup
              label={
                <>
                  <span>{zh ? "时间" : "Time"}</span>
                </>
              }
              aria-label="Audit time"
              onChange={(value) => {
                setTimeFilter(value);
                setPage(1);
              }}
              value={timeFilter}
              options={[
                { value: "all", label: <>{zh ? "全部时间" : "All time"}</> },
                {
                  value: "24h",
                  label: <>{zh ? "最近 24 小时" : "Last 24 hours"}</>,
                },
                { value: "7d", label: <>{zh ? "最近 7 天" : "Last 7 days"}</> },
                {
                  value: "30d",
                  label: <>{zh ? "最近 30 天" : "Last 30 days"}</>,
                },
              ]}
            />
          </div>
          {filteredCount === 0 ? (
            <EmptyState
              action={
                <button onClick={resetAuditFilters} type="button">
                  {zh ? "清除筛选" : "Clear filters"}
                </button>
              }
              description={
                zh
                  ? "调整搜索词、操作者、动作、结果或时间范围。"
                  : "Adjust the search, actor, action, result, or time range."
              }
              title={zh ? "没有匹配的审批流程" : "No approval flows match"}
            />
          ) : (
            <>
              {viewMode === "grouped" ? (
                <div
                  aria-label={zh ? "审批流程" : "Approval flows"}
                  className="audit-approval-list"
                  role="list"
                >
                  {pageGroups.map((group) => {
                    const expanded = expandedRequestId === group.request_id;
                    const detailId = `audit-flow-${group.request_id}`;
                    const exposureReport = exposureReportFromAudit(
                      group.events,
                    );
                    const reviewProgress = reviewProgressFromAudit(
                      group.events,
                    );
                    const exposurePolicy = exposurePolicyFromAudit(
                      group.events,
                    );
                    const auditBreaches = exposureReport
                      ? exposureReport.surfaces
                          .filter(
                            (surface) =>
                              surface.actual_level >
                              (exposurePolicy.surfaces.find(
                                (entry) => entry.surface === surface.surface,
                              )?.max_level ?? 0),
                          )
                          .map((surface) => surface.surface as ExposureSurface)
                      : [];
                    const visibleAuditSurfaces =
                      exposureReport?.surfaces.filter(
                        (surface) =>
                          !(
                            surface.actual_level === 0 &&
                            surface.evidence_state === "observed"
                          ),
                      ) ?? [];
                    const requestEvidence =
                      requestEvidenceById[group.request_id] ?? null;
                    const evidenceLoading =
                      requestEvidenceLoadingId === group.request_id;
                    return (
                      <article
                        className="audit-approval-group"
                        data-decision-path={group.decision_path}
                        data-result={group.result}
                        key={group.request_id}
                        role="listitem"
                      >
                        <button
                          aria-controls={detailId}
                          aria-expanded={expanded}
                          aria-label={`${operationCodeLabel(group.result, zh)} · ${group.resource ?? group.request_id} · ${auditDecisionPathLabel(group.decision_path, zh)}`}
                          className="audit-approval-row"
                          onClick={() =>
                            setExpandedRequestId(
                              expanded ? null : group.request_id,
                            )
                          }
                          type="button"
                        >
                          <span className="audit-approval-heading">
                            <span
                              className={`audit-result result-${group.result}`}
                            >
                              {operationCodeLabel(group.result, zh)}
                            </span>
                            <strong>
                              {group.resource ?? group.request_id}
                            </strong>
                            <span className="audit-path-label">
                              {auditDecisionPathLabel(group.decision_path, zh)}
                            </span>
                          </span>
                          <span className="audit-approval-meta">
                            <span>{group.requested_by ?? "—"}</span>
                            <span>
                              {group.policy_mode
                                ? operationCodeLabel(group.policy_mode, zh)
                                : "—"}
                            </span>
                            <span>
                              {auditDurationLabel(group.duration_ms, zh)} ·{" "}
                              {group.events.length} {zh ? "个事件" : "events"}
                            </span>
                            <time dateTime={group.finished_at}>
                              {auditTimestampLabel(group.finished_at, zh)}
                            </time>
                          </span>
                          <span
                            aria-hidden="true"
                            className="audit-decision-spine"
                          >
                            {group.phases.map((phase) => (
                              <span
                                className="audit-decision-node"
                                data-phase={phase.kind}
                                key={phase.kind}
                              >
                                <span />
                                <small>{auditPhaseLabel(phase.kind, zh)}</small>
                              </span>
                            ))}
                          </span>
                          <ChevronDown
                            aria-hidden="true"
                            className="audit-approval-chevron"
                            size={18}
                            strokeWidth={1.75}
                          />
                        </button>
                        {expanded ? (
                          <>
                            {reviewProgress ? (
                              <ReviewProgressRail
                                progress={reviewProgress}
                                zh={zh}
                              />
                            ) : null}
                            <div
                              className="audit-approval-detail"
                              id={detailId}
                            >
                              <div className="audit-flow-summary">
                                <p>
                                  {group.summary_note ??
                                    (zh
                                      ? "此流程没有附加说明。"
                                      : "No additional rationale was recorded.")}
                                </p>
                                <dl>
                                  <div>
                                    <dt>{zh ? "请求 ID" : "Request ID"}</dt>
                                    <dd>{group.request_id}</dd>
                                  </div>
                                  <div>
                                    <dt>{zh ? "风险" : "Risk"}</dt>
                                    <dd>{group.risk_score ?? "—"}</dd>
                                  </div>
                                  <div>
                                    <dt>{zh ? "最终操作者" : "Final actor"}</dt>
                                    <dd>{group.final_actor ?? "—"}</dd>
                                  </div>
                                </dl>
                              </div>
                              <div className="audit-approval-evidence">
                                {exposureReport ? (
                                  <section className="audit-exposure-snapshot">
                                    <header>
                                      <strong>
                                        {zh
                                          ? "审批时暴露面快照"
                                          : "Exposure snapshot at decision time"}
                                      </strong>
                                      <span>
                                        {exposureReport.chain_summary}
                                      </span>
                                    </header>
                                    {visibleAuditSurfaces.length > 0 ? (
                                      <div className="request-exposure-annotations">
                                        {visibleAuditSurfaces.map((surface) => (
                                          <details
                                            className={
                                              auditBreaches.includes(
                                                surface.surface,
                                              )
                                                ? "is-breached"
                                                : undefined
                                            }
                                            key={surface.surface}
                                            open
                                          >
                                            <summary>
                                              <strong>
                                                {operationCodeLabel(
                                                  surface.surface,
                                                  zh,
                                                )}
                                              </strong>
                                              <span>
                                                {surface.actual_level} ·{" "}
                                                {operationCodeLabel(
                                                  surface.evidence_state,
                                                  zh,
                                                )}
                                              </span>
                                            </summary>
                                            <p>{surface.summary}</p>
                                          </details>
                                        ))}
                                      </div>
                                    ) : null}
                                    <ExposureRadar
                                      breachedSurfaces={auditBreaches}
                                      locale={zh ? "zh-CN" : "en-US"}
                                      primary={actualExposurePolicy(
                                        exposureReport,
                                      )}
                                      primaryLabel={
                                        zh ? "当时实际判定" : "Observed then"
                                      }
                                      secondary={exposurePolicy}
                                      secondaryLabel={
                                        zh ? "当时允许上限" : "Allowed then"
                                      }
                                    />
                                  </section>
                                ) : null}
                                {requestEvidence ? (
                                  <section className="audit-call-chain-evidence">
                                    <header>
                                      <div>
                                        <strong>
                                          {zh
                                            ? "审批调用链与精确标记"
                                            : "Approval call chain and precise evidence"}
                                        </strong>
                                        <p>
                                          {zh
                                            ? "左侧原文与右侧解释按角标对应；悬停或聚焦角标可联动查看。"
                                            : "References link the original evidence on the left to explanations on the right; hover or focus a reference to follow it."}
                                        </p>
                                      </div>
                                      <span>
                                        {
                                          requestCallChainForDisplay(
                                            requestEvidence,
                                          ).length
                                        }{" "}
                                        {zh ? "个节点" : "nodes"}
                                      </span>
                                    </header>
                                    <RequestCallChain
                                      callChain={requestCallChainForDisplay(
                                        requestEvidence,
                                      )}
                                      inlineSources={
                                        requestEvidence.provider_input
                                          ?.sanitized_context.inline_sources ??
                                        []
                                      }
                                      report={
                                        exposureReport ??
                                        requestEvidence.llm_suggestion
                                          ?.exposure_report
                                      }
                                      zh={zh}
                                    />
                                  </section>
                                ) : evidenceLoading ? (
                                  <p className="audit-call-chain-state">
                                    {zh
                                      ? "正在装载调用链和本地源码证据…"
                                      : "Loading call-chain and local source evidence…"}
                                  </p>
                                ) : requestEvidenceError ? (
                                  <p
                                    className="audit-call-chain-state is-error"
                                    role="alert"
                                  >
                                    {requestEvidenceError}
                                  </p>
                                ) : null}
                                <div className="audit-phase-list">
                                  {group.phases.map((phase) => (
                                    <section
                                      className="audit-phase"
                                      data-phase={phase.kind}
                                      key={phase.kind}
                                    >
                                      <header>
                                        <span aria-hidden="true" />
                                        <div>
                                          <strong>
                                            {auditPhaseLabel(phase.kind, zh)}
                                          </strong>
                                          <small>
                                            {auditPhaseSummary(phase, zh)}
                                          </small>
                                        </div>
                                        <time dateTime={phase.finished_at}>
                                          {auditTimestampLabel(
                                            phase.finished_at,
                                            zh,
                                          )}
                                        </time>
                                      </header>
                                      <div className="audit-phase-events">
                                        {condensedAuditEvents(phase.events).map(
                                          ({ record, compactedCount }) => (
                                            <button
                                              className="audit-phase-event"
                                              key={record.id}
                                              onClick={() =>
                                                setSelected(record)
                                              }
                                              type="button"
                                            >
                                              <span>
                                                {operationCodeLabel(
                                                  record.action,
                                                  zh,
                                                )}
                                                {compactedCount > 1 ? (
                                                  <small className="audit-event-compacted">
                                                    {zh
                                                      ? `${compactedCount} 次注解增量`
                                                      : `${compactedCount} annotation increments`}
                                                  </small>
                                                ) : null}
                                              </span>
                                              <span>{record.actor}</span>
                                              <span>
                                                {operationCodeLabel(
                                                  resultFor(record),
                                                  zh,
                                                )}
                                              </span>
                                              <time
                                                dateTime={record.created_at}
                                              >
                                                {auditTimestampLabel(
                                                  record.created_at,
                                                  zh,
                                                )}
                                              </time>
                                            </button>
                                          ),
                                        )}
                                      </div>
                                    </section>
                                  ))}
                                </div>
                              </div>
                              <ApprovalChat
                                requestId={group.request_id}
                                zh={zh}
                              />
                            </div>
                          </>
                        ) : null}
                      </article>
                    );
                  })}
                </div>
              ) : (
                <div
                  aria-label={zh ? "审计事件" : "Audit events"}
                  className="audit-event-list"
                  role="list"
                >
                  {pageRecords.map((record) => (
                    <div key={record.id} role="listitem">
                      <button
                        className="audit-event-row"
                        onClick={() => setSelected(record)}
                        type="button"
                      >
                        <strong>{operationCodeLabel(record.action, zh)}</strong>
                        <span>{record.actor}</span>
                        <span
                          className={`audit-result result-${resultFor(record)}`}
                        >
                          {operationCodeLabel(resultFor(record), zh)}
                        </span>
                        <time dateTime={record.created_at}>
                          {auditTimestampLabel(record.created_at, zh)}
                        </time>
                        {record.note ? <small>{record.note}</small> : null}
                      </button>
                    </div>
                  ))}
                </div>
              )}
              <Pagination
                label="Audit pagination"
                nextLabel="Next audit page"
                onPageChange={setPage}
                page={page}
                pageCount={pageCount}
                previousLabel="Previous audit page"
              />
            </>
          )}
        </div>
      )}
      <Drawer
        closeLabel={zh ? "关闭审计事件详情" : "Close audit event details"}
        description={
          selected ? `${selected.actor} · ${selected.created_at}` : undefined
        }
        footer={
          <button onClick={() => setSelected(null)} type="button">
            {zh ? "关闭" : "Close"}
          </button>
        }
        onClose={() => setSelected(null)}
        open={selected !== null}
        title={zh ? "事件详情" : "Event details"}
      >
        {selected ? (
          <div className="audit-detail">
            <dl className="field-list">
              <div>
                <dt>ID</dt>
                <dd>{selected.id}</dd>
              </div>
              <div>
                <dt>{zh ? "请求 ID" : "Request ID"}</dt>
                <dd>{selected.request_id}</dd>
              </div>
              <div>
                <dt>{zh ? "动作" : "Action"}</dt>
                <dd>{operationCodeLabel(selected.action, zh)}</dd>
              </div>
              <div>
                <dt>{zh ? "结果" : "Result"}</dt>
                <dd>{operationCodeLabel(resultFor(selected), zh)}</dd>
              </div>
              <div>
                <dt>{zh ? "备注" : "Note"}</dt>
                <dd>{selected.note ?? "—"}</dd>
              </div>
            </dl>
            <h3>{zh ? "完整载荷" : "Full payload"}</h3>
            <pre>
              {JSON.stringify(
                sanitizePayloadForDisplay(selected.payload),
                null,
                2,
              )}
            </pre>
          </div>
        ) : null}
      </Drawer>
    </section>
  );
}
function DiagnosticsPage({
  controller,
  zh,
}: {
  controller: SettingsPageController;
  zh: boolean;
}): JSX.Element {
  type Health = {
    protocol_version: number;
    health: "ready" | "degraded";
    pid: number;
    started_at: string;
  };
  type Diagnostic = {
    error: {
      code: string;
      user_message: string;
      internal_message?: string | null;
      severity: string;
      timestamp: string;
      correlation_id: string;
      source: { kind: string; backend_id?: string; adapter_id?: string };
      public_context?: Record<string, string>;
      internal_context?: Record<string, string>;
      retryable: boolean;
    };
    acknowledged_at?: string | null;
  };
  type DiagnosticPage = {
    items: Diagnostic[];
    total: number;
    page: number;
    page_size: number;
  };
  type DiagnosticQuery = {
    acknowledgement: string;
    page: number;
    severity: string;
  };
  const [health, setHealth] = useState<Health | null>(null);
  const [diagnostics, setDiagnostics] = useState<Diagnostic[]>([]);
  const [diagnosticTotal, setDiagnosticTotal] = useState(0);
  const [diagnosticsLoaded, setDiagnosticsLoaded] = useState(false);
  const [healthError, setHealthError] = useState<string | null>(null);
  const [diagnosticError, setDiagnosticError] = useState<string | null>(null);
  const [actionError, setActionError] = useState<string | null>(null);
  const [acpProbe, setAcpProbe] = useState<AcpProbeResult | null>(null);
  const [acpProbeError, setAcpProbeError] = useState<string | null>(null);
  const [probingAcp, setProbingAcp] = useState(false);
  const [refreshing, setRefreshing] = useState(false);
  const [acknowledgingId, setAcknowledgingId] = useState<string | null>(null);
  const [severityFilter, setSeverityFilter] = useState("all");
  const [acknowledgementFilter, setAcknowledgementFilter] =
    useState("unacknowledged");
  const [page, setPage] = useState(1);
  const refreshGenerationRef = useRef(0);
  const acknowledgementOperationRef = useRef(0);
  const mountedRef = useRef(true);
  const diagnosticsRef = useRef<Diagnostic[]>([]);
  const diagnosticTotalRef = useRef(0);
  const diagnosticQueryRef = useRef<DiagnosticQuery>({
    acknowledgement: "unacknowledged",
    page: 1,
    severity: "all",
  });
  diagnosticsRef.current = diagnostics;
  diagnosticTotalRef.current = diagnosticTotal;
  diagnosticQueryRef.current = {
    acknowledgement: acknowledgementFilter,
    page,
    severity: severityFilter,
  };
  const desktopRuntime = Boolean(
    (window as Window & { __TAURI_INTERNALS__?: object }).__TAURI_INTERNALS__,
  );
  const settings = controller.settingsDraft ?? controller.settings;

  const refresh = useCallback(async (): Promise<void> => {
    if (!desktopRuntime) return;
    const generation = refreshGenerationRef.current + 1;
    refreshGenerationRef.current = generation;
    setActionError(null);
    setRefreshing(true);
    const query = diagnosticQueryRef.current;
    const [healthResult, diagnosticResult] = await Promise.allSettled([
      invoke<Health>("daemon_health"),
      invoke<DiagnosticPage>("list_diagnostic_errors", {
        acknowledgement: query.acknowledgement,
        page: query.page,
        pageSize: 20,
        severity: query.severity === "all" ? null : query.severity,
      }),
    ]);
    if (!mountedRef.current || refreshGenerationRef.current !== generation) {
      return;
    }
    if (healthResult.status === "fulfilled") {
      setHealth(healthResult.value);
      setHealthError(null);
    } else {
      setHealthError(
        healthResult.reason instanceof Error
          ? healthResult.reason.message
          : String(healthResult.reason),
      );
    }
    if (diagnosticResult.status === "fulfilled") {
      diagnosticsRef.current = diagnosticResult.value.items;
      diagnosticTotalRef.current = diagnosticResult.value.total;
      setDiagnostics(diagnosticsRef.current);
      setDiagnosticTotal(diagnosticTotalRef.current);
      setDiagnosticsLoaded(true);
      setDiagnosticError(null);
    } else {
      setDiagnosticError(
        diagnosticResult.reason instanceof Error
          ? diagnosticResult.reason.message
          : String(diagnosticResult.reason),
      );
    }
    setRefreshing(false);
  }, [desktopRuntime]);

  useEffect(() => {
    mountedRef.current = true;
    return () => {
      mountedRef.current = false;
      refreshGenerationRef.current += 1;
      acknowledgementOperationRef.current += 1;
    };
  }, []);
  useEffect(() => {
    if (!desktopRuntime) return;
    void refresh();
  }, [acknowledgementFilter, desktopRuntime, page, refresh, severityFilter]);
  useEffect(() => {
    if (!desktopRuntime) return;
    const intervalId = window.setInterval(() => {
      void refresh();
    }, 15_000);
    return () => window.clearInterval(intervalId);
  }, [desktopRuntime, refresh]);

  async function acknowledge(correlationId: string): Promise<void> {
    const operation = acknowledgementOperationRef.current + 1;
    acknowledgementOperationRef.current = operation;
    const acknowledgedRecord = diagnosticsRef.current.find(
      (entry) => entry.error.correlation_id === correlationId,
    );
    setActionError(null);
    setAcknowledgingId(correlationId);
    try {
      const changed = await invoke<boolean>("acknowledge_diagnostic_error", {
        correlationId,
      });
      if (!changed) {
        throw new Error(
          zh
            ? "诊断记录不存在或已经确认"
            : "Diagnostic was not found or was already acknowledged",
        );
      }
      if (
        !mountedRef.current ||
        acknowledgementOperationRef.current !== operation
      ) {
        return;
      }
      refreshGenerationRef.current += 1;
      const acknowledgedAt = new Date().toISOString();
      const query = diagnosticQueryRef.current;
      const current = diagnosticsRef.current;
      const existing = current.some(
        (entry) => entry.error.correlation_id === correlationId,
      );
      let next = current;
      let nextTotal = diagnosticTotalRef.current;
      if (query.acknowledgement === "unacknowledged") {
        next = current.filter(
          (entry) => entry.error.correlation_id !== correlationId,
        );
        if (existing) nextTotal = Math.max(0, nextTotal - 1);
      } else if (query.acknowledgement === "all") {
        next = current.map((entry) =>
          entry.error.correlation_id === correlationId
            ? { ...entry, acknowledged_at: acknowledgedAt }
            : entry,
        );
      } else {
        const severityMatches =
          query.severity === "all" ||
          acknowledgedRecord?.error.severity === query.severity;
        if (existing) {
          next = current.map((entry) =>
            entry.error.correlation_id === correlationId
              ? { ...entry, acknowledged_at: acknowledgedAt }
              : entry,
          );
        } else if (acknowledgedRecord && severityMatches && query.page === 1) {
          next = [
            { ...acknowledgedRecord, acknowledged_at: acknowledgedAt },
            ...current,
          ].slice(0, 20);
          nextTotal += 1;
        }
      }
      diagnosticsRef.current = next;
      diagnosticTotalRef.current = nextTotal;
      setDiagnostics(next);
      setDiagnosticTotal(nextTotal);
      setAcknowledgingId(null);
      void refresh();
    } catch (reason) {
      if (
        !mountedRef.current ||
        acknowledgementOperationRef.current !== operation
      ) {
        return;
      }
      setActionError(reason instanceof Error ? reason.message : String(reason));
    } finally {
      if (
        mountedRef.current &&
        acknowledgementOperationRef.current === operation
      ) {
        setAcknowledgingId(null);
      }
    }
  }

  async function runAcpProbes(): Promise<void> {
    if (!desktopRuntime || !settings || controller.validationMessage) return;
    setProbingAcp(true);
    setAcpProbe(null);
    setAcpProbeError(null);
    try {
      const result = await invoke<AcpProbeResult>("test_acp_connection", {
        profile: normalizeProbeProfile(settings.acp_profile),
      });
      setAcpProbe(result);
    } catch (reason) {
      setAcpProbeError(
        reason instanceof Error ? reason.message : String(reason),
      );
    } finally {
      setProbingAcp(false);
    }
  }

  function formatSource(source: Diagnostic["error"]["source"]): string {
    const detail = source.backend_id ?? source.adapter_id;
    const kind = operationCodeLabel(source.kind, zh);
    return detail ? `${kind} · ${detail}` : kind;
  }

  const severities = ["critical", "error", "warning", "info"];
  const diagnosticPageCount = Math.max(1, Math.ceil(diagnosticTotal / 20));
  const activeDiagnosticFilter =
    severityFilter !== "all" || acknowledgementFilter !== "unacknowledged";
  useEffect(() => {
    if (!refreshing) {
      setPage((current) => Math.min(current, diagnosticPageCount));
    }
  }, [diagnosticPageCount, refreshing]);

  function ContextDetails({
    title,
    values,
  }: {
    title: string;
    values: Record<string, string> | undefined;
  }): JSX.Element | null {
    const entries = Object.entries(values ?? {});
    if (entries.length === 0) return null;
    return (
      <div>
        <strong>{title}</strong>
        <dl className="field-list">
          {entries.map(([key, value]) => (
            <div key={key}>
              <dt>{key}</dt>
              <dd>{value}</dd>
            </div>
          ))}
        </dl>
      </div>
    );
  }

  return (
    <section className="workspace-page diagnostics-page">
      <PageHeader
        icon={Activity}
        primaryAction={
          <button
            disabled={!desktopRuntime || refreshing}
            onClick={() => void refresh()}
            type="button"
          >
            <RefreshCw aria-hidden="true" size={16} />
            {refreshing
              ? zh
                ? "刷新中…"
                : "Refreshing…"
              : zh
                ? "刷新"
                : "Refresh"}
          </button>
        }
        description={
          zh
            ? "检查本地代理的健康状态与可见错误。"
            : "Inspect local-agent health and visible errors."
        }
        eyebrow="SYSTEM OBSERVABILITY"
        title={zh ? "诊断" : "Diagnostics"}
      />
      {healthError ? (
        <p className="workspace-alert" role="alert">
          {zh ? "Daemon 健康检查失败：" : "Daemon health failed: "}
          {healthError}
        </p>
      ) : null}
      {diagnosticError ? (
        <p className="workspace-alert" role="alert">
          {zh ? "诊断记录加载失败：" : "Diagnostic loading failed: "}
          {diagnosticError}
        </p>
      ) : null}
      {actionError ? (
        <p className="workspace-alert" role="alert">
          {zh ? "诊断操作失败：" : "Diagnostic action failed: "}
          {actionError}
        </p>
      ) : null}
      {acpProbeError ? (
        <p className="workspace-alert" role="alert">
          {zh ? "ACP 探针失败：" : "ACP probes failed: "}
          {acpProbeError}
        </p>
      ) : null}
      <section
        aria-label={zh ? "Daemon 状态" : "Daemon status"}
        className="daemon-status-strip"
      >
        <div>
          <span>{zh ? "Daemon" : "Daemon"}</span>
          <strong
            className={
              health?.health === "ready" ? "status-approved" : "status-pending"
            }
          >
            {health
              ? operationCodeLabel(health.health, zh)
              : healthError
                ? zh
                  ? "不可用"
                  : "Unavailable"
                : desktopRuntime
                  ? zh
                    ? "正在检查"
                    : "Checking"
                  : zh
                    ? "仅桌面运行时"
                    : "Desktop runtime only"}
          </strong>
        </div>
        <div>
          <span>PID</span>
          <strong>{health?.pid ?? "—"}</strong>
        </div>
        <div>
          <span>{zh ? "协议" : "Protocol"}</span>
          <strong>{health?.protocol_version ?? "—"}</strong>
        </div>
        <div>
          <span>{zh ? "启动时间" : "Started"}</span>
          <strong>{health?.started_at ?? "—"}</strong>
        </div>
      </section>
      <article className="diagnostics-probe-panel">
        <div>
          <p className="eyebrow">ACP RUNTIME</p>
          <h2>{zh ? "ACP 探针" : "ACP probes"}</h2>
          {acpProbe ? (
            <AcpProbeDetails probe={acpProbe} zh={zh} />
          ) : (
            <p>
              {zh
                ? "分别检查基础连接与真实模型就绪状态。"
                : "Checks the basic connection and real model readiness separately."}
            </p>
          )}
          <button
            disabled={
              !desktopRuntime ||
              probingAcp ||
              !settings ||
              controller.validationMessage !== null
            }
            onClick={() => void runAcpProbes()}
            type="button"
          >
            <Activity aria-hidden="true" size={16} />
            {probingAcp
              ? zh
                ? "探测中…"
                : "Running ACP probes…"
              : zh
                ? "运行 ACP 探针"
                : "Run ACP probes"}
          </button>
        </div>
      </article>
      <section
        aria-labelledby="diagnostic-errors-title"
        className="diagnostic-errors-section"
      >
        <header className="diagnostic-errors-heading">
          <div>
            <p className="eyebrow">ERROR LEDGER</p>
            <h2 id="diagnostic-errors-title">
              {zh ? "诊断错误" : "Diagnostic errors"}
            </h2>
          </div>
          <strong
            className={
              diagnosticTotal === 0 ? "status-approved" : "status-rejected"
            }
          >
            {diagnosticTotal}
          </strong>
        </header>
        <div className="operations-toolbar diagnostics-toolbar">
          <SlidersHorizontal
            aria-hidden="true"
            focusable="false"
            size={16}
            strokeWidth={1.75}
          />
          <label>
            <span>{zh ? "严重度" : "Severity"}</span>
            <select
              aria-label="Diagnostic severity"
              onChange={(event) => {
                setSeverityFilter(event.currentTarget.value);
                setPage(1);
              }}
              value={severityFilter}
            >
              <option value="all">
                {zh ? "全部严重度" : "All severities"}
              </option>
              {severities.map((severity) => (
                <option key={severity} value={severity}>
                  {operationCodeLabel(severity, zh)}
                </option>
              ))}
            </select>
          </label>
          <ChoiceGroup
            label={
              <>
                <span>{zh ? "确认状态" : "Acknowledgement"}</span>
              </>
            }
            aria-label="Acknowledgement status"
            onChange={(value) => {
              setAcknowledgementFilter(value);
              setPage(1);
            }}
            value={acknowledgementFilter}
            options={[
              { value: "all", label: <>{zh ? "全部" : "All"}</> },
              {
                value: "unacknowledged",
                label: <>{zh ? "未确认" : "Unacknowledged"}</>,
              },
              {
                value: "acknowledged",
                label: <>{zh ? "已确认" : "Acknowledged"}</>,
              },
            ]}
          />
        </div>
        {!diagnosticsLoaded && refreshing ? (
          <p className="operations-loading" role="status">
            {zh ? "正在加载诊断错误…" : "Loading diagnostic errors…"}
          </p>
        ) : !diagnosticsLoaded && diagnosticError ? (
          <ErrorState
            action={
              <button onClick={() => void refresh()} type="button">
                {zh ? "重试" : "Retry"}
              </button>
            }
            description={diagnosticError}
            title={zh ? "无法加载诊断错误" : "Diagnostic errors unavailable"}
          />
        ) : diagnosticTotal === 0 && !activeDiagnosticFilter ? (
          <EmptyState
            action={<span>{zh ? "系统状态正常" : "System healthy"}</span>}
            description={
              zh
                ? "Daemon 当前没有报告可见错误。"
                : "The daemon is not reporting any visible errors."
            }
            eyebrow={zh ? "健康" : "HEALTHY"}
            title={zh ? "没有诊断错误" : "No diagnostic errors"}
          />
        ) : diagnosticTotal === 0 ? (
          <EmptyState
            action={
              <button
                onClick={() => {
                  setSeverityFilter("all");
                  setAcknowledgementFilter("all");
                  setPage(1);
                }}
                type="button"
              >
                {zh ? "清除筛选" : "Clear filters"}
              </button>
            }
            description={
              zh
                ? "尝试其它严重度或确认状态。"
                : "Try another severity or acknowledgement state."
            }
            title={zh ? "没有匹配的错误" : "No errors match"}
          />
        ) : (
          <>
            <div className="diagnostic-error-list">
              {diagnostics.map((diagnostic) => (
                <article
                  className="diagnostic-record"
                  key={diagnostic.error.correlation_id}
                >
                  <header>
                    <div>
                      <strong>
                        {operationCodeLabel(diagnostic.error.code, zh)}
                      </strong>
                      <span>
                        {operationCodeLabel(diagnostic.error.severity, zh)}
                      </span>
                    </div>
                    <time dateTime={diagnostic.error.timestamp}>
                      {diagnostic.error.timestamp}
                    </time>
                  </header>
                  <p className="diagnostic-message">
                    {diagnostic.error.user_message}
                  </p>
                  <dl className="field-list">
                    <div>
                      <dt>{zh ? "来源" : "Source"}</dt>
                      <dd>{formatSource(diagnostic.error.source)}</dd>
                    </div>
                    <div>
                      <dt>{zh ? "关联 ID" : "Correlation ID"}</dt>
                      <dd>{diagnostic.error.correlation_id}</dd>
                    </div>
                    <div>
                      <dt>{zh ? "状态" : "Status"}</dt>
                      <dd>
                        {diagnostic.acknowledged_at
                          ? `${zh ? "已确认" : "Acknowledged"} · ${diagnostic.acknowledged_at}`
                          : zh
                            ? "未确认"
                            : "Unacknowledged"}
                      </dd>
                    </div>
                  </dl>
                  <details>
                    <summary>{zh ? "完整详情" : "Full details"}</summary>
                    <p>
                      {zh ? "可重试" : "Retryable"}:{" "}
                      {diagnostic.error.retryable
                        ? zh
                          ? "是"
                          : "Yes"
                        : zh
                          ? "否"
                          : "No"}
                    </p>
                    {diagnostic.error.internal_message ? (
                      <code>{diagnostic.error.internal_message}</code>
                    ) : null}
                    <ContextDetails
                      title={zh ? "公开上下文" : "Public context"}
                      values={diagnostic.error.public_context}
                    />
                    <ContextDetails
                      title={zh ? "内部上下文" : "Internal context"}
                      values={diagnostic.error.internal_context}
                    />
                  </details>
                  {!diagnostic.acknowledged_at ? (
                    <button
                      className="ghost"
                      disabled={acknowledgingId !== null}
                      onClick={() =>
                        void acknowledge(diagnostic.error.correlation_id)
                      }
                      type="button"
                    >
                      {acknowledgingId === diagnostic.error.correlation_id
                        ? zh
                          ? "确认中…"
                          : "Acknowledging…"
                        : zh
                          ? "确认"
                          : "Acknowledge"}
                    </button>
                  ) : null}
                </article>
              ))}
            </div>
            {diagnosticPageCount > 1 ? (
              <Pagination
                label="Diagnostic pagination"
                nextLabel="Next diagnostic page"
                onPageChange={setPage}
                page={page}
                pageCount={diagnosticPageCount}
                previousLabel="Previous diagnostic page"
              />
            ) : null}
          </>
        )}
      </section>
    </section>
  );
}
function SettingsInput(props: {
  controller: SettingsPageController;
  field: keyof DesktopSettings;
  label: string;
  type?: "number" | "password" | "text";
  min?: number;
  step?: number;
  textarea?: boolean;
  value: number | string;
}): JSX.Element {
  const disabled = props.controller.isLoading || props.controller.isSaving;
  return (
    <label
      className={
        props.textarea ? "settings-field settings-field-wide" : "settings-field"
      }
      data-testid={`settings-field-${props.field}`}
    >
      <span>{props.label}</span>
      {props.textarea ? (
        <textarea
          data-settings-field={props.field}
          disabled={disabled}
          onChange={(event) =>
            props.controller.onFieldChange(
              props.field,
              event.currentTarget.value,
            )
          }
          rows={4}
          value={String(props.value)}
        />
      ) : (
        <input
          data-settings-field={props.field}
          disabled={disabled}
          min={props.min}
          onChange={(event) =>
            props.controller.onFieldChange(
              props.field,
              event.currentTarget.value,
            )
          }
          step={props.step}
          type={props.type ?? "text"}
          value={props.value}
        />
      )}
    </label>
  );
}

function SecretSettingsInput(props: {
  controller: SettingsPageController;
  field: "openai_api_key" | "claude_api_key";
  label: string;
  locale: Locale;
  providerLabel: string;
  value: string;
}): JSX.Element {
  const disabled = props.controller.isLoading || props.controller.isSaving;
  const labelId = `${props.field}-label`;
  return (
    <div
      className="settings-field settings-secret-field"
      data-testid={`settings-field-${props.field}`}
    >
      <span id={labelId}>{props.label}</span>
      <SecretInput
        locale={props.locale}
        fieldName={`${props.providerLabel} API key`}
        aria-labelledby={labelId}
        data-settings-field={props.field}
        disabled={disabled}
        onChange={(event) =>
          props.controller.onFieldChange(props.field, event.currentTarget.value)
        }
        value={props.value}
      />
    </div>
  );
}

function SettingsSection(props: {
  title: string;
  description: string;
  children: ReactNode;
  id?: string;
  testId: string;
}): JSX.Element {
  return (
    <section
      className="settings-section"
      data-testid={props.testId}
      id={props.id}
    >
      <div className="settings-section-heading">
        <h2 tabIndex={-1}>{props.title}</h2>
        <p>{props.description}</p>
      </div>
      {props.children}
    </section>
  );
}

const settingsChoiceIcons: Partial<Record<string, LucideIcon>> = {
  manual_only: UserRoundCheck,
  assisted: Sparkles,
  llm_automatic: Bot,
  openai_compatible: Network,
  claude: Sparkles,
  acp: Terminal,
};

function SettingsOption(props: {
  checked: boolean;
  description: string;
  disabled: boolean;
  label: string;
  name: string;
  onChange: () => void;
  value: string;
}): JSX.Element {
  const Icon = settingsChoiceIcons[props.value] ?? Settings2;
  return (
    <label
      className={props.checked ? "settings-option active" : "settings-option"}
    >
      <input
        checked={props.checked}
        disabled={props.disabled}
        name={props.name}
        onChange={props.onChange}
        type="radio"
        value={props.value}
      />
      <span>
        <strong className="settings-option-title">
          <Icon aria-hidden="true" size={21} strokeWidth={1.75} />
          {props.label}
        </strong>
        <small>{props.description}</small>
      </span>
    </label>
  );
}

type PasswordAutoApprovalField =
  | "llm_auto_approve_password_edits"
  | "llm_auto_approve_password_renames"
  | "llm_auto_approve_password_refreshes"
  | "llm_auto_approve_password_deletes";

type LlmApprovalDecisionField =
  | "llm_approval_deny_enabled"
  | "llm_approval_escalate_enabled";

function SettingsToggle(props: {
  checked: boolean;
  controller: SettingsPageController;
  description: string;
  field: PasswordAutoApprovalField;
  label: string;
  policyEnabled: boolean;
}): JSX.Element {
  const Icon = {
    llm_auto_approve_password_edits: Pencil,
    llm_auto_approve_password_renames: TextCursorInput,
    llm_auto_approve_password_refreshes: RefreshCw,
    llm_auto_approve_password_deletes: Trash2,
  }[props.field];
  const disabled =
    props.controller.isLoading ||
    props.controller.isSaving ||
    !props.policyEnabled;
  return (
    <label
      className={props.checked ? "settings-option active" : "settings-option"}
    >
      <input
        checked={props.checked}
        data-settings-field={props.field}
        disabled={disabled}
        onChange={(event) =>
          props.controller.onFieldChange(
            props.field,
            String(event.currentTarget.checked),
          )
        }
        type="checkbox"
      />
      <span>
        <strong className="settings-option-title">
          <Icon aria-hidden="true" size={21} strokeWidth={1.75} />
          {props.label}
        </strong>
        <small>{props.description}</small>
      </span>
    </label>
  );
}

function ApprovalDecisionToggle(props: {
  badge?: string;
  checked: boolean;
  controller: SettingsPageController;
  description: string;
  disabled: boolean;
  field?: LlmApprovalDecisionField;
  label: string;
}): JSX.Element {
  const Icon =
    props.field === "llm_approval_deny_enabled"
      ? ShieldX
      : props.field === "llm_approval_escalate_enabled"
        ? UserRoundCheck
        : ShieldCheck;
  const className = [
    "settings-option",
    props.checked ? "active" : "",
    props.disabled ? "settings-option-locked" : "",
  ]
    .filter(Boolean)
    .join(" ");
  return (
    <label className={className}>
      <input
        checked={props.checked}
        data-settings-field={props.field ?? "llm_approval_allow_enabled"}
        disabled={
          props.controller.isLoading ||
          props.controller.isSaving ||
          props.disabled
        }
        onChange={(event) => {
          if (props.field) {
            props.controller.onFieldChange(
              props.field,
              String(event.currentTarget.checked),
            );
          }
        }}
        type="checkbox"
      />
      <span>
        <strong className="settings-option-title">
          <Icon aria-hidden="true" size={21} strokeWidth={1.75} />
          {props.label}
          {props.badge ? (
            <span className="settings-option-badge">{props.badge}</span>
          ) : null}
        </strong>
        <small>{props.description}</small>
      </span>
    </label>
  );
}

function PolicySettings(props: {
  controller: SettingsPageController;
  locale: Locale;
  settings: DesktopSettings;
}): JSX.Element {
  const { controller, locale, settings } = props;
  const disabled = controller.isLoading || controller.isSaving;
  const options = [
    ["manual_only", "policyHumanReviewDesc"],
    ["assisted", "policyAssistDesc"],
    ["llm_automatic", "policyAutomaticDesc"],
  ] as const;
  const passwordPermissions = [
    [
      "llm_auto_approve_password_edits",
      "settingsPasswordAutoEdit",
      "settingsPasswordAutoEditHelp",
    ],
    [
      "llm_auto_approve_password_renames",
      "settingsPasswordAutoRename",
      "settingsPasswordAutoRenameHelp",
    ],
    [
      "llm_auto_approve_password_refreshes",
      "settingsPasswordAutoRefresh",
      "settingsPasswordAutoRefreshHelp",
    ],
    [
      "llm_auto_approve_password_deletes",
      "settingsPasswordAutoDelete",
      "settingsPasswordAutoDeleteHelp",
    ],
  ] as const;
  const automaticPolicyEnabled =
    settings.default_policy_mode === "llm_automatic";
  return (
    <>
      <SettingsSection
        description={t(locale, "settingsPolicyHelp")}
        testId="settings-policy-section"
        title={t(locale, "settingsPolicyTitle")}
      >
        <div className="settings-option-list">
          {options.map(([value, description]) => (
            <SettingsOption
              checked={settings.default_policy_mode === value}
              description={t(locale, description)}
              disabled={disabled}
              key={value}
              label={translateCode(locale, value)}
              name="defaultPolicyMode"
              onChange={() => controller.onPolicyModeChange(value)}
              value={value}
            />
          ))}
        </div>
      </SettingsSection>
      <SettingsSection
        description={t(locale, "settingsLlmDecisionHelp")}
        testId="settings-llm-decisions-section"
        title={t(locale, "settingsLlmDecisionTitle")}
      >
        <div className="settings-option-list">
          <ApprovalDecisionToggle
            badge={t(locale, "settingsLlmDecisionRequired")}
            checked
            controller={controller}
            description={t(locale, "settingsLlmDecisionAllowHelp")}
            disabled
            label={t(locale, "settingsLlmDecisionAllow")}
          />
          <ApprovalDecisionToggle
            badge={
              settings.llm_approval_deny_enabled &&
              !settings.llm_approval_escalate_enabled
                ? t(locale, "settingsLlmDecisionLastFallback")
                : undefined
            }
            checked={settings.llm_approval_deny_enabled}
            controller={controller}
            description={t(locale, "settingsLlmDecisionDenyHelp")}
            disabled={
              settings.llm_approval_deny_enabled &&
              !settings.llm_approval_escalate_enabled
            }
            field="llm_approval_deny_enabled"
            label={t(locale, "settingsLlmDecisionDeny")}
          />
          <ApprovalDecisionToggle
            badge={
              settings.llm_approval_escalate_enabled &&
              !settings.llm_approval_deny_enabled
                ? t(locale, "settingsLlmDecisionLastFallback")
                : undefined
            }
            checked={settings.llm_approval_escalate_enabled}
            controller={controller}
            description={t(locale, "settingsLlmDecisionEscalateHelp")}
            disabled={
              settings.llm_approval_escalate_enabled &&
              !settings.llm_approval_deny_enabled
            }
            field="llm_approval_escalate_enabled"
            label={t(locale, "settingsLlmDecisionEscalate")}
          />
        </div>
        <p className="settings-help">
          {t(locale, "settingsLlmDecisionConstraint")}
        </p>
      </SettingsSection>
      <SettingsSection
        description={t(locale, "settingsPasswordAutoApprovalHelp")}
        testId="settings-password-auto-approval-section"
        title={t(locale, "settingsPasswordAutoApprovalTitle")}
      >
        <div className="settings-option-list">
          {passwordPermissions.map(([field, label, description]) => (
            <SettingsToggle
              checked={settings[field]}
              controller={controller}
              description={t(locale, description)}
              field={field}
              key={field}
              label={t(locale, label)}
              policyEnabled={automaticPolicyEnabled}
            />
          ))}
        </div>
        {!automaticPolicyEnabled ? (
          <p className="settings-help">
            {t(locale, "settingsPasswordAutoApprovalDisabled")}
          </p>
        ) : null}
      </SettingsSection>
      <SettingsSection
        description={t(locale, "settingsLlmHelp")}
        testId="settings-llm-section"
        title={t(locale, "settingsLlmTitle")}
      >
        <div className="settings-form-grid">
          <SettingsInput
            controller={controller}
            field="request_template"
            label={t(locale, "settingsRequestTemplate")}
            textarea
            value={settings.request_template}
          />
          <SettingsInput
            controller={controller}
            field="llm_advice_template"
            label={t(locale, "settingsLlmAdviceTemplate")}
            textarea
            value={settings.llm_advice_template}
          />
        </div>
        <p className="settings-help">
          {t(locale, "settingsTemplateVariables")}
        </p>
      </SettingsSection>
    </>
  );
}

function normalizeProbeProfile(profile: AcpProfile): AcpProfile {
  if (profile.version_mode === "latest") {
    return { ...profile, version: null, program: null, args: [] };
  }
  if (profile.version_mode === "pinned") {
    return {
      ...profile,
      version: profile.version?.trim() || null,
      program: null,
      args: [],
    };
  }
  return { ...profile, agent_kind: "custom", version: null };
}

function AcpSettings(props: {
  announceValidation?: boolean;
  customInAdvanced?: boolean;
  controller: SettingsPageController;
  locale: Locale;
  settings: DesktopSettings;
}): JSX.Element {
  const { controller, locale, settings } = props;
  const [probe, setProbe] = useState<AcpProbeResult | null>(null);
  const [probeError, setProbeError] = useState<string | null>(null);
  const [testing, setTesting] = useState(false);
  const profile = settings.acp_profile;
  const disabled = controller.isLoading || controller.isSaving;
  const summary = buildAcpProgramSummary(settings);
  const pinnedInvalid =
    controller.validationMessage !== null && profile.version_mode === "pinned";
  const customProgramInvalid =
    controller.validationMessage !== null &&
    profile.version_mode === "custom" &&
    !profile.program?.trim();
  const customFields =
    profile.version_mode === "custom" ? (
      <>
        <label className="settings-field">
          <span>{t(locale, "settingsAcpCustomProgram")}</span>
          <input
            aria-describedby={
              customProgramInvalid ? "settings-validation-message" : undefined
            }
            aria-invalid={customProgramInvalid ? "true" : undefined}
            data-settings-validation-target={
              customProgramInvalid ? "true" : undefined
            }
            disabled={disabled}
            onChange={(event) =>
              updateProfile({
                ...profile,
                program: event.currentTarget.value,
              })
            }
            value={profile.program ?? ""}
          />
        </label>
        <label className="settings-field settings-field-wide">
          <span>{t(locale, "settingsAcpCustomArgs")}</span>
          <textarea
            disabled={disabled}
            onChange={(event) =>
              updateProfile({
                ...profile,
                args: event.currentTarget.value.split("\n"),
              })
            }
            rows={4}
            value={(profile.args ?? []).join("\n")}
          />
        </label>
      </>
    ) : null;

  function updateProfile(next: AcpProfile): void {
    setProbe(null);
    setProbeError(null);
    controller.onAcpProfileChange(next);
  }

  async function testConnection(): Promise<void> {
    setTesting(true);
    setProbe(null);
    setProbeError(null);
    try {
      setProbe(
        await invoke<AcpProbeResult>("test_acp_connection", {
          profile: normalizeProbeProfile(profile),
        }),
      );
    } catch (reason) {
      setProbeError(reason instanceof Error ? reason.message : String(reason));
    } finally {
      setTesting(false);
    }
  }

  return (
    <SettingsSection
      description={t(locale, "providerAcpDesc")}
      id="settings-acp-section"
      testId="settings-acp-section"
      title={t(locale, "settingsAcpTitle")}
    >
      <div className="settings-form-grid">
        <ChoiceGroup
          className="settings-field"
          label={
            <>
              <span>{t(locale, "settingsAcpAgent")}</span>
            </>
          }
          disabled={disabled}
          onChange={(value) => {
            const agentKind = value as AcpProfile["agent_kind"];
            updateProfile({
              ...profile,
              agent_kind: agentKind,
              session_options: {},
              version_mode:
                agentKind === "custom"
                  ? "custom"
                  : profile.version_mode === "custom"
                    ? "latest"
                    : profile.version_mode,
            });
          }}
          value={profile.agent_kind}
          options={[
            { value: "codex", icon: Terminal, label: <>Codex</> },
            { value: "claude_code", icon: Sparkles, label: <>Claude Code</> },
            { value: "open_code", icon: Code2, label: <>OpenCode</> },
            {
              value: "custom",
              icon: Settings2,
              label: <>{t(locale, "settingsAcpCustom")}</>,
            },
          ]}
        />
        <ChoiceGroup
          className="settings-field"
          label={t(locale, "settingsAcpVersionPolicy")}
          data-testid="settings-acp-version-mode"
          disabled={disabled || profile.agent_kind === "custom"}
          onChange={(value) =>
            updateProfile({
              ...profile,
              version_mode: value as AcpProfile["version_mode"],
            })
          }
          value={profile.version_mode}
          options={
            profile.agent_kind === "custom"
              ? [{ value: "custom", label: t(locale, "settingsAcpCustom") }]
              : [
                  {
                    value: "latest",
                    icon: RefreshCw,
                    label: t(locale, "settingsAcpLatest"),
                  },
                  {
                    value: "pinned",
                    icon: Pin,
                    label: t(locale, "settingsAcpPinned"),
                  },
                ]
          }
        />
        {profile.version_mode === "pinned" ? (
          <label className="settings-field">
            <span>{t(locale, "settingsAcpExactVersion")}</span>
            <input
              aria-describedby={
                pinnedInvalid ? "settings-validation-message" : undefined
              }
              aria-invalid={pinnedInvalid ? "true" : undefined}
              data-settings-validation-target={
                pinnedInvalid ? "true" : undefined
              }
              disabled={disabled}
              onChange={(event) =>
                updateProfile({
                  ...profile,
                  version: event.currentTarget.value,
                })
              }
              placeholder="1.2.3"
              value={profile.version ?? ""}
            />
          </label>
        ) : null}
        {props.customInAdvanced && customFields ? (
          <details className="agents-advanced settings-field-wide">
            <summary>{locale === "zh-CN" ? "高级" : "Advanced"}</summary>
            <div className="settings-form-grid">{customFields}</div>
          </details>
        ) : (
          customFields
        )}
        <SettingsInput
          controller={controller}
          field="acp_timeout_secs"
          label={t(locale, "acpTimeout")}
          min={1}
          step={1}
          type="number"
          value={settings.acp_timeout_secs}
        />
      </div>
      <AcpSessionOptions
        profile={normalizeProbeProfile(profile)}
        disabled={disabled}
        zh={locale === "zh-CN"}
        onChange={updateProfile}
      />
      <dl className="settings-runtime-summary">
        <div>
          <dt>{t(locale, "settingsAcpConfiguredCommand")}</dt>
          <dd>
            <code>{summary.currentCommand}</code>
          </dd>
        </div>
      </dl>
      {controller.validationMessage ? (
        <p
          className="workspace-alert"
          data-testid="settings-validation-error"
          id="settings-validation-message"
          role={props.announceValidation === false ? undefined : "alert"}
        >
          {controller.validationMessage}
        </p>
      ) : null}
      {probeError ? (
        <p className="workspace-alert" role="alert">
          {probeError}
        </p>
      ) : null}
      {probe ? <AcpProbeDetails probe={probe} zh={locale === "zh-CN"} /> : null}
      <button
        disabled={disabled || testing || controller.validationMessage !== null}
        onClick={() => void testConnection()}
        type="button"
      >
        {testing
          ? t(locale, "settingsAcpTesting")
          : t(locale, "settingsAcpTest")}
      </button>
    </SettingsSection>
  );
}

function settingsStatusText(
  controller: SettingsPageController,
  locale: Locale,
): string {
  if (controller.isLoading) {
    return t(locale, "settingsLoading");
  }
  if (
    controller.errorMessage &&
    !controller.settings &&
    !controller.settingsDraft
  ) {
    return t(locale, "settingsStatusUnavailable");
  }
  if (controller.errorMessage) {
    return t(locale, "settingsStatusError");
  }
  if (!controller.settings) {
    return t(locale, "settingsStatusUnavailable");
  }
  if (controller.isSaving) {
    return t(locale, "saving");
  }
  if (controller.validationMessage) {
    return t(locale, "settingsSaveInvalid");
  }
  return controller.hasUnsavedChanges
    ? t(locale, "settingsSaveDirty")
    : t(locale, "settingsSaveSaved");
}

function policiesStatusText(
  controller: SettingsPageController,
  locale: Locale,
): string {
  if (controller.isLoading) {
    return locale === "zh-CN" ? "正在加载策略…" : "Loading policies…";
  }
  if (
    !controller.settings ||
    (controller.errorMessage && !controller.settingsDraft)
  ) {
    return locale === "zh-CN" ? "策略不可用" : "Policies unavailable";
  }
  if (controller.errorMessage) {
    return locale === "zh-CN" ? "策略需要处理" : "Policies need attention";
  }
  if (controller.isSaving) {
    return t(locale, "saving");
  }
  if (controller.validationMessage) {
    return locale === "zh-CN"
      ? "智能体配置需要处理"
      : "Agent configuration needs attention";
  }
  return controller.hasUnsavedChanges
    ? t(locale, "settingsSaveDirty")
    : t(locale, "settingsSaveSaved");
}

function PoliciesPage(props: {
  controller: SettingsPageController;
  locale: Locale;
  onNavigate: (view: WorkspaceView) => void;
}): JSX.Element {
  const { controller, locale } = props;
  const settings = controller.settingsDraft ?? controller.settings;
  const saveStatus = policiesStatusText(controller, locale);

  return (
    <section className="workspace-page policies-page">
      <PageHeader
        icon={ShieldCheck}
        description={
          locale === "zh-CN"
            ? "定义请求如何审批、自动化可以执行哪些变更，以及长期保留哪些上下文。"
            : "Define how requests are reviewed, what automation may change, and which context is remembered."
        }
        eyebrow={locale === "zh-CN" ? "决策策略" : "DECISION POLICY"}
        title={locale === "zh-CN" ? "策略" : "Policies"}
      />
      {!settings && controller.errorMessage ? (
        <ErrorState
          action={
            <button onClick={controller.onReload} type="button">
              {t(locale, "settingsRetry")}
            </button>
          }
          description={controller.errorMessage}
          eyebrow={locale === "zh-CN" ? "策略不可用" : "POLICIES UNAVAILABLE"}
          title={
            locale === "zh-CN" ? "无法加载策略" : "Policies could not be loaded"
          }
        />
      ) : !settings ? (
        <p className="settings-loading" role="status">
          {locale === "zh-CN" ? "正在加载策略…" : "Loading policies…"}
        </p>
      ) : (
        <form
          className="policies-form"
          data-testid="policies-page-form"
          onSubmit={(event) => {
            event.preventDefault();
            if (controller.canSave) controller.onSave();
          }}
        >
          <div className="policies-body" data-testid="policies-page-body">
            {controller.errorMessage ? (
              <p
                className="workspace-alert"
                data-testid="settings-error-banner"
                role="alert"
              >
                {controller.errorMessage}
              </p>
            ) : null}
            {controller.noticeMessage ? (
              <p
                className="settings-notice"
                data-testid="settings-notice-banner"
                role="status"
              >
                {controller.noticeMessage}
              </p>
            ) : null}
            <div className="policies-content">
              <PolicySettings
                controller={controller}
                locale={locale}
                settings={settings}
              />
            </div>
          </div>
          <div className="settings-save-bar" data-testid="settings-save-bar">
            {controller.validationMessage ? (
              <div className="settings-save-validation">
                <span data-testid="settings-save-validation" role="alert">
                  {controller.validationMessage}
                </span>
                <button
                  data-testid="open-agents-settings"
                  onClick={() => props.onNavigate("agents")}
                  type="button"
                >
                  {locale === "zh-CN"
                    ? "打开智能体与模型"
                    : "Open Agents & Models"}
                </button>
              </div>
            ) : (
              <span aria-live="polite">{saveStatus}</span>
            )}
            <button
              className="primary"
              data-testid="save-settings-button"
              disabled={!controller.canSave}
              type="submit"
            >
              {controller.isSaving ? t(locale, "saving") : t(locale, "save")}
            </button>
          </div>
        </form>
      )}
    </section>
  );
}
