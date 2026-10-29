import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { ArrowUpRight, ShieldCheck, X } from "lucide-react";
import { useCallback, useEffect, useRef, useState, type JSX } from "react";

import { getBrowserStorage } from "../browserStorage";
import { isLocale, LOCALE_STORAGE_KEY, type Locale } from "../i18n";
import type { CompactApprovalRequest } from "./CompactApproval";
import "./approval-notification.css";

export type ApprovalNotificationApi = {
  loadRequests: () => Promise<CompactApprovalRequest[]>;
  loadLocale: () => Promise<Locale>;
  openFullDetails: (requestId: string) => Promise<void>;
  dismiss: () => Promise<void>;
  subscribe: (onQueueChanged: () => void) => Promise<() => void>;
};

const defaultApi: ApprovalNotificationApi = {
  loadRequests: () => invoke("compact_approval_requests"),
  async loadLocale() {
    const settings = await invoke<{ locale: Locale }>("desktop_settings");
    return settings.locale;
  },
  openFullDetails: (requestId) =>
    invoke("open_full_request_details", { requestId }),
  dismiss: () => invoke("dismiss_approval_popup"),
  subscribe: (onQueueChanged) =>
    listen("plankton://approval-queue", onQueueChanged),
};

function initialLocale(): Locale {
  const stored = getBrowserStorage().getItem(LOCALE_STORAGE_KEY);
  return isLocale(stored) ? stored : "en";
}

export function ApprovalNotification({
  api = defaultApi,
}: {
  api?: ApprovalNotificationApi;
}): JSX.Element {
  const [locale, setLocale] = useState<Locale>(initialLocale);
  const [requests, setRequests] = useState<CompactApprovalRequest[]>([]);
  const [loading, setLoading] = useState(true);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState(false);
  const loadVersion = useRef(0);
  const current = requests.at(-1) ?? null;
  const zh = locale === "zh-CN";

  const reload = useCallback(async (): Promise<void> => {
    const version = ++loadVersion.current;
    try {
      const loaded = await api.loadRequests();
      if (version !== loadVersion.current) return;
      setRequests(
        loaded.filter((request) => request.approval_status === "pending"),
      );
      setError(false);
    } catch {
      if (version === loadVersion.current) setError(true);
    } finally {
      if (version === loadVersion.current) setLoading(false);
    }
  }, [api]);

  useEffect(() => {
    let disposed = false;
    let unlisten: (() => void) | undefined;
    void api
      .subscribe(() => {
        void reload();
      })
      .then((dispose) => {
        if (disposed) {
          dispose();
          return;
        }
        unlisten = dispose;
        void reload();
      })
      .catch(() => {
        if (!disposed) {
          setError(true);
          setLoading(false);
        }
      });
    void api
      .loadLocale()
      .then((loaded) => {
        if (!disposed && isLocale(loaded)) setLocale(loaded);
      })
      .catch(() => {
        /* The saved browser locale remains a usable fallback. */
      });
    function onLocaleChanged(event: StorageEvent): void {
      if (event.key === LOCALE_STORAGE_KEY && isLocale(event.newValue)) {
        setLocale(event.newValue);
      }
    }
    window.addEventListener("storage", onLocaleChanged);
    return () => {
      disposed = true;
      loadVersion.current += 1;
      unlisten?.();
      window.removeEventListener("storage", onLocaleChanged);
    };
  }, [api, reload]);

  const act = useCallback(
    async (action: () => Promise<void>): Promise<void> => {
      if (busy) return;
      setBusy(true);
      try {
        await action();
      } catch {
        setError(true);
      } finally {
        setBusy(false);
      }
    },
    [busy],
  );

  useEffect(() => {
    function onKeyDown(event: KeyboardEvent): void {
      if (event.key === "Escape") {
        event.preventDefault();
        void act(api.dismiss);
      }
    }
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [act, api]);

  return (
    <main className="approval-notification" lang={locale}>
      <header className="approval-notification__header">
        <span className="approval-notification__mark">
          <ShieldCheck size={19} aria-hidden="true" />
        </span>
        <strong>PLANKTON</strong>
        <span className="approval-notification__count" aria-live="polite">
          {requests.length > 1
            ? zh
              ? `${requests.length} 项待审批`
              : `${requests.length} waiting`
            : zh
              ? "凭据访问"
              : "Credential access"}
        </span>
        <button
          className="approval-notification__close"
          type="button"
          aria-label={zh ? "稍后处理" : "Remind me later"}
          disabled={busy}
          onClick={() => {
            void act(api.dismiss);
          }}
        >
          <X size={16} aria-hidden="true" />
        </button>
      </header>
      <section className="approval-notification__content" aria-live="polite">
        <h1>
          {loading
            ? zh
              ? "正在获取审批…"
              : "Loading request…"
            : current
              ? zh
                ? "需要你审批"
                : "Your approval is needed"
              : zh
                ? "审批已处理"
                : "All caught up"}
        </h1>
        {current ? (
          <>
            <code
              className="approval-notification__resource"
              title={current.resource}
            >
              {current.resource}
            </code>
            <p className="approval-notification__reason" title={current.reason}>
              {current.reason || (zh ? "未填写申请原因" : "No reason provided")}
            </p>
            <p className="approval-notification__requester">
              <span>{zh ? "申请方" : "From"}</span>
              <strong title={current.requested_by}>
                {current.requested_by || (zh ? "未知" : "Unknown")}
              </strong>
            </p>
          </>
        ) : (
          <p className="approval-notification__reason">
            {loading
              ? zh
                ? "请稍候"
                : "One moment"
              : zh
                ? "你可以关闭此提醒。"
                : "You can dismiss this notification."}
          </p>
        )}
        {error ? (
          <div className="approval-notification__error" role="alert">
            {zh ? "操作未完成，请重试。" : "Could not complete this action."}
            <button
              type="button"
              onClick={() => {
                void reload();
              }}
            >
              {zh ? "重试" : "Retry"}
            </button>
          </div>
        ) : null}
      </section>
      <footer className="approval-notification__footer">
        <button
          className="approval-notification__later"
          disabled={busy}
          type="button"
          onClick={() => {
            void act(api.dismiss);
          }}
        >
          {zh ? "稍后处理" : "Later"}
        </button>
        <button
          className="approval-notification__open"
          disabled={busy || !current}
          type="button"
          onClick={() => {
            if (current) void act(() => api.openFullDetails(current.id));
          }}
        >
          {zh ? "查看并审批" : "Review request"}
          <ArrowUpRight size={15} aria-hidden="true" />
        </button>
      </footer>
    </main>
  );
}
