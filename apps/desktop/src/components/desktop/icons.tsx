import {
  Activity,
  Bot,
  Cable,
  Check,
  ChevronLeft,
  ChevronRight,
  Copy,
  Ellipsis,
  Eye,
  EyeOff,
  Inbox,
  KeyRound,
  ListFilter,
  RefreshCw,
  ScrollText,
  Search,
  ShieldCheck,
  Trash2,
  Terminal,
  X,
} from "lucide-react";
import type { JSX, SVGProps } from "react";
import { planktonMarkPath } from "../../generated/planktonMark";

export {
  Activity,
  Bot,
  Cable,
  Check,
  ChevronLeft,
  ChevronRight,
  Copy,
  Ellipsis,
  Eye,
  EyeOff,
  Inbox,
  KeyRound,
  ListFilter,
  RefreshCw,
  ScrollText,
  Search,
  ShieldCheck,
  Trash2,
  Terminal,
  X,
};

export const workspaceIcons = {
  requests: Inbox,
  passwords: KeyRound,
  connections: Cable,
  agents: Bot,
  policies: ShieldCheck,
  audit: ScrollText,
  diagnostics: Activity,
} as const;

export function BrandMark(
  props: Omit<SVGProps<SVGSVGElement>, "children" | "viewBox">,
): JSX.Element {
  return (
    <svg
      {...props}
      aria-hidden="true"
      data-brand-mark
      focusable="false"
      viewBox="0 0 64 64"
    >
      <path d={planktonMarkPath} fill="currentColor" fillRule="evenodd" />
    </svg>
  );
}
