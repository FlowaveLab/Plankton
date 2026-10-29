import { invoke } from "@tauri-apps/api/core";
import { BookmarkCheck, SlidersHorizontal } from "lucide-react";
import { useCallback, useEffect, useRef, useState, type JSX } from "react";

import type {
  AccessRequest,
  CredentialExposureReport,
  RememberExposureOptions,
} from "../types";
import {
  defaultExposurePolicy,
  EXPOSURE_SURFACES,
  ExposurePolicyEditor,
  exposurePolicyNeedsNetworkAllowlist,
  parseExposurePolicy,
  type CredentialExposurePolicy,
  type ExposureSurface,
  type NetworkDestinationRule,
} from "./ExposurePolicy";
import "./remember-exposure.css";

type CandidateIssue =
  | "missing_report"
  | "incomplete_report"
  | "unknown_evidence"
  | "missing_destinations"
  | "invalid_destinations"
  | "unbounded_surface"
  | "direct_policy";
export type ExposureCandidate = {
  policy: CredentialExposurePolicy;
  issue: CandidateIssue | null;
};

function destinationHostname(destination: string): string | null {
  if (!destination.trim() || /\s|\\/.test(destination)) return null;
  try {
    const url = new URL(
      destination.includes("://") ? destination : `https://${destination}`,
    );
    const host = url.hostname.replace(/\.+$/, "").toLowerCase();
    if (url.username || url.password || !host || host.length > 253) return null;
    return host
      .split(".")
      .every(
        (label) =>
          label.length <= 63 && /^[a-z0-9](?:[a-z0-9-]*[a-z0-9])?$/.test(label),
      )
      ? host
      : null;
  } catch {
    return null;
  }
}

function ruleIncludesHost(rule: NetworkDestinationRule, host: string): boolean {
  if (rule.kind === "regex") {
    // Rust and JavaScript regex semantics differ. Preserve the expression, but use an exact
    // host for this request instead of claiming the expression already authorizes it.
    return false;
  }
  const domain = rule.domain.replace(/\.+$/, "").toLowerCase();
  return rule.kind === "exact_domain"
    ? domain === host
    : (rule.include_apex && host === domain) || host.endsWith(`.${domain}`);
}

/** Suggest only established, representable scope; never infer destinations from free text. */
export function deriveExposureCandidate(
  original: CredentialExposurePolicy,
  report?: CredentialExposureReport | null,
): ExposureCandidate {
  const unchanged = (issue: CandidateIssue): ExposureCandidate => ({
    policy: structuredClone(original),
    issue,
  });
  if (original.access_mode !== "protected") return unchanged("direct_policy");
  if (!report) return unchanged("missing_report");
  if (
    !report.chain_summary.trim() ||
    report.surfaces.length !== EXPOSURE_SURFACES.length ||
    EXPOSURE_SURFACES.some(
      (surface) =>
        report.surfaces.filter((entry) => entry.surface === surface).length !==
        1,
    )
  )
    return unchanged("incomplete_report");
  if (report.surfaces.some((entry) => entry.evidence_state === "unknown"))
    return unchanged("unknown_evidence");
  if (
    report.surfaces.some(
      (entry) =>
        !entry.summary.trim() ||
        !Number.isInteger(entry.actual_level) ||
        entry.actual_level < 0 ||
        entry.actual_level > 2 ||
        (entry.evidence_state === "not_observed" && entry.actual_level !== 0),
    )
  )
    return unchanged("incomplete_report");
  if (
    report.surfaces.some(
      (entry) => entry.surface !== "network" && entry.actual_level > 1,
    )
  )
    return unchanged("unbounded_surface");
  const network = report.surfaces.find((entry) => entry.surface === "network")!;
  const destinations = network.network_destinations ?? [];
  if (network.actual_level > 0 && destinations.length === 0)
    return unchanged("missing_destinations");
  const hosts = destinations.map(destinationHostname);
  if (hosts.some((host) => host === null))
    return unchanged("invalid_destinations");
  const policy = structuredClone(original);
  for (const entry of policy.surfaces) {
    const observed = report.surfaces.find(
      (surface) => surface.surface === entry.surface,
    )!;
    if (observed.evidence_state !== "observed" || observed.actual_level === 0)
      continue;
    if (entry.surface !== "network") {
      entry.max_level = Math.max(entry.max_level, observed.actual_level);
      continue;
    }
    // Level 2 can mean a known host lies outside the previous allowlist. Keep the new
    // permission bounded to exact observed hosts; never turn it into unrestricted network use.
    if (entry.max_level >= 2) continue;
    const rules =
      entry.max_level === 0 ? [] : [...(entry.network_allowlist ?? [])];
    entry.max_level = Math.max(entry.max_level, 1);
    for (const host of hosts) {
      if (host && !rules.some((rule) => ruleIncludesHost(rule, host)))
        rules.push({ kind: "exact_domain", domain: host });
    }
    entry.network_allowlist = rules;
  }
  return { policy, issue: null };
}

