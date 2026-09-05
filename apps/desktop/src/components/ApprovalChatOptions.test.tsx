// @vitest-environment jsdom
import { act } from "react";
import { createRoot } from "react-dom/client";
import { expect, it, vi } from "vitest";
import { ApprovalChat } from "./ApprovalChat";
import type { ApprovalChatApi, ApprovalChatSnapshot } from "../approvalChatApi";
import type { AcpProfile } from "../types";
vi.mock("./AcpSessionOptions", () => ({
  AcpSessionOptions: ({
    onChange,
    profile,
    context,
  }: {
    onChange: (p: AcpProfile) => void;
    profile: AcpProfile;
    context: string;
  }) => (
    <button
      onClick={() =>
        onChange({ ...profile, session_options: { model: "chat-only-model" } })
      }
    >
      {context} change model
    </button>
  ),
}));
Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
it("routes model changes through the chat-only endpoint and updates its selector", async () => {
  const idle: ApprovalChatSnapshot = {
    request_id: "request",
    conversation_id: "request",
    title: "",
    updated_at: "2026-09-06",
    session_id: "session",
    state: "idle",
    messages: [],
    error: null,
    acp_profile: { agent_kind: "codex", version_mode: "latest" },
  };
  const api: ApprovalChatApi = {
    history: vi.fn(async () => [idle]),
    create: vi.fn(async () => idle),
    rename: vi.fn(async () => idle),
    load: vi.fn(async () => idle),
    send: vi.fn(async () => idle),
    stop: vi.fn(async () => idle),
    subscribe: vi.fn(async () => () => {}),
    setOptions: vi.fn(async (_request, _conversation, session_options) => ({
      ...idle,
      acp_profile: { ...idle.acp_profile!, session_options },
    })),
  };
  const container = document.createElement("div");
  document.body.append(container);
  const root = createRoot(container);
  await act(async () =>
    root.render(<ApprovalChat api={api} requestId="request" zh />),
  );
  const details = container.querySelector("details")!;
  await act(async () => {
    details.open = true;
    details.dispatchEvent(new Event("toggle"));
  });
  const options = container.querySelector<HTMLDetailsElement>(
    ".approval-chat__options",
  )!;
  expect(options).not.toBeNull();
  await act(async () => {
    options.open = true;
    options.dispatchEvent(new Event("toggle"));
  });
  const change = Array.from(container.querySelectorAll("button")).find(
    (button) => button.textContent === "chat change model",
  )!;
  await act(async () => change.click());
  expect(api.setOptions).toHaveBeenCalledWith("request", "request", {
    model: "chat-only-model",
  });
  expect(api.send).not.toHaveBeenCalled();
  expect(
    container.querySelector(".approval-chat__options summary")?.textContent,
  ).toContain("chat-only-model");
  await act(async () => root.unmount());
  container.remove();
});
