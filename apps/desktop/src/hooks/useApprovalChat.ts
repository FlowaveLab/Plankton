import { useCallback, useEffect, useRef, useState } from "react";
import type { ApprovalChatApi, ApprovalChatSnapshot } from "../approvalChatApi";
import { getBrowserStorage } from "../browserStorage";

export function isChatActive(
  snapshot: ApprovalChatSnapshot | undefined,
): boolean {
  return (
    !!snapshot && ["queued", "running", "stopping"].includes(snapshot.state)
  );
}

export function chatTitle(snapshot: ApprovalChatSnapshot, zh: boolean): string {
  return (
    snapshot.title ||
    (snapshot.conversation_id === snapshot.request_id
      ? zh
        ? "原审批对话"
        : "Original review"
      : zh
        ? "新对话"
        : "New conversation")
  );
}

function errorText(reason: unknown): string {
  return reason instanceof Error ? reason.message : String(reason);
}

export function useApprovalChat(
  requestId: string,
  open: boolean,
  api: ApprovalChatApi,
) {
  const storageKey = `plankton.chat.selected.${requestId}`;
  const [selectedId, setSelectedId] = useState(
    () => getBrowserStorage().getItem(storageKey) ?? requestId,
  );
  const [sessions, setSessions] = useState<
    Record<string, ApprovalChatSnapshot>
  >({});
  const [drafts, setDrafts] = useState<Record<string, string>>({});
  const [errors, setErrors] = useState<Record<string, string | null>>({});
  const [pending, setPending] = useState<Record<string, boolean>>({});
  const [loading, setLoading] = useState(true);
  const [managing, setManaging] = useState(false);
  const [connectionError, setConnectionError] = useState<string | null>(null);
  const [reload, setReload] = useState(0);
  const alive = useRef(true);
  const sending = useRef(new Set<string>());
  const managingRef = useRef(false);
  const revisions = useRef(new Map<string, number>());
  const latest = useRef(sessions);
  latest.current = sessions;

  useEffect(() => {
    alive.current = true;
    return () => {
      alive.current = false;
    };
  }, []);

  const accept = useCallback(
    (next: ApprovalChatSnapshot) => {
      if (!alive.current || next.request_id !== requestId) return;
      setSessions((current) => ({ ...current, [next.conversation_id]: next }));
    },
    [requestId],
  );

  const select = useCallback(
    (id: string) => {
      setSelectedId(id);
      // Only the selected id is stored in the webview; transcripts stay in the native store.
      try {
        getBrowserStorage().setItem(storageKey, id);
      } catch {
        /* Selection still works when webview storage is full. */
      }
    },
    [storageKey],
  );

  useEffect(() => {
    if (!open) return;
    let disposed = false;
    let unsubscribe: (() => void) | undefined;
    setLoading(true);
    setConnectionError(null);
    void (async () => {
      try {
        unsubscribe = await api.subscribe((next) => {
          if (disposed || next.request_id !== requestId) return;
          const id = next.conversation_id;
          revisions.current.set(id, (revisions.current.get(id) ?? 0) + 1);
          accept(next);
        });
        if (disposed) {
          unsubscribe();
          return;
        }
        const before = new Map(revisions.current);
        const history = await api.history(requestId);
        if (disposed) return;
        for (const item of history) {
          if (
            (before.get(item.conversation_id) ?? 0) ===
            (revisions.current.get(item.conversation_id) ?? 0)
          )
            accept(item);
        }
        setSelectedId((current) =>
          history.some((item) => item.conversation_id === current)
            ? current
            : (history[0]?.conversation_id ?? requestId),
        );
      } catch (reason) {
        if (!disposed) setConnectionError(errorText(reason));
      } finally {
        if (!disposed) setLoading(false);
      }
    })();
    return () => {
      disposed = true;
      unsubscribe?.();
    };
  }, [accept, api, open, reload, requestId]);

  const setDraft = (draft: string) =>
    setDrafts((current) => ({ ...current, [selectedId]: draft }));
  const snapshot = sessions[selectedId];
  const draft = drafts[selectedId] ?? "";
  const active = isChatActive(snapshot);
  const history = Object.values(sessions).sort(
    (a, b) =>
      b.updated_at.localeCompare(a.updated_at) ||
      a.conversation_id.localeCompare(b.conversation_id),
  );

  const send = async (): Promise<boolean> => {
    const id = selectedId;
    const text = draft.trim();
    if (
      !text ||
      [...text].length > 8000 ||
      active ||
      sending.current.has(id) ||
      loading ||
      managingRef.current ||
      !snapshot
    )
      return false;
    sending.current.add(id);
    setPending((current) => ({ ...current, [id]: true }));
    setDrafts((current) => ({ ...current, [id]: "" }));
    setErrors((current) => ({ ...current, [id]: null }));
    const revision = revisions.current.get(id) ?? 0;
    try {
      const next = await api.send(requestId, text, id);
      if (revision === (revisions.current.get(id) ?? 0)) accept(next);
      return true;
    } catch (reason) {
      if (!alive.current) return false;
      setErrors((current) => ({ ...current, [id]: errorText(reason) }));
      // Keep an edited next draft intact if the user typed during the request.
      setDrafts((current) => ({ ...current, [id]: current[id] || text }));
      try {
        const beforeReload = revisions.current.get(id) ?? 0;
        const reloaded = await api.load(requestId, id);
        if (beforeReload === (revisions.current.get(id) ?? 0)) accept(reloaded);
      } catch {
        /* Preserve the send error and last visible transcript for retry. */
      }
      return false;
    } finally {
      sending.current.delete(id);
      if (alive.current) setPending((current) => ({ ...current, [id]: false }));
    }
  };

  const stop = async (): Promise<void> => {
    const id = selectedId;
    if (
      !isChatActive(latest.current[id]) ||
      latest.current[id]?.state === "stopping"
    )
      return;
    setErrors((current) => ({ ...current, [id]: null }));
    const revision = revisions.current.get(id) ?? 0;
    try {
      const next = await api.stop(requestId, id);
      if (revision === (revisions.current.get(id) ?? 0)) accept(next);
    } catch (reason) {
      if (alive.current)
        setErrors((current) => ({ ...current, [id]: errorText(reason) }));
    }
  };

  const create = async (): Promise<void> => {
    if (managingRef.current) return;
    managingRef.current = true;
    setManaging(true);
    setConnectionError(null);
    try {
      const next = await api.create(requestId);
      if (!alive.current) return;
      accept(next);
      select(next.conversation_id);
    } catch (reason) {
      if (alive.current) setConnectionError(errorText(reason));
    } finally {
      managingRef.current = false;
      if (alive.current) setManaging(false);
    }
  };

  const rename = async (title: string): Promise<boolean> => {
    const id = selectedId;
    if (managingRef.current || !title.trim()) return false;
    managingRef.current = true;
    setManaging(true);
    try {
      const next = await api.rename(requestId, id, title.trim());
      // Renaming must not replace a newer streamed message with an older snapshot.
      setSessions((current) =>
        current[id]
          ? { ...current, [id]: { ...current[id], title: next.title } }
          : current,
      );
      return true;
    } catch (reason) {
      if (alive.current)
        setErrors((current) => ({ ...current, [id]: errorText(reason) }));
      return false;
    } finally {
      managingRef.current = false;
      if (alive.current) setManaging(false);
    }
  };

  const setOptions = async (options: Record<string, string>): Promise<void> => {
    const id = selectedId;
    if (!api.setOptions || managingRef.current) return;
    managingRef.current = true;
    setManaging(true);
    try {
      const next = await api.setOptions(requestId, id, options);
      // Updating configuration must not overwrite messages streamed while saving it.
      setSessions((current) =>
        current[id]
          ? {
              ...current,
              [id]: { ...current[id], acp_profile: next.acp_profile },
            }
          : current,
      );
    } catch (reason) {
      if (alive.current)
        setErrors((current) => ({ ...current, [id]: errorText(reason) }));
    } finally {
      managingRef.current = false;
      if (alive.current) setManaging(false);
    }
  };

  return {
    history,
    snapshot,
    selectedId,
    select,
    draft,
    setDraft,
    active,
    pending: !!pending[selectedId],
    loading,
    managing,
    error: errors[selectedId] ?? snapshot?.error,
    connectionError,
    retry: () => setReload((value) => value + 1),
    send,
    stop,
    create,
    rename,
    setOptions,
  };
}
