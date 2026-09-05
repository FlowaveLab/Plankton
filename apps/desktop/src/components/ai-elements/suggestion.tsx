import type { ButtonHTMLAttributes, HTMLAttributes, JSX } from "react";

export function Suggestions(
  props: HTMLAttributes<HTMLDivElement>,
): JSX.Element {
  return (
    <div
      {...props}
      className={`ai-suggestions ${props.className ?? ""}`}
      role={props.role ?? "group"}
    />
  );
}

export function Suggestion(
  props: ButtonHTMLAttributes<HTMLButtonElement> & { suggestion: string },
): JSX.Element {
  const { suggestion, ...attributes } = props;
  return (
    <button
      {...attributes}
      className={`ai-suggestion ${attributes.className ?? ""}`}
      type={attributes.type ?? "button"}
    >
      {attributes.children ?? suggestion}
    </button>
  );
}
