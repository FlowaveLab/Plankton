import { AcpSessionOptions } from "./AcpSessionOptions";
import {
  Bot,
  Check,
  ChevronDown,
  Copy,
  History,
  MessageSquare,
  Pencil,
  Plus,
  Send,
  Sparkles,
  Square,
  X,
} from "lucide-react";
import { useRef, useState, type JSX } from "react";
import type { StickToBottomContext } from "use-stick-to-bottom";
import {
  approvalChatApi,
  type ApprovalChatApi,
  type ApprovalChatMessage,
  type ApprovalChatSnapshot,
} from "../approvalChatApi";
import {
  chatTitle,
  isChatActive,
  useApprovalChat,
} from "../hooks/useApprovalChat";
import { ApprovalChatMessageBody } from "./ApprovalChatMessageBody";
import {
  Conversation,
  ConversationContent,
  ConversationEmptyState,
  ConversationScrollButton,
} from "./ai-elements/conversation";
import { Message, MessageContent } from "./ai-elements/message";
import {
  PromptInput,
  PromptInputBody,
  PromptInputFooter,
  PromptInputSubmit,
  PromptInputTextarea,
} from "./ai-elements/prompt-input";
import { Suggestion, Suggestions } from "./ai-elements/suggestion";

export type {
  ApprovalChatApi,
  ApprovalChatMessage,
  ApprovalChatSnapshot,
} from "../approvalChatApi";

type ApprovalChatProps = {
  requestId: string;
  zh: boolean;
  api?: ApprovalChatApi;
};

const quickActions = [
  {
    zh: "解释转人工原因",
    en: "Explain escalation",
    promptZh:
      "结合当前审批证据，解释为什么转为人工审批，并指出最关键的不确定项。",
    promptEn:
      "Explain why this request escalated to human review and identify the most important uncertainty.",
  },
  {
    zh: "补齐缺失证据",
    en: "Complete evidence",
    promptZh:
      "检查当前调用链中还缺少哪些证据；可以读取已授权的脚本，并给出可验证的结论。",
    promptEn:
      "Inspect the available call-chain evidence and authorized scripts, then close the important evidence gaps.",
  },
  {
    zh: "调整可见范围",
    en: "Adjust visibility",
    promptZh:
      "检查当前字段可见范围；如有必要，使用 plankton 将范围调整为满足本次请求的最小权限，并清楚说明实际变更。",
    promptEn:
      "Inspect the field visibility scope and, if needed, use plankton to apply the minimum scope required for this request; report the exact change.",
  },
  {
    zh: "给出更安全方案",
    en: "Safer alternative",
    promptZh:
      "给出完成同一目标但暴露面更小的命令或操作方案，不要显示任何凭据值。",
    promptEn:
      "Propose a safer way to achieve the same goal with less exposure. Never reveal a credential value.",
  },
];

function statusLabel(
  snapshot: ApprovalChatSnapshot | undefined,
  zh: boolean,
): string {
  switch (snapshot?.state) {
    case "queued":
      return zh ? "等待详细解释完成" : "Waiting for review details";
    case "running":
      return zh ? "正在生成" : "Generating";
    case "stopping":
      return zh ? "正在停止" : "Stopping";
    case "failed":
      return zh ? "生成失败" : "Failed";
    default:
      return zh ? "可继续对话" : "Ready to continue";
  }
}

function MessageActions({
  message,
  zh,
}: {
  message: ApprovalChatMessage;
  zh: boolean;
}): JSX.Element {
  const [copied, setCopied] = useState(false);
  const [failed, setFailed] = useState(false);
  return (
    <div className="approval-chat__message-actions">
      <button
        type="button"
        aria-label={zh ? "复制回复" : "Copy response"}
        onClick={() => {
          if (!navigator.clipboard) {
            setFailed(true);
            return;
          }
          void navigator.clipboard.writeText(message.content).then(
            () => {
              setCopied(true);
              setFailed(false);
            },
            () => setFailed(true),
          );
        }}
      >
        {copied ? <Check size={12} /> : <Copy size={12} />}
        {copied ? (zh ? "已复制" : "Copied") : zh ? "复制" : "Copy"}
      </button>
      {failed ? (
        <span role="status">
          {zh ? "复制失败，请选择文本复制" : "Select text to copy"}
        </span>
      ) : null}
      {message.state === "stopped" ? (
        <span>{zh ? "已停止" : "Stopped"}</span>
      ) : null}
      {message.state === "error" ? (
        <span>{zh ? "未完成" : "Incomplete"}</span>
      ) : null}
    </div>
  );
}

export function ApprovalChat(props: ApprovalChatProps): JSX.Element {
  return <ApprovalChatPanel key={props.requestId} {...props} />;
}

