import type { SanitizedPromptContext } from "./types";

export type EvidenceSource = Omit<
  SanitizedPromptContext["inline_sources"][number],
  "argument_index"
>;

// Display snapshots using their recorded IDs. Stdin source has no argv index.
export function executionEvidenceSources(
  captured: string | null | undefined,
  inline: EvidenceSource[] = [],
): EvidenceSource[] {
  const sources = new Map(
    inline.map((source) => [
      `${source.node_index}:${source.source_id}`,
      source,
    ]),
  );
  if (!captured) return [...sources.values()];
  let parsed: unknown;
  try {
    parsed = JSON.parse(captured);
  } catch {
    return [...sources.values()];
  }
  if (!Array.isArray(parsed)) return [...sources.values()];
  for (const candidate of parsed as unknown[]) {
    if (!candidate || typeof candidate !== "object") continue;
    const source = candidate as Record<string, unknown>;
    if (
      typeof source.source_id !== "string" ||
      typeof source.node_index !== "number" ||
      !Number.isInteger(source.node_index) ||
      source.node_index < 0 ||
      typeof source.language !== "string" ||
      typeof source.content !== "string"
    )
      continue;
    const evidence: EvidenceSource = {
      source_id: source.source_id,
      node_index: source.node_index,
      language: source.language,
      lines: source.content
        .split("\n")
        .map((text: string, index: number) => ({ line: index + 1, text })),
    };
    sources.set(`${evidence.node_index}:${evidence.source_id}`, evidence);
  }
  return [...sources.values()];
}
