import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { Menu } from "lucide-react";
import { useCallback, useEffect, useRef, useState, type JSX } from "react";

import type { Locale } from "../i18n";
import type { DashboardData, DecisionCommand } from "../types";
import { BrandMark, workspaceIcons } from "./desktop/icons";
import {
  OperationsPage,
  type SettingsPageController,
} from "./desktop/OperationsPages";
import { Drawer } from "./desktop/PagePrimitives";
import {
  PasswordVaultPage,
  type PasswordMigrationHandoff,
} from "./desktop/PasswordVaultPage";
import { workspaceNav, type WorkspaceView } from "./desktop/workspaceTypes";

type DesktopWorkspaceProps = {
  locale: Locale;
  dashboard?: DashboardData | null;
  errorMessage: string | null;
  focusedRequestId: string | null;
  isSubmitting: boolean;
  noteDraft: string;
  onDismissError: () => void;
  onDecision: (requestId: string, decision: DecisionCommand) => Promise<void>;
  onLocaleChange: (locale: Locale) => void;
  onNoteChange: (note: string) => void;
  settingsController: SettingsPageController;
};

function navLabel(locale: Locale, en: string, zh: string): string {
  return locale === "zh-CN" ? zh : en;
}

export function DesktopWorkspace(props: DesktopWorkspaceProps): JSX.Element {
  const [activeView, setActiveView] = useState<WorkspaceView>("requests");
  const [navigationOpen, setNavigationOpen] = useState(false);
  const [incomingDraftId, setIncomingDraftId] = useState<string | null>(null);
  const [incomingEditItemId, setIncomingEditItemId] = useState<string | null>(
    null,
  );
  const [incomingMigration, setIncomingMigration] =
    useState<PasswordMigrationHandoff | null>(null);
  const [incomingVaultManager, setIncomingVaultManager] = useState(false);
  const [draftError, setDraftError] = useState<string | null>(null);
  const workspaceContentRef = useRef<HTMLDivElement>(null);
  const zh = props.locale === "zh-CN";
  const navigateTo = useCallback((view: WorkspaceView): void => {
    setActiveView(view);
    setNavigationOpen(false);
  }, []);

  useEffect(() => {
    if (workspaceContentRef.current) {
      workspaceContentRef.current.scrollTop = 0;
    }
  }, [activeView]);

  useEffect(() => {
    if (props.focusedRequestId) {
      navigateTo("requests");
    }
  }, [navigateTo, props.focusedRequestId]);

  useEffect(() => {
    function handleKeyDown(event: KeyboardEvent): void {
      if ((event.metaKey || event.ctrlKey) && event.key.toLowerCase() === "k") {
        event.preventDefault();
        navigateTo("passwords");
      }
    }
    window.addEventListener("keydown", handleKeyDown);
    return () => window.removeEventListener("keydown", handleKeyDown);
  }, [navigateTo]);

  useEffect(() => {
    if (
      !(window as Window & { __TAURI_INTERNALS__?: object }).__TAURI_INTERNALS__
    ) {
      return;
    }
    let active = true;
    void invoke<string | null>("consume_password_draft")
      .then((draftId) => {
        if (active && draftId) {
          setIncomingDraftId(draftId);
          navigateTo("passwords");
        }
      })
      .catch((reason: unknown) => {
        setDraftError(
          reason instanceof Error ? reason.message : String(reason),
        );
      });
    void invoke<string | null>("consume_password_edit")
      .then((itemId) => {
        if (active && itemId) {
          setIncomingEditItemId(itemId);
          navigateTo("passwords");
        }
      })
      .catch((reason: unknown) => {
        setDraftError(
          reason instanceof Error ? reason.message : String(reason),
        );
      });
    void invoke<PasswordMigrationHandoff | null>("consume_password_migration")
      .then((handoff) => {
        if (active && handoff) {
          setIncomingMigration(handoff);
          navigateTo("passwords");
        }
      })
      .catch((reason: unknown) => {
        setDraftError(
          reason instanceof Error ? reason.message : String(reason),
        );
      });
    void invoke<boolean>("consume_local_vault_manager")
      .then((open) => {
        if (active && open) {
          setIncomingVaultManager(true);
          navigateTo("passwords");
        }
      })
      .catch((reason: unknown) => {
        setDraftError(
          reason instanceof Error ? reason.message : String(reason),
        );
      });
    const unlisten = listen<string>("plankton://password-draft", (event) => {
      if (event.payload) {
        setIncomingDraftId(event.payload);
        navigateTo("passwords");
      }
    });
    const unlistenNavigation = listen<string>(
      "plankton://navigate",
      (event) => {
        const target = workspaceNav.find(
          (entry) => entry.id === event.payload,
        )?.id;
        if (target) {
          navigateTo(target);
        }
      },
    );
    const unlistenEdit = listen<string>("plankton://password-edit", (event) => {
      if (event.payload) {
        setIncomingEditItemId(event.payload);
        navigateTo("passwords");
      }
    });
    const unlistenMigration = listen<PasswordMigrationHandoff>(
      "plankton://password-migration",
      (event) => {
        if (event.payload) {
          setIncomingMigration(event.payload);
          navigateTo("passwords");
        }
      },
    );
    const unlistenVaultManager = listen(
      "plankton://local-vault-manager",
      () => {
        setIncomingVaultManager(true);
        navigateTo("passwords");
      },
    );
    return () => {
      active = false;
      void unlisten.then((dispose) => dispose());
      void unlistenNavigation.then((dispose) => dispose());
      void unlistenEdit.then((dispose) => dispose());
      void unlistenMigration.then((dispose) => dispose());
      void unlistenVaultManager.then((dispose) => dispose());
    };
  }, [navigateTo]);

  useEffect(() => {
    if (
      !(window as Window & { __TAURI_INTERNALS__?: object }).__TAURI_INTERNALS__
    ) {
      return;
    }
    const preference = window.matchMedia("(prefers-reduced-motion: reduce)");
    const synchronize = (): void => {
      void invoke("set_tray_reduced_motion", {
        reducedMotion: preference.matches,
      }).catch((reason: unknown) => {
        setDraftError(
          reason instanceof Error ? reason.message : String(reason),
        );
      });
    };
    synchronize();
    preference.addEventListener("change", synchronize);
    return () => preference.removeEventListener("change", synchronize);
  }, []);

  return (
    <main className="desktop-workspace" data-testid="desktop-workspace">
      <aside className="workspace-navigation">
        <div className="workspace-brand">
          <BrandMark />
          <div>
            <strong>PLANKTON</strong>
            <small>{zh ? "本地控制台" : "LOCAL CONSOLE"}</small>
          </div>
        </div>
        <nav aria-label="Workspace navigation">
          {workspaceNav.map((entry) => {
            const NavigationIcon = workspaceIcons[entry.id];
            return (
              <button
                aria-current={
                  activeView === entry.id ||
                  (activeView === "audit" && entry.id === "requests")
                    ? "page"
                    : undefined
                }
                className={
                  activeView === entry.id ||
                  (activeView === "audit" && entry.id === "requests")
                    ? "workspace-nav-item active"
                    : "workspace-nav-item"
                }
                key={entry.id}
                onClick={() => navigateTo(entry.id)}
                type="button"
              >
                <NavigationIcon
                  aria-hidden="true"
                  focusable="false"
                  size={18}
                  strokeWidth={1.75}
                />
                {navLabel(props.locale, entry.en, entry.zh)}
              </button>
            );
          })}
        </nav>
        <button
          aria-expanded={navigationOpen}
          aria-label={zh ? "打开导航菜单" : "Open navigation menu"}
          className="page-icon-button workspace-mobile-menu"
          onClick={() => setNavigationOpen(true)}
          type="button"
        >
          <Menu
            aria-hidden="true"
            focusable="false"
            size={20}
            strokeWidth={1.75}
          />
        </button>
        <div className="workspace-nav-footer">
          <button
            className="locale-toggle"
            onClick={() => props.onLocaleChange(zh ? "en" : "zh-CN")}
            type="button"
          >
            {zh ? "English" : "中文"}
          </button>
          <p>{zh ? "⌘K 打开密码库" : "⌘K opens Passwords"}</p>
        </div>
      </aside>
      <Drawer
        closeLabel={zh ? "关闭导航菜单" : "Close navigation menu"}
        description={
          zh ? "选择一个工作区能力。" : "Choose a workspace capability."
        }
        onClose={() => setNavigationOpen(false)}
        open={navigationOpen}
        title={zh ? "导航" : "Navigation"}
      >
        <nav
          aria-label={zh ? "工作区导航菜单" : "Workspace navigation menu"}
          className="workspace-navigation-drawer"
        >
          {workspaceNav.map((entry) => {
            const NavigationIcon = workspaceIcons[entry.id];
            const active =
              activeView === entry.id ||
              (activeView === "audit" && entry.id === "requests");
            return (
              <button
                aria-current={active ? "page" : undefined}
                className={
                  active ? "workspace-nav-item active" : "workspace-nav-item"
                }
                data-dialog-initial-focus={active ? "true" : undefined}
                key={entry.id}
                onClick={() => navigateTo(entry.id)}
                type="button"
              >
                <NavigationIcon
                  aria-hidden="true"
                  focusable="false"
                  size={18}
                  strokeWidth={1.75}
                />
                {navLabel(props.locale, entry.en, entry.zh)}
              </button>
            );
          })}
        </nav>
      </Drawer>
      <div
        aria-label={zh ? "工作区内容" : "Workspace content"}
        className="workspace-content"
        data-workspace-scroll-viewport="true"
        ref={workspaceContentRef}
        role="region"
        tabIndex={0}
      >
        {props.errorMessage ? (
          <p className="workspace-alert" role="alert">
            {props.errorMessage}
            <button onClick={props.onDismissError} type="button">
              {zh ? "关闭" : "Dismiss"}
            </button>
          </p>
        ) : null}
        {draftError ? (
          <p className="workspace-alert" role="alert">
            {draftError}
            <button onClick={() => setDraftError(null)} type="button">
              {zh ? "关闭" : "Dismiss"}
            </button>
          </p>
        ) : null}
        {activeView === "passwords" ? (
          <PasswordVaultPage
            incomingDraftId={incomingDraftId}
            incomingEditItemId={incomingEditItemId}
            incomingMigration={incomingMigration}
            incomingVaultManager={incomingVaultManager}
            locale={props.locale}
            onDraftConsumed={() => setIncomingDraftId(null)}
            onEditConsumed={() => setIncomingEditItemId(null)}
            onMigrationConsumed={() => setIncomingMigration(null)}
            onVaultManagerConsumed={() => setIncomingVaultManager(false)}
          />
        ) : (
          <OperationsPage
            dashboard={props.dashboard}
            focusedRequestId={props.focusedRequestId}
            isSubmitting={props.isSubmitting}
            locale={props.locale}
            noteDraft={props.noteDraft}
            onDecision={props.onDecision}
            onNavigate={navigateTo}
            onNoteChange={props.onNoteChange}
            settingsController={props.settingsController}
            view={activeView}
          />
        )}
      </div>
    </main>
  );
}
