import { describe, expect, it } from "vitest";

import { t, translateCode } from "./i18n";

describe("i18n", () => {
  it("keeps default UI copy in English", () => {
    expect(t("en", "toolbarTitle")).toBe("Approvals");
  });

  it("renders interpolated UI strings", () => {
    expect(t("zh-CN", "openCount", { count: 3 })).toBe("3 个待处理");
  });

  it("productizes manual_only for external UI labels", () => {
    expect(translateCode("en", "manual_only")).toBe("Human Review");
    expect(translateCode("zh-CN", "manual_only")).toBe("人工审批");
  });

  it("treats ACP provider codes as ACP in user-facing labels", () => {
    expect(translateCode("en", "acp")).toBe("ACP");
    expect(translateCode("zh-CN", "acp")).toBe("ACP");
    expect(translateCode("en", "acp_codex")).toBe("ACP");
    expect(translateCode("zh-CN", "acp_codex")).toBe("ACP");
  });

  it("documents the full prompt i18n variable surface in settings help", () => {
    expect(t("en", "settingsTemplateVariables")).toContain(
      "{{ i18n.current_language }}",
    );
    expect(t("en", "settingsTemplateVariables")).not.toContain(
      "{{ i18n.resource_label }}",
    );
    expect(t("zh-CN", "settingsTemplateVariables")).toContain(
      "{{ i18n.current_language }}",
    );
  });
});
