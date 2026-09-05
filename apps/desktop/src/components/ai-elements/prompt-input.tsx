import type {
  ButtonHTMLAttributes,
  FormHTMLAttributes,
  HTMLAttributes,
  JSX,
  TextareaHTMLAttributes,
} from "react";
import { forwardRef } from "react";

export function PromptInput(
  props: FormHTMLAttributes<HTMLFormElement>,
): JSX.Element {
  return (
    <form {...props} className={`ai-prompt-input ${props.className ?? ""}`} />
  );
}

export function PromptInputBody(
  props: HTMLAttributes<HTMLDivElement>,
): JSX.Element {
  return (
    <div
      {...props}
      className={`ai-prompt-input__body ${props.className ?? ""}`}
    />
  );
}

export const PromptInputTextarea = forwardRef<
  HTMLTextAreaElement,
  TextareaHTMLAttributes<HTMLTextAreaElement>
>(function PromptInputTextarea(props, ref): JSX.Element {
  return (
    <textarea
      {...props}
      className={`ai-prompt-input__textarea ${props.className ?? ""}`}
      ref={ref}
    />
  );
});

export function PromptInputFooter(
  props: HTMLAttributes<HTMLDivElement>,
): JSX.Element {
  return (
    <div
      {...props}
      className={`ai-prompt-input__footer ${props.className ?? ""}`}
    />
  );
}

export function PromptInputSubmit(
  props: ButtonHTMLAttributes<HTMLButtonElement> & {
    status?: "idle" | "submitted" | "streaming" | "error";
    onStop?: () => void;
  },
): JSX.Element {
  const { status = "idle", onStop, onClick, ...attributes } = props;
  const generating = status === "submitted" || status === "streaming";
  return (
    <button
      {...attributes}
      className={`ai-prompt-input__submit ${attributes.className ?? ""}`}
      data-status={status}
      onClick={(event) => {
        if (generating && onStop) {
          event.preventDefault();
          onStop();
          return;
        }
        onClick?.(event);
      }}
      type={generating && onStop ? "button" : (attributes.type ?? "submit")}
    />
  );
}