function ApprovalChatPanel({
  requestId,
  zh,
  api = approvalChatApi,
}: ApprovalChatProps): JSX.Element {
  const [open, setOpen] = useState(false);
  const [optionsOpen, setOptionsOpen] = useState(false);
  const [search, setSearch] = useState("");
  const [renameId, setRenameId] = useState<string | null>(null);
  const [title, setTitle] = useState("");
  const inputRef = useRef<HTMLTextAreaElement>(null);
  const scrollContext = useRef<StickToBottomContext>(null);
  const chat = useApprovalChat(requestId, open, api);
  const current = chat.snapshot;
  const messages = current?.messages ?? [];
  const queued = current?.state === "queued";
  const stopping = current?.state === "stopping";
  const characters = [...chat.draft].length;
  const tooLong = characters > 8000;
  const sessions = chat.history.filter((session) =>
    `${chatTitle(session, zh)} ${session.messages.find((message) => message.role === "user")?.content ?? ""}`
      .toLocaleLowerCase()
      .includes(search.toLocaleLowerCase()),
  );
  const select = (id: string) => {
    chat.select(id);
    setRenameId(null);
  };

  return (
    <details
      className="approval-chat"
      onToggle={(event) => setOpen(event.currentTarget.open)}
    >
      <summary>
        <span>
          <Bot aria-hidden="true" size={17} />
          <strong>
            {zh ? "与审批 Agent 对话" : "Chat with the review agent"}
          </strong>
        </span>
        <span
          className="approval-chat__status"
          data-state={current?.state ?? "idle"}
        >
          {chat.history.some(isChatActive)
            ? zh
              ? "对话进行中"
              : "Conversation active"
            : chat.history.length
              ? `${chat.history.length} ${zh ? "个对话" : "conversations"}`
              : zh
                ? "分析 · 跟进"
                : "Explore · Follow up"}
          <ChevronDown
            className="approval-chat__chevron"
            aria-hidden="true"
            size={16}
          />
        </span>
      </summary>
      {open ? (
        <div className="approval-chat__body">
          <aside
            className="approval-chat__history"
            aria-label={zh ? "对话历史" : "Conversation history"}
          >
            <header>
              <span>
                <History size={14} />
                {zh ? "对话历史" : "History"}
              </span>
              <small>{chat.history.length}</small>
            </header>
            <button
              className="approval-chat__new"
              type="button"
              disabled={chat.loading || chat.managing}
              onClick={() => void chat.create()}
            >
              <Plus size={14} />
              {zh ? "新建对话" : "New conversation"}
            </button>
            <input
              className="approval-chat__search"
              type="search"
              aria-label={zh ? "搜索对话" : "Search conversations"}
              placeholder={zh ? "搜索对话…" : "Search conversations…"}
              value={search}
              onChange={(event) => setSearch(event.currentTarget.value)}
            />
            <nav
              className="approval-chat__sessions"
              aria-label={zh ? "已保存的对话" : "Saved conversations"}
            >
              {sessions.map((session) => (
                <button
                  type="button"
                  key={session.conversation_id}
                  aria-current={
                    session.conversation_id === chat.selectedId
                      ? "true"
                      : undefined
                  }
                  onClick={() => select(session.conversation_id)}
                >
                  <span className="approval-chat__session-title">
                    <MessageSquare size={13} />
                    <strong>{chatTitle(session, zh)}</strong>
                  </span>
                  <small>
                    <span>
                      {
                        session.messages.filter(
                          (message) => message.role === "user",
                        ).length
                      }{" "}
                      {zh ? "轮对话" : "turns"}
                    </span>
                    {isChatActive(session) ? (
                      <span className="approval-chat__live">
                        {zh ? "进行中" : "Active"}
                      </span>
                    ) : (
                      <time dateTime={session.updated_at}>
                        {new Date(session.updated_at).toLocaleDateString(
                          zh ? "zh-CN" : "en-US",
                          { month: "short", day: "numeric" },
                        )}
                      </time>
                    )}
                  </small>
                </button>
              ))}
              {!sessions.length && !chat.loading ? (
                <p className="approval-chat__history-empty">
                  {zh ? "没有匹配的对话" : "No matching conversations"}
                </p>
              ) : null}
            </nav>
            <p className="approval-chat__history-note">
              {zh
                ? "当前审批的对话会保存在本机，稍后可继续。"
                : "Conversations for this review are saved on this device."}
            </p>
          </aside>
          <section
            className="approval-chat__main"
            aria-label={zh ? "当前对话" : "Current conversation"}
          >
            <header className="approval-chat__toolbar">
              <div className="approval-chat__current-title">
                <strong>
                  {current
                    ? chatTitle(current, zh)
                    : zh
                      ? "载入对话"
                      : "Loading conversation"}
                </strong>
                <span role="status" data-state={current?.state}>
                  {statusLabel(current, zh)}
                </span>
              </div>
              <div className="approval-chat__toolbar-actions">
                <button
                  type="button"
                  aria-label={zh ? "重命名对话" : "Rename conversation"}
                  disabled={!current || chat.loading}
                  onClick={() => {
                    setRenameId(chat.selectedId);
                    setTitle(current ? chatTitle(current, zh) : "");
                  }}
                >
                  <Pencil size={14} />
                </button>
                <button
                  className="approval-chat__mobile-new"
                  type="button"
                  aria-label={zh ? "新建对话" : "New conversation"}
                  disabled={chat.loading || chat.managing}
                  onClick={() => void chat.create()}
                >
                  <Plus size={15} />
                </button>
              </div>
            </header>
            <select
              className="approval-chat__mobile-select"
              aria-label={zh ? "切换对话" : "Switch conversation"}
              value={chat.selectedId}
              onChange={(event) => select(event.currentTarget.value)}
            >
              {chat.history.map((session) => (
                <option
                  key={session.conversation_id}
                  value={session.conversation_id}
                >
                  {chatTitle(session, zh)}
                  {isChatActive(session)
                    ? zh
                      ? " · 进行中"
                      : " · Active"
                    : ""}
                </option>
              ))}
            </select>
            {current?.acp_profile && api.setOptions ? (
              <details
                className="approval-chat__options"
                onToggle={(event) => setOptionsOpen(event.currentTarget.open)}
              >
                <summary>
                  {zh ? "Chat 模型与思考设置" : "Chat model and reasoning"}
                  <span>
                    {Object.entries(current.acp_profile.session_options ?? {})
                      .map(([id, value]) => `${id}: ${value}`)
                      .join(" · ") ||
                      (zh ? "使用 Chat 默认配置" : "Chat defaults")}
                  </span>
                </summary>
                {optionsOpen ? (
                  <AcpSessionOptions
                    context="chat"
                    profile={current.acp_profile}
                    disabled={chat.managing}
                    zh={zh}
                    onChange={(profile) =>
                      void chat.setOptions(profile.session_options ?? {})
                    }
                  />
                ) : null}
              </details>
            ) : null}
            {renameId === chat.selectedId ? (
              <form
                className="approval-chat__rename"
                onSubmit={(event) => {
                  event.preventDefault();
                  void chat.rename(title).then((saved) => {
                    if (saved) setRenameId(null);
                  });
                }}
              >
                <input
                  aria-label={zh ? "对话名称" : "Conversation title"}
                  value={title}
                  maxLength={80}
                  autoFocus
                  onChange={(event) => setTitle(event.currentTarget.value)}
                  onKeyDown={(event) => {
                    if (event.key === "Escape") setRenameId(null);
                  }}
                />
                <button type="submit" disabled={!title.trim() || chat.managing}>
                  {zh ? "保存" : "Save"}
                </button>
                <button
                  type="button"
                  aria-label={zh ? "取消重命名" : "Cancel rename"}
                  onClick={() => setRenameId(null)}
                >
                  <X size={14} />
                </button>
              </form>
            ) : null}
            <Conversation
              key={chat.selectedId}
              contextRef={scrollContext}
              aria-label={zh ? "审批对话消息" : "Approval chat messages"}
            >
              <ConversationContent>
                {chat.loading && !current ? (
                  <p className="approval-chat__loading" role="status">
                    {zh ? "正在载入对话…" : "Loading conversations…"}
                  </p>
                ) : messages.length ? (
                  messages.map((message) => (
                    <Message from={message.role} key={message.id}>
                      <MessageContent>
                        <div className="approval-chat__message-meta">
                          <span>
                            {message.role === "user"
                              ? zh
                                ? "你"
                                : "You"
                              : message.role === "assistant"
                                ? "PLANKTON AGENT"
                                : zh
                                  ? "系统"
                                  : "System"}
                          </span>
                          <time dateTime={message.created_at}>
                            {new Date(message.created_at).toLocaleTimeString(
                              zh ? "zh-CN" : "en-US",
                              {
                                hour: "2-digit",
                                minute: "2-digit",
                                hour12: false,
                              },
                            )}
                          </time>
                        </div>
                        <ApprovalChatMessageBody message={message} zh={zh} />
                        {message.role === "assistant" &&
                        message.kind === "text" &&
                        message.content &&
                        !["queued", "streaming"].includes(message.state) ? (
                          <MessageActions message={message} zh={zh} />
                        ) : null}
                      </MessageContent>
                    </Message>
                  ))
                ) : (
                  <ConversationEmptyState
                    icon={
                      <Sparkles
                        aria-hidden="true"
                        size={26}
                        strokeWidth={1.4}
                      />
                    }
                    title={
                      zh
                        ? "从这条审批，继续探索"
                        : "Continue exploring this review"
                    }
                    description={
                      zh
                        ? "分析证据、追问原因，或讨论下一步操作。每个对话独立保留上下文。"
                        : "Explore evidence, ask why, or discuss next steps. Each conversation keeps its own context."
                    }
                  />
                )}
                {chat.pending && !chat.active ? (
                  <p className="approval-chat__loading" role="status">
                    {zh ? "正在发送…" : "Sending…"}
                  </p>
                ) : null}
              </ConversationContent>
              <ConversationScrollButton
                aria-label={zh ? "滚动到最新消息" : "Scroll to latest message"}
              />
            </Conversation>
            <div className="approval-chat__composer">
              {chat.connectionError ? (
                <div className="approval-chat__error" role="alert">
                  <span>{chat.connectionError}</span>
                  <button type="button" onClick={chat.retry}>
                    {zh ? "重新连接" : "Reconnect"}
                  </button>
                </div>
              ) : null}
              {chat.error ? (
                <p className="approval-chat__error" role="alert">
                  {chat.error}
                </p>
              ) : null}
              <Suggestions aria-label={zh ? "快捷操作" : "Quick actions"}>
                {quickActions.map((action) => (
                  <Suggestion
                    key={action.en}
                    suggestion={zh ? action.promptZh : action.promptEn}
                    onClick={() => {
                      chat.setDraft(zh ? action.promptZh : action.promptEn);
                      inputRef.current?.focus();
                    }}
                  >
                    {zh ? action.zh : action.en}
                  </Suggestion>
                ))}
              </Suggestions>
              <PromptInput
                onSubmit={(event) => {
                  event.preventDefault();
                  if (
                    chat.active ||
                    chat.pending ||
                    tooLong ||
                    !chat.draft.trim()
                  )
                    return;
                  void scrollContext.current?.scrollToBottom({
                    animation: "instant",
                  });
                  void chat.send();
                }}
              >
                <PromptInputBody>
                  <PromptInputTextarea
                    aria-label={
                      zh ? "给审批 Agent 的消息" : "Message to review agent"
                    }
                    aria-invalid={tooLong}
                    ref={inputRef}
                    value={chat.draft}
                    onChange={(event) =>
                      chat.setDraft(event.currentTarget.value)
                    }
                    onKeyDown={(event) => {
                      if (
                        event.key === "Enter" &&
                        !event.shiftKey &&
                        !event.nativeEvent.isComposing &&
                        event.keyCode !== 229
                      ) {
                        event.preventDefault();
                        event.currentTarget.form?.requestSubmit();
                      }
                    }}
                    placeholder={
                      zh
                        ? "输入问题，继续分析当前审批…"
                        : "Ask a question about this review…"
                    }
                    rows={3}
                  />
                </PromptInputBody>
                <PromptInputFooter>
                  <span>
                    {tooLong
                      ? zh
                        ? "消息不能超过 8,000 字"
                        : "Message exceeds 8,000 characters"
                      : zh
                        ? "Enter 发送 · Shift + Enter 换行"
                        : "Enter to send · Shift + Enter for a new line"}
                    {characters > 7000 ? ` · ${characters}/8000` : ""}
                  </span>
                  <PromptInputSubmit
                    aria-label={
                      chat.active
                        ? zh
                          ? queued
                            ? "取消待发送消息"
                            : "停止生成"
                          : queued
                            ? "Cancel queued message"
                            : "Stop generating"
                        : zh
                          ? "发送消息"
                          : "Send message"
                    }
                    disabled={
                      stopping ||
                      (!chat.active &&
                        (chat.pending ||
                          chat.managing ||
                          chat.loading ||
                          !current ||
                          !!chat.connectionError ||
                          !chat.draft.trim() ||
                          tooLong))
                    }
                    onStop={() => void chat.stop()}
                    status={chat.active ? "streaming" : "idle"}
                  >
                    {chat.active ? (
                      <Square
                        aria-hidden="true"
                        fill="currentColor"
                        size={11}
                      />
                    ) : (
                      <Send aria-hidden="true" size={14} />
                    )}
                    {chat.active
                      ? zh
                        ? queued
                          ? "取消排队"
                          : stopping
                            ? "停止中"
                            : "停止"
                        : queued
                          ? "Cancel queue"
                          : stopping
                            ? "Stopping"
                            : "Stop"
                      : zh
                        ? "发送"
                        : "Send"}
                  </PromptInputSubmit>
                </PromptInputFooter>
              </PromptInput>
            </div>
          </section>
        </div>
      ) : null}
    </details>
  );
}