const surfaceLabel = (surface: ExposureSurface, zh: boolean): string =>
  ({
    llm_context: zh ? "LLM 回显" : "LLM context",
    network: zh ? "网络发送" : "Network",
    local_persistence: zh ? "本地持久化" : "Local storage",
    terminal_log: zh ? "终端 / 日志" : "Terminal / logs",
    process_propagation: zh ? "进程传递" : "Process handoff",
  })[surface];

function candidateChanges(
  original: CredentialExposurePolicy,
  policy: CredentialExposurePolicy,
  zh: boolean,
): string[] {
  return policy.surfaces.flatMap((entry) => {
    const before = original.surfaces.find(
      (surface) => surface.surface === entry.surface,
    )!;
    const domains = (entry.network_allowlist ?? [])
      .filter(
        (rule) =>
          rule.kind === "exact_domain" &&
          (before.max_level === 0 ||
            !(before.network_allowlist ?? []).some((old) =>
              ruleIncludesHost(old, rule.domain),
            )),
      )
      .map((rule) => (rule.kind === "exact_domain" ? rule.domain : ""));
    if (entry.max_level === before.max_level && domains.length === 0) return [];
    if (entry.surface === "network" && entry.max_level === 1) {
      const allowed =
        domains.length > 0
          ? domains
          : (entry.network_allowlist ?? [])
              .filter((rule) => rule.kind === "exact_domain")
              .map((rule) => rule.domain);
      return [
        `${zh ? "新增允许域名：" : "Add allowed domains:"} ${allowed.join("、") || (zh ? "已有白名单" : "the existing allowlist")}`,
      ];
    }
    return [
      `${surfaceLabel(entry.surface, zh)} ${before.max_level} → ${entry.max_level}`,
    ];
  });
}

function issueLabel(issue: CandidateIssue, zh: boolean): string {
  const labels: Record<CandidateIssue, [string, string]> = {
    missing_report: [
      "尚无暴露报告，请手动调整范围或仅批准本次。",
      "No exposure report yet. Adjust scope manually or approve this request only.",
    ],
    incomplete_report: [
      "暴露报告不完整，请手动核对范围。",
      "The exposure report is incomplete. Check the scope manually.",
    ],
    unknown_evidence: [
      "仍有未知证据，不能自动推导安全范围。可手动调整或仅批准本次。",
      "Some evidence is unknown, so a safe scope cannot be inferred. Adjust manually or approve this request only.",
    ],
    missing_destinations: [
      "报告未确认网络目标，请手动填写允许的域名。",
      "Network destinations are not established. Enter the allowed domains manually.",
    ],
    invalid_destinations: [
      "部分网络目标无法转换为明确域名，请手动核对。",
      "Some network destinations cannot be represented as exact domains. Check them manually.",
    ],
    unbounded_surface: [
      "报告包含超出受控范围的暴露，请手动核对后调整。",
      "The report includes exposure outside controlled scope. Check it before adjusting.",
    ],
    direct_policy: [
      "此凭据当前为直接可见，请先在密码库设置受保护范围。",
      "This credential currently allows direct access. Set a protected scope in the vault first.",
    ],
  };
  return labels[issue][zh ? 0 : 1];
}

