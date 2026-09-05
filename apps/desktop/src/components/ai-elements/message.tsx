import {
  memo,
  type ComponentProps,
  type HTMLAttributes,
  type JSX,
} from "react";
import { Streamdown, type StreamdownTranslations } from "streamdown";

const chineseTranslations: Partial<StreamdownTranslations> = {
  close: "关闭",
  copied: "已复制",
  copyCode: "复制代码",
  copyLink: "复制链接",
  copyTable: "复制表格",
  copyTableAsCsv: "复制为 CSV",
  copyTableAsMarkdown: "复制为 Markdown",
  copyTableAsTsv: "复制为 TSV",
  downloadFile: "下载代码",
  downloadTable: "下载表格",
  downloadTableAsCsv: "下载 CSV",
  downloadTableAsMarkdown: "下载 Markdown",
  viewFullscreen: "全屏查看",
  exitFullscreen: "退出全屏",
  openLink: "打开链接",
  openExternalLink: "打开外部链接",
  imageNotAvailable: "图片不可用",
};

export type MessageRole = "user" | "assistant" | "system";

export function Message(
  props: HTMLAttributes<HTMLDivElement> & { from: MessageRole },
): JSX.Element {
  const { from, ...attributes } = props;
  return (
    <article
      {...attributes}
      className={`ai-message ai-message--${from} ${attributes.className ?? ""}`}
      data-role={from}
    />
  );
}

export function MessageContent(
  props: HTMLAttributes<HTMLDivElement>,
): JSX.Element {
  return (
    <div
      {...props}
      className={`ai-message__content ${props.className ?? ""}`}
    />
  );
}

export const MessageResponse = memo(function MessageResponse(
  props: ComponentProps<typeof Streamdown> & { zh?: boolean },
): JSX.Element {
  const { zh, ...attributes } = props;
  return (
    <Streamdown
      translations={zh ? chineseTranslations : undefined}
      {...attributes}
      className={`ai-message__response ${props.className ?? ""}`}
    />
  );
});
