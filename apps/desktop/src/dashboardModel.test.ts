import { describe, expect, it } from "vitest";

import {
  auditResultFor,
  buildAuditApprovalGroups,
  escapeHtml,
  getDashboardSummary,
  getRequestAuditRecords,
  getResolvedAutoDecisionEntries,
  getResolvedReviewRequestEntries,
  getSuggestionTrace,
  getSuggestionSummary,
  getSelectedRequest,
  toKeyValueEntries,
} from "./dashboardModel";
import type { DashboardData } from "./types";

const dashboardFixture: DashboardData = {
  pending_requests: [
    {
      id: "request-1",
      context: {
        resource: "secret/api-token",
        reason: "Run smoke test",
        requested_by: "alice",
        script_path: "scripts/smoke.sh",
        call_chain: [{ resolved_file_path: "scripts/smoke.sh" }],
        env_vars: {
          API_ENV: "dev",
        },
        metadata: {
          team: "security",
        },
        created_at: "2026-04-09T12:00:00Z",
      },
      policy_mode: "manual_only",
      approval_status: "pending",
      evaluation_state: "not_required",
      final_decision: null,
      provider_kind: "mock",
      rendered_prompt: "prompt",
      llm_suggestion: null,
      automatic_decision: null,
      created_at: "2026-04-09T12:00:00Z",
      updated_at: "2026-04-09T12:00:00Z",
      resolved_at: null,
    },
    {
      id: "request-2",
      context: {
        resource: "env/prod-db",
        reason: "Investigate issue",
        requested_by: "bob",
        script_path: null,
        call_chain: [],
        env_vars: {},
        metadata: {
          incident: "INC-42",
        },
        created_at: "2026-04-09T12:05:00Z",
      },
      policy_mode: "manual_only",
      approval_status: "pending",
      evaluation_state: "not_required",
      final_decision: null,
      provider_kind: "mock",
      rendered_prompt: "prompt",
      llm_suggestion: null,
      automatic_decision: null,
      created_at: "2026-04-09T12:05:00Z",
      updated_at: "2026-04-09T12:05:00Z",
      resolved_at: null,
    },
  ],
  recent_audit_records: [
    {
      id: "audit-1",
      request_id: "request-1",
      action: "request_submitted",
      actor: "alice",
      note: null,
      payload: {
        resource: "secret/api-token",
      },
      created_at: "2026-04-09T12:00:00Z",
    },
    {
      id: "audit-2",
      request_id: "request-2",
      action: "request_submitted",
      actor: "bob",
      note: "Needs incident access",
      payload: {
        resource: "env/prod-db",
      },
      created_at: "2026-04-09T12:05:00Z",
    },
  ],
};