function isNativeDesktop(): boolean {
  return Boolean(
    (window as Window & { __TAURI_INTERNALS__?: object }).__TAURI_INTERNALS__,
  );
}

export function RememberExposure(props: {
  request: Pick<AccessRequest, "id" | "policy_mode"> & {
    evaluation_state?: AccessRequest["evaluation_state"];
    context: Pick<AccessRequest["context"], "resource_metadata">;
    llm_suggestion?: Pick<
      NonNullable<AccessRequest["llm_suggestion"]>,
      "exposure_report"
    > | null;
  };
  zh: boolean;
  busy: boolean;
  onApprove: (options: RememberExposureOptions) => Promise<void>;
}): JSX.Element {
  const { request, zh, busy, onApprove } = props;
  const snapshot =
    parseExposurePolicy(
      request.context.resource_metadata?.credential_exposure_policy_v1,
    ) ?? defaultExposurePolicy();
  const [open, setOpen] = useState(false);
  const [original, setOriginal] = useState(snapshot);
  const [policy, setPolicy] = useState(snapshot);
  const [loading, setLoading] = useState(isNativeDesktop);
  const [loadError, setLoadError] = useState<string | null>(null);
  const generation = useRef(0);
  const reloadPolicy = useCallback(async () => {
    if (!isNativeDesktop()) return;
    const version = ++generation.current;
    setLoading(true);
    setLoadError(null);
    try {
      const current = await invoke<CredentialExposurePolicy>(
        "request_current_exposure_policy",
        { requestId: request.id },
      );
      if (version !== generation.current) return;
      if (
        !current ||
        !Array.isArray(current.surfaces) ||
        current.surfaces.length !== EXPOSURE_SURFACES.length ||
        EXPOSURE_SURFACES.some(
          (surface) =>
            current.surfaces.filter((entry) => entry.surface === surface)
              .length !== 1,
        )
      ) {
        throw new Error(
          "Current exposure policy is unavailable. Reload before saving a scope.",
        );
      }
      setOriginal(current);
      setPolicy(current);
    } catch (reason) {
      if (version === generation.current)
        setLoadError(reason instanceof Error ? reason.message : String(reason));
    } finally {
      if (version === generation.current) setLoading(false);
    }
  }, [request.id]);
  useEffect(() => {
    void reloadPolicy();
    return () => {
      generation.current += 1;
    };
  }, [reloadPolicy]);
  const candidate = deriveExposureCandidate(
    original,
    request.llm_suggestion?.exposure_report,
  );
  const changes = candidateChanges(original, candidate.policy, zh);
  const evaluating =
    request.evaluation_state === "queued" ||
    request.evaluation_state === "running";
  const reviewFailed =
    request.evaluation_state === "failed" ||
    request.evaluation_state === "interrupted";
  const candidateUnavailable =
    Boolean(candidate.issue) || evaluating || reviewFailed;
  const missingAllowlist = exposurePolicyNeedsNetworkAllowlist(policy);
  const changed = JSON.stringify(policy) !== JSON.stringify(original);
  const unavailable = busy || loading || loadError !== null;

  return (
    <section
      className="remember-exposure"
      aria-label={zh ? "记住审批范围" : "Remember approval scope"}
    >
      <div className="remember-exposure-heading">
        <BookmarkCheck aria-hidden="true" size={17} />
        <strong>{zh ? "记住本次范围" : "Remember this scope"}</strong>
        <span>{zh ? "仅此凭据" : "This credential only"}</span>
      </div>
      <div className="remember-exposure-preview" aria-live="polite">
        {loading ? (
          <p role="status">
            {zh ? "正在读取当前暴露域…" : "Loading current exposure policy…"}
          </p>
        ) : loadError ? (
          <p role="alert">{loadError}</p>
        ) : evaluating ? (
          <p>
            {zh
              ? "正在核对暴露证据，完成后可应用本次范围。"
              : "Checking exposure evidence. Apply this scope when the review is ready."}
          </p>
        ) : reviewFailed ? (
          <p>
            {zh
              ? "本次自动审阅未完成，请手动核对范围。"
              : "Automatic review did not complete. Check the scope manually."}
          </p>
        ) : candidate.issue ? (
          <p>{issueLabel(candidate.issue, zh)}</p>
        ) : changes.length > 0 ? (
          <>
            <span>
              {zh ? "将新增允许范围：" : "Will add to the allowed scope:"}
            </span>
            <ul>
              {changes.map((change) => (
                <li key={change}>{change}</li>
              ))}
            </ul>
          </>
        ) : (
          <p>
            {zh
              ? "本次已确认范围已被现有策略覆盖，可直接批准。"
              : "The current policy already covers this confirmed scope. You can approve directly."}
          </p>
        )}
      </div>
      <p className="remember-exposure-mode">
        {request.policy_mode !== "llm_automatic"
          ? zh
            ? "当前为人工或辅助审批，保存范围后仍需按该模式审批。"
            : "Human or assisted review is active. Saving a scope keeps that review mode in place."
          : zh
            ? "自动模式下，后续证据完整且范围内的请求将自动处理。"
            : "Automatic mode handles later requests with complete evidence inside this scope."}
      </p>
      <div className="remember-exposure-actions">
        <button
          className="primary remember-exposure-quick"
          disabled={unavailable || candidateUnavailable || changes.length === 0}
          onClick={() =>
            void onApprove({
              exposurePolicy: candidate.policy,
              expectedExposurePolicy: original,
            })
          }
          type="button"
        >
          {busy
            ? zh
              ? "保存并批准中…"
              : "Saving and approving…"
            : zh
              ? "应用本次范围并批准"
              : "Apply this scope and approve"}
        </button>
        <button
          aria-expanded={open}
          aria-controls={`remember-exposure-${request.id}`}
          className="remember-exposure-toggle"
          disabled={unavailable}
          onClick={() => {
            if (!open) setPolicy(candidate.policy);
            setOpen(!open);
          }}
          type="button"
        >
          <SlidersHorizontal aria-hidden="true" size={14} />
          {open
            ? zh
              ? "收起手动调整"
              : "Close manual settings"
            : zh
              ? "手动调整范围"
              : "Adjust scope manually"}
        </button>
        {loadError ? (
          <button
            className="remember-exposure-reload"
            type="button"
            disabled={busy || loading}
            onClick={() => void reloadPolicy()}
          >
            {zh ? "重新加载当前范围" : "Reload current scope"}
          </button>
        ) : null}
      </div>
      {open ? (
        <div
          id={`remember-exposure-${request.id}`}
          className="remember-exposure-body"
        >
          <p>
            {zh
              ? "已填入本次可确认的最小范围；现有允许项保持不变。调整后会保存到此凭据。"
              : "Starts with the smallest confirmed scope and preserves existing permissions. Your edits will be saved to this credential."}
          </p>
          <button
            className="remember-exposure-reload"
            type="button"
            disabled={busy || loading}
            onClick={() => void reloadPolicy()}
          >
            {zh ? "重新加载当前范围" : "Reload current scope"}
          </button>
          <fieldset disabled={unavailable}>
            <ExposurePolicyEditor
              compact
              protectedOnly
              locale={zh ? "zh-CN" : "en"}
              onChange={setPolicy}
              value={policy}
            />
          </fieldset>
          {missingAllowlist ? (
            <p className="remember-exposure-notice" role="alert">
              {zh
                ? "受控网络使用需要填写允许的域名。"
                : "Controlled network use needs an allowed domain."}
            </p>
          ) : null}
          <button
            className="primary"
            disabled={
              unavailable ||
              !changed ||
              missingAllowlist ||
              policy.access_mode !== "protected"
            }
            onClick={() =>
              void onApprove({
                exposurePolicy: policy,
                expectedExposurePolicy: original,
              })
            }
            type="button"
          >
            {busy
              ? zh
                ? "保存并批准中…"
                : "Saving and approving…"
              : zh
                ? "通过并记住暴露域"
                : "Approve and remember exposure"}
          </button>
        </div>
      ) : null}
    </section>
  );
}
