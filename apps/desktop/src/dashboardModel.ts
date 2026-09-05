import type {
  AccessRequest,
  AuditRecord,
  AutomaticDecisionTrace,
  DashboardData,
  ProviderTrace,
} from "./types";

export type DashboardSummary = {
  pendingCount: number;
  requesterCount: number;
  auditCount: number;
  newestRequest: AccessRequest | null;
};

export type ResolvedAutoDecisionView = {
  request_id: string;
  resource: string | null;
  reason: string | null;
  requested_by: string | null;
  submitted_at: string | null;
  recorded_at: string;
  approval_status: string | null;
  final_decision: string | null;
  automatic_decision: AutomaticDecisionTrace;
};

export type SuggestionTraceView = {
  provider_kind: string | null;
  provider_model: string | null;
  provider_response_id: string | null;
  x_request_id: string | null;
  usage_total_tokens: number | null;
  rendered_prompt: string | null;
  provider_trace: ProviderTrace | null;
};

export type SuggestionSummaryView = {
  provider_kind: string | null;
  provider_model: string | null;
  suggested_decision: string | null;
  rationale_summary: string | null;
  risk_score: number | null;
  template_version: string | null;
  generated_at: string;
  error: string | null;
};

export type ResolvedReviewRequestView = {
  request_id: string;
  resource: string | null;
  reason: string | null;
  requested_by: string | null;
  policy_mode: string | null;
  submitted_at: string | null;
  recorded_at: string;
  approval_status: string | null;
  final_decision: string | null;
  reviewed_by: string | null;
  decision_note: string | null;
};

export type AuditDecisionPath =
  | "automatic"
  | "human"
  | "overridden"
  | "failed"
  | "pending";

export type AuditPhaseKind =
  | "request"
  | "model"
  | "policy"
  | "human"
  | "final"
  | "activity";

export type AuditPhase = {
  kind: AuditPhaseKind;
  events: AuditRecord[];
  started_at: string;
  finished_at: string;
};

export type AuditApprovalGroup = {
  request_id: string;
  resource: string | null;
  requested_by: string | null;
  reason: string | null;
  policy_mode: string | null;
  result: string;
  decision_path: AuditDecisionPath;
  final_actor: string | null;
  summary_note: string | null;
  risk_score: number | null;
  started_at: string;
  finished_at: string;
  duration_ms: number;
  events: AuditRecord[];
  phases: AuditPhase[];
};

