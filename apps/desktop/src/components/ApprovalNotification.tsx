import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import {
  ArrowUpRight,
  CircleCheck,
  CircleX,
  LoaderCircle,
  TriangleAlert,
  X,
} from "lucide-react";
import { useCallback, useEffect, useRef, useState, type JSX } from "react";

import { getBrowserStorage } from "../browserStorage";
import { isLocale, LOCALE_STORAGE_KEY, type Locale } from "../i18n";
import "./approval-notification.css";

export type ApprovalNotificationData = {
  id: string;
  request_id: string;
  stage: "evaluating" | "approved" | "rejected" | "failed";
  resource: string;
  resource_label: string;
  requested_by: string;
  reason: string;
  decision_reason: string;
};

export type ApprovalNotificationApi = {
  loadCurrent: () => Promise<ApprovalNotificationData | null>;
  loadLocale: () => Promise<Locale>;
  openFullDetails: (requestId: string) => Promise<void>;
  dismiss: (notificationId: string) => Promise<void>;
  disable: () => Promise<void>;
  subscribe: (
    onNotification: (notification: ApprovalNotificationData | null) => void,
  ) => Promise<() => void>;
};

const defaultApi: ApprovalNotificationApi = {
  loadCurrent: () => invoke("approval_notification_current"),
  async loadLocale() {
    const settings = await invoke<{ locale: Locale }>("desktop_settings");
    return settings.locale;
  },
  openFullDetails: (requestId) =>
    invoke("open_full_request_details", { requestId }),
  dismiss: (notificationId) =>
    invoke("dismiss_approval_notification", { notificationId }),
  disable: () => invoke("disable_approval_notifications"),
  subscribe: (onNotification) =>
    listen<ApprovalNotificationData | null>(
      "plankton://approval-notification",
      (event) => onNotification(event.payload),
    ),
};

function initialLocale(): Locale {
  const stored = getBrowserStorage().getItem(LOCALE_STORAGE_KEY);
  return isLocale(stored) ? stored : "en";
}

const STAGES = {
  evaluating: { zh: "自动审批中", en: "Reviewing access", icon: LoaderCircle },
  approved: { zh: "访问已通过", en: "Access approved", icon: CircleCheck },
  rejected: { zh: "访问已拒绝", en: "Access rejected", icon: CircleX },
  failed: { zh: "审批未完成", en: "Review failed", icon: TriangleAlert },
} as const;

