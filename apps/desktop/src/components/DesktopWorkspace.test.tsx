// @vitest-environment jsdom

import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { act, type ReactNode } from "react";
import ReactDOM from "react-dom/client";
import { afterEach, describe, expect, it, vi } from "vitest";

import type { DashboardData } from "../types";
import { DesktopWorkspace } from "./DesktopWorkspace";

type RenderHarness = { container: HTMLDivElement; unmount: () => void };

Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });

const tauriMocks = vi.hoisted(() => ({
  listeners: new Map<string, (event: { payload: string }) => void>(),
}));

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(async () => null),
}));

vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn(
    (
      eventName: string,
      listener: (event: { payload: string }) => void,
    ): Promise<() => void> => {
      tauriMocks.listeners.set(eventName, listener);
      return Promise.resolve(() => tauriMocks.listeners.delete(eventName));
    },
  ),
}));

const workspaceStyles = readFileSync(
  resolve(process.cwd(), "src/components/desktop/workspace.css"),
  "utf8",
);
const SETTINGS_CONTROLLER = {
  settings: null,
  settingsDraft: null,
  isLoading: true,
  isSaving: false,
  errorMessage: null,
  noticeMessage: null,
  hasUnsavedChanges: false,
  canSave: false,
  validationMessage: null,
  onSave: () => {},
  onReload: () => {},
  onPolicyModeChange: () => {},
  onProviderKindChange: () => {},
  onAcpProfileChange: () => {},
  onFieldChange: () => {},
};

function render(node: ReactNode): RenderHarness {
  const container = document.createElement("div");
  document.body.appendChild(container);
  const root = ReactDOM.createRoot(container);
  act(() => root.render(node));
  return {
    container,
    unmount() {
      act(() => root.unmount());
      container.remove();
    },
  };
}

function installWorkspaceStyles(): CSSStyleSheet {
  const style = document.createElement("style");
  style.textContent = workspaceStyles;
  document.head.appendChild(style);
  const sheet = style.sheet;
  if (!(sheet instanceof CSSStyleSheet)) {
    throw new Error("Workspace stylesheet did not produce a CSSStyleSheet");
  }
  return sheet;
}

function mediaRule(
  sheet: CSSStyleSheet,
  condition: string,
  selector: string,
): CSSStyleRule | undefined {
  const media = Array.from(sheet.cssRules).find(
    (rule) =>
      "conditionText" in rule &&
      (rule as CSSMediaRule).conditionText === condition,
  ) as CSSMediaRule | undefined;
  return Array.from(media?.cssRules ?? [])
    .reverse()
    .find(
      (rule) =>
        "selectorText" in rule &&
        (rule as CSSStyleRule).selectorText
          .split(",")
          .map((entry) => entry.trim())
          .includes(selector),
    ) as CSSStyleRule | undefined;
}

function styleRule(
  sheet: CSSStyleSheet,
  selector: string,
): CSSStyleRule | undefined {
  return Array.from(sheet.cssRules).find(
    (rule) =>
      "selectorText" in rule &&
      (rule as CSSStyleRule).selectorText
        .split(",")
        .map((entry) => entry.trim())
        .includes(selector),
  ) as CSSStyleRule | undefined;
}

function effectiveStyleRule(
  sheet: CSSStyleSheet,
  selector: string,
): CSSStyleRule | undefined {
  return Array.from(sheet.cssRules)
    .reverse()
    .find(
      (rule) =>
        "selectorText" in rule &&
        ((rule as CSSStyleRule).selectorText.replace(/\s+/g, " ").trim() ===
          selector ||
          (rule as CSSStyleRule).selectorText
            .split(",")
            .map((entry) => entry.trim())
            .includes(selector)),
    ) as CSSStyleRule | undefined;
}