export function escapeHtml(value: string): string {
  return value
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;")
    .replace(/"/g, "&quot;")
    .replace(/'/g, "&#39;");
}

export function getSelectedRequest(
  dashboard: DashboardData | null,
  selectedRequestId: string | null,
): AccessRequest | null {
  if (!dashboard) {
    return null;
  }

  return (
    dashboard.pending_requests.find(
      (request) => request.id === selectedRequestId,
    ) ??
    dashboard.pending_requests[0] ??
    null
  );
}

export function getDashboardSummary(
  dashboard: DashboardData | null,
): DashboardSummary {
  const pendingRequests = dashboard?.pending_requests ?? [];
  const newestRequest = pendingRequests.reduce<AccessRequest | null>(
    (currentNewest, request) => {
      if (!currentNewest) {
        return request;
      }

      return Date.parse(request.created_at) >
        Date.parse(currentNewest.created_at)
        ? request
        : currentNewest;
    },
    null,
  );

  return {
    pendingCount: pendingRequests.length,
    requesterCount: new Set(
      pendingRequests.map((request) => request.context.requested_by),
    ).size,
    auditCount: new Set(
      dashboard?.recent_audit_records.map((record) => record.request_id) ?? [],
    ).size,
    newestRequest,
  };
}

export function getRequestAuditRecords(
  records: AuditRecord[],
  requestId: string | null,
): AuditRecord[] {
  if (!requestId) {
    return [];
  }

  return records.filter((record) => record.request_id === requestId);
}

export function stringifyPayload(payload: Record<string, unknown>): string {
  return JSON.stringify(payload, null, 2);
}

export function toKeyValueEntries(
  values: Record<string, string>,
): Array<[string, string]> {
  return Object.entries(values).sort(([left], [right]) =>
    left.localeCompare(right),
  );
}

function readString(value: unknown): string | null {
  return typeof value === "string" && value.length > 0 ? value : null;
}

function readBoolean(value: unknown): boolean {
  return typeof value === "boolean" ? value : false;
}

function readNullableNumber(value: unknown): number | null {
  return typeof value === "number" ? value : null;
}

function readStringArray(value: unknown): string[] {
  return Array.isArray(value)
    ? value.filter((item): item is string => typeof item === "string")
    : [];
}

function readObject(value: unknown): Record<string, unknown> | null {
  if (!value || typeof value !== "object" || Array.isArray(value)) {
    return null;
  }

  return value as Record<string, unknown>;
}

export function auditResultFor(record: AuditRecord): string {
  const approvalStatus = readString(record.payload.approval_status);
  const decision = readString(record.payload.decision);
  const autoDisposition = readString(record.payload.auto_disposition);

  switch (record.action) {
    case "approval_recorded":
    case "human_decision_overrode_llm":
      if (approvalStatus === "approved" || decision === "allow") {
        return "approved";
      }
      if (approvalStatus === "rejected" || decision === "deny") {
        return "rejected";
      }
      return approvalStatus ?? "recorded";
    case "automatic_decision_recorded":
      if (autoDisposition === "allow") return "approved";
      if (autoDisposition === "deny") return "rejected";
      if (autoDisposition === "escalate") return "escalate";
      return approvalStatus ?? "recorded";
    case "automatic_escalated_to_human":
      return "escalate";
    case "llm_suggestion_failed":
      return "failed";
    case "llm_suggestion_generated":
      return "generated";
    case "llm_review_details_updated":
      return "updated";
    case "memory_evaluated":
      if (readString(record.payload.memory_status) === "error") return "failed";
      if (readString(record.payload.memory_status) === "updated") {
        return "updated";
      }
      return "recorded";
    default:
      return (
        readString(record.payload.result) ??
        readString(record.payload.status) ??
        "recorded"
      );
  }
}

function auditPhaseKind(record: AuditRecord): AuditPhaseKind {
  switch (record.action) {
    case "request_submitted":
      return "request";
    case "llm_suggestion_generated":
    case "llm_suggestion_failed":
    case "llm_review_details_updated":
    case "memory_evaluated":
      return "model";
    case "automatic_decision_recorded":
    case "automatic_escalated_to_human":
      return "policy";
    case "human_decision_overrode_llm":
      return "human";
    case "approval_recorded":
      return record.actor === "system_auto" ? "final" : "human";
    default:
      return "activity";
  }
}

const auditPhaseOrder: AuditPhaseKind[] = [
  "request",
  "model",
  "policy",
  "human",
  "final",
  "activity",
];

function buildAuditPhases(events: AuditRecord[]): AuditPhase[] {
  const eventsByPhase = new Map<AuditPhaseKind, AuditRecord[]>();
  for (const event of events) {
    const kind = auditPhaseKind(event);
    const phaseEvents = eventsByPhase.get(kind) ?? [];
    phaseEvents.push(event);
    eventsByPhase.set(kind, phaseEvents);
  }

  return auditPhaseOrder.flatMap((kind) => {
    const phaseEvents = eventsByPhase.get(kind);
    if (!phaseEvents || phaseEvents.length === 0) return [];
    return [
      {
        kind,
        events: phaseEvents,
        started_at: phaseEvents[0].created_at,
        finished_at: phaseEvents[phaseEvents.length - 1].created_at,
      },
    ];
  });
}

function groupDecisionPath(events: AuditRecord[]): AuditDecisionPath {
  const finalApproval = [...events]
    .reverse()
    .find((event) => event.action === "approval_recorded");
  const override = [...events]
    .reverse()
    .find((event) => event.action === "human_decision_overrode_llm");

  if (finalApproval && finalApproval.actor !== "system_auto") {
    const suggested = override
      ? readString(override.payload.suggested_decision)
      : null;
    const finalDecision = readString(finalApproval.payload.decision);
    if (
      (suggested === "allow" || suggested === "deny") &&
      finalDecision !== null &&
      suggested !== finalDecision
    ) {
      return "overridden";
    }
    return "human";
  }
  if (finalApproval?.actor === "system_auto") return "automatic";
  if (events.some((event) => event.action === "llm_suggestion_failed")) {
    return "failed";
  }
  if (events.some((event) => event.action === "automatic_escalated_to_human")) {
    return "human";
  }
  return "pending";
}

function groupResult(events: AuditRecord[]): string {
  const finalApproval = [...events]
    .reverse()
    .find((event) => event.action === "approval_recorded");
  if (finalApproval) return auditResultFor(finalApproval);
  if (events.some((event) => event.action === "llm_suggestion_failed")) {
    return "failed";
  }
  if (events.some((event) => event.action === "automatic_escalated_to_human")) {
    return "pending";
  }
  return "pending";
}

export function buildAuditApprovalGroups(
  records: AuditRecord[],
): AuditApprovalGroup[] {
  const grouped = new Map<string, AuditRecord[]>();
  for (const record of records) {
    const requestEvents = grouped.get(record.request_id) ?? [];
    requestEvents.push(record);
    grouped.set(record.request_id, requestEvents);
  }

  return Array.from(grouped.entries())
    .map(([requestId, unsortedEvents]) => {
      const events = [...unsortedEvents].sort(
        (left, right) =>
          Date.parse(left.created_at) - Date.parse(right.created_at) ||
          left.created_at.localeCompare(right.created_at) ||
          left.id.localeCompare(right.id),
      );
      const submission = events.find(
        (event) => event.action === "request_submitted",
      );
      const finalApproval = [...events]
        .reverse()
        .find((event) => event.action === "approval_recorded");
      const automaticDecision = [...events]
        .reverse()
        .find((event) => event.action === "automatic_decision_recorded");
      const suggestion = [...events]
        .reverse()
        .find(
          (event) =>
            event.action === "llm_review_details_updated" ||
            event.action === "llm_suggestion_generated" ||
            event.action === "llm_suggestion_failed",
        );
      const startedAt = events[0].created_at;
      const finishedAt = events[events.length - 1].created_at;
      const startedMs = Date.parse(startedAt);
      const finishedMs = Date.parse(finishedAt);

      return {
        request_id: requestId,
        resource: readString(submission?.payload.resource),
        requested_by: submission?.actor || null,
        reason: submission?.note ?? null,
        policy_mode: readString(submission?.payload.policy_mode),
        result: groupResult(events),
        decision_path: groupDecisionPath(events),
        final_actor: finalApproval?.actor ?? null,
        summary_note:
          finalApproval?.note ??
          automaticDecision?.note ??
          suggestion?.note ??
          null,
        risk_score:
          readNullableNumber(automaticDecision?.payload.risk_score) ??
          readNullableNumber(suggestion?.payload.risk_score),
        started_at: startedAt,
        finished_at: finishedAt,
        duration_ms:
          Number.isFinite(startedMs) && Number.isFinite(finishedMs)
            ? Math.max(0, finishedMs - startedMs)
            : 0,
        events,
        phases: buildAuditPhases(events),
      } satisfies AuditApprovalGroup;
    })
    .sort(
      (left, right) =>
        Date.parse(right.finished_at) - Date.parse(left.finished_at) ||
        right.request_id.localeCompare(left.request_id),
    );
}

function readProviderTrace(value: unknown): ProviderTrace | null {
  const record = readObject(value);
  if (!record) {
    return null;
  }

  const reviewProgressRecord = readObject(record.review_progress);
  const reviewProgressState = readString(reviewProgressRecord?.state);
  const reviewProgress =
    reviewProgressRecord &&
    (reviewProgressState === "running" ||
      reviewProgressState === "complete" ||
      reviewProgressState === "partial" ||
      reviewProgressState === "failed")
      ? {
          state: reviewProgressState as NonNullable<
            ProviderTrace["review_progress"]
          >["state"],
          completed_units:
            readNullableNumber(reviewProgressRecord.completed_units) ?? 0,
          total_units:
            readNullableNumber(reviewProgressRecord.total_units) ?? 0,
          error: readString(reviewProgressRecord.error),
          updated_at: readString(reviewProgressRecord.updated_at) ?? "",
        }
      : null;
  const providerTrace: ProviderTrace = {
    rendered_prompt: readString(record.rendered_prompt),
    transport: readString(record.transport),
    protocol: readString(record.protocol),
    api_version: readString(record.api_version),
    output_format: readString(record.output_format),
    stop_reason: readString(record.stop_reason),
    package_name: readString(record.package_name),
    package_version: readString(record.package_version),
    session_id: readString(record.session_id),
    client_request_id: readString(record.client_request_id),
    agent_name: readString(record.agent_name),
    agent_version: readString(record.agent_version),
    beta_headers: readStringArray(record.beta_headers),
    ...(reviewProgress ? { review_progress: reviewProgress } : {}),
  };

  return Object.entries(providerTrace).some(([key, field]) =>
    key === "beta_headers"
      ? Array.isArray(field) && field.length > 0
      : field !== null,
  )
    ? providerTrace
    : null;
}

function readUsageTotalTokens(value: unknown): number | null {
  const record = readObject(value);
  if (!record) {
    return null;
  }

  return readNullableNumber(record.total_tokens);
}

function parseSuggestionSummary(
  record: AuditRecord,
): SuggestionSummaryView | null {
  if (
    record.action !== "llm_review_details_updated" &&
    record.action !== "llm_suggestion_generated" &&
    record.action !== "llm_suggestion_failed"
  ) {
    return null;
  }

  const providerKind =
    record.actor.trim().length > 0
      ? record.actor
      : readString(record.payload.provider_kind);
  const providerModel = readString(record.payload.provider_model);
  const suggestedDecision = readString(record.payload.suggested_decision);
  const rationaleSummary =
    record.action === "llm_suggestion_generated" ||
    record.action === "llm_review_details_updated"
      ? (readString(record.payload.rationale_summary) ?? record.note)
      : null;
  const error =
    record.action === "llm_suggestion_failed"
      ? (record.note ?? readString(record.payload.error))
      : readString(record.payload.error);
  const riskScore = readNullableNumber(record.payload.risk_score);
  const templateVersion = readString(record.payload.template_version);

  if (
    !providerKind &&
    !providerModel &&
    !suggestedDecision &&
    !rationaleSummary &&
    riskScore === null &&
    !error
  ) {
    return null;
  }

  return {
    provider_kind: providerKind,
    provider_model: providerModel,
    suggested_decision: suggestedDecision,
    rationale_summary: rationaleSummary,
    risk_score: riskScore,
    template_version: templateVersion,
    generated_at: record.created_at,
    error,
  };
}

function parseAutomaticDecision(record: AuditRecord):
  | (AutomaticDecisionTrace & {
      approval_status: string | null;
      final_decision: string | null;
    })
  | null {
  const autoDisposition = readString(record.payload.auto_disposition);
  const decisionSource = readString(record.payload.decision_source);

  if (!autoDisposition || !decisionSource) {
    return null;
  }

  return {
    auto_disposition: autoDisposition,
    decision_source: decisionSource,
    matched_rule_ids: readStringArray(record.payload.matched_rule_ids),
    secret_exposure_risk: readBoolean(record.payload.secret_exposure_risk),
    provider_called: readBoolean(record.payload.provider_called),
    suggested_decision: readString(record.payload.suggested_decision),
    risk_score: readNullableNumber(record.payload.risk_score),
    template_id: readString(record.payload.template_id),
    template_version: readString(record.payload.template_version),
    prompt_contract_version: readString(record.payload.prompt_contract_version),
    provider_kind: readString(record.payload.provider_kind),
    provider_model: readString(record.payload.provider_model),
    x_request_id: readString(record.payload.x_request_id),
    provider_response_id: readString(record.payload.provider_response_id),
    auto_rationale_summary:
      readString(record.payload.auto_rationale_summary) ?? record.note ?? "",
    fail_closed: readBoolean(record.payload.fail_closed),
    evaluated_at: readString(record.payload.evaluated_at) ?? record.created_at,
    approval_status: readString(record.payload.approval_status),
    final_decision: readString(record.payload.final_decision),
  };
}

export function getResolvedAutoDecisionEntries(
  records: AuditRecord[],
): ResolvedAutoDecisionView[] {
  const sortedRecords = [...records].sort(
    (left, right) => Date.parse(right.created_at) - Date.parse(left.created_at),
  );
  const submissions = new Map<string, AuditRecord>();
  const results: ResolvedAutoDecisionView[] = [];
  const seenRequestIds = new Set<string>();

  for (const record of sortedRecords) {
    if (record.action === "request_submitted") {
      submissions.set(record.request_id, record);
    }
  }

  for (const record of sortedRecords) {
    if (
      record.action !== "automatic_decision_recorded" ||
      seenRequestIds.has(record.request_id)
    ) {
      continue;
    }

    const automaticDecision = parseAutomaticDecision(record);
    if (
      !automaticDecision ||
      (automaticDecision.auto_disposition !== "allow" &&
        automaticDecision.auto_disposition !== "deny")
    ) {
      continue;
    }

    const submission = submissions.get(record.request_id);

    results.push({
      request_id: record.request_id,
      resource: readString(submission?.payload.resource),
      reason: submission?.note ?? null,
      requested_by: submission?.actor ?? null,
      submitted_at: submission?.created_at ?? null,
      recorded_at: record.created_at,
      approval_status: automaticDecision.approval_status,
      final_decision: automaticDecision.final_decision,
      automatic_decision: automaticDecision,
    });
    seenRequestIds.add(record.request_id);
  }

  return results.sort(
    (left, right) =>
      Date.parse(right.recorded_at) - Date.parse(left.recorded_at),
  );
}

export function getResolvedReviewRequestEntries(
  records: AuditRecord[],
): ResolvedReviewRequestView[] {
  const sortedRecords = [...records].sort(
    (left, right) => Date.parse(right.created_at) - Date.parse(left.created_at),
  );
  const submissions = new Map<string, AuditRecord>();
  const results: ResolvedReviewRequestView[] = [];
  const seenRequestIds = new Set<string>();

  for (const record of sortedRecords) {
    if (record.action === "request_submitted") {
      submissions.set(record.request_id, record);
    }
  }

  for (const record of sortedRecords) {
    if (
      record.action !== "approval_recorded" ||
      record.actor === "system_auto" ||
      seenRequestIds.has(record.request_id)
    ) {
      continue;
    }

    const submission = submissions.get(record.request_id);

    results.push({
      request_id: record.request_id,
      resource: readString(submission?.payload.resource),
      reason: submission?.note ?? null,
      requested_by: submission?.actor ?? null,
      policy_mode: readString(submission?.payload.policy_mode),
      submitted_at: submission?.created_at ?? null,
      recorded_at: record.created_at,
      approval_status: readString(record.payload.approval_status),
      final_decision: readString(record.payload.decision),
      reviewed_by: record.actor || null,
      decision_note: record.note ?? null,
    });
    seenRequestIds.add(record.request_id);
  }

  return results.sort(
    (left, right) =>
      Date.parse(right.recorded_at) - Date.parse(left.recorded_at),
  );
}

export function getSuggestionSummary(
  records: AuditRecord[],
): SuggestionSummaryView | null {
  const suggestionRecord = [...records]
    .sort(
      (left, right) =>
        Date.parse(right.created_at) - Date.parse(left.created_at),
    )
    .find(
      (record) =>
        record.action === "llm_review_details_updated" ||
        record.action === "llm_suggestion_generated" ||
        record.action === "llm_suggestion_failed",
    );

  return suggestionRecord ? parseSuggestionSummary(suggestionRecord) : null;
}

export function getSuggestionTrace(
  records: AuditRecord[],
): SuggestionTraceView | null {
  const suggestionRecord = [...records]
    .sort(
      (left, right) =>
        Date.parse(right.created_at) - Date.parse(left.created_at),
    )
    .find(
      (record) =>
        record.action === "llm_review_details_updated" ||
        record.action === "llm_suggestion_generated" ||
        record.action === "llm_suggestion_failed",
    );

  if (!suggestionRecord) {
    return null;
  }

  const providerTrace = readProviderTrace(
    suggestionRecord.payload.provider_trace,
  );
  const providerModel = readString(suggestionRecord.payload.provider_model);
  const providerKind =
    suggestionRecord.actor.trim().length > 0 ? suggestionRecord.actor : null;

  if (!providerTrace && !providerModel && !providerKind) {
    return null;
  }

  return {
    provider_kind: providerKind,
    provider_model: providerModel,
    provider_response_id: readString(
      suggestionRecord.payload.provider_response_id,
    ),
    x_request_id: readString(suggestionRecord.payload.x_request_id),
    usage_total_tokens: readUsageTotalTokens(suggestionRecord.payload.usage),
    rendered_prompt: providerTrace?.rendered_prompt ?? null,
    provider_trace: providerTrace,
  };
}
