import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { Bot, Check, ExternalLink, ShieldAlert, X } from "lucide-react";
import { SplitPane } from "./desktop/PagePrimitives";
import { useCallback, useEffect, useRef, useState, type JSX } from "react";

import {
  callChainEntryName,
  callChainEntryPath,
  codeAgentBoundaryIndex,
} from "../callChainPresentation";
import { getPreviewHighlightResult } from "../codePreview";
import type {
  CallChainEntry,
  CredentialExposureReport,
  DecisionCommand,
  SanitizedPromptContext,
} from "../types";
import {
  ExposureRadar,
  defaultExposurePolicy,
  type CredentialExposurePolicy,
  type ExposureSurface,
} from "./ExposurePolicy";
import {
  PreciseEvidencePanel,
  displayAnnotationsByNode,
} from "./desktop/OperationsPages";
import "./desktop/password-vault.css";
import { ApprovalMarkdown } from "./ApprovalMarkdown";

const APPROVAL_QUEUE_EVENT = "plankton://approval-queue";

export type CompactApprovalRequest = {
  locale?: string;
  created_at?: string;
  resource_metadata?: Record<string, string>;
  id: string;
  resource: string;
  requested_by: string;
  reason: string;
  context: string;
  call_chain: CallChainEntry[];
  suggestion: string;
  suggested_decision: string;
  risk_score: number | null;
  exposure_report?: CredentialExposureReport | null;
  inline_sources?: SanitizedPromptContext["inline_sources"];
  exposure_policy?: CredentialExposurePolicy;
  approval_status: "pending" | "approved" | "rejected";
  evaluation_state:
    | "not_required"
    | "queued"
    | "running"
    | "completed"
    | "failed"
    | "interrupted"
    | "superseded";
  review_progress?: {
    state: "running" | "complete" | "partial" | "failed";
    completed_units: number;
    total_units: number;
    error?: string | null;
    updated_at: string;
  } | null;
};

export type CompactApprovalApi = {
  loadRequests: () => Promise<CompactApprovalRequest[]>;
  decide: (
    requestId: string,
    decision: DecisionCommand,
    note: string | null,
  ) => Promise<void>;
  openFullDetails: (requestId: string) => Promise<void>;
  close: () => Promise<void>;
  subscribe: (onQueueChanged: () => void) => Promise<() => void>;
};

const defaultApi: CompactApprovalApi = {
  async loadRequests() {
    return invoke<CompactApprovalRequest[]>("compact_approval_requests");
  },
  async decide(requestId, decision, note) {
    await invoke(decision, { requestId, note });
  },
  async openFullDetails(requestId) {
    await invoke("open_full_request_details", { requestId });
  },
  async close() {
    await getCurrentWindow().hide();
  },
  async subscribe(onQueueChanged) {
    return listen(APPROVAL_QUEUE_EVENT, onQueueChanged);
  },
};