export function ApprovalNotification({
  api = defaultApi,
}: {
  api?: ApprovalNotificationApi;
}): JSX.Element {
  const [locale, setLocale] = useState<Locale>(initialLocale);
  const [current, setCurrent] = useState<ApprovalNotificationData | null>(null);
  const [loading, setLoading] = useState(true);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<"load" | "action" | null>(null);
  const currentId = useRef<string | null>(null);
  const loadVersion = useRef(0);
  const actionVersion = useRef(0);
  const actionBusy = useRef(false);
  const timer = useRef<number | undefined>(undefined);
  const zh = locale === "zh-CN";

  const stopTimer = useCallback(() => {
    window.clearTimeout(timer.current);
    timer.current = undefined;
  }, []);

  const accept = useCallback(
    (notification: ApprovalNotificationData | null) => {
      if (notification?.id !== currentId.current) {
        stopTimer();
        actionVersion.current += 1;
        actionBusy.current = false;
        setBusy(false);
        setError(null);
      }
      currentId.current = notification?.id ?? null;
      setCurrent(notification);
      setLoading(false);
    },
    [stopTimer],
  );

  const reload = useCallback(async (): Promise<void> => {
    const version = ++loadVersion.current;
    try {
      const notification = await api.loadCurrent();
      if (version !== loadVersion.current) return;
      accept(notification);
      setError(null);
    } catch {
      if (version === loadVersion.current) setError("load");
    } finally {
      if (version === loadVersion.current) setLoading(false);
    }
  }, [api, accept]);

  useEffect(() => {
    let disposed = false;
    let unlisten: (() => void) | undefined;
    void api
      .subscribe((notification) => {
        if (disposed) return;
        // A delayed initial read must never replace a newer event.
        loadVersion.current += 1;
        accept(notification);
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
          setError("load");
          setLoading(false);
        }
      });
    void api
      .loadLocale()
      .then((loaded) => {
        if (!disposed && isLocale(loaded)) setLocale(loaded);
      })
      .catch(() => {
        /* Browser locale remains a usable fallback. */
      });
    function onLocaleChanged(event: StorageEvent): void {
      if (event.key === LOCALE_STORAGE_KEY && isLocale(event.newValue))
        setLocale(event.newValue);
    }
    window.addEventListener("storage", onLocaleChanged);
    return () => {
      disposed = true;
      loadVersion.current += 1;
      actionVersion.current += 1;
      currentId.current = null;
      stopTimer();
      unlisten?.();
      window.removeEventListener("storage", onLocaleChanged);
    };
  }, [api, accept, reload, stopTimer]);

  const performAction = useCallback(
    async (
      notificationId: string,
      action: () => Promise<void>,
    ): Promise<void> => {
      if (currentId.current !== notificationId || actionBusy.current) return;
      const version = ++actionVersion.current;
      actionBusy.current = true;
      stopTimer();
      setBusy(true);
      setError(null);
      try {
        await action();
        if (
          version === actionVersion.current &&
          currentId.current === notificationId
        )
          accept(null);
      } catch {
        if (
          version === actionVersion.current &&
          currentId.current === notificationId
        )
          setError("action");
      } finally {
        if (version === actionVersion.current) {
          actionBusy.current = false;
          setBusy(false);
        }
      }
    },
    [accept, stopTimer],
  );

  useEffect(() => {
    stopTimer();
    if (!current || current.stage === "evaluating") return;
    const id = current.id;
    timer.current = window.setTimeout(() => {
      if (currentId.current === id)
        void performAction(id, () => api.dismiss(id));
    }, 5000);
    return stopTimer;
  }, [current?.id, current?.stage, api, performAction, stopTimer]);

  useEffect(() => {
    function onKeyDown(event: KeyboardEvent): void {
      const id = currentId.current;
      if (event.key === "Escape" && id) {
        event.preventDefault();
        void performAction(id, () => api.dismiss(id));
      }
    }
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [api, performAction]);

  const stage = current ? STAGES[current.stage] : null;
  const Icon = stage?.icon ?? LoaderCircle;
  return (
    <main
      className="approval-notification"
      data-stage={current?.stage}
      lang={locale}
    >
      <header className="approval-notification__header">
        <Icon
          className="approval-notification__stage-icon"
          size={15}
          aria-hidden="true"
        />
        <strong aria-live="polite">
          {stage
            ? zh
              ? stage.zh
              : stage.en
            : loading
              ? zh
                ? "正在同步…"
                : "Loading…"
              : zh
                ? "暂无自动审批提示"
                : "No active notification"}
        </strong>
        <span className="approval-notification__brand">PLANKTON</span>
        <button
          className="approval-notification__close"
          type="button"
          aria-label={zh ? "关闭提示" : "Dismiss notification"}
          disabled={busy || !current}
          onClick={() => {
            if (current)
              void performAction(current.id, () => api.dismiss(current.id));
          }}
        >
          <X size={14} aria-hidden="true" />
        </button>
      </header>
      <section className="approval-notification__content" aria-live="polite">
        {current ? (
          <>
            <div className="approval-notification__identity">
              <strong
                className="approval-notification__resource"
                title={current.resource}
              >
                {current.resource_label || current.resource}
              </strong>
              <span
                className="approval-notification__requester"
                title={current.requested_by}
              >
                {current.requested_by}
              </span>
            </div>
            <p className="approval-notification__reason" title={current.reason}>
              <span>{zh ? "申请" : "Request"} · </span>
              {current.reason || (zh ? "未填写原因" : "No reason provided")}
            </p>
            {current.stage !== "evaluating" ? (
              <p
                className="approval-notification__decision"
                title={current.decision_reason}
              >
                <span>{zh ? "决定" : "Decision"} · </span>
                {current.decision_reason ||
                  (zh ? "查看详情了解结果" : "See details for the result")}
              </p>
            ) : null}
          </>
        ) : null}
        {error ? (
          <p className="approval-notification__error" role="alert">
            {zh ? "操作未完成，请重试。" : "Action failed. Please retry."}
            {error === "load" ? (
              <button type="button" onClick={() => void reload()}>
                {zh ? "重试" : "Retry"}
              </button>
            ) : null}
          </p>
        ) : null}
      </section>
      <footer className="approval-notification__footer">
        <button
          className="approval-notification__mute"
          disabled={busy || !current}
          type="button"
          onClick={() => {
            if (current) void performAction(current.id, api.disable);
          }}
        >
          {zh ? "不再提示" : "Don't show again"}
        </button>
        <button
          className="approval-notification__open"
          disabled={busy || !current}
          type="button"
          onClick={() => {
            if (!current) return;
            const notification = current;
            void performAction(notification.id, async () => {
              await api.openFullDetails(notification.request_id);
              if (currentId.current === notification.id)
                await api.dismiss(notification.id);
            });
          }}
        >
          {zh ? "查看详情" : "Details"}
          <ArrowUpRight size={12} aria-hidden="true" />
        </button>
      </footer>
    </main>
  );
}
