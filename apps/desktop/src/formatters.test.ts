import { describe, expect, it } from "vitest";

import {
  formatDecision,
  formatElapsed,
  formatShortId,
  formatStatus,
  formatTimestamp,
} from "./formatters";

describe("formatters", () => {
  it("formats snake_case statuses", () => {
    expect(formatStatus("manual_only")).toBe("Manual Only");
  });

  it("formats kebab-case labels", () => {
    expect(formatStatus("request-submitted")).toBe("Request Submitted");
  });

  it("shows pending when there is no final decision", () => {
    expect(formatDecision(null)).toBe("Pending");
  });

  it("returns fallback text for missing timestamps", () => {
    expect(formatTimestamp(null, "Never")).toBe("Never");
  });

  it("formats elapsed timestamps into compact labels", () => {
    expect(
      formatElapsed("2026-04-09T10:00:00Z", Date.parse("2026-04-09T12:30:00Z")),
    ).toBe("2h ago");
  });

  it("formats elapsed timestamps in Chinese when locale is zh-CN", () => {
    expect(
      formatElapsed(
        "2026-04-09T10:00:00Z",
        Date.parse("2026-04-09T12:30:00Z"),
        "zh-CN",
      ),
    ).toBe("2小时前");
  });

  it("shortens identifiers for compact UI labels", () => {
    expect(formatShortId("1234567890abcdef")).toBe("12345678...");
  });
});
