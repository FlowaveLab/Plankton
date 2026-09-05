import { ArrowDown } from "lucide-react";
import { StickToBottom, useStickToBottomContext } from "use-stick-to-bottom";
import {
  useCallback,
  type ComponentProps,
  type HTMLAttributes,
  type JSX,
  type ReactNode,
} from "react";

export function Conversation(
  props: ComponentProps<typeof StickToBottom>,
): JSX.Element {
  return (
    <StickToBottom
      {...props}
      className={`ai-conversation ${props.className ?? ""}`}
      initial={props.initial ?? "instant"}
      resize={props.resize ?? "instant"}
      role={props.role ?? "log"}
    />
  );
}

export function ConversationContent(
  props: ComponentProps<typeof StickToBottom.Content>,
): JSX.Element {
  return (
    <StickToBottom.Content
      {...props}
      scrollClassName={`ai-conversation__scroll ${props.scrollClassName ?? ""}`}
      className={`ai-conversation__content ${props.className ?? ""}`}
    />
  );
}

export function ConversationScrollButton(
  props: ComponentProps<"button">,
): JSX.Element | null {
  const { isAtBottom, scrollToBottom } = useStickToBottomContext();
  const handleClick = useCallback(() => {
    void scrollToBottom();
  }, [scrollToBottom]);
  if (isAtBottom) return null;
  return (
    <button
      {...props}
      className={`ai-conversation__scroll-button ${props.className ?? ""}`}
      onClick={(event) => {
        props.onClick?.(event);
        if (!event.defaultPrevented) handleClick();
      }}
      type="button"
    >
      {props.children ?? <ArrowDown aria-hidden="true" size={15} />}
    </button>
  );
}

export function ConversationEmptyState(
  props: {
    title: string;
    description?: string;
    icon?: ReactNode;
  } & HTMLAttributes<HTMLDivElement>,
): JSX.Element {
  const { title, description, icon, ...attributes } = props;
  return (
    <div
      {...attributes}
      className={`ai-conversation__empty ${attributes.className ?? ""}`}
    >
      {icon}
      <span>
        <strong>{title}</strong>
        {description ? <small>{description}</small> : null}
      </span>
    </div>
  );
}
