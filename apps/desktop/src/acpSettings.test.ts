import { describe, expect, it } from "vitest";

import { ACP_DEFAULT_PROGRAM, buildAcpProgramSummary } from "./acpSettings";

describe("acpSettings", () => {
  it("reports the default Codex starter when ACP settings match defaults", () => {
    const summary = buildAcpProgramSummary({
      acp_profile: {
        agent_kind: "codex",
        version_mode: "latest",
      },
    });

    expect(summary.usesDefaultStarter).toBe(true);
    expect(summary.currentCommand).toBe(
      "npx -y @agentclientprotocol/codex-acp@latest",
    );
  });

  it("reports a custom ACP client", () => {
    const summary = buildAcpProgramSummary({
      acp_profile: {
        agent_kind: "custom",
        version_mode: "custom",
        program: "uvx",
        args: ["my-acp-client", "--stdio"],
      },
    });

    expect(summary.usesDefaultStarter).toBe(false);
    expect(summary.currentProgram).toBe("uvx");
    expect(summary.currentArgs).toBe("my-acp-client --stdio");
    expect(summary.currentCommand).toBe("uvx my-acp-client --stdio");
  });

  it("builds a pinned OpenCode command", () => {
    const summary = buildAcpProgramSummary({
      acp_profile: {
        agent_kind: "open_code",
        version_mode: "pinned",
        version: "1.2.3",
      },
    });

    expect(summary.usesDefaultStarter).toBe(false);
    expect(summary.currentProgram).toBe(ACP_DEFAULT_PROGRAM);
    expect(summary.currentArgs).toBe("-y opencode-ai@1.2.3 acp");
  });

  it("builds a pinned Codex command with the maintained package", () => {
    const summary = buildAcpProgramSummary({
      acp_profile: {
        agent_kind: "codex",
        version_mode: "pinned",
        version: "0.12.3",
      },
    });

    expect(summary.usesDefaultStarter).toBe(false);
    expect(summary.currentCommand).toBe(
      "npx -y @agentclientprotocol/codex-acp@0.12.3",
    );
  });

  it("preserves Claude Code pinned command semantics", () => {
    const summary = buildAcpProgramSummary({
      acp_profile: {
        agent_kind: "claude_code",
        version_mode: "pinned",
        version: "1.4.2",
      },
    });

    expect(summary.currentCommand).toBe(
      "npx -y @zed-industries/claude-code-acp@1.4.2",
    );
  });
});
