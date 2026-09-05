import { describe, expect, it } from "vitest";
import { preferredRequestGroup, requestGroup } from "./requestPriority";
import type { AccessRequest } from "./types";

function request(overrides: Partial<AccessRequest> = {}): AccessRequest {
  return {
    id: "test-request",
    context: {
      resource: "secret/test",
      reason: "Test",
      requested_by: "test-agent",
      script_path: null,
      call_chain: [],
      env_vars: {},
      metadata: {},
      resource_tags: [],
      resource_metadata: {},
      created_at: "2026-09-05T00:00:00Z",
    },
    policy_mode: "llm_automatic",
    approval_status: "pending",
    evaluation_state: "completed",
    final_decision: null,
    provider_kind: "acp",
    rendered_prompt: "",
    llm_suggestion: null,
    automatic_decision: null,
    created_at: "2026-09-05T00:00:00Z",
    updated_at: "2026-09-05T00:00:00Z",
    resolved_at: null,
    ...overrides,
  };
}

describe("request queue priority", () => {
  it.each(["failed", "interrupted", "completed", "not_required"] as const)(
    "routes %s evaluations to human review",
    (evaluation_state) => {
      expect(requestGroup(request({ evaluation_state }))).toBe("awaiting");
    },
  );
  it.each(["queued", "running"] as const)(
    "keeps %s evaluations in progress",
    (evaluation_state) => {
      expect(requestGroup(request({ evaluation_state }))).toBe("evaluating");
    },
  );
  it("uses the final decision before a stale evaluation state", () => {
    expect(
      requestGroup(
        request({ approval_status: "approved", evaluation_state: "running" }),
      ),
    ).toBe("completed");
    expect(
      requestGroup(
        request({
          resolved_at: "2026-09-05T00:01:00Z",
          evaluation_state: "failed",
        }),
      ),
    ).toBe("completed");
  });
  it("prioritizes human, then automatic, then historical requests regardless of input order", () => {
    const automatic = request({ evaluation_state: "running" });
    const human = request({ evaluation_state: "failed" });
    const completed = request({ approval_status: "approved" });
    expect(preferredRequestGroup([automatic, completed, human])).toBe(
      "awaiting",
    );
    expect(preferredRequestGroup([completed, automatic])).toBe("evaluating");
    expect(preferredRequestGroup([completed])).toBe("completed");
    expect(preferredRequestGroup([])).toBe("completed");
  });
});
