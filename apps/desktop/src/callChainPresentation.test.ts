import { describe, expect, it } from "vitest";

import {
  callChainEntryName,
  codeAgentBoundaryIndex,
  isCodeAgentEntry,
} from "./callChainPresentation";
import type { CallChainEntry } from "./types";

describe("callChainPresentation", () => {
  it("finds the first code-agent process in an outer-to-inner call chain", () => {
    const chain: CallChainEntry[] = [
      { process_name: "launchd", executable_path: "/sbin/launchd" },
      {
        process_name: "node",
        executable_path: "/opt/homebrew/bin/node",
        argv: ["node", "@openai/codex", "exec"],
      },
      {
        process_name: "zsh",
        argv: ["zsh", "-lc", "plankton get resource"],
      },
    ];

    expect(codeAgentBoundaryIndex(chain)).toBe(1);
    expect(isCodeAgentEntry(chain[1])).toBe(true);
    expect(isCodeAgentEntry(chain[2])).toBe(false);
  });

  it("recognizes Claude Code and other common coding agents", () => {
    expect(isCodeAgentEntry({ executable_path: "/usr/local/bin/claude" })).toBe(
      true,
    );
    expect(
      isCodeAgentEntry({ executable_path: "/opt/homebrew/bin/opencode" }),
    ).toBe(true);
    expect(isCodeAgentEntry({ executable_path: "/usr/bin/cc" })).toBe(false);
    expect(isCodeAgentEntry({ process_name: "cc", argv: ["cc", "tool"] })).toBe(
      true,
    );
  });

  it("derives a compact readable name for structured entries", () => {
    expect(
      callChainEntryName({
        resolved_file_path: "/workspace/scripts/review.sh",
      }),
    ).toBe("/workspace/scripts/review.sh");
    expect(
      callChainEntryName({
        process_name: "python3",
        executable_path: "/usr/bin/python3",
      }),
    ).toBe("python3");
  });
});
