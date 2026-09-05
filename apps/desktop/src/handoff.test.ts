import { describe, expect, it } from "vitest";

import {
  normalizeHandoffRequestId,
  resolvePendingHandoffRequestId,
  surfaceForWindowLabel,
} from "./handoff";
import type { DashboardData } from "./types";

const dashboard: DashboardData = {
  pending_requests: [
    {
      id: "request-123",
      context: {
        resource: "secret/demo",
        reason: "Need access",
        requested_by: "alice",
        script_path: null,
        call_chain: [],
        env_vars: {},
        metadata: {},
        created_at: "2026-04-10T00:00:00Z",
      },
      policy_mode: "manual_only",
      approval_status: "pending",
      evaluation_state: "not_required",
      final_decision: null,
      provider_kind: null,
      rendered_prompt: "",
      llm_suggestion: null,
      automatic_decision: null,
      created_at: "2026-04-10T00:00:00Z",
      updated_at: "2026-04-10T00:00:00Z",
      resolved_at: null,
    },
  ],
  recent_audit_records: [],
};

describe("handoff selection", () => {
  it("routes only the approval window to the compact surface", () => {
    expect(surfaceForWindowLabel("approval")).toBe("compact");
    expect(surfaceForWindowLabel("password-change")).toBe("password-change");
    expect(surfaceForWindowLabel("main")).toBe("main");
    expect(surfaceForWindowLabel("unexpected")).toBe("main");
  });

  it("normalizes request ids", () => {
    expect(normalizeHandoffRequestId("  request-123  ")).toBe("request-123");
    expect(normalizeHandoffRequestId("   ")).toBeNull();
  });

  it("resolves a pending request id when it exists", () => {
    expect(resolvePendingHandoffRequestId(dashboard, "request-123")).toBe(
      "request-123",
    );
  });

  it("returns null when the handoff request is not in the queue yet", () => {
    expect(resolvePendingHandoffRequestId(dashboard, "request-999")).toBeNull();
  });
});