function effectiveMediaRule(
  sheet: CSSStyleSheet,
  condition: string,
  selector: string,
): CSSStyleRule | undefined {
  return Array.from(sheet.cssRules)
    .filter(
      (rule) =>
        "conditionText" in rule &&
        (rule as CSSMediaRule).conditionText === condition,
    )
    .reverse()
    .flatMap((rule) => Array.from((rule as CSSMediaRule).cssRules).reverse())
    .find(
      (rule) =>
        "selectorText" in rule &&
        (rule as CSSStyleRule).selectorText
          .split(",")
          .map((entry) => entry.trim())
          .includes(selector),
    ) as CSSStyleRule | undefined;
}

afterEach(() => {
  tauriMocks.listeners.clear();
  delete (
    window as Window & {
      __TAURI_INTERNALS__?: object;
    }
  ).__TAURI_INTERNALS__;
  document.body.innerHTML = "";
  document.head.innerHTML = "";
});

describe("DesktopWorkspace", () => {
  it("renders a custom brand mark and decorative SVG navigation icons", () => {
    const view = render(
      <DesktopWorkspace
        errorMessage={null}
        focusedRequestId={null}
        isSubmitting={false}
        noteDraft=""
        onDecision={async () => {}}
        onDismissError={() => {}}
        locale="en"
        onLocaleChange={() => {}}
        onNoteChange={() => {}}
        settingsController={SETTINGS_CONTROLLER}
      />,
    );

    const brandMark = view.container.querySelector(
      ".workspace-brand svg[data-brand-mark]",
    );
    expect(brandMark).not.toBeNull();
    expect(brandMark?.getAttribute("aria-hidden")).toBe("true");

    const navigation = view.container.querySelector(
      'nav[aria-label="Workspace navigation"]',
    );
    const navigationButtons = Array.from(
      navigation?.querySelectorAll("button") ?? [],
    );
    expect(navigationButtons).toHaveLength(6);
    expect(
      navigationButtons.some((button) => button.textContent === "Audit"),
    ).toBe(false);
    for (const button of navigationButtons) {
      const icon = button.querySelector("svg");
      expect(icon).not.toBeNull();
      expect(icon?.getAttribute("aria-hidden")).toBe("true");
      expect(icon?.getAttribute("focusable")).toBe("false");
      expect(icon?.getAttribute("width")).toBe("18");
      expect(icon?.getAttribute("height")).toBe("18");
      expect(icon?.getAttribute("stroke-width")).toBe("1.75");
    }

    view.unmount();
  });

  it("exposes the content pane as the scrollable workspace viewport", () => {
    const sheet = installWorkspaceStyles();
    const view = render(
      <DesktopWorkspace
        errorMessage={null}
        focusedRequestId={null}
        isSubmitting={false}
        noteDraft=""
        onDecision={async () => {}}
        onDismissError={() => {}}
        locale="en"
        onLocaleChange={() => {}}
        onNoteChange={() => {}}
        settingsController={SETTINGS_CONTROLLER}
      />,
    );

    const viewport = view.container.querySelector(
      '[data-workspace-scroll-viewport="true"]',
    );
    expect(viewport).not.toBeNull();
    expect(viewport?.getAttribute("role")).toBe("region");
    expect(viewport?.getAttribute("aria-label")).toBe("Workspace content");
    expect(
      styleRule(sheet, ".desktop-workspace .workspace-content")?.style
        .overflowY,
    ).toBe("auto");

    view.unmount();
  });

  it("keeps the approval progress rail borderless", () => {
    const sheet = installWorkspaceStyles();
    const rail = styleRule(sheet, ".desktop-workspace .approval-state-rail");

    expect(rail?.style.borderTop).toBe("0px");
    expect(rail?.style.borderBottom).toBe("0px");
  });

  it("keeps language, password filters, and 36px controls in the stylesheet contract", () => {
    const sheet = installWorkspaceStyles();
    const rootRule = styleRule(sheet, ".desktop-workspace");
    expect(rootRule?.style.getPropertyValue("--control-height")).toBe("36px");

    const fields = styleRule(sheet, ".desktop-workspace input");
    expect(fields?.style.minHeight).toBe("var(--control-height)");
    const checkRow = styleRule(sheet, ".desktop-workspace .check-row");
    expect(checkRow?.style.minHeight).toBe("var(--control-height)");
    const alertDismiss = styleRule(
      sheet,
      ".desktop-workspace .workspace-alert button",
    );
    expect(alertDismiss?.style.minHeight).toBe("var(--control-height)");
    expect(alertDismiss?.style.minWidth).toBe("var(--control-height)");

    const compactFooter = mediaRule(
      sheet,
      "(max-width: 940px)",
      ".desktop-workspace .workspace-nav-footer",
    );
    expect(compactFooter?.style.display).toBe("flex");
    const narrowSidebar = mediaRule(
      sheet,
      "(max-width: 620px)",
      ".desktop-workspace .vault-sidebar",
    );
    expect(narrowSidebar?.style.display).not.toBe("none");
    expect(narrowSidebar?.style.overflowY).toBe("auto");

    const policiesBody = styleRule(sheet, ".desktop-workspace .policies-body");
    expect(policiesBody?.style.overflowY).toBe("");
    const saveBar = styleRule(sheet, ".desktop-workspace .settings-save-bar");
    expect(saveBar?.style.position).toBe("sticky");
    expect(saveBar?.style.bottom).toBe("0px");
  });

  it("centers audit connectors and ends the spine at the final decision", () => {
    const sheet = installWorkspaceStyles();
    const connector = styleRule(
      sheet,
      ".desktop-workspace .audit-decision-node:not(:last-child)::after",
    );
    expect(connector?.style.top).toBe("50%");
    expect(connector?.style.transform).toBe("translateY(-50%)");

    const node = styleRule(sheet, ".desktop-workspace .audit-decision-node");
    expect(node?.style.flexGrow).toBe("1");
    expect(node?.style.flexShrink).toBe("1");
    expect(node?.style.flexBasis).toBe("0px");
    const finalNode = styleRule(
      sheet,
      ".desktop-workspace .audit-decision-node:last-child",
    );
    expect(finalNode?.style.flexGrow).toBe("0");
    expect(finalNode?.style.flexShrink).toBe("0");
    expect(finalNode?.style.flexBasis).toBe("auto");
    expect(finalNode?.style.minWidth).toBe("0");
  });

  it("keeps audit evidence independently scrollable while letting full-width chat expand below it", () => {
    const sheet = installWorkspaceStyles();
    const detail = effectiveStyleRule(
      sheet,
      ".desktop-workspace .audit-approval-detail",
    );
    const evidence = effectiveStyleRule(
      sheet,
      ".desktop-workspace .audit-approval-evidence",
    );
    const nestedChain = effectiveStyleRule(
      sheet,
      ".desktop-workspace .audit-call-chain-evidence .request-call-chain-list",
    );
    const mobileDetail = effectiveMediaRule(
      sheet,
      "(max-width: 620px)",
      ".desktop-workspace .audit-approval-detail",
    );
    const chat = effectiveStyleRule(
      sheet,
      ".desktop-workspace .audit-approval-detail > .approval-chat",
    );

    expect(detail?.style.height).toBe("auto");
    expect(detail?.style.maxHeight).toBe("none");
    expect(detail?.style.gridTemplateRows).toContain("72vh");
    expect(detail?.style.overflow).toBe("visible");
    expect(evidence?.style.minHeight).toBe("0");
    expect(evidence?.style.overflowY).toBe("auto");
    expect(chat?.style.gridColumn).toBe("1 / -1");
    expect(nestedChain?.style.maxHeight).toBe("none");
    expect(nestedChain?.style.overflow).toBe("visible");
    expect(mobileDetail?.style.height).toBe("auto");
    expect(mobileDetail?.style.maxHeight).toBe("none");
    expect(mobileDetail?.style.overflow).toBe("visible");
  });

  it("uses a compact red call-chain dossier instead of detached blue cards", () => {
    const sheet = installWorkspaceStyles();
    const heading = effectiveStyleRule(
      sheet,
      ".desktop-workspace .request-call-chain-heading",
    );
    const progress = effectiveStyleRule(
      sheet,
      ".desktop-workspace .request-review-progress",
    );
    const edgeProgress = effectiveStyleRule(
      sheet,
      ".desktop-workspace .request-review-progress--edge",
    );
    const evidenceRow = effectiveStyleRule(
      sheet,
      ":is(.desktop-workspace, .compact-approval) .request-evidence-workbench__row",
    );
    const axisDot = effectiveStyleRule(
      sheet,
      ":is(.desktop-workspace, .compact-approval) .request-evidence-workbench__axis::after",
    );
    const activeNote = effectiveStyleRule(
      sheet,
      ':is(.desktop-workspace, .compact-approval) .request-evidence-workbench__notes li[data-active="true"]',
    );
    const command = effectiveStyleRule(
      sheet,
      ".desktop-workspace .request-call-chain-command",
    );
    const evidenceReference = effectiveStyleRule(
      sheet,
      ":is(.desktop-workspace .workspace-page, .compact-approval) button.request-evidence-reference",
    );

    expect(heading?.style.gridTemplateColumns).toContain("0.62fr");
    expect(progress?.style.height).toBe("auto");
    expect(progress?.style.display).toBe("grid");
    expect(progress?.style.borderRadius).toBe("0");
    expect(edgeProgress?.style.margin).toContain("var(--space-5)");
    expect(edgeProgress?.style.margin).toContain("var(--space-4)");
    expect(evidenceRow?.style.gridTemplateColumns).toContain("22px");
    expect(axisDot?.style.background).toBe("var(--red)");
    expect(activeNote?.style.boxShadow).toContain("var(--red)");
    expect(command?.style.background).toBe("var(--ink)");
    expect(evidenceReference?.style.display).toBe("inline");
    expect(evidenceReference?.style.minHeight).toBe("0");
    expect(evidenceReference?.style.height).toBe("auto");
    expect(evidenceReference?.style.border).toBe("0px");
  });

  it("keeps persistent navigation and opens the guided password entry workflow", () => {
    const view = render(
      <DesktopWorkspace
        errorMessage={null}
        focusedRequestId={null}
        isSubmitting={false}
        noteDraft=""
        onDecision={async () => {}}
        onDismissError={() => {}}
        locale="en"
        onLocaleChange={() => {}}
        onNoteChange={() => {}}
        settingsController={SETTINGS_CONTROLLER}
      />,
    );

    expect(
      view.container.querySelector('nav[aria-label="Workspace navigation"]'),
    ).not.toBeNull();
    expect(view.container.textContent).toContain("Requests");
    expect(view.container.textContent).toContain("Passwords");

    const passwords = Array.from(
      view.container.querySelectorAll("button"),
    ).find((button) => button.textContent?.includes("Passwords"));
    act(() => passwords?.click());
    expect(view.container.textContent).toContain("Add or import");

    const create = Array.from(view.container.querySelectorAll("button")).find(
      (button) => button.textContent === "Add or import",
    );
    act(() => create?.click());
    expect(view.container.textContent).toContain("Create a password");
    expect(view.container.textContent).toContain("Import existing");
    expect(view.container.textContent).not.toContain("plankton password add");
    expect(view.container.querySelector('[role="dialog"]')).not.toBeNull();

    view.unmount();
  });

  it("uses the shared Drawer for compact navigation without hiding capabilities", async () => {
    const sheet = installWorkspaceStyles();
    const view = render(
      <DesktopWorkspace
        errorMessage={null}
        focusedRequestId={null}
        isSubmitting={false}
        noteDraft=""
        onDecision={async () => {}}
        onDismissError={() => {}}
        locale="en"
        onLocaleChange={() => {}}
        onNoteChange={() => {}}
        settingsController={SETTINGS_CONTROLLER}
      />,
    );

    const trigger = view.container.querySelector(
      'button[aria-label="Open navigation menu"]',
    ) as HTMLButtonElement | null;
    expect(trigger?.querySelector("svg.lucide-menu")).not.toBeNull();
    expect(
      styleRule(sheet, ".desktop-workspace .workspace-mobile-menu")?.style
        .display,
    ).toBe("none");
    expect(trigger ? getComputedStyle(trigger).display : undefined).toBe(
      "none",
    );
    expect(
      mediaRule(
        sheet,
        "(max-width: 940px)",
        ".desktop-workspace .workspace-navigation > nav",
      )?.style.display,
    ).toBe("none");
    expect(
      mediaRule(
        sheet,
        "(max-width: 940px)",
        ".desktop-workspace .workspace-mobile-menu",
      )?.style.display,
    ).toBe("inline-grid");

    trigger?.focus();
    act(() => trigger?.click());
    const drawer = view.container.querySelector(
      '[role="dialog"][data-page-drawer="true"]',
    );
    const drawerNavigation = drawer?.querySelector(
      'nav[aria-label="Workspace navigation menu"]',
    );
    const drawerButtons = Array.from(
      drawerNavigation?.querySelectorAll("button") ?? [],
    );
    expect(drawerButtons).toHaveLength(6);
    expect(document.activeElement?.textContent).toContain("Requests");
    expect(
      drawerNavigation?.querySelector('button[aria-current="page"]')
        ?.textContent,
    ).toContain("Requests");

    const agents = drawerButtons.find((button) =>
      button.textContent?.includes("Agents"),
    );
    await act(async () => agents?.click());
    expect(
      view.container.querySelector('[data-page-drawer="true"]'),
    ).toBeNull();
    expect(document.activeElement).toBe(trigger);
    expect(
      view.container.querySelector(
        'nav[aria-label="Workspace navigation"] button[aria-current="page"]',
      )?.textContent,
    ).toContain("Agents");

    trigger?.focus();
    act(() => trigger?.click());
    act(() => {
      document.dispatchEvent(
        new KeyboardEvent("keydown", { bubbles: true, key: "Escape" }),
      );
    });
    expect(
      view.container.querySelector('[data-page-drawer="true"]'),
    ).toBeNull();
    expect(document.activeElement).toBe(trigger);

    view.unmount();
  });

  it("keeps external navigation visible in the compact menu", async () => {
    Object.assign(window, {
      __TAURI_INTERNALS__: {},
      matchMedia: vi.fn(() => ({
        matches: false,
        addEventListener: vi.fn(),
        removeEventListener: vi.fn(),
      })),
    });
    const view = render(
      <DesktopWorkspace
        errorMessage={null}
        focusedRequestId={null}
        isSubmitting={false}
        noteDraft=""
        onDecision={async () => {}}
        onDismissError={() => {}}
        locale="en"
        onLocaleChange={() => {}}
        onNoteChange={() => {}}
        settingsController={SETTINGS_CONTROLLER}
      />,
    );
    const trigger = view.container.querySelector(
      'button[aria-label="Open navigation menu"]',
    ) as HTMLButtonElement | null;
    act(() => trigger?.click());

    await act(async () => {
      tauriMocks.listeners.get("plankton://navigate")?.({
        payload: "policies",
      });
    });
    expect(
      view.container.querySelector('[data-page-drawer="true"]'),
    ).toBeNull();
    expect(
      view.container.querySelector(
        'nav[aria-label="Workspace navigation"] button[aria-current="page"]',
      )?.textContent,
    ).toContain("Policies");

    act(() => trigger?.click());
    expect(
      view.container.querySelector(
        'nav[aria-label="Workspace navigation menu"] button[aria-current="page"]',
      )?.textContent,
    ).toContain("Policies");

    view.unmount();
  });

  it("resets the workspace scroll position when the active view changes", () => {
    const view = render(
      <DesktopWorkspace
        errorMessage={null}
        focusedRequestId={null}
        isSubmitting={false}
        noteDraft=""
        onDecision={async () => {}}
        onDismissError={() => {}}
        locale="en"
        onLocaleChange={() => {}}
        onNoteChange={() => {}}
        settingsController={SETTINGS_CONTROLLER}
      />,
    );
    const viewport = view.container.querySelector(
      '[data-workspace-scroll-viewport="true"]',
    ) as HTMLDivElement | null;
    if (!viewport) {
      throw new Error("Expected workspace viewport");
    }
    viewport.scrollTop = 240;
    const passwords = Array.from(
      view.container.querySelectorAll<HTMLButtonElement>(
        'nav[aria-label="Workspace navigation"] button',
      ),
    ).find((button) => button.textContent?.includes("Passwords"));

    act(() => passwords?.click());
    expect(viewport.scrollTop).toBe(0);

    view.unmount();
  });

  it("does not substitute demo credentials when the daemon is unavailable", async () => {
    const view = render(
      <DesktopWorkspace
        errorMessage={null}
        focusedRequestId={null}
        isSubmitting={false}
        noteDraft=""
        onDecision={async () => {}}
        onDismissError={() => {}}
        locale="en"
        onLocaleChange={() => {}}
        onNoteChange={() => {}}
        settingsController={SETTINGS_CONTROLLER}
      />,
    );
    const passwords = Array.from(
      view.container.querySelectorAll("button"),
    ).find((button) => button.textContent?.includes("Passwords"));
    await act(async () => {
      passwords?.click();
    });

    expect(view.container.textContent).toContain("No passwords yet");
    expect(view.container.textContent).toContain(
      "Daemon catalog is unavailable in this preview.",
    );
    expect(view.container.textContent).not.toContain("Matched tag: production");
    expect(view.container.textContent).not.toContain("Demo state");
    view.unmount();
  });

  it("submits real approve and reject actions and navigates to the audit page", () => {
    const onDecision = vi.fn(async () => {});
    const dashboard: DashboardData = {
      pending_requests: [
        {
          id: "request-1",
          context: {
            resource: "secret/service/token",
            reason: "Deploy the service",
            requested_by: "agent",
            script_path: null,
            call_chain: [],
            env_vars: {},
            metadata: {},
            created_at: "2026-07-29T00:00:00Z",
          },
          policy_mode: "manual_only",
          approval_status: "pending",
          evaluation_state: "not_required",
          final_decision: null,
          provider_kind: null,
          rendered_prompt: "",
          llm_suggestion: null,
          automatic_decision: null,
          created_at: "2026-07-29T00:00:00Z",
          updated_at: "2026-07-29T00:00:00Z",
          resolved_at: null,
        },
      ],
      recent_audit_records: [],
    };
    const view = render(
      <DesktopWorkspace
        dashboard={dashboard}
        errorMessage={null}
        focusedRequestId={null}
        isSubmitting={false}
        locale="en"
        noteDraft=""
        onDecision={onDecision}
        onDismissError={() => {}}
        onLocaleChange={() => {}}
        onNoteChange={() => {}}
        settingsController={SETTINGS_CONTROLLER}
      />,
    );

    const approve = Array.from(view.container.querySelectorAll("button")).find(
      (button) => button.textContent === "Approve",
    );
    act(() => approve?.click());
    expect(onDecision).toHaveBeenCalledWith("request-1", "approve_request");

    const audit = Array.from(view.container.querySelectorAll("button")).find(
      (button) => button.textContent === "Open full audit record",
    );
    act(() => audit?.click());
    expect(view.container.textContent).toContain("No audit records yet");

    view.unmount();
  });
});
