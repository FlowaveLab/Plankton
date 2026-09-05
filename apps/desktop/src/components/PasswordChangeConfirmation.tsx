import { invoke } from "@tauri-apps/api/core";
import { emit } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { useCallback, useEffect, useMemo, useState, type JSX } from "react";

import { PASSWORD_CATALOG_CHANGED_EVENT } from "../passwordCatalogEvents";
import {
  ExposureRadar,
  parseExposurePolicy,
  type ExposureSurface,
} from "./ExposurePolicy";
import "./desktop/password-vault.css";
import {
  groupPasswordChangeItems,
  type PasswordChangeImpact,
  type PasswordChangeItemDiff,
} from "../passwordChangePresentation";

type PasswordChangeStatus = {
  batch_id: string;
  change_id: string;
  version: number;
  confirmed_version?: number | null;
  state:
    | "pending_confirmation"
    | "confirmed"
    | "committing"
    | "committed"
    | "rejected"
    | "conflict"
    | "failed";
  reason: string;
  requested_by: string;
  diff: {
    items: PasswordChangeItemDiff[];
    changed_items: number;
    changed_fields: number;
    breaking_changes: number;
  };
  successor_change_id?: string | null;
  updated_at: string;
  error?: string | null;
};

const CHANGE_SETTLE_MS = 1_500;

