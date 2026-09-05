import { Streamdown } from "streamdown";
import type { JSX } from "react";
import "./ApprovalMarkdown.css";

/** Presentation only: persisted provider text remains unchanged. */
export function ApprovalMarkdown({
  children,
}: {
  children: string;
}): JSX.Element {
  return (
    <Streamdown
      className="approval-markdown"
      mode="static"
      controls={false}
      skipHtml
      components={{
        img: () => null,
        strong: ({ children }) => <strong>{children}</strong>,
      }}
    >
      {children}
    </Streamdown>
  );
}
