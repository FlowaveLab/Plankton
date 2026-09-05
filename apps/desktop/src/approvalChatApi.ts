import type { AcpProfile } from "./types";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

const APPROVAL_CHAT_EVENT = "plankton://approval-chat";

export type ApprovalChatMessage = {
  id: string;
  role: "user" | "assistant" | "system";
  kind: "text" | "thought" | "tool_call";
  content: string;
  state: "complete" | "queued" | "streaming" | "stopped" | "error";
  created_at: string;
  tool_call: {
    id: string;
    title: string;
    kind: string;
    status: string;
    input: string | null;
  } | null;
};

export type ApprovalChatSnapshot = {
  acp_profile?: AcpProfile | null;
  request_id: string;
  conversation_id: string;
  title: string;
  updated_at: string;
  session_id: string | null;
  state: "idle" | "queued" | "running" | "stopping" | "failed" | "released";
  messages: ApprovalChatMessage[];
  error: string | null;
};

export type ApprovalChatApi = {
  setOptions?: (
    requestId: string,
    conversationId: string,
    sessionOptions: Record<string, string>,
  ) => Promise<ApprovalChatSnapshot>;
  history: (requestId: string) => Promise<ApprovalChatSnapshot[]>;
  create: (requestId: string) => Promise<ApprovalChatSnapshot>;
  rename: (
    requestId: string,
    conversationId: string,
    title: string,
  ) => Promise<ApprovalChatSnapshot>;
  load: (
    requestId: string,
    conversationId?: string,
  ) => Promise<ApprovalChatSnapshot>;
  send: (
    requestId: string,
    message: string,
    conversationId: string,
  ) => Promise<ApprovalChatSnapshot>;
  stop: (
    requestId: string,
    conversationId: string,
  ) => Promise<ApprovalChatSnapshot>;
  subscribe: (
    onSnapshot: (snapshot: ApprovalChatSnapshot) => void,
  ) => Promise<() => void>;
};

export const approvalChatApi: ApprovalChatApi = {
  setOptions(requestId, conversationId, sessionOptions) {
    return invoke<ApprovalChatSnapshot>("update_approval_chat_options", {
      requestId,
      conversationId,
      sessionOptions,
    });
  },
  history(requestId) {
    return invoke<ApprovalChatSnapshot[]>("approval_chat_history", {
      requestId,
    });
  },
  create(requestId) {
    return invoke<ApprovalChatSnapshot>("create_approval_chat", { requestId });
  },
  rename(requestId, conversationId, title) {
    return invoke<ApprovalChatSnapshot>("rename_approval_chat", {
      requestId,
      conversationId,
      title,
    });
  },
  load(requestId, conversationId) {
    return invoke<ApprovalChatSnapshot>("approval_chat_snapshot", {
      requestId,
      conversationId,
    });
  },
  send(requestId, message, conversationId) {
    return invoke<ApprovalChatSnapshot>("send_approval_chat_message", {
      requestId,
      conversationId,
      message,
    });
  },
  stop(requestId, conversationId) {
    return invoke<ApprovalChatSnapshot>("stop_approval_chat", {
      requestId,
      conversationId,
    });
  },
  async subscribe(onSnapshot) {
    return listen<ApprovalChatSnapshot>(APPROVAL_CHAT_EVENT, (event) => {
      onSnapshot(event.payload);
    });
  },
};
