import type { DashboardData } from "./types";

export type DesktopSurface = "compact" | "password-change" | "main";

export function surfaceForWindowLabel(label: string): DesktopSurface {
  if (label === "approval") return "compact";
  if (label === "password-change") return "password-change";
  return "main";
}

export function normalizeHandoffRequestId(
  value: string | null | undefined,
): string | null {
  const trimmed = value?.trim();
  return trimmed ? trimmed : null;
}

export function resolvePendingHandoffRequestId(
  dashboard: DashboardData,
  requestId: string | null,
): string | null {
  const normalized = normalizeHandoffRequestId(requestId);
  if (!normalized) {
    return null;
  }

  return (
    dashboard.pending_requests.find((request) => request.id === normalized)
      ?.id ?? null
  );
}
