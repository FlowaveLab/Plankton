// @vitest-environment jsdom
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, describe, expect, it, vi } from "vitest";
import { useApprovalChat, chatTitle, isChatActive } from "./useApprovalChat";
import type { ApprovalChatApi, ApprovalChatSnapshot } from "../approvalChatApi";

Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
const original: ApprovalChatSnapshot = {
  request_id: "approval",
  conversation_id: "approval",
  session_id: "agent-original",
  title: "",
  updated_at: "2026-09-05T00:00:00Z",
  state: "idle",
  messages: [],
  error: null,
};
const second: ApprovalChatSnapshot = {
  ...original,
  conversation_id: "second",
  session_id: null,
  title: "Follow-up",
  updated_at: "2026-09-05T01:00:00Z",
};
let root: Root | undefined;
let chat: ReturnType<typeof useApprovalChat>;
let emit: (snapshot: ApprovalChatSnapshot) => void;
const makeApi = (): ApprovalChatApi => ({
  history: vi.fn(async () => [original, second]),
  load: vi.fn(async () => original),
  send: vi.fn(async () => original),
  stop: vi.fn(async () => original),
  create: vi.fn(async () => second),
  rename: vi.fn(async (_request, _id, title) => ({ ...second, title })),
  subscribe: vi.fn(async (listener) => {
    emit = listener;
    return () => {};
  }),
});
function Harness({ api }: { api: ApprovalChatApi }) {
  chat = useApprovalChat("approval", true, api);
  return null;
}
async function mount(api: ApprovalChatApi) {
  const container = document.createElement("div");
  document.body.append(container);
  root = createRoot(container);
  await act(async () => root?.render(<Harness api={api} />));
}
function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (reason: Error) => void;
  const promise = new Promise<T>((yes, no) => {
    resolve = yes;
    reject = no;
  });
  return { promise, resolve, reject };
}
afterEach(() => {
  act(() => root?.unmount());
  root = undefined;
  localStorage.clear();
  document.body.innerHTML = "";
});

describe("approval chat sessions", () => {
  it("switches independent drafts and keeps the selected conversation after reopening", async () => {
    const api = makeApi();
    await mount(api);
    act(() => chat.setDraft("Original draft"));
    act(() => chat.select("second"));
    expect(chat.draft).toBe("");
    act(() => chat.setDraft("Second draft"));
    act(() => chat.select("approval"));
    expect(chat.draft).toBe("Original draft");
    act(() => chat.select("second"));
    expect(chat.draft).toBe("Second draft");
    act(() => root?.unmount());
    root = undefined;
    await mount(api);
    expect(chat.selectedId).toBe("second");
    expect(chat.snapshot?.session_id).toBe(null);
  });

  it("guards double submission before the first streaming event arrives", async () => {
    const api = makeApi();
    const send = deferred<ApprovalChatSnapshot>();
    api.send = vi.fn(() => send.promise);
    await mount(api);
    act(() => chat.setDraft("Question"));
    let first!: Promise<boolean>;
    act(() => {
      first = chat.send();
      void chat.send();
    });
    expect(api.send).toHaveBeenCalledTimes(1);
    expect(chat.pending).toBe(true);
    await act(async () => {
      send.resolve(original);
      await first;
    });
    expect(chat.pending).toBe(false);
  });

  it("routes background streaming events without changing the selected session", async () => {
    const api = makeApi();
    await mount(api);
    act(() => chat.select("second"));
    act(() => emit({ ...original, state: "running", title: "Background" }));
    expect(chat.selectedId).toBe("second");
    expect(chat.active).toBe(false);
    expect(
      chat.history.find((item) => item.conversation_id === "approval")?.state,
    ).toBe("running");
    act(() =>
      emit({
        ...second,
        request_id: "other-approval",
        title: "Wrong approval",
      }),
    );
    expect(chat.snapshot?.title).toBe("Follow-up");
  });

  it("does not replace streamed output with an older send response", async () => {
    const api = makeApi();
    const send = deferred<ApprovalChatSnapshot>();
    api.send = vi.fn(() => send.promise);
    await mount(api);
    act(() => chat.setDraft("Question"));
    let sending!: Promise<boolean>;
    act(() => {
      sending = chat.send();
    });
    act(() =>
      emit({ ...original, title: "Newest streamed state", state: "running" }),
    );
    await act(async () => {
      send.resolve(original);
      await sending;
    });
    expect(chat.snapshot?.title).toBe("Newest streamed state");
    expect(chat.active).toBe(true);
  });

  it("preserves a send error and restores the draft even if reloading succeeds", async () => {
    const api = makeApi();
    api.send = vi.fn(async () => {
      throw new Error("Disconnected");
    });
    await mount(api);
    act(() => chat.setDraft("Retry this question"));
    await act(async () => {
      await chat.send();
    });
    expect(chat.error).toBe("Disconnected");
    expect(chat.draft).toBe("Retry this question");
    expect(chat.pending).toBe(false);
  });

  it("retains a newly typed draft after a failed pending send", async () => {
    const api = makeApi();
    const send = deferred<ApprovalChatSnapshot>();
    api.send = vi.fn(() => send.promise);
    await mount(api);
    act(() => chat.setDraft("Question"));
    let sending!: Promise<boolean>;
    act(() => {
      sending = chat.send();
    });
    act(() => chat.setDraft("Next question"));
    await act(async () => {
      send.reject(new Error("Disconnected"));
      await sending;
    });
    expect(chat.draft).toBe("Next question");
  });

  it("subscribes before history loads and keeps events newer than the history response", async () => {
    const api = makeApi();
    const history = deferred<ApprovalChatSnapshot[]>();
    api.history = vi.fn(() => history.promise);
    await mount(api);
    act(() => emit({ ...original, title: "Latest", state: "running" }));
    await act(async () => history.resolve([original]));
    expect(chat.snapshot?.title).toBe("Latest");
    expect(chat.active).toBe(true);
  });

  it("creates and renames a session without mutating another transcript", async () => {
    const api = makeApi();
    await mount(api);
    await act(async () => chat.create());
    expect(chat.selectedId).toBe("second");
    await act(async () => {
      await chat.rename("Evidence");
    });
    expect(api.rename).toHaveBeenCalledWith("approval", "second", "Evidence");
    expect(chat.snapshot?.title).toBe("Evidence");
    expect(
      chat.history.find((item) => item.conversation_id === "approval")?.title,
    ).toBe("");
  });

  it("rejects oversized input and sends stop to the selected running session", async () => {
    const api = makeApi();
    await mount(api);
    act(() => chat.setDraft("字".repeat(8001)));
    await act(async () => {
      expect(await chat.send()).toBe(false);
    });
    expect(api.send).not.toHaveBeenCalled();
    act(() => {
      chat.select("second");
      emit({ ...second, state: "running" });
    });
    await act(async () => chat.stop());
    expect(api.stop).toHaveBeenCalledWith("approval", "second");
  });

  it("labels original, untitled and active sessions consistently", () => {
    expect(chatTitle(original, true)).toBe("原审批对话");
    expect(chatTitle({ ...second, title: "" }, false)).toBe("New conversation");
    expect(isChatActive(undefined)).toBe(false);
    for (const state of ["queued", "running", "stopping"] as const)
      expect(isChatActive({ ...original, state })).toBe(true);
    for (const state of ["idle", "released", "failed"] as const)
      expect(isChatActive({ ...original, state })).toBe(false);
  });
});
