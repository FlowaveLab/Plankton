// @vitest-environment jsdom

import { act, type ReactNode } from "react";
import ReactDOM from "react-dom/client";
import { afterEach, describe, expect, it, vi } from "vitest";

import {
  ApprovalChat,
  type ApprovalChatApi,
  type ApprovalChatSnapshot,
} from "./ApprovalChat";

Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });

const idle: ApprovalChatSnapshot = {
  request_id: "request-1",
  conversation_id: "request-1",
  title: "",
  updated_at: "2026-09-05T08:00:00Z",
  session_id: "session-original",
  state: "idle",
  messages: [],
  error: null,
};

function render(node: ReactNode): {
  container: HTMLDivElement;
  unmount: () => void;
} {
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

async function flush(): Promise<void> {
  await act(async () => {
    await Promise.resolve();
    await Promise.resolve();
  });
}

afterEach(() => {
  document.body.innerHTML = "";
  localStorage.clear();
});

describe("ApprovalChat", () => {
  it("is collapsed by default and loads the original approval session on open", async () => {
    const api: ApprovalChatApi = {
      create: vi.fn(async () => idle),
      rename: vi.fn(async () => idle),
      history: vi.fn(async () => [idle]),
      load: vi.fn(async () => idle),
      send: vi.fn(async () => idle),
      stop: vi.fn(async () => idle),
      subscribe: vi.fn(async () => () => {}),
    };
    const view = render(
      <ApprovalChat api={api} requestId="request-1" zh={false} />,
    );

    const chat = view.container.querySelector("details");
    expect(chat?.open).toBe(false);
    expect(api.history).not.toHaveBeenCalled();
    act(() => {
      if (chat) {
        chat.open = true;
        chat.dispatchEvent(new Event("toggle"));
      }
    });
    await flush();

    expect(api.history).toHaveBeenCalledWith("request-1");
    expect(view.container.textContent).toContain("Original review");
    expect(view.container.textContent).toContain("Adjust visibility");
    view.unmount();
  });

  it("renders streamed assistant chunks and prevents a second send while running", async () => {
    let listener: ((snapshot: ApprovalChatSnapshot) => void) | null = null;
    let finishSend: ((snapshot: ApprovalChatSnapshot) => void) | null = null;
    const api: ApprovalChatApi = {
      create: vi.fn(async () => idle),
      rename: vi.fn(async () => idle),
      history: vi.fn(async () => [idle]),
      load: vi.fn(async () => idle),
      send: vi.fn(
        () =>
          new Promise<ApprovalChatSnapshot>((resolve) => {
            finishSend = resolve;
          }),
      ),
      stop: vi.fn(async () => idle),
      subscribe: vi.fn(async (next) => {
        listener = next;
        return () => {};
      }),
    };
    const view = render(
      <ApprovalChat api={api} requestId="request-1" zh={true} />,
    );
    const details = view.container.querySelector("details");
    act(() => {
      if (details) {
        details.open = true;
        details.dispatchEvent(new Event("toggle"));
      }
    });
    await flush();

    const input = view.container.querySelector<HTMLTextAreaElement>(
      'textarea[aria-label="给审批 Agent 的消息"]',
    );
    act(() => {
      const setter = Object.getOwnPropertyDescriptor(
        HTMLTextAreaElement.prototype,
        "value",
      )?.set;
      setter?.call(input, "继续分析");
      input?.dispatchEvent(new Event("input", { bubbles: true }));
    });
    await act(async () => input?.form?.requestSubmit());
    expect(api.send).toHaveBeenCalledWith("request-1", "继续分析", "request-1");

    act(() => {
      listener?.({
        ...idle,
        state: "running",
        messages: [
          {
            id: "user-1",
            role: "user",
            kind: "text",
            content: "继续分析",
            state: "complete",
            created_at: "2026-08-16T08:00:00Z",
            tool_call: null,
          },
          {
            id: "assistant-1",
            role: "assistant",
            kind: "text",
            content: "正在核对调用链",
            state: "streaming",
            created_at: "2026-08-16T08:00:01Z",
            tool_call: null,
          },
        ],
      });
    });
    expect(view.container.textContent).toContain("正在核对调用链");
    expect(input?.disabled).toBe(false);
    expect(
      Array.from(
        view.container.querySelectorAll<HTMLButtonElement>("button"),
      ).filter((button) => button.textContent?.includes("调整可见范围"))[0]
        ?.disabled,
    ).toBe(false);

    await act(async () => finishSend?.(idle));
    view.unmount();
  });

  it("fills the composer from suggestions without sending automatically", async () => {
    const api: ApprovalChatApi = {
      create: vi.fn(async () => idle),
      rename: vi.fn(async () => idle),
      history: vi.fn(async () => [idle]),
      load: vi.fn(async () => idle),
      send: vi.fn(async () => idle),
      stop: vi.fn(async () => idle),
      subscribe: vi.fn(async () => () => {}),
    };
    const view = render(
      <ApprovalChat api={api} requestId="request-1" zh={true} />,
    );
    const details = view.container.querySelector("details");
    act(() => {
      if (details) {
        details.open = true;
        details.dispatchEvent(new Event("toggle"));
      }
    });
    await flush();

    act(() => {
      Array.from(view.container.querySelectorAll<HTMLButtonElement>("button"))
        .find((button) => button.textContent === "解释转人工原因")
        ?.click();
    });

    const input = view.container.querySelector<HTMLTextAreaElement>("textarea");
    expect(input?.value).toContain("解释为什么转为人工审批");
    expect(api.send).not.toHaveBeenCalled();
    view.unmount();
  });

  it("renders ACP thinking and tool calls as structured activity", async () => {
    const activity: ApprovalChatSnapshot = {
      ...idle,
      state: "running",
      messages: [
        {
          id: "thought-1",
          role: "assistant",
          kind: "thought",
          content: "先检查调用链是否包含网络边界。",
          state: "streaming",
          created_at: "2026-08-16T08:00:00Z",
          tool_call: null,
        },
        {
          id: "tool-1",
          role: "assistant",
          kind: "tool_call",
          content: "",
          state: "streaming",
          created_at: "2026-08-16T08:00:01Z",
          tool_call: {
            id: "call-1",
            title: "读取调用链文件",
            kind: "read",
            status: "in_progress",
            input: '{"path":"/plankton-review/chain.md"}',
          },
        },
      ],
    };
    const api: ApprovalChatApi = {
      create: vi.fn(async () => idle),
      rename: vi.fn(async () => idle),
      history: vi.fn(async () => [activity]),
      load: vi.fn(async () => activity),
      send: vi.fn(async () => activity),
      stop: vi.fn(async () => activity),
      subscribe: vi.fn(async () => () => {}),
    };
    const view = render(
      <ApprovalChat api={api} requestId="request-1" zh={true} />,
    );
    const details = view.container.querySelector("details");
    act(() => {
      if (details) {
        details.open = true;
        details.dispatchEvent(new Event("toggle"));
      }
    });
    await flush();

    expect(view.container.textContent).toContain("思考过程");
    expect(view.container.textContent).toContain("先检查调用链");
    expect(view.container.textContent).toContain("读取调用链文件");
    expect(view.container.textContent).toContain("执行中");
    expect(view.container.textContent).toContain("查看调用参数");
    view.unmount();
  });

  it("shows a stop action while streaming and requests cancellation", async () => {
    const running: ApprovalChatSnapshot = {
      ...idle,
      state: "running",
      messages: [
        {
          id: "assistant-running",
          role: "assistant",
          kind: "text",
          content: "正在核对",
          state: "streaming",
          created_at: "2026-08-16T08:00:01Z",
          tool_call: null,
        },
      ],
    };
    const stopping: ApprovalChatSnapshot = { ...running, state: "stopping" };
    const api: ApprovalChatApi = {
      create: vi.fn(async () => idle),
      rename: vi.fn(async () => idle),
      history: vi.fn(async () => [running]),
      load: vi.fn(async () => running),
      send: vi.fn(async () => running),
      stop: vi.fn(async () => stopping),
      subscribe: vi.fn(async () => () => {}),
    };
    const view = render(
      <ApprovalChat api={api} requestId="request-1" zh={true} />,
    );
    const details = view.container.querySelector("details");
    act(() => {
      if (details) {
        details.open = true;
        details.dispatchEvent(new Event("toggle"));
      }
    });
    await flush();

    const stop = view.container.querySelector<HTMLButtonElement>(
      'button[aria-label="停止生成"]',
    );
    expect(stop?.textContent).toContain("停止");
    await act(async () => stop?.click());
    expect(api.stop).toHaveBeenCalledWith("request-1", "request-1");
    view.unmount();
  });

  it("shows a cancellable queued message while review details are still running", async () => {
    const queued: ApprovalChatSnapshot = {
      ...idle,
      state: "queued",
      messages: [
        {
          id: "user-queued",
          role: "user",
          kind: "text",
          content: "继续解释",
          state: "complete",
          created_at: "2026-08-16T08:00:00Z",
          tool_call: null,
        },
        {
          id: "assistant-queued",
          role: "assistant",
          kind: "text",
          content: "",
          state: "queued",
          created_at: "2026-08-16T08:00:00Z",
          tool_call: null,
        },
      ],
    };
    const stopping: ApprovalChatSnapshot = { ...queued, state: "stopping" };
    const api: ApprovalChatApi = {
      create: vi.fn(async () => idle),
      rename: vi.fn(async () => idle),
      history: vi.fn(async () => [queued]),
      load: vi.fn(async () => queued),
      send: vi.fn(async () => queued),
      stop: vi.fn(async () => stopping),
      subscribe: vi.fn(async () => () => {}),
    };
    const view = render(
      <ApprovalChat api={api} requestId="request-1" zh={true} />,
    );
    const details = view.container.querySelector("details");
    act(() => {
      if (details) {
        details.open = true;
        details.dispatchEvent(new Event("toggle"));
      }
    });
    await flush();

    expect(view.container.textContent).toContain("等待详细解释完成");
    expect(view.container.textContent).toContain(
      "已排队；详细解释完成后自动开始",
    );
    const cancel = view.container.querySelector<HTMLButtonElement>(
      'button[aria-label="取消待发送消息"]',
    );
    expect(cancel?.textContent).toContain("取消排队");
    await act(async () => cancel?.click());
    expect(api.stop).toHaveBeenCalledWith("request-1", "request-1");
    view.unmount();
  });
});
