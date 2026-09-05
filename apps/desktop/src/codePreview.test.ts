import { describe, expect, it } from "vitest";

import {
  getPreviewHighlightResult,
  getSupportedPreviewExtensions,
} from "./codePreview";

describe("codePreview", () => {
  it("supports the common script extensions required by the UI contract", () => {
    expect(getSupportedPreviewExtensions()).toEqual([
      "bash",
      "bat",
      "cjs",
      "cmd",
      "fish",
      "js",
      "jsx",
      "mjs",
      "ps1",
      "py",
      "sh",
      "zsh",
    ]);
  });

  it("highlights supported script previews", () => {
    const result = getPreviewHighlightResult(
      "/tmp/review-script.ps1",
      'Write-Host "hello"',
    );

    expect(result.highlighted).toBe(true);
    expect(result.label).toBe("ps1");
    expect(result.html).toContain("hljs");
  });

  it("falls back to escaped plain text for unsupported extensions", () => {
    const result = getPreviewHighlightResult(
      "/tmp/review-script.txt",
      "<xml>raw</xml>",
    );

    expect(result.highlighted).toBe(false);
    expect(result.label).toBe("plain text");
    expect(result.html).toBe("&lt;xml&gt;raw&lt;/xml&gt;");
  });

  it("uses shebang fallback when the path extension is missing", () => {
    const result = getPreviewHighlightResult(
      "/tmp/review-script",
      "#!/usr/bin/env bash\necho test\n",
    );

    expect(result.highlighted).toBe(true);
    expect(result.label).toBe("bash");
    expect(result.html).toContain("hljs");
  });

  it("uses python shebang fallback when the extension is unreliable", () => {
    const result = getPreviewHighlightResult(
      "/tmp/review-script.data",
      "#!/usr/bin/python3\nprint('ok')\n",
    );

    expect(result.highlighted).toBe(true);
    expect(result.label).toBe("python");
    expect(result.html).toContain("hljs");
  });

  it("recognizes Python heredocs before the surrounding shell extension", () => {
    const result = getPreviewHighlightResult(
      "/tmp/review-script.sh",
      "python3 <<'PY'\nfor item in range(3):\n    print(item)\nPY\n",
    );

    expect(result.highlighted).toBe(true);
    expect(result.label).toBe("python heredoc");
    expect(result.html).toContain("hljs-keyword");
  });

  it("recognizes JavaScript heredocs and extensionless JavaScript", () => {
    const heredoc = getPreviewHighlightResult(
      null,
      "node <<'JS'\nconst answer = 42;\nconsole.log(answer);\nJS\n",
    );
    const inferred = getPreviewHighlightResult(
      null,
      "const answer = values.map((value) => value.id);",
    );

    expect(heredoc.label).toBe("javascript heredoc");
    expect(heredoc.html).toContain("hljs-keyword");
    expect(inferred.label).toBe("javascript");
    expect(inferred.html).toContain("hljs-keyword");
  });

  it("recognizes an extensionless bash command without coloring prose", () => {
    const bashResult = getPreviewHighlightResult(
      null,
      'set -euo pipefail\nexport TARGET=production\necho "${TARGET}"',
    );
    const proseResult = getPreviewHighlightResult(
      null,
      "Review the production target before approval.",
    );

    expect(bashResult.label).toBe("bash");
    expect(bashResult.highlighted).toBe(true);
    expect(proseResult.highlighted).toBe(false);
  });
});