function messageFrom(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

function impactLabel(impact: PasswordChangeImpact): string {
  switch (impact) {
    case "references":
      return "影响现有引用";
    case "locator":
      return "更改上游位置";
    case "refresh":
      return "刷新快照";
    case "delete":
      return "删除";
    case "metadata":
      return "普通修改";
    case "exposure_policy":
      return "暴露面控制";
  }
}

export function PasswordChangeConfirmation(): JSX.Element {
  const [changes, setChanges] = useState<PasswordChangeStatus[]>([]);
  const [isLoading, setIsLoading] = useState(true);
  const [isSubmitting, setIsSubmitting] = useState(false);
  const [errorMessage, setErrorMessage] = useState<string | null>(null);
  const current = changes[0] ?? null;
  const groupedItems = useMemo(
    () => groupPasswordChangeItems(current?.diff.items ?? []),
    [current],
  );
  const isSettling = current
    ? Date.now() - Date.parse(current.updated_at) < CHANGE_SETTLE_MS
    : false;

  const load = useCallback(async (): Promise<PasswordChangeStatus[]> => {
    try {
      const next = await invoke<PasswordChangeStatus[]>(
        "pending_password_changes",
      );
      setChanges(next);
      setErrorMessage(null);
      return next;
    } catch (error) {
      setErrorMessage(messageFrom(error));
      return [];
    } finally {
      setIsLoading(false);
    }
  }, []);

  useEffect(() => {
    void load();
    const timer = window.setInterval(() => {
      if (!isSubmitting) void load();
    }, 600);
    return () => window.clearInterval(timer);
  }, [isSubmitting, load]);

  useEffect(() => {
    if (!isLoading && changes.length === 0 && !errorMessage) {
      void getCurrentWindow().hide();
    }
  }, [changes.length, errorMessage, isLoading]);

  async function closeWhenEmpty(): Promise<void> {
    const remaining = await load();
    if (remaining.length === 0) {
      await getCurrentWindow().hide();
    }
  }

  async function confirm(): Promise<void> {
    if (!current || isSubmitting) return;
    setIsSubmitting(true);
    setErrorMessage(null);
    try {
      const committed = await invoke<PasswordChangeStatus>(
        "confirm_password_change_command",
        {
          changeId: current.change_id,
          confirmedVersion: current.version,
        },
      );
      if (committed.state === "committed") {
        try {
          await emit(PASSWORD_CATALOG_CHANGED_EVENT, {
            change_id: committed.change_id,
          });
        } catch (eventError) {
          console.error(
            "Password catalog refresh event could not be delivered.",
            eventError,
          );
        }
      }
      await closeWhenEmpty();
    } catch (error) {
      setErrorMessage(messageFrom(error));
    } finally {
      setIsSubmitting(false);
    }
  }

  async function reject(): Promise<void> {
    if (!current || isSubmitting) return;
    setIsSubmitting(true);
    setErrorMessage(null);
    try {
      await invoke("reject_password_change_command", {
        changeId: current.change_id,
        note: null,
      });
      await closeWhenEmpty();
    } catch (error) {
      setErrorMessage(messageFrom(error));
    } finally {
      setIsSubmitting(false);
    }
  }

  return (
    <main className="password-change-confirmation">
      <style>{styles}</style>
      <header>
        <div>
          <p className="eyebrow">PASSWORD CHANGE</p>
          <h1>密码库变更确认</h1>
        </div>
        {current ? (
          <span className="queue-position">1 / {changes.length}</span>
        ) : null}
      </header>

      {errorMessage ? (
        <section className="error" role="alert">
          <strong>变更状态无法加载</strong>
          <span>{errorMessage}</span>
        </section>
      ) : null}

      {isLoading && !current ? <p className="empty">正在加载变更…</p> : null}
      {!isLoading && !current && !errorMessage ? (
        <section className="empty">
          <h2>没有等待确认的变更</h2>
          <button onClick={() => void getCurrentWindow().hide()} type="button">
            关闭
          </button>
        </section>
      ) : null}

      {current ? (
        <>
          <section className="change-context">
            <div>
              <span>请求者</span>
              <strong>{current.requested_by}</strong>
            </div>
            <div>
              <span>原因</span>
              <strong>{current.reason}</strong>
            </div>
            <code>{current.change_id}</code>
          </section>

          <section className="change-summary">
            <strong>{groupedItems.length} 个密码项</strong>
            {current.diff.changed_fields > 0 ? (
              <span>{current.diff.changed_fields} 个字段变更</span>
            ) : null}
            {current.diff.breaking_changes > 0 ? (
              <span className="breaking">
                {current.diff.breaking_changes} 项影响引用
              </span>
            ) : null}
            <small>累计版本 {current.version}</small>
          </section>

          <div className="diff-list">
            {groupedItems.map((item) => (
              <section className="item-diff" key={item.item_id}>
                <header>
                  <div>
                    <h2>{item.title}</h2>
                    <span className="item-vaults">
                      保险库：
                      {item.vaults.length > 0
                        ? item.vaults.join("、")
                        : "未记录"}
                    </span>
                    {item.record_ids.length > 1 ? (
                      <span>{item.record_ids.length} 个字段</span>
                    ) : null}
                  </div>
                  <code>{item.item_id}</code>
                </header>
                <dl>
                  {item.entries.map((entry) => (
                    <div key={`${entry.path}:${entry.label}`}>
                      <dt>
                        <strong>{entry.label}</strong>
                        <span data-impact={entry.impact}>
                          {impactLabel(entry.impact)}
                        </span>
                      </dt>
                      <dd>
                        {entry.impact === "exposure_policy" &&
                        parseExposurePolicy(entry.after) ? (
                          <div className="exposure-policy-diff">
                            <ExposureRadar
                              attentionLabel="红色色块：新增暴露范围"
                              breachedSurfaces={parseExposurePolicy(
                                entry.after,
                              )!
                                .surfaces.filter(
                                  (surface) =>
                                    surface.max_level >
                                    (parseExposurePolicy(
                                      entry.before,
                                    )?.surfaces.find(
                                      (before) =>
                                        before.surface === surface.surface,
                                    )?.max_level ?? 0),
                                )
                                .map(
                                  (surface) =>
                                    surface.surface as ExposureSurface,
                                )}
                              primary={parseExposurePolicy(entry.after)!}
                              primaryLabel="修改后"
                              secondary={parseExposurePolicy(entry.before)}
                              secondaryLabel="修改前"
                              locale="zh-CN"
                            />
                            <p className="exposure-policy-mode-diff">
                              <span>访问方式</span>
                              <del>
                                {parseExposurePolicy(entry.before)
                                  ?.access_mode ?? "protected"}
                              </del>
                              <strong aria-hidden="true">→</strong>
                              <ins>
                                {parseExposurePolicy(entry.after)?.access_mode}
                              </ins>
                            </p>
                          </div>
                        ) : (
                          <>
                            {entry.before ? <del>{entry.before}</del> : null}
                            {entry.after ? <ins>{entry.after}</ins> : null}
                          </>
                        )}
                      </dd>
                    </div>
                  ))}
                </dl>
              </section>
            ))}
          </div>

          <footer>
            <button
              disabled={isSubmitting}
              onClick={() => void reject()}
              type="button"
            >
              拒绝
            </button>
            <button
              className="primary"
              disabled={isSubmitting || isSettling || groupedItems.length === 0}
              onClick={() => void confirm()}
              type="button"
            >
              {isSubmitting
                ? "正在确认…"
                : isSettling
                  ? "正在汇总本批变更…"
                  : `确认 ${groupedItems.reduce((sum, item) => sum + item.entries.length, 0)} 项更改`}
            </button>
          </footer>
        </>
      ) : null}
    </main>
  );
}

const styles = `
  :root {
    color-scheme: light;
    color: #171716;
    background: #f4f1ea;
    font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif;
  }
  body {
    min-width: 0;
    min-height: 100vh;
    margin: 0;
    background: #f4f1ea;
  }
  .password-change-confirmation,
  .password-change-confirmation *,
  .password-change-confirmation *::before,
  .password-change-confirmation *::after { box-sizing: border-box; }
  .password-change-confirmation {
    --ink: #171716;
    --paper: #f4f1ea;
    --surface: #fffefb;
    --red: #f2381e;
    --red-dark: #bb2910;
    --rule: #cfcac1;
    --muted: #706d67;
    min-height: 100vh;
    display: flex;
    flex-direction: column;
    gap: 16px;
    padding: 24px;
    color: var(--ink);
    background: var(--paper);
    line-height: 1.45;
  }
  .password-change-confirmation > header {
    display: flex;
    justify-content: space-between;
    align-items: start;
    gap: 20px;
    padding: 18px 20px;
    border-left: 4px solid var(--red);
    color: var(--surface);
    background: var(--ink);
  }
  .password-change-confirmation .eyebrow {
    margin: 0 0 5px;
    color: var(--red);
    font: 800 10px/1.2 ui-monospace, SFMono-Regular, Menlo, monospace;
    letter-spacing: .13em;
  }
  .password-change-confirmation h1 {
    margin: 0;
    font: 600 clamp(26px, 4vw, 36px)/1.05 Georgia, "Times New Roman", serif;
    letter-spacing: -.035em;
  }
  .password-change-confirmation .queue-position {
    flex: none;
    border: 1px solid var(--red);
    border-radius: 0;
    padding: 5px 9px;
    color: var(--surface);
    background: var(--red);
    font: 700 11px/1.2 ui-monospace, SFMono-Regular, Menlo, monospace;
  }
  .password-change-confirmation .error {
    display: grid;
    gap: 4px;
    border: 1px solid var(--ink);
    border-left: 4px solid var(--red);
    background: #fff0ee;
    padding: 12px 14px;
    color: #8f2119;
  }
  .password-change-confirmation .change-context {
    display: grid;
    grid-template-columns: minmax(120px, .65fr) minmax(0, 2fr);
    gap: 12px 20px;
    padding: 16px;
    background: var(--surface);
    border: 1px solid var(--ink);
    border-left: 4px solid var(--red);
    border-radius: 0;
  }
  .password-change-confirmation .change-context div { display: grid; gap: 4px; }
  .password-change-confirmation .change-context span {
    color: var(--muted);
    font-size: 10px;
    font-weight: 800;
    letter-spacing: .08em;
    text-transform: uppercase;
  }
  .password-change-confirmation .change-context code {
    grid-column: 1 / -1;
    color: var(--muted);
    overflow-wrap: anywhere;
    font-size: 11px;
  }
  .password-change-confirmation .change-summary {
    display: flex;
    align-items: center;
    gap: 8px;
    flex-wrap: wrap;
  }
  .password-change-confirmation .change-summary span,
  .password-change-confirmation .change-summary strong,
  .password-change-confirmation .change-summary small {
    border: 1px solid var(--rule);
    border-radius: 0;
    background: var(--surface);
    padding: 5px 9px;
    font-size: 11px;
  }
  .password-change-confirmation .change-summary strong {
    border-color: var(--ink);
    color: var(--surface);
    background: var(--ink);
  }
  .password-change-confirmation .change-summary .breaking {
    border-color: var(--red);
    color: #fff;
    background: var(--red);
    font-weight: 750;
  }
  .password-change-confirmation .diff-list {
    min-height: 0;
    overflow: auto;
    display: grid;
    gap: 12px;
    padding-right: 3px;
    scrollbar-gutter: stable;
  }
  .password-change-confirmation .item-diff {
    overflow: hidden;
    border: 1px solid var(--ink);
    border-radius: 0;
    background: var(--surface);
  }
  .password-change-confirmation .item-diff > header {
    display: flex;
    justify-content: space-between;
    align-items: baseline;
    gap: 12px;
    padding: 14px 16px;
    border-bottom: 1px solid var(--ink);
    background: color-mix(in srgb, var(--paper) 72%, var(--surface));
  }
  .password-change-confirmation .item-diff > header > div {
    display: flex;
    align-items: baseline;
    gap: 8px;
    min-width: 0;
    flex-wrap: wrap;
  }
  .password-change-confirmation .item-diff h2 {
    margin: 0 4px 0 0;
    font: 600 18px/1.15 Georgia, "Times New Roman", serif;
  }
  .password-change-confirmation .item-diff header span {
    flex: none;
    border: 1px solid var(--rule);
    color: var(--muted);
    background: var(--surface);
    padding: 3px 6px;
    font-size: 10px;
  }
  .password-change-confirmation .item-diff header .item-vaults {
    max-width: 240px;
    overflow: hidden;
    border-color: var(--ink);
    text-overflow: ellipsis;
    white-space: nowrap;
    color: var(--ink);
  }
  .password-change-confirmation .item-diff code {
    max-width: 42%;
    overflow-wrap: anywhere;
    color: var(--muted);
    font-size: 10px;
  }
  .password-change-confirmation dl { margin: 0; }
  .password-change-confirmation dl > div {
    display: grid;
    grid-template-columns: minmax(140px, 180px) minmax(0, 1fr);
    gap: 16px;
    padding: 13px 16px;
    border-bottom: 1px solid var(--rule);
  }
  .password-change-confirmation dl > div:last-child { border-bottom: 0; }
  .password-change-confirmation dt { display: grid; align-content: start; gap: 5px; }
  .password-change-confirmation dt strong { font-size: 13px; }
  .password-change-confirmation dt span { color: var(--muted); font-size: 11px; }
  .password-change-confirmation dt span[data-impact="references"],
  .password-change-confirmation dt span[data-impact="locator"],
  .password-change-confirmation dt span[data-impact="delete"] {
    color: var(--red-dark);
    font-weight: 700;
  }
  .password-change-confirmation dd {
    min-width: 0;
    display: grid;
    gap: 6px;
    margin: 0;
  }
  .password-change-confirmation .exposure-policy-diff {
    display: grid;
    gap: 8px;
    padding: 10px 12px 12px;
    border: 1px solid var(--rule);
    background: var(--surface);
  }
  .password-change-confirmation .exposure-policy-mode-diff {
    display: flex;
    align-items: center;
    justify-content: center;
    gap: 8px;
    margin: 0;
    color: var(--muted);
    font-size: 11px;
  }
  .password-change-confirmation .exposure-policy-mode-diff > span {
    margin-right: 2px;
    font-weight: 700;
  }
  .password-change-confirmation .exposure-policy-mode-diff del,
  .password-change-confirmation .exposure-policy-mode-diff ins {
    border: 1px solid var(--rule);
    padding: 3px 7px;
    background: #fff;
    color: var(--ink);
    font: 650 10px/1.2 ui-monospace, SFMono-Regular, Menlo, monospace;
  }
  .password-change-confirmation .exposure-policy-mode-diff ins {
    border-color: var(--red);
    background: var(--paper);
    color: var(--ink);
  }
  .password-change-confirmation del,
  .password-change-confirmation ins {
    overflow-wrap: anywhere;
    border-radius: 0;
    padding: 5px 7px;
    text-decoration: none;
    font: 12px/1.45 ui-monospace, SFMono-Regular, Menlo, monospace;
  }
  .password-change-confirmation del { background: #fff0ee; color: #8f2119; }
  .password-change-confirmation ins { background: var(--paper); color: var(--ink); }
  .password-change-confirmation footer {
    display: flex;
    justify-content: flex-end;
    gap: 10px;
    margin-top: auto;
    padding-top: 16px;
    border-top: 1px solid var(--ink);
  }
  .password-change-confirmation button {
    min-height: 36px;
    border: 1px solid var(--ink);
    border-radius: 0;
    padding: 8px 14px;
    color: var(--ink);
    background: var(--surface);
    cursor: pointer;
    font: inherit;
    font-size: 12px;
  }
  .password-change-confirmation button:hover:not(:disabled) { background: #e7e3dc; }
  .password-change-confirmation button:focus-visible {
    outline: 3px solid var(--red);
    outline-offset: 2px;
  }
  .password-change-confirmation footer .primary {
    border-color: var(--red);
    color: #fff;
    background: var(--red);
    font-weight: 750;
  }
  .password-change-confirmation footer .primary:hover:not(:disabled) {
    border-color: var(--red-dark);
    background: var(--red-dark);
  }
  .password-change-confirmation button:disabled { cursor: not-allowed; opacity: .55; }
  .password-change-confirmation .empty {
    margin: auto;
    text-align: center;
    color: var(--muted);
  }
  .password-change-confirmation .empty h2 {
    color: var(--ink);
    font: 600 26px/1.1 Georgia, "Times New Roman", serif;
  }
  @media (max-width: 620px) {
    .password-change-confirmation { padding: 16px; }
    .password-change-confirmation > header { padding: 16px; }
    .password-change-confirmation .change-context { grid-template-columns: 1fr; }
    .password-change-confirmation .change-context code { grid-column: auto; }
    .password-change-confirmation .item-diff > header { align-items: start; flex-direction: column; }
    .password-change-confirmation .item-diff code { max-width: 100%; }
    .password-change-confirmation dl > div { grid-template-columns: 1fr; gap: 8px; }
  }
`;
