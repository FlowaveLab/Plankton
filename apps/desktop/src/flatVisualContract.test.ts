import { readFileSync } from "node:fs";
import { resolve } from "node:path";
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
});