describe("dashboardModel", () => {
  it("escapes HTML before it is rendered into templates", () => {
    expect(escapeHtml('<script>alert("x")</script>')).toBe(
      "&lt;script&gt;alert(&quot;x&quot;)&lt;/script&gt;",
    );
  });

  it("computes queue and audit summary information", () => {
    expect(getDashboardSummary(dashboardFixture)).toEqual({
      pendingCount: 2,
      requesterCount: 2,
      auditCount: 2,
      newestRequest: dashboardFixture.pending_requests[1],
    });
  });

  it("returns the selected request and falls back to the first pending item", () => {
    expect(getSelectedRequest(dashboardFixture, "request-2")?.id).toBe(
      "request-2",
    );
    expect(getSelectedRequest(dashboardFixture, "missing")?.id).toBe(
      "request-1",
    );
  });

  it("filters audit records for the selected request", () => {
    expect(
      getRequestAuditRecords(
        dashboardFixture.recent_audit_records,
        "request-2",
      ),
    ).toHaveLength(1);
    expect(
      getRequestAuditRecords(dashboardFixture.recent_audit_records, null),
    ).toEqual([]);
  });

  it("sorts key value entries for consistent rendering", () => {
    expect(
      toKeyValueEntries({
        zebra: "2",
        alpha: "1",
      }),
    ).toEqual([
      ["alpha", "1"],
      ["zebra", "2"],
    ]);
  });

  it("derives recent resolved auto decisions from audit records", () => {
    const entries = getResolvedAutoDecisionEntries([
      ...dashboardFixture.recent_audit_records,
      {
        id: "audit-3",
        request_id: "request-2",
        action: "automatic_decision_recorded",
        actor: "system_auto",
        note: "Matched allowlist and low risk",
        payload: {
          auto_disposition: "allow",
          decision_source: "llm_low_risk_allow",
          approval_status: "approved",
          final_decision: "allow",
          provider_called: true,
          secret_exposure_risk: false,
          matched_rule_ids: ["dev_allow"],
          auto_rationale_summary: "Matched allowlist and low risk",
          fail_closed: false,
          evaluated_at: "2026-04-09T12:06:00Z",
          provider_kind: "openai_compatible",
          provider_model: "gpt-5.4-mini",
        },
        created_at: "2026-04-09T12:06:00Z",
      },
      {
        id: "audit-4",
        request_id: "request-1",
        action: "automatic_decision_recorded",
        actor: "system_auto",
        note: "Escalated due to secret exposure risk",
        payload: {
          auto_disposition: "escalate",
          decision_source: "secret_exposure_guardrail",
          approval_status: "pending",
          final_decision: null,
          provider_called: false,
          secret_exposure_risk: true,
          matched_rule_ids: [],
          auto_rationale_summary: "Escalated due to secret exposure risk",
          fail_closed: true,
          evaluated_at: "2026-04-09T12:01:00Z",
        },
        created_at: "2026-04-09T12:01:00Z",
      },
    ]);

    expect(entries).toEqual([
      {
        request_id: "request-2",
        resource: "env/prod-db",
        reason: "Needs incident access",
        requested_by: "bob",
        submitted_at: "2026-04-09T12:05:00Z",
        recorded_at: "2026-04-09T12:06:00Z",
        approval_status: "approved",
        final_decision: "allow",
        automatic_decision: expect.objectContaining({
          auto_disposition: "allow",
          decision_source: "llm_low_risk_allow",
          provider_called: true,
          matched_rule_ids: ["dev_allow"],
        }),
      },
    ]);
  });

  it("orders resolved auto decisions from newest to oldest", () => {
    const entries = getResolvedAutoDecisionEntries([
      {
        id: "audit-10",
        request_id: "request-10",
        action: "request_submitted",
        actor: "alice",
        note: "First auto request",
        payload: {
          resource: "secret/first",
        },
        created_at: "2026-04-09T12:01:00Z",
      },
      {
        id: "audit-11",
        request_id: "request-11",
        action: "request_submitted",
        actor: "bob",
        note: "Second auto request",
        payload: {
          resource: "secret/second",
        },
        created_at: "2026-04-09T12:02:00Z",
      },
      {
        id: "audit-12",
        request_id: "request-11",
        action: "automatic_decision_recorded",
        actor: "system_auto",
        note: "Second allowed",
        payload: {
          auto_disposition: "allow",
          decision_source: "llm_low_risk_allow",
          approval_status: "approved",
          final_decision: "allow",
          provider_called: true,
          secret_exposure_risk: false,
          matched_rule_ids: ["rule-2"],
          auto_rationale_summary: "Second allowed",
        },
        created_at: "2026-04-09T12:04:00Z",
      },
      {
        id: "audit-13",
        request_id: "request-10",
        action: "automatic_decision_recorded",
        actor: "system_auto",
        note: "First allowed",
        payload: {
          auto_disposition: "allow",
          decision_source: "llm_low_risk_allow",
          approval_status: "approved",
          final_decision: "allow",
          provider_called: true,
          secret_exposure_risk: false,
          matched_rule_ids: ["rule-1"],
          auto_rationale_summary: "First allowed",
        },
        created_at: "2026-04-09T12:03:00Z",
      },
    ]);

    expect(entries.map((entry) => entry.request_id)).toEqual([
      "request-11",
      "request-10",
    ]);
  });

  it("keeps resolved auto entries even when request submission metadata is absent", () => {
    const entries = getResolvedAutoDecisionEntries([
      {
        id: "audit-5",
        request_id: "request-3",
        action: "automatic_decision_recorded",
        actor: "system_auto",
        note: "Denied by production guardrail",
        payload: {
          auto_disposition: "deny",
          decision_source: "production_guardrail",
          approval_status: "rejected",
          final_decision: "deny",
          provider_called: false,
          secret_exposure_risk: false,
          matched_rule_ids: ["prod_deny"],
          auto_rationale_summary: "Denied by production guardrail",
          fail_closed: false,
          evaluated_at: "2026-04-09T12:10:00Z",
        },
        created_at: "2026-04-09T12:10:00Z",
      },
    ]);

    expect(entries).toEqual([
      expect.objectContaining({
        request_id: "request-3",
        resource: null,
        reason: null,
        requested_by: null,
        automatic_decision: expect.objectContaining({
          auto_disposition: "deny",
          decision_source: "production_guardrail",
        }),
      }),
    ]);
  });

  it("derives recent resolved review requests from approval audit records", () => {
    const entries = getResolvedReviewRequestEntries([
      ...dashboardFixture.recent_audit_records,
      {
        id: "audit-5",
        request_id: "request-2",
        action: "llm_suggestion_generated",
        actor: "claude",
        note: "Low-risk incident access",
        payload: {
          suggested_decision: "allow",
          risk_score: 18,
          template_version: "2",
          provider_model: "claude-3-7-sonnet-20250219",
        },
        created_at: "2026-04-09T12:05:30Z",
      },
      {
        id: "audit-6",
        request_id: "request-2",
        action: "approval_recorded",
        actor: "reviewer.alice",
        note: "Approved for incident response",
        payload: {
          approval_status: "approved",
          decision: "allow",
        },
        created_at: "2026-04-09T12:06:00Z",
      },
      {
        id: "audit-7",
        request_id: "request-3",
        action: "approval_recorded",
        actor: "system_auto",
        note: "Automatically allowed",
        payload: {
          approval_status: "approved",
          decision: "allow",
        },
        created_at: "2026-04-09T12:07:00Z",
      },
    ]);

    expect(entries).toEqual([
      {
        request_id: "request-2",
        resource: "env/prod-db",
        reason: "Needs incident access",
        requested_by: "bob",
        policy_mode: null,
        submitted_at: "2026-04-09T12:05:00Z",
        recorded_at: "2026-04-09T12:06:00Z",
        approval_status: "approved",
        final_decision: "allow",
        reviewed_by: "reviewer.alice",
        decision_note: "Approved for incident response",
      },
    ]);
  });

  it("orders resolved review requests from newest to oldest", () => {
    const entries = getResolvedReviewRequestEntries([
      {
        id: "audit-20",
        request_id: "request-20",
        action: "request_submitted",
        actor: "alice",
        note: "First review request",
        payload: {
          resource: "secret/first-review",
          policy_mode: "assisted",
        },
        created_at: "2026-04-09T12:01:00Z",
      },
      {
        id: "audit-21",
        request_id: "request-21",
        action: "request_submitted",
        actor: "bob",
        note: "Second review request",
        payload: {
          resource: "secret/second-review",
          policy_mode: "manual_only",
        },
        created_at: "2026-04-09T12:02:00Z",
      },
      {
        id: "audit-22",
        request_id: "request-21",
        action: "approval_recorded",
        actor: "reviewer.bob",
        note: "Second approved",
        payload: {
          approval_status: "approved",
          decision: "allow",
        },
        created_at: "2026-04-09T12:04:00Z",
      },
      {
        id: "audit-23",
        request_id: "request-20",
        action: "approval_recorded",
        actor: "reviewer.alice",
        note: "First approved",
        payload: {
          approval_status: "approved",
          decision: "allow",
        },
        created_at: "2026-04-09T12:03:00Z",
      },
    ]);

    expect(entries.map((entry) => entry.request_id)).toEqual([
      "request-21",
      "request-20",
    ]);
  });

  it("extracts suggestion summary for resolved review detail rendering", () => {
    const summary = getSuggestionSummary([
      {
        id: "audit-8",
        request_id: "request-4",
        action: "llm_suggestion_generated",
        actor: "claude",
        note: "Provider suggests allow",
        payload: {
          suggested_decision: "allow",
          risk_score: 12,
          template_version: "4",
          provider_model: "claude-3-7-sonnet-20250219",
        },
        created_at: "2026-04-09T12:08:00Z",
      },
    ]);

    expect(summary).toEqual({
      provider_kind: "claude",
      provider_model: "claude-3-7-sonnet-20250219",
      suggested_decision: "allow",
      rationale_summary: "Provider suggests allow",
      risk_score: 12,
      template_version: "4",
      generated_at: "2026-04-09T12:08:00Z",
      error: null,
    });
  });

  it("extracts ACP provider trace from request audit records", () => {
    const trace = getSuggestionTrace([
      {
        id: "audit-6",
        request_id: "request-4",
        action: "llm_suggestion_generated",
        actor: "acp_codex",
        note: "Low-risk ACP suggestion",
        payload: {
          provider_model: "codex@1.2.3",
          provider_trace: {
            rendered_prompt: null,
            transport: "stdio",
            package_name: "@zed-industries/codex-acp",
            package_version: "0.11.1",
            session_id: "session-123",
            client_request_id: "client-456",
            agent_name: "codex",
            agent_version: "1.2.3",
          },
        },
        created_at: "2026-04-09T12:11:00Z",
      },
    ]);

    expect(trace).toEqual({
      provider_kind: "acp_codex",
      provider_model: "codex@1.2.3",
      provider_response_id: null,
      x_request_id: null,
      usage_total_tokens: null,
      rendered_prompt: null,
      provider_trace: {
        rendered_prompt: null,
        transport: "stdio",
        protocol: null,
        api_version: null,
        output_format: null,
        stop_reason: null,
        package_name: "@zed-industries/codex-acp",
        package_version: "0.11.1",
        session_id: "session-123",
        client_request_id: "client-456",
        agent_name: "codex",
        agent_version: "1.2.3",
        beta_headers: [],
      },
    });
  });

  it("extracts Claude provider trace and identifiers from request audit records", () => {
    const trace = getSuggestionTrace([
      {
        id: "audit-7",
        request_id: "request-5",
        action: "llm_suggestion_generated",
        actor: "claude",
        note: "Claude low-risk suggestion",
        payload: {
          provider_model: "claude-3-7-sonnet-20250219",
          provider_response_id: "msg_123",
          x_request_id: "req_claude_123",
          provider_trace: {
            rendered_prompt: "Review config/dev-readonly",
            transport: "https",
            protocol: "anthropic_messages",
            api_version: "2023-06-01",
            output_format: "json_schema",
            stop_reason: "end_turn",
          },
        },
        created_at: "2026-04-09T12:12:00Z",
      },
    ]);

    expect(trace).toEqual({
      provider_kind: "claude",
      provider_model: "claude-3-7-sonnet-20250219",
      provider_response_id: "msg_123",
      x_request_id: "req_claude_123",
      usage_total_tokens: null,
      rendered_prompt: "Review config/dev-readonly",
      provider_trace: {
        rendered_prompt: "Review config/dev-readonly",
        transport: "https",
        protocol: "anthropic_messages",
        api_version: "2023-06-01",
        output_format: "json_schema",
        stop_reason: "end_turn",
        package_name: null,
        package_version: null,
        session_id: null,
        client_request_id: null,
        agent_name: null,
        agent_version: null,
        beta_headers: [],
      },
    });
  });

  it("groups an automatic approval into semantic audit phases", () => {
    const groups = buildAuditApprovalGroups([
      {
        id: "event-4",
        request_id: "request-auto",
        action: "approval_recorded",
        actor: "system_auto",
        note: "Automatically approved",
        payload: { approval_status: "approved", decision: "allow" },
        created_at: "2026-08-04T14:49:30Z",
      },
      {
        id: "event-1",
        request_id: "request-auto",
        action: "request_submitted",
        actor: "jpx",
        note: "Read deployment username",
        payload: {
          resource: "secret/stke-username",
          policy_mode: "llm_automatic",
        },
        created_at: "2026-08-04T14:49:15Z",
      },
      {
        id: "event-3",
        request_id: "request-auto",
        action: "automatic_decision_recorded",
        actor: "system_auto",
        note: "Low risk",
        payload: { auto_disposition: "allow", risk_score: 12 },
        created_at: "2026-08-04T14:49:29Z",
      },
      {
        id: "event-2",
        request_id: "request-auto",
        action: "llm_suggestion_generated",
        actor: "acp",
        note: "Allow",
        payload: { suggested_decision: "allow", risk_score: 12 },
        created_at: "2026-08-04T14:49:28Z",
      },
    ]);

    expect(groups).toHaveLength(1);
    expect(groups[0]).toMatchObject({
      request_id: "request-auto",
      resource: "secret/stke-username",
      requested_by: "jpx",
      result: "approved",
      decision_path: "automatic",
      risk_score: 12,
      duration_ms: 15_000,
    });
    expect(groups[0].events.map((event) => event.id)).toEqual([
      "event-1",
      "event-2",
      "event-3",
      "event-4",
    ]);
    expect(groups[0].phases.map((phase) => phase.kind)).toEqual([
      "request",
      "model",
      "policy",
      "final",
    ]);
  });

  it("keeps paired escalation and human audit events under one approval", () => {
    const groups = buildAuditApprovalGroups([
      {
        id: "human-1",
        request_id: "request-human",
        action: "request_submitted",
        actor: "jpx",
        note: "Read deployment token",
        payload: { resource: "secret/stke-token" },
        created_at: "2026-08-04T14:49:30Z",
      },
      {
        id: "human-2",
        request_id: "request-human",
        action: "llm_suggestion_generated",
        actor: "acp",
        note: "Needs review",
        payload: { suggested_decision: "escalate", risk_score: 61 },
        created_at: "2026-08-04T14:49:47Z",
      },
      {
        id: "human-3",
        request_id: "request-human",
        action: "automatic_decision_recorded",
        actor: "system_auto",
        note: "Escalated",
        payload: { auto_disposition: "escalate", risk_score: 61 },
        created_at: "2026-08-04T14:49:47.001Z",
      },
      {
        id: "human-4",
        request_id: "request-human",
        action: "automatic_escalated_to_human",
        actor: "system_auto",
        note: "Human review required",
        payload: { auto_disposition: "escalate" },
        created_at: "2026-08-04T14:49:47.002Z",
      },
      {
        id: "human-5",
        request_id: "request-human",
        action: "approval_recorded",
        actor: "desktop-reviewer",
        note: null,
        payload: { approval_status: "approved", decision: "allow" },
        created_at: "2026-08-04T14:49:53Z",
      },
      {
        id: "human-6",
        request_id: "request-human",
        action: "human_decision_overrode_llm",
        actor: "desktop-reviewer",
        note: null,
        payload: { decision: "allow", suggested_decision: "escalate" },
        created_at: "2026-08-04T14:49:53.001Z",
      },
    ]);

    expect(groups[0]).toMatchObject({
      result: "approved",
      decision_path: "human",
      final_actor: "desktop-reviewer",
      risk_score: 61,
    });
    expect(groups[0].events).toHaveLength(6);
    expect(
      groups[0].phases.find((phase) => phase.kind === "policy")?.events,
    ).toHaveLength(2);
    expect(
      groups[0].phases.find((phase) => phase.kind === "human")?.events,
    ).toHaveLength(2);
    expect(
      groups[0].phases
        .find((phase) => phase.kind === "human")
        ?.events.map((event) => event.id),
    ).toEqual(["human-5", "human-6"]);
    expect(auditResultFor(groups[0].events[4])).toBe("approved");
  });
});