function messageFrom(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

function decisionLabel(value: string, zh = false): string {
  switch (value) {
    case "allow":
      return zh ? "模型建议批准" : "Recommendation: approve";
    case "deny":
      return zh ? "模型建议拒绝" : "Recommendation: reject";
    case "escalate":
      return zh ? "模型建议人工复核" : "Recommendation: inspect carefully";
    default:
      return zh ? "等待人工决定" : "Human decision required";
  }
}

function CompactCallChain({
  callChain,
  exposureReport,
  inlineSources,
  zh = false,
}: {
  callChain: CallChainEntry[];
  exposureReport?: CredentialExposureReport | null;
  inlineSources: SanitizedPromptContext["inline_sources"];
  zh?: boolean;
}): JSX.Element {
  const agentBoundary = codeAgentBoundaryIndex(callChain);
  const annotationsByNode = displayAnnotationsByNode(callChain, exposureReport);

  return (
    <section aria-label="Call chain" className="compact-approval__call-chain">
      <header>
        <div>
          <Bot aria-hidden="true" size={15} strokeWidth={1.75} />
          <strong>{zh ? "调用链与证据" : "Call chain"}</strong>
        </div>
        <span>
          {zh
            ? `${callChain.length} 层`
            : `${callChain.length} ${callChain.length === 1 ? "step" : "steps"}`}
        </span>
      </header>
      {callChain.length > 0 ? (
        <ol>
          {callChain.map((entry, index) => {
            const agentScoped = agentBoundary >= 0 && index >= agentBoundary;
            const agentStart = index === agentBoundary;
            const argumentsList = (entry.argv ?? []).filter(Boolean);
            const guidance = exposureReport?.node_assessments.find(
              (assessment) => assessment.node_index === index,
            );
            const annotations = annotationsByNode.get(index) ?? [];
            return (
              <li
                className={agentScoped ? "agent-scoped" : undefined}
                data-agent-start={agentStart}
                key={`${index}:${callChainEntryPath(entry)}`}
              >
                <span>{index + 1}</span>
                <div>
                  <strong>{callChainEntryName(entry)}</strong>
                  <code>{callChainEntryPath(entry)}</code>
                  {argumentsList.length > 0 ? (
                    <div className="compact-approval__call-chain-args">
                      {argumentsList.map((argument, argumentIndex) => {
                        const highlighted = getPreviewHighlightResult(
                          null,
                          argument,
                        );
                        return (
                          <code
                            data-language={highlighted.label}
                            key={`${index}:argument:${argumentIndex}`}
                            {...(highlighted.highlighted
                              ? {
                                  dangerouslySetInnerHTML: {
                                    __html: highlighted.html,
                                  },
                                }
                              : { children: argument })}
                          />
                        );
                      })}
                    </div>
                  ) : null}
                  {guidance ? (
                    <aside className="compact-approval__call-chain-help">
                      <small>{zh ? "节点说明" : "Automatic guidance"}</small>
                      <ApprovalMarkdown>{guidance.summary}</ApprovalMarkdown>
                      {guidance.capabilities.length > 0 ? (
                        <span>{guidance.capabilities.join(" · ")}</span>
                      ) : null}
                    </aside>
                  ) : null}
                  {annotations.length > 0 ? (
                    <div className="compact-approval__reference-scope desktop-workspace">
                      <div className="workspace-page">
                        <PreciseEvidencePanel
                          annotations={annotations}
                          inlineSources={inlineSources}
                          node={entry}
                          nodeIndex={index}
                          zh={zh}
                        />
                      </div>
                    </div>
                  ) : null}
                </div>
              </li>
            );
          })}
        </ol>
      ) : (
        <p>
          {zh ? "未记录调用链上下文。" : "No call-chain context was captured."}
        </p>
      )}
    </section>
  );
}

function CompactEvaluationProgress({
  request,
}: {
  request: CompactApprovalRequest;
}): JSX.Element | null {
  const progress = request.review_progress;
  const initialRunning =
    request.evaluation_state === "queued" ||
    request.evaluation_state === "running";
  if (!initialRunning && !progress) return null;
  const completed = progress?.completed_units ?? 0;
  const total = progress?.total_units ?? 1;
  const percent = Math.min(
    100,
    Math.round((completed / Math.max(1, total)) * 100),
  );
  const active = initialRunning || progress?.state === "running";
  const showMessage =
    Boolean(progress?.error) ||
    progress?.state === "partial" ||
    progress?.state === "failed";
  return (
    <section
      aria-live="polite"
      className="compact-approval__evaluation"
      data-has-message={showMessage ? "true" : "false"}
      data-state={active ? "running" : (progress?.state ?? "complete")}
      role="status"
    >
      {showMessage ? (
        <div>
          <strong>
            {progress?.state === "running"
              ? "Guidance ready · repairing evidence"
              : progress?.state === "partial"
                ? "Evidence details partially complete"
                : "Evidence detail generation stopped"}
          </strong>
          <span>{`${completed} / ${total}`}</span>
        </div>
      ) : null}
      <div
        aria-label="Automatic guidance progress"
        aria-valuemax={100}
        aria-valuemin={0}
        aria-valuenow={initialRunning ? undefined : percent}
        className="compact-approval__evaluation-track"
        role="progressbar"
      >
        <i style={{ width: initialRunning ? "34%" : `${percent}%` }} />
      </div>
      {progress?.error ? <small>{progress.error}</small> : null}
    </section>
  );
}

export function CompactApproval(props: {
  api?: CompactApprovalApi;
}): JSX.Element {
  const api = props.api ?? defaultApi;
  const headingRef = useRef<HTMLHeadingElement | null>(null);
  const [requests, setRequests] = useState<CompactApprovalRequest[]>([]);
  const [selectedRequestId, setSelectedRequestId] = useState<string | null>(
    null,
  );
  const [note, setNote] = useState("");
  const [isLoading, setIsLoading] = useState(true);
  const [isSubmitting, setIsSubmitting] = useState(false);
  const [errorMessage, setErrorMessage] = useState<string | null>(null);
  const current =
    requests.find((request) => request.id === selectedRequestId) ??
    requests[0] ??
    null;
  const currentIndex = current
    ? requests.findIndex((request) => request.id === current.id)
    : -1;
  const decisionRecorded = current?.approval_status !== "pending";
  const zh = current?.locale === "zh-CN";
  const metadata = current?.resource_metadata ?? {};
  const itemTitle =
    metadata.item_title ||
    metadata.field_label ||
    (zh ? "凭据访问请求" : "Credential access request");
  const collection = [
    metadata.vault,
    metadata.collection || metadata.group || metadata.section,
  ]
    .filter(Boolean)
    .join(" / ");

  const loadRequests = useCallback(async (): Promise<
    CompactApprovalRequest[] | null
  > => {
    setIsLoading(true);
    setErrorMessage(null);
    try {
      const loaded = await api.loadRequests();
      const pending = loaded
        .filter((request) => request.approval_status === "pending")
        .sort((a, b) => (b.created_at ?? "").localeCompare(a.created_at ?? ""));
      setRequests(pending);
      setSelectedRequestId((selected) =>
        pending.some((request) => request.id === selected)
          ? selected
          : (pending[0]?.id ?? null),
      );
      return pending;
    } catch (error) {
      setErrorMessage(messageFrom(error));
      return null;
    } finally {
      setIsLoading(false);
    }
  }, [api]);

  useEffect(() => {
    headingRef.current?.focus();
  }, []);

  useEffect(() => {
    void loadRequests();
    let disposed = false;
    let unlisten: (() => void) | null = null;
    void api
      .subscribe(() => {
        void loadRequests();
      })
      .then((dispose) => {
        if (disposed) {
          dispose();
        } else {
          unlisten = dispose;
        }
      })
      .catch((error: unknown) => {
        setErrorMessage(messageFrom(error));
      });
    return () => {
      disposed = true;
      unlisten?.();
    };
  }, [api, loadRequests]);

  useEffect(() => {
    const shouldPoll = requests.some(
      (request) =>
        request.evaluation_state === "queued" ||
        request.evaluation_state === "running" ||
        request.review_progress?.state === "running",
    );
    if (!shouldPoll) return;
    const timer = window.setInterval(() => {
      void api
        .loadRequests()
        .then((loaded) => {
          const pending = loaded
            .filter((request) => request.approval_status === "pending")
            .sort((a, b) =>
              (b.created_at ?? "").localeCompare(a.created_at ?? ""),
            );
          setRequests(pending);
          setSelectedRequestId((selected) =>
            pending.some((request) => request.id === selected)
              ? selected
              : (pending[0]?.id ?? null),
          );
        })
        .catch((error: unknown) => setErrorMessage(messageFrom(error)));
    }, 500);
    return () => window.clearInterval(timer);
  }, [api, requests]);

  useEffect(() => {
    function handleKeyDown(event: KeyboardEvent): void {
      if (event.key === "Escape") {
        event.preventDefault();
        void api.close().catch((error: unknown) => {
          setErrorMessage(messageFrom(error));
        });
      }
    }
    window.addEventListener("keydown", handleKeyDown);
    return () => window.removeEventListener("keydown", handleKeyDown);
  }, [api]);

  useEffect(() => {
    setNote("");
  }, [current?.id]);

  async function close(): Promise<void> {
    try {
      await api.close();
    } catch (error) {
      setErrorMessage(messageFrom(error));
    }
  }

  async function decide(decision: DecisionCommand): Promise<void> {
    if (!current || isSubmitting || current.approval_status !== "pending") {
      return;
    }
    setIsSubmitting(true);
    setErrorMessage(null);
    try {
      await api.decide(current.id, decision, note.trim() || null);
      setNote("");
      const remaining = await loadRequests();
      const remainingAfterDecision = (remaining ?? requests).filter(
        (request) => request.id !== current.id,
      );
      setRequests(remainingAfterDecision);
      if (remainingAfterDecision.length === 0) {
        await api.close();
      }
    } catch (error) {
      setErrorMessage(messageFrom(error));
    } finally {
      setIsSubmitting(false);
    }
  }

  async function openFullDetails(): Promise<void> {
    if (!current || isSubmitting) {
      return;
    }
    setIsSubmitting(true);
    setErrorMessage(null);
    try {
      await api.openFullDetails(current.id);
    } catch (error) {
      setErrorMessage(messageFrom(error));
    } finally {
      setIsSubmitting(false);
    }
  }

  return (
    <main className="compact-approval">
      <style>{compactApprovalStyles}</style>
      <header className="compact-approval__header">
        <div>
          <p className="compact-approval__eyebrow">
            {zh ? "人工审批" : "Human review"}
          </p>
          <h1 ref={headingRef} tabIndex={-1}>
            {zh ? "需要你作出决定" : "Approval required"}
          </h1>
        </div>
        <button
          aria-label={zh ? "关闭" : "Close"}
          title={zh ? "关闭审批窗口" : "Close approval window"}
          className="compact-approval__icon-button"
          onClick={() => {
            void close();
          }}
          type="button"
        >
          <X aria-hidden="true" size={18} strokeWidth={1.75} />
          <span className="compact-approval__visually-hidden">Close</span>
        </button>
      </header>

      {errorMessage ? (
        <section className="compact-approval__error" role="alert">
          <strong>Approval data could not be loaded.</strong>
          <span>{errorMessage}</span>
          <button
            onClick={() => {
              void loadRequests();
            }}
            type="button"
          >
            Try again
          </button>
        </section>
      ) : null}

      {isLoading && !current ? (
        <section
          aria-live="polite"
          className="compact-approval__state"
          role="status"
        >
          Loading approval context…
        </section>
      ) : null}

      {!isLoading && !current && !errorMessage ? (
        <section className="compact-approval__state">
          <ShieldAlert aria-hidden="true" size={24} strokeWidth={1.75} />
          <h2>No approval is waiting</h2>
          <p>The request may already have been resolved in another window.</p>
          <button
            onClick={() => {
              void close();
            }}
            type="button"
          >
            Close
          </button>
        </section>
      ) : null}

      {current ? (
        <div
          className="compact-approval__body"
          data-has-queue={requests.length > 1}
        >
          <SplitPane
            listVisible={requests.length > 1}
            resizable
            initialListWidth={220}
            minListWidth={160}
            minDetailWidth={480}
            storageKey="plankton-compact-request-list-width"
            listLabel={zh ? "待审批请求" : "Pending approvals"}
            detailLabel={zh ? "审批内容" : "Approval details"}
            list={
              <nav
                aria-label="Pending approvals"
                className="compact-approval__switcher"
              >
                <strong>
                  {requests.length} {zh ? "项待审批" : "waiting"}
                </strong>
                {requests.map((request, index) => (
                  <button
                    aria-label={`Open approval ${index + 1}: ${request.resource_metadata?.item_title || request.reason}`}
                    aria-pressed={request.id === current.id}
                    disabled={isSubmitting}
                    key={request.id}
                    onClick={() => setSelectedRequestId(request.id)}
                    type="button"
                  >
                    <small>
                      {[
                        request.resource_metadata?.vault,
                        request.resource_metadata?.section,
                      ]
                        .filter(Boolean)
                        .join(" / ")}
                    </small>
                    <strong>
                      {request.resource_metadata?.item_title ||
                        (zh ? "凭据访问请求" : "Credential access request")}
                    </strong>
                    <span className="compact-approval__switcher-field">
                      {request.resource_metadata?.field_label ||
                        request.resource_metadata?.field_key}
                    </span>
                    <p>{request.reason}</p>
                    <small>{request.requested_by}</small>
                  </button>
                ))}
              </nav>
            }
            detail={
              <div className="compact-approval__review">
                <div className="compact-approval__review-scroll">
                  <div
                    className="compact-approval__rail"
                    aria-label="Approval state"
                  >
                    <span>{zh ? "请求已接收" : "Request"}</span>
                    <i aria-hidden="true" />
                    <span>{zh ? "评估" : "Evaluation"}</span>
                    <i aria-hidden="true" />
                    <strong>{zh ? "人工决定" : "Human decision"}</strong>
                  </div>

                  <section className="compact-approval__request">
                    <div className="compact-approval__queue">
                      <span>
                        {collection || (zh ? "凭据访问" : "Credential access")}
                      </span>
                      <strong>{`${currentIndex + 1} of ${requests.length}`}</strong>
                    </div>
                    <h2>{itemTitle}</h2>
                    <p className="compact-approval__field">
                      {metadata.field_label || metadata.field_key}
                    </p>
                    <p className="compact-approval__intent">{current.reason}</p>
                    <details className="compact-approval__request-details">
                      <summary>{zh ? "请求详情" : "Request details"}</summary>
                      <details className="compact-approval__identifiers">
                        <summary>
                          {zh ? "资源与请求 ID" : "Resource and request IDs"}
                        </summary>
                        <code>{current.resource}</code>
                        <code>{current.id}</code>
                      </details>
                      <dl>
                        <div>
                          <dt>{zh ? "请求者" : "Requester"}</dt>
                          <dd>{current.requested_by}</dd>
                        </div>
                        <div>
                          <dt>{zh ? "请求意图" : "Reason"}</dt>
                          <dd>{current.reason}</dd>
                        </div>
                        <div>
                          <dt>{zh ? "执行脚本" : "Context"}</dt>
                          <dd className="compact-approval__mono">
                            {current.context}
                          </dd>
                        </div>
                      </dl>
                    </details>
                  </section>

                  <section className="compact-approval__suggestion">
                    <div className="compact-approval__guidance-text">
                      <div>
                        <strong>
                          <small>
                            {zh ? "评估建议" : "Automatic guidance"}
                          </small>
                          {decisionLabel(current.suggested_decision, zh)}
                        </strong>
                        <span>
                          {current.risk_score === null
                            ? zh
                              ? "暂无风险评分"
                              : "Risk not scored"
                            : `${zh ? "风险" : "Risk"} ${current.risk_score} / 100`}
                        </span>
                      </div>
                      <ApprovalMarkdown>
                        {zh &&
                        current.suggestion ===
                          "No automatic recommendation is available."
                          ? "此请求由人工判断，暂无模型建议。"
                          : current.suggestion}
                      </ApprovalMarkdown>
                      {current.exposure_report?.surfaces
                        .filter(
                          (surface) =>
                            surface.evidence_state === "unknown" ||
                            surface.actual_level >
                              ((
                                current.exposure_policy ??
                                defaultExposurePolicy()
                              ).surfaces.find(
                                (limit) => limit.surface === surface.surface,
                              )?.max_level ?? 0),
                        )
                        .map((surface) => (
                          <div
                            className="approval-exposure-attention"
                            key={surface.surface}
                          >
                            <strong>
                              {
                                {
                                  llm_context: zh ? "LLM 回显" : "LLM context",
                                  network: zh ? "网络发送" : "Network",
                                  local_persistence: zh
                                    ? "本地持久化"
                                    : "Local storage",
                                  terminal_log: zh
                                    ? "终端 / 日志"
                                    : "Terminal / logs",
                                  process_propagation: zh
                                    ? "进程传递"
                                    : "Process handoff",
                                }[surface.surface]
                              }{" "}
                              ·{" "}
                              {surface.evidence_state === "unknown"
                                ? zh
                                  ? "证据不足"
                                  : "Insufficient evidence"
                                : zh
                                  ? "超出允许范围"
                                  : "Over allowed limit"}
                            </strong>
                            <ApprovalMarkdown>
                              {surface.summary}
                            </ApprovalMarkdown>
                          </div>
                        ))}
                    </div>
                    {current.exposure_report ? (
                      <ExposureRadar
                        compact
                        breachedSurfaces={current.exposure_report.surfaces
                          .filter(
                            (surface) =>
                              surface.actual_level >
                              ((
                                current.exposure_policy ??
                                defaultExposurePolicy()
                              ).surfaces.find(
                                (entry) => entry.surface === surface.surface,
                              )?.max_level ?? 0),
                          )
                          .map((surface) => surface.surface as ExposureSurface)}
                        primary={{
                          ...defaultExposurePolicy(),
                          surfaces: defaultExposurePolicy().surfaces.map(
                            (entry) => ({
                              ...entry,
                              max_level:
                                current.exposure_report?.surfaces.find(
                                  (surface) =>
                                    surface.surface === entry.surface,
                                )?.actual_level ?? 2,
                            }),
                          ),
                        }}
                        locale={zh ? "zh-CN" : "en"}
                        primaryLabel={zh ? "实际暴露" : "Observed"}
                        secondary={
                          current.exposure_policy ?? defaultExposurePolicy()
                        }
                        secondaryLabel={zh ? "允许上限" : "Allowed"}
                      />
                    ) : null}
                  </section>
                  <CompactEvaluationProgress request={current} />
                  {current.exposure_report?.chain_summary ? (
                    <details className="compact-approval__chain-summary">
                      <summary>
                        {zh ? "调用链摘要" : "Call-chain summary"}
                      </summary>
                      <ApprovalMarkdown>
                        {current.exposure_report.chain_summary}
                      </ApprovalMarkdown>
                    </details>
                  ) : null}

                  <label className="compact-approval__note">
                    <span>
                      {zh ? "审批备注（可选）" : "Decision note (optional)"}
                    </span>
                    <textarea
                      aria-label="Decision note"
                      disabled={isSubmitting || decisionRecorded}
                      onChange={(event) => setNote(event.currentTarget.value)}
                      placeholder={
                        zh
                          ? "补充备注，保存在审计记录中"
                          : "Add context for the audit record"
                      }
                      rows={2}
                      value={note}
                    />
                  </label>

                  <CompactCallChain
                    callChain={current.call_chain}
                    exposureReport={current.exposure_report}
                    inlineSources={current.inline_sources ?? []}
                    zh={zh}
                  />
                </div>

                <footer className="compact-approval__actions">
                  <button
                    className="compact-approval__reject"
                    disabled={isSubmitting || decisionRecorded}
                    onClick={() => {
                      void decide("reject_request");
                    }}
                    type="button"
                  >
                    <X aria-hidden="true" size={16} strokeWidth={1.75} />
                    {zh ? "拒绝" : "Reject"}
                  </button>
                  <button
                    className="compact-approval__approve"
                    disabled={isSubmitting || decisionRecorded}
                    onClick={() => {
                      void decide("approve_request");
                    }}
                    type="button"
                  >
                    <Check aria-hidden="true" size={16} strokeWidth={1.75} />
                    {zh ? "批准" : "Approve"}
                  </button>
                  <button
                    className="compact-approval__details"
                    disabled={isSubmitting}
                    onClick={() => {
                      void openFullDetails();
                    }}
                    type="button"
                  >
                    {zh ? "打开完整详情" : "Open full details"}
                    <ExternalLink
                      aria-hidden="true"
                      size={16}
                      strokeWidth={1.75}
                    />
                  </button>
                </footer>
              </div>
            }
          />
        </div>
      ) : null}
    </main>
  );
}

const compactApprovalStyles = `
  .compact-approval__request h2 { margin: 0 0 8px; font: 650 20px/1.4 -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif; }
  .compact-approval__field { margin: 0 0 12px; color: #706d67; font-size: 12px; }
  .compact-approval__identifiers { color: #706d67; font-size: 11px; }
  .compact-approval__identifiers code { display: block; margin-top: 8px; overflow-wrap: anywhere; }
  .compact-approval summary { cursor: pointer; }
  .compact-approval__suggestion > .approval-markdown { margin-top: 12px; }
  .compact-approval__chain-summary { margin-top: 12px; font-size: 12px; }
  .compact-approval__chain-summary .approval-markdown { margin-top: 10px; }

  :root {
    color: #171716;
    background: #f4f1ea;
    font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif;
  }
  body {
    min-width: 0;
    min-height: 100vh;
    margin: 0;
    background: #f4f1ea;
    overflow: hidden;
  }
  button, textarea { font: inherit; }
  .compact-approval {
    box-sizing: border-box;
    height: 100vh;
    min-height: 0;
    display: flex;
    flex-direction: column;
    padding: 20px;
    color: #171716;
    background: #f4f1ea;
  }
  .compact-approval *, .compact-approval *::before, .compact-approval *::after {
    box-sizing: border-box;
  }
  .compact-approval__header,
  .compact-approval__queue,
  .compact-approval__guidance-text > div,
  .compact-approval__actions {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
  }
  .compact-approval__header { margin-bottom: 16px; }
  .compact-approval__eyebrow {
    margin: 0 0 4px;
    color: #f2381e;
    font: 700 11px/1.2 ui-monospace, SFMono-Regular, Menlo, monospace;
    letter-spacing: .12em;
    text-transform: uppercase;
  }
  .compact-approval h1 {
    margin: 0;
    font: 650 25px/1.3 -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif;
  }
  .compact-approval h1:focus { outline: none; }
  .compact-approval button {
    min-height: 36px;
    border: 1px solid #171716;
    border-radius: 0;
    padding: 8px 12px;
    color: #171716;
    background: #fffefb;
    cursor: pointer;
  }
  .compact-approval button:focus-visible,
  .compact-approval textarea:focus-visible {
    outline: 2px solid #f2381e;
    outline-offset: 2px;
  }
  .compact-approval button:disabled { cursor: wait; opacity: .58; }
  .compact-approval__icon-button {
    display: grid;
    width: 36px;
    padding: 0;
    place-items: center;
  }
  .compact-approval__visually-hidden {
    position: absolute;
    width: 1px;
    height: 1px;
    overflow: hidden;
    clip: rect(0 0 0 0);
  }
  .compact-approval__body {
    min-width: 0;
    min-height: 0;
    flex: 1 1 auto;
  }
  .compact-approval__body[data-has-queue="true"] {
    display: grid;
    grid-template-columns: 112px minmax(0, 1fr);
    gap: 12px;
  }
  .compact-approval__review {
    min-width: 0;
    min-height: 0;
    height: 100%;
    display: flex;
    flex-direction: column;
    overflow: hidden;
  }
  .compact-approval__review-scroll {
    min-height: 0;
    flex: 1 1 auto;
    display: flex;
    flex-direction: column;
    padding-right: 5px;
    overflow-y: auto;
    overscroll-behavior: contain;
    scrollbar-gutter: stable;
  }
  .compact-approval__switcher {
    min-height: 0;
    display: flex;
    flex-direction: column;
    gap: 5px;
    padding-right: 8px;
    overflow-y: auto;
    border-right: 1px solid #cfcac1;
  }
  .compact-approval__switcher > strong {
    position: sticky;
    top: 0;
    z-index: 1;
    padding: 2px 0 7px;
    color: #706d67;
    background: #f4f1ea;
    font: 700 10px/1.2 ui-monospace, SFMono-Regular, Menlo, monospace;
    text-transform: uppercase;
  }
  .compact-approval__switcher button {
    min-height: 60px;
    display: grid;
    grid-template-rows: auto minmax(0, 1fr);
    gap: 5px;
    padding: 8px;
    text-align: left;
    border-color: #cfcac1;
  }
  .compact-approval__switcher button[aria-pressed="true"] {
    border-color: #171716;
    color: #fffefb;
    background: #171716;
  }
  .compact-approval__switcher button > span {
    font: 700 10px/1 ui-monospace, SFMono-Regular, Menlo, monospace;
  }
  .compact-approval__switcher button > small {
    display: -webkit-box;
    overflow: hidden;
    font-size: 10px;
    line-height: 1.25;
    overflow-wrap: anywhere;
    -webkit-box-orient: vertical;
    -webkit-line-clamp: 2;
  }
  .compact-approval__rail {
    display: grid;
    grid-template-columns: auto 1fr auto 1fr auto;
    align-items: center;
    gap: 8px;
    margin-bottom: 12px;
    color: #706d67;
    font-size: 11px;
  }
  .compact-approval__rail i {
    height: 1px;
    background: #cfCAC1;
  }
  .compact-approval__rail strong { color: #f2381e; }
  .compact-approval__request,
  .compact-approval__suggestion,
  .compact-approval__error,
  .compact-approval__state {
    border: 1px solid #171716;
    background: #fffefb;
  }
  .compact-approval__request { padding: 16px; }
  .compact-approval__queue {
    margin-bottom: 12px;
    color: #706d67;
    font-size: 11px;
  }
  .compact-approval__queue span {
    max-width: 310px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .compact-approval__request > code {
    display: block;
    overflow-wrap: anywhere;
    font: 700 13px/1.45 ui-monospace, SFMono-Regular, Menlo, monospace;
  }
  .compact-approval dl { margin: 16px 0 0; }
  .compact-approval dl div {
    display: grid;
    grid-template-columns: 76px 1fr;
    gap: 12px;
    border-top: 1px solid #cfcac1;
    padding: 8px 0;
  }
  .compact-approval dt { color: #706d67; font-size: 12px; }
  .compact-approval dd { margin: 0; font-size: 13px; overflow-wrap: anywhere; }
  .compact-approval dl div:nth-child(n + 2) dd {
    max-height: 44px;
    overflow-y: auto;
    overscroll-behavior: auto;
  }
  .compact-approval__mono {
    font-family: ui-monospace, SFMono-Regular, Menlo, monospace;
  }
  .compact-approval__suggestion {
    border-top: 0;
    padding: 12px 16px;
  }
  .compact-approval__evaluation {
    position: sticky;
    top: 0;
    z-index: 3;
    display: grid;
    gap: 7px;
    border: 1px solid #171716;
    border-top: 0;
    padding: 10px 16px;
    background: #fffefb;
  }
  .compact-approval__evaluation[data-has-message="false"] {
    gap: 0;
    padding: 0;
    background: #f9d9d2;
  }
  .compact-approval__evaluation[data-has-message="false"] .compact-approval__evaluation-track {
    height: 4px;
  }
  .compact-approval__evaluation > div:first-child {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 10px;
  }
  .compact-approval__evaluation strong { font-size: 11px; }
  .compact-approval__evaluation span,
  .compact-approval__evaluation small {
    color: #706d67;
    font: 10px/1.35 ui-monospace, SFMono-Regular, Menlo, monospace;
  }
  .compact-approval__evaluation-track {
    height: 3px;
    overflow: hidden;
    background: #d9d4ca;
  }
  .compact-approval__evaluation-track i {
    display: block;
    height: 100%;
    background: #f2381e;
    transition: width 180ms ease-out;
  }
  .compact-approval__evaluation[data-state="running"] .compact-approval__evaluation-track i {
    animation: compact-progress 1.4s ease-in-out infinite alternate;
  }
  .compact-approval__evaluation[data-state="failed"],
  .compact-approval__evaluation[data-state="partial"] {
    border-left: 3px solid #b32212;
  }
  @keyframes compact-progress {
    from { transform: translateX(-24%); }
    to { transform: translateX(180%); }
  }
  .compact-approval__guidance-text > div > strong {
    display: grid;
    gap: 3px;
    font-size: 12px;
  }
  .compact-approval__guidance-text > div > strong small,
  .compact-approval__call-chain-help small {
    color: #f2381e;
    font: 700 9px/1.2 ui-monospace, SFMono-Regular, Menlo, monospace;
    letter-spacing: .08em;
    text-transform: uppercase;
  }
  .compact-approval__guidance-text > div > span {
    color: #706d67;
    font: 11px/1.2 ui-monospace, SFMono-Regular, Menlo, monospace;
  }
  .compact-approval__suggestion > p {
    max-height: 46px;
    margin: 8px 0 0;
    overflow-y: auto;
    color: #706d67;
    font-size: 12px;
    line-height: 1.4;
  }
  .compact-approval__suggestion p.compact-approval__chain-summary {
    border-left: 3px solid #f2381e;
    padding-left: 9px;
    color: #171716;
  }
  .compact-approval__note {
    display: block;
    margin: 10px 0;
  }
  .compact-approval__note > span {
    display: block;
    margin-bottom: 6px;
    font-size: 12px;
    font-weight: 700;
  }
  .compact-approval textarea {
    width: 100%;
    resize: none;
    border: 1px solid #cfcac1;
    border-radius: 0;
    padding: 8px;
    color: #171716;
    background: #fffefb;
  }
  .compact-approval__call-chain {
    min-height: 220px;
    flex: 0 0 auto;
    display: grid;
    grid-template-rows: auto auto;
    border: 1px solid #171716;
    background: #fffefb;
  }
  .compact-approval__call-chain > header {
    min-height: 32px;
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 10px;
    padding: 6px 9px;
    border-bottom: 1px solid #cfcac1;
  }
  .compact-approval__call-chain > header > div {
    display: flex;
    align-items: center;
    gap: 7px;
  }
  .compact-approval__call-chain > header strong { font-size: 11px; }
  .compact-approval__call-chain > header span {
    color: #706d67;
    font: 700 9px/1 ui-monospace, SFMono-Regular, Menlo, monospace;
  }
  .compact-approval__call-chain > ol {
    margin: 0;
    padding: 0;
    overflow: visible;
    list-style: none;
  }
  .compact-approval__call-chain > ol > li {
    display: grid;
    grid-template-columns: 24px minmax(0, 1fr);
    gap: 7px;
    padding: 7px 9px;
    border-bottom: 1px solid #e1ddd5;
  }
  .compact-approval__call-chain > ol > li.agent-scoped {
    background: #d8d1c7;
  }
  .compact-approval__call-chain > ol > li[data-agent-start="true"] {
    box-shadow: inset 4px 0 0 #171716;
  }
  .compact-approval__call-chain > ol > li > span {
    width: 21px;
    height: 21px;
    display: grid;
    place-items: center;
    color: #fffefb;
    background: #171716;
    font: 700 9px/1 ui-monospace, SFMono-Regular, Menlo, monospace;
  }
  .compact-approval__call-chain > ol > li > div {
    min-width: 0;
    display: grid;
    grid-template-columns: minmax(0, 1fr);
    gap: 2px;
  }
  .compact-approval__call-chain > ol > li > div > strong { font-size: 10px; }
  .compact-approval__call-chain > ol > li > div > code {
    font: 9px/1.35 ui-monospace, SFMono-Regular, Menlo, monospace;
    white-space: pre-wrap;
    overflow-wrap: anywhere;
  }
  .compact-approval__call-chain > p {
    margin: 0;
    padding: 12px;
    color: #706d67;
    font-size: 11px;
  }
  .compact-approval__call-chain-args {
    min-width: 0;
    display: grid;
    grid-template-columns: minmax(0, 1fr);
    gap: 2px;
    margin-top: 3px;
  }
  .compact-approval__call-chain-args code {
    white-space: pre-wrap;
    overflow-wrap: anywhere;
    padding: 3px 5px;
    background: rgba(255, 254, 251, .62);
  }
  .compact-approval__call-chain-help {
    display: grid;
    gap: 3px;
    margin-top: 5px;
    padding: 7px 8px;
    border-left: 3px solid #f2381e;
    background: rgba(255, 254, 251, .72);
  }
  .compact-approval__call-chain-help p {
    margin: 0;
    font-size: 10px;
    line-height: 1.4;
  }
  .compact-approval__call-chain-help span {
    color: #706d67;
    font: 9px/1.35 ui-monospace, SFMono-Regular, Menlo, monospace;
  }
  .compact-approval__reference-scope.desktop-workspace {
    min-width: 0;
    display: block;
    grid-template-columns: none;
    width: 100%;
    height: auto;
    min-height: 0;
    margin-top: 6px;
    overflow: visible;
    background: transparent;
  }
  .compact-approval__reference-scope .workspace-page {
    width: 100%;
    max-width: none;
    margin: 0;
  }
  .compact-approval__reference-scope .request-evidence-workbench {
    border-color: #cfcac1;
  }
  .compact-approval__call-chain .hljs-comment,
  .compact-approval__call-chain .hljs-quote { color: #69655f; font-style: italic; }
  .compact-approval__call-chain .hljs-keyword,
  .compact-approval__call-chain .hljs-literal,
  .compact-approval__call-chain .hljs-built_in,
  .compact-approval__call-chain .hljs-meta { color: #c92d19; font-weight: 700; }
  .compact-approval__call-chain .hljs-string,
  .compact-approval__call-chain .hljs-title,
  .compact-approval__call-chain .hljs-variable,
  .compact-approval__call-chain .hljs-params { color: #171716; font-weight: 700; }
  .compact-approval__actions {
    flex: 0 0 auto;
    flex-wrap: wrap;
    margin-top: 10px;
    padding-top: 10px;
    border-top: 1px solid #cfcac1;
    background: #f4f1ea;
  }
  .compact-approval__actions button {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    gap: 8px;
  }
  .compact-approval__reject,
  .compact-approval__approve { flex: 1; }
  .compact-approval__approve {
    border-color: #f2381e !important;
    color: #fffefb !important;
    background: #f2381e !important;
  }
  .compact-approval__details { width: 100%; border-color: #cfcac1 !important; }
  .compact-approval__error,
  .compact-approval__state {
    display: grid;
    gap: 8px;
    padding: 24px;
  }
  .compact-approval__error { border-color: #f2381e; }
  .compact-approval__error span,
  .compact-approval__state p { color: #706d67; }
  .compact-approval__state h2,
  .compact-approval__state p { margin: 0; }
  @media (prefers-reduced-motion: reduce) {
    .compact-approval * { scroll-behavior: auto !important; }
  }

  .compact-approval__suggestion { display: grid; grid-template-columns: minmax(0, 1fr) 220px; gap: 16px; }
  .compact-approval__guidance-text { min-width: 0; }
  .compact-approval__guidance-text > div { display: flex; justify-content: space-between; gap: 12px; }
  .compact-approval__guidance-text > .approval-markdown { margin-top: 10px; }
  .compact-approval__suggestion .exposure-radar__plot { height: 175px; min-height: 175px; }
  .compact-approval__request .compact-approval__intent { margin: 8px 0; font-size: 13px; line-height: 1.6; overflow-wrap: anywhere; }
  .compact-approval__request-details { margin-top: 8px; font-size: 11px; }
  .compact-approval__request-details > summary { cursor: pointer; color: #706d67; }
  .compact-approval__request { padding-bottom: 12px; }
  @media (max-width: 560px) { .compact-approval__suggestion { grid-template-columns: minmax(0, 1fr); } }
  .compact-approval__suggestion > .exposure-radar { min-width: 0; width: 100%; }
  .compact-approval__suggestion .exposure-radar__plot { min-width: 0; width: 100%; }

  .compact-approval .request-evidence-workbench { --red: #f2381e; --surface: #fff; --paper: #f2efe8; --rule: #d4cfc7; --ink: #171716; --muted-ink: #706d67; --font-mono: ui-monospace, monospace; margin-top: 12px; }
  .compact-approval .request-evidence-workbench__notes { display: block; padding: 0; margin: 0; }
  .compact-approval .request-evidence-workbench__notes li { display: block; padding: 0; margin: 0; }
  .compact-approval .request-evidence-workbench__notes li > div { min-width: 0; }
  .compact-approval .request-evidence-workbench__note { width: 100%; text-align: left; }

  .compact-approval button.compact-approval__icon-button { display: inline-flex; align-items: center; justify-content: center; flex: 0 0 32px; width: 32px; height: 32px; min-height: 32px; padding: 0; border: 0; background: transparent; color: #706d67; }
  .compact-approval button.compact-approval__icon-button:hover { color: #171716; background: #e8e3d9; }
  .compact-approval__header { align-items: flex-start; }
  .compact-approval__body[data-has-queue="true"] { display: block; }
  .compact-approval .page-split-pane { display: grid; grid-template-columns: minmax(0, 1fr); height: 100%; min-height: 0; }
  .compact-approval .page-split-pane[data-resizable="true"] { grid-template-columns: minmax(160px, min(var(--page-split-list-width), 35%)) 12px minmax(0, 1fr); }
  .compact-approval .page-split-pane-list, .compact-approval .page-split-pane-detail { min-width: 0; min-height: 0; overflow: hidden; }
  .compact-approval .page-split-pane-resizer { cursor: col-resize; touch-action: none; display: flex; justify-content: center; padding: 0 5px; }
  .compact-approval .page-split-pane-resizer > span { width: 1px; height: 100%; background: #cfcac1; }
  .compact-approval .page-split-pane-resizer:hover > span, .compact-approval .page-split-pane-resizer:focus-visible > span { background: #f2381e; }
  .compact-approval .page-split-pane-resizer:focus-visible { outline: 1px solid #f2381e; }
  .compact-approval__switcher { height: 100%; padding: 0; border: 0; }
  .compact-approval__switcher button { display: flex; flex-direction: column; align-items: flex-start; gap: 5px; padding: 12px; flex-shrink: 0; }
  .compact-approval__switcher button > strong { font: 650 14px/1.5 -apple-system, BlinkMacSystemFont, sans-serif; }
  .compact-approval__switcher button > small, .compact-approval__switcher-field { color: #706d67; font-size: 11px; overflow-wrap: anywhere; }
  .compact-approval__switcher button > p { margin: 3px 0; font-size: 12px; line-height: 1.55; display: -webkit-box; -webkit-line-clamp: 3; -webkit-box-orient: vertical; overflow: hidden; }
  .compact-approval__switcher button[aria-pressed="true"] > * { color: #fff8ef; }
  @media (max-width: 650px) { .compact-approval__body[data-has-queue="true"] .compact-approval__suggestion { grid-template-columns: minmax(0, 1fr); } }
`;
