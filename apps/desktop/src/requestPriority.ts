import type { AccessRequest } from "./types";

export type RequestGroup = "awaiting" | "evaluating" | "completed";

export function requestGroup(request: AccessRequest): RequestGroup {
  if (request.approval_status !== "pending" || request.resolved_at) {
    return "completed";
  }
  if (
    request.evaluation_state === "queued" ||
    request.evaluation_state === "running"
  ) {
    return "evaluating";
  }
  // Failed and interrupted evaluations need a human decision, too.
  return "awaiting";
}

export function preferredRequestGroup(
  requests: readonly AccessRequest[],
): RequestGroup {
  const groups = requests.map(requestGroup);
  if (groups.includes("awaiting")) return "awaiting";
  if (groups.includes("evaluating")) return "evaluating";
  return "completed";
}
