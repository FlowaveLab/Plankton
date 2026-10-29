import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { parse } from "postcss";
import { describe, expect, it } from "vitest";

const visualSources = [
  "src/styles.css",
  "src/components/ChoiceGroup.css",
  "src/components/approval-chat.css",
  "src/components/SecretInput.css",
  "src/components/desktop/workspace.css",
  "src/components/desktop/password-vault.css",
  "src/components/CompactApproval.tsx",
  "src/components/PasswordChangeConfirmation.tsx",
  "src/components/ExposurePolicy.tsx",
].map((path) => ({
  path,
  source: readFileSync(resolve(process.cwd(), path), "utf8"),
}));

function declarationsFor(
  path: string,
  selector: string,
): Record<string, string> {
  const source = visualSources.find((entry) => entry.path === path)?.source;
  if (!source) throw new Error(`Missing visual source: ${path}`);
  const declarations: Record<string, string> = {};
  for (const node of parse(source).nodes) {
    if (
      node.type !== "rule" ||
      !node.selectors.some((value) => value.replace(/\s+/g, " ") === selector)
    ) {
      continue;
    }
    for (const child of node.nodes) {
      if (child.type === "decl") declarations[child.prop] = child.value;
    }
  }
  return declarations;
}

describe("flat visual contract", () => {
  it("limits interface and chart colors to paper, white, black, and red tones", () => {
    for (const { path, source } of visualSources) {
      const hexColors = Array.from(
        source.matchAll(/#([0-9a-f]{6}|[0-9a-f]{3})\b/gi),
        (match) => {
          const hex =
            match[1].length === 3
              ? [...match[1]].map((digit) => digit + digit).join("")
              : match[1];
          return [0, 2, 4].map((offset) =>
            Number.parseInt(hex.slice(offset, offset + 2), 16),
          );
        },
      );
      const rgbColors = Array.from(
        source.matchAll(/rgba?\(\s*(\d+)[,\s]+(\d+)[,\s]+(\d+)/g),
        (match) => match.slice(1, 4).map(Number),
      );
      for (const [red, green, blue] of [...hexColors, ...rgbColors]) {
        const allowed =
          red >= green &&
          green >= blue &&
          green - blue <= Math.max(24, (red - green) * 0.35);
        expect(allowed, `${path}: rgb(${red}, ${green}, ${blue})`).toBe(true);
      }
    }
  });

  it("keeps every declared component corner square", () => {
    const globalStyles = visualSources.find(
      ({ path }) => path === "src/styles.css",
    )?.source;
    expect(globalStyles).toContain("--radius-sm: 0;");
    expect(globalStyles).toMatch(/border-radius:\s*0\s*!important;/);

    for (const { path, source } of visualSources) {
      const declarations = Array.from(
        source.matchAll(/border-radius:\s*([^;}\n]+)/g),
        (match) => match[1].trim(),
      );
      for (const declaration of declarations) {
        expect(["0", "0 !important", "var(--radius-sm)"], path).toContain(
          declaration,
        );
      }
    }
  });

  it("removes native depth from every select and uses a flat chevron", () => {
    const globalStyles = visualSources.find(
      ({ path }) => path === "src/styles.css",
    )?.source;
    const workspaceStyles = visualSources.find(
      ({ path }) => path === "src/components/desktop/workspace.css",
    )?.source;

    expect(globalStyles).toMatch(
      /select\s*{[^}]*appearance:\s*none;[^}]*box-shadow:\s*none\s*!important;/s,
    );
    expect(workspaceStyles).toMatch(
      /\.desktop-workspace select\s*{[^}]*appearance:\s*none;[^}]*background-image:\s*url\([^}]*box-shadow:\s*none;/s,
    );
  });

  it("keeps nested approval and exposure sections open instead of stacking cards", () => {
    const workspacePath = "src/components/desktop/workspace.css";
    const exposurePath = "src/components/desktop/password-vault.css";
    for (const selector of [
      ".desktop-workspace .request-review-controls",
      ".desktop-workspace .request-dossier-entry",
      ".desktop-workspace .request-call-chain-entry",
      ".desktop-workspace .diagnostic-errors-section",
    ]) {
      expect(declarationsFor(workspacePath, selector), selector).toMatchObject({
        border: "0",
        background: "transparent",
      });
    }
    for (const selector of [
      ".exposure-policy-editor",
      ".exposure-policy-summary",
      ".exposure-policy-radar-editor",
    ]) {
      expect(declarationsFor(exposurePath, selector), selector).toMatchObject({
        border: "0",
        padding: "0",
      });
    }
    expect(
      declarationsFor(exposurePath, ".exposure-policy-workbench")[
        "grid-template-columns"
      ],
    ).toBe("minmax(0, 1.1fr) minmax(0, 0.9fr)");
    expect(
      declarationsFor(exposurePath, ".exposure-policy-editor")[
        "container-type"
      ],
    ).toBe("inline-size");
    expect(
      declarationsFor(
        exposurePath,
        ":is(.desktop-workspace, .compact-approval) .exposure-axis-editor fieldset",
      ),
    ).toMatchObject({ border: "0", padding: "0" });
  });

  it("avoids duplicate section rules and keeps radar help below its title", () => {
    expect(
      declarationsFor(
        "src/components/desktop/workspace.css",
        ".desktop-workspace .settings-section > .settings-option:last-child",
      )["border-bottom"],
    ).toBe("0");
    expect(
      declarationsFor(
        "src/components/desktop/password-vault.css",
        ".exposure-policy-radar-editor > header",
      ).display,
    ).toBe("grid");
  });
});
