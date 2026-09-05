import type { DesktopSettings } from "./types";

export const ACP_DEFAULT_PROGRAM = "npx";
export const ACP_DEFAULT_ARGS = "-y @agentclientprotocol/codex-acp@latest";
const CODEX_ACP_PACKAGE = "@agentclientprotocol/codex-acp";
const U64_MAX = 18_446_744_073_709_551_615n;
const RUST_SEMVER_PATTERN =
  /^(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)(?:-([0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*))?(?:\+([0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*))?$/;

type AcpSettingsInput = Pick<DesktopSettings, "acp_profile">;

export type AcpProgramSummary = {
  defaultCommand: string;
  currentProgram: string;
  currentArgs: string;
  currentCommand: string;
  usesDefaultStarter: boolean;
};

function formatCommand(program: string, args: string): string {
  return `${program}${args ? ` ${args}` : ""}`;
}

export function normalizeRustSemanticVersion(
  value: string | null | undefined,
): string | null {
  const normalized = value?.trim() ?? "";
  const match = RUST_SEMVER_PATTERN.exec(normalized);
  if (!match) {
    return null;
  }

  for (const component of match.slice(1, 4)) {
    if (BigInt(component) > U64_MAX) {
      return null;
    }
  }

  const prerelease = match[4];
  if (
    prerelease
      ?.split(".")
      .some(
        (identifier) =>
          identifier.length > 1 &&
          identifier.startsWith("0") &&
          /^[0-9]+$/.test(identifier),
      )
  ) {
    return null;
  }

  return normalized;
}

export function buildAcpProgramSummary(
  settings: AcpSettingsInput | null,
): AcpProgramSummary {
  const profile = settings?.acp_profile ?? {
    agent_kind: "codex",
    version_mode: "latest",
  };
  const packageName =
    profile.agent_kind === "claude_code"
      ? "@zed-industries/claude-code-acp"
      : profile.agent_kind === "open_code"
        ? "opencode-ai"
        : CODEX_ACP_PACKAGE;
  const selector =
    profile.version_mode === "pinned" ? profile.version : "latest";
  const currentProgram =
    profile.version_mode === "custom"
      ? profile.program?.trim() || "(missing program)"
      : ACP_DEFAULT_PROGRAM;
  const currentArgs =
    profile.version_mode === "custom"
      ? (profile.args ?? []).join(" ")
      : [
          "-y",
          `${packageName}@${selector || "latest"}`,
          ...(profile.agent_kind === "open_code" ? ["acp"] : []),
        ].join(" ");

  return {
    defaultCommand: formatCommand(ACP_DEFAULT_PROGRAM, ACP_DEFAULT_ARGS),
    currentProgram,
    currentArgs,
    currentCommand: formatCommand(currentProgram, currentArgs),
    usesDefaultStarter:
      profile.agent_kind === "codex" && profile.version_mode === "latest",
  };
}
