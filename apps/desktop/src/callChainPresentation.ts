import type { CallChainEntry, StructuredCallChainNode } from "./types";

const CODE_AGENT_TOKEN =
  /(?:^|[\\/\s@._-])(codex|claude(?:-code)?|opencode|aider|cursor|windsurf|cline|roo(?:-code)?|gemini(?:-cli)?|qwen(?:-code)?|copilot|openhands|goose|amp)(?:$|[\\/\s@._-])/i;

function structuredNodeText(node: StructuredCallChainNode): string {
  return [
    node.process_name,
    node.executable_path,
    node.resolved_file_path,
    ...(node.argv ?? []),
  ]
    .filter((value): value is string => Boolean(value?.trim()))
    .join(" ");
}

export function callChainEntryText(entry: CallChainEntry): string {
  return structuredNodeText(entry);
}

export function isCodeAgentEntry(entry: CallChainEntry): boolean {
  const text = callChainEntryText(entry);
  if (CODE_AGENT_TOKEN.test(text)) {
    return true;
  }

  return /^cc$/i.test(entry.process_name?.trim() ?? "");
}

export function codeAgentBoundaryIndex(callChain: CallChainEntry[]): number {
  return callChain.findIndex(isCodeAgentEntry);
}

export function callChainEntryPath(entry: CallChainEntry): string {
  return (
    entry.resolved_file_path?.trim() ||
    entry.executable_path?.trim() ||
    entry.process_name?.trim() ||
    "Unknown process"
  );
}

export function callChainEntryName(entry: CallChainEntry): string {
  return entry.process_name?.trim() || callChainEntryPath(entry);
}
