import { describe, expect, it } from "vitest";
import { executionEvidenceSources } from "./executionEvidence";

describe("captured execution evidence", () => {
  it("preserves Node stdin code and captured file contents without inventing argv provenance", () => {
    const result = executionEvidenceSources(
      JSON.stringify([
        {
          node_index: 0,
          source_id: "execution:0:stdin",
          language: "javascript",
          kind: "stdin",
          content: "const key = process.env.TOKEN;\nconsume(key);",
        },
        {
          node_index: 1,
          source_id: "file:/workspace/client.py",
          language: "python",
          kind: "file",
          content: "consume(os.environ['TOKEN'])",
        },
      ]),
    );
    expect(result).toHaveLength(2);
    expect(result[0].lines[1]).toEqual({ line: 2, text: "consume(key);" });
    expect(result[0]).not.toHaveProperty("argument_index");
    expect(result[1].node_index).toBe(1);
    expect(result[1].lines[0].text).toBe("consume(os.environ['TOKEN'])");
  });

  it("keeps historical source evidence if an old metadata payload cannot be parsed", () => {
    const historical = {
      node_index: 0,
      source_id: "legacy",
      language: "python",
      lines: [{ line: 1, text: "consume()" }],
    };
    expect(executionEvidenceSources("invalid", [historical])).toEqual([
      historical,
    ]);
    expect(
      executionEvidenceSources('[{"content":"missing identity"}]', [
        historical,
      ]),
    ).toEqual([historical]);
  });
});
