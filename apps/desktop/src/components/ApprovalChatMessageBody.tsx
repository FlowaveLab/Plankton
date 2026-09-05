import { Brain, Wrench } from "lucide-react";
import { useState, type JSX } from "react";
import type { ApprovalChatMessage } from "../approvalChatApi";
import { MessageResponse } from "./ai-elements/message";

function toolStatusLabel(status: string, zh: boolean): string {
  const labels: Record<string, [string, string]> = {
    pending: ["等待", "Pending"],
    in_progress: ["执行中", "Running"],
    completed: ["已完成", "Completed"],
    failed: ["失败", "Failed"],
  };
  const label = labels[status];
  return label ? label[zh ? 0 : 1] : status;
}

function ApprovalChatThought(props: {
  message: ApprovalChatMessage;
  zh: boolean;
}): JSX.Element {
  const [open, setOpen] = useState(props.message.state === "streaming");
  return (
    <details
      className="approval-chat__thinking"
      data-state={props.message.state}
      onToggle={(event) => setOpen(event.currentTarget.open)}
      open={open}
    >
      <summary>
        <Brain aria-hidden="true" size={14} strokeWidth={1.7} />
        <span>{props.zh ? "思考过程" : "Thinking"}</span>
        {props.message.state === "streaming" ? (
          <small>{props.zh ? "生成中" : "Streaming"}</small>
        ) : null}
      </summary>
      <MessageResponse
        zh={props.zh}
        aria-live={props.message.state === "streaming" ? "polite" : undefined}
        isAnimating={props.message.state === "streaming"}
      >
        {props.message.content}
      </MessageResponse>
    </details>
  );
}

export function ApprovalChatMessageBody(props: {
  message: ApprovalChatMessage;
  zh: boolean;
}): JSX.Element {
  const { message, zh } = props;
  if (message.role === "user")
    return <p className="approval-chat__user-text">{message.content}</p>;
  if (message.kind === "thought") {
    return <ApprovalChatThought message={message} zh={zh} />;
  }
  if (message.kind === "tool_call" && message.tool_call) {
    const tool = message.tool_call;
    return (
      <section className="approval-chat__tool-call" data-status={tool.status}>
        <header>
          <Wrench aria-hidden="true" size={14} strokeWidth={1.7} />
          <strong>{tool.title}</strong>
          <span>{tool.kind}</span>
          <small>{toolStatusLabel(tool.status, zh)}</small>
        </header>
        {tool.input ? (
          <details>
            <summary>{zh ? "查看调用参数" : "View tool input"}</summary>
            <pre>
              <code>{tool.input}</code>
            </pre>
          </details>
        ) : null}
      </section>
    );
  }
  if (
    !message.content &&
    (message.state === "stopped" || message.state === "error")
  ) {
    return (
      <p className="approval-chat__message-note">
        {message.state === "stopped"
          ? zh
            ? "生成已停止"
            : "Generation stopped"
          : zh
            ? "未能生成回复"
            : "No response generated"}
      </p>
    );
  }
  return (
    <MessageResponse
      zh={zh}
      mode={message.state === "streaming" ? "streaming" : "static"}
      aria-live={message.state === "streaming" ? "polite" : undefined}
      isAnimating={message.state === "streaming"}
    >
      {message.content ||
        (message.state === "queued"
          ? zh
            ? "已排队；详细解释完成后自动开始…"
            : "Queued; starts automatically after review details finish…"
          : zh
            ? "正在生成…"
            : "Streaming…")}
    </MessageResponse>
  );
}
