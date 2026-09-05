import {
  useEffect,
  useId,
  useRef,
  useState,
  type CSSProperties,
  type JSX,
  type ReactNode,
  type RefObject,
} from "react";

import { ChevronLeft, ChevronRight, X } from "./icons";
import { Inbox, TriangleAlert, type LucideIcon } from "lucide-react";

type PageHeaderProps = {
  icon?: LucideIcon;
  title: string;
  description?: string;
  eyebrow?: string;
  status?: ReactNode;
  primaryAction?: ReactNode;
};

export function PageHeader(props: PageHeaderProps): JSX.Element {
  const Icon = props.icon;
  return (
    <header className="page-header">
      <div className="page-header-copy">
        {props.eyebrow ? <p className="eyebrow">{props.eyebrow}</p> : null}
        <h1>
          {Icon ? (
            <Icon
              aria-hidden="true"
              focusable="false"
              size={22}
              strokeWidth={1.75}
            />
          ) : null}
          <span>{props.title}</span>
        </h1>
        {props.description ? <p>{props.description}</p> : null}
      </div>
      {props.status || props.primaryAction ? (
        <div className="page-header-actions">
          {props.status ? (
            <div className="page-header-status" role="status">
              {props.status}
            </div>
          ) : null}
          {props.primaryAction}
        </div>
      ) : null}
    </header>
  );
}

type GuidedStateProps = {
  icon?: LucideIcon;
  title: string;
  description: string;
  action: ReactNode;
  eyebrow?: ReactNode;
};

export function EmptyState(props: GuidedStateProps): JSX.Element {
  const titleId = useId();
  const Icon = props.icon ?? Inbox;
  return (
    <section
      aria-labelledby={titleId}
      className="page-state page-empty-state"
      data-state="empty"
    >
      <Icon
        className="page-state-icon"
        aria-hidden="true"
        focusable="false"
        size={32}
        strokeWidth={1.5}
      />
      {props.eyebrow ? <p className="eyebrow">{props.eyebrow}</p> : null}
      <h2 id={titleId}>{props.title}</h2>
      <p>{props.description}</p>
      <div className="page-state-action">{props.action}</div>
    </section>
  );
}

export function ErrorState(props: GuidedStateProps): JSX.Element {
  const titleId = useId();
  return (
    <section
      aria-labelledby={titleId}
      className="page-state page-error-state"
      data-state="error"
      role="alert"
    >
      <TriangleAlert
        className="page-state-icon"
        aria-hidden="true"
        focusable="false"
        size={32}
        strokeWidth={1.5}
      />
      {props.eyebrow ? <p className="eyebrow">{props.eyebrow}</p> : null}
      <h2 id={titleId}>{props.title}</h2>
      <p>{props.description}</p>
      <div className="page-state-action">{props.action}</div>
    </section>
  );
}

type SplitPaneProps = {
  list: ReactNode;
  detail: ReactNode;
  listLabel: string;
  detailLabel: string;
  listVisible?: boolean;
  resizable?: boolean;
  storageKey?: string;
  initialListWidth?: number;
  minListWidth?: number;
  minDetailWidth?: number;
};

export function SplitPane(props: SplitPaneProps): JSX.Element {
  const paneRef = useRef<HTMLDivElement | null>(null);
  const initialListWidth = props.initialListWidth ?? 340;
  const minListWidth = props.minListWidth ?? 240;
  const minDetailWidth = props.minDetailWidth ?? 380;
  const [listWidth, setListWidth] = useState(initialListWidth);
  const [dragging, setDragging] = useState(false);
  const listVisible = props.listVisible ?? true;

  useEffect(() => {
    if (!props.storageKey) return;
    const storage = window.localStorage;
    if (!storage) return;
    const stored = Number.parseFloat(storage.getItem(props.storageKey) ?? "");
    if (Number.isFinite(stored)) setListWidth(stored);
  }, [props.storageKey]);

  useEffect(() => {
    if (!dragging) return;
    function resize(event: PointerEvent): void {
      const bounds = paneRef.current?.getBoundingClientRect();
      if (!bounds) return;
      const maximum = Math.max(minListWidth, bounds.width - minDetailWidth);
      setListWidth(
        Math.min(maximum, Math.max(minListWidth, event.clientX - bounds.left)),
      );
    }
    function stop(): void {
      setDragging(false);
    }
    window.addEventListener("pointermove", resize);
    window.addEventListener("pointerup", stop, { once: true });
    return () => {
      window.removeEventListener("pointermove", resize);
      window.removeEventListener("pointerup", stop);
    };
  }, [dragging, minListWidth, minDetailWidth]);

  useEffect(() => {
    if (props.storageKey && !dragging) {
      window.localStorage?.setItem(props.storageKey, String(listWidth));
    }
  }, [dragging, listWidth, props.storageKey]);

  const style = {
    "--page-split-list-width": `${listWidth}px`,
  } as CSSProperties;

  return (
    <div
      className="page-split-pane"
      data-list-visible={listVisible}
      data-resizable={Boolean(props.resizable && listVisible)}
      ref={paneRef}
      style={style}
    >
      {listVisible ? (
        <section aria-label={props.listLabel} className="page-split-pane-list">
          {props.list}
        </section>
      ) : null}
      {props.resizable && listVisible ? (
        <div
          aria-label="Resize request list"
          aria-orientation="vertical"
          aria-valuemax={720}
          aria-valuemin={minListWidth}
          aria-valuenow={Math.round(listWidth)}
          className="page-split-pane-resizer"
          onDoubleClick={() => setListWidth(initialListWidth)}
          onKeyDown={(event) => {
            if (event.key !== "ArrowLeft" && event.key !== "ArrowRight") return;
            event.preventDefault();
            const delta = event.key === "ArrowLeft" ? -24 : 24;
            setListWidth((current) =>
              Math.min(
                Math.max(
                  minListWidth,
                  (paneRef.current?.getBoundingClientRect().width || 1100) -
                    minDetailWidth,
                ),
                Math.max(minListWidth, current + delta),
              ),
            );
          }}
          onPointerDown={(event) => {
            event.preventDefault();
            setDragging(true);
          }}
          role="separator"
          tabIndex={0}
        >
          <span aria-hidden="true" />
        </div>
      ) : null}
      <section
        aria-label={props.detailLabel}
        className="page-split-pane-detail"
      >
        {props.detail}
      </section>
    </div>
  );
}

type PaginationProps = {
  page: number;
  pageCount: number;
  onPageChange: (page: number) => void;
  label?: string;
  previousLabel?: string;
  nextLabel?: string;
};

export function Pagination(props: PaginationProps): JSX.Element | null {
  if (props.pageCount <= 1) {
    return null;
  }

  const currentPage = Math.min(Math.max(1, props.page), props.pageCount);
  const previousLabel = props.previousLabel ?? "Previous page";
  const nextLabel = props.nextLabel ?? "Next page";

  return (
    <nav aria-label={props.label ?? "Pagination"} className="page-pagination">
      <button
        aria-label={previousLabel}
        disabled={currentPage === 1}
        onClick={() => props.onPageChange(currentPage - 1)}
        type="button"
      >
        <ChevronLeft
          aria-hidden="true"
          focusable="false"
          size={16}
          strokeWidth={1.75}
        />
      </button>
      <span aria-live="polite">
        {currentPage} / {props.pageCount}
      </span>
      <button
        aria-label={nextLabel}
        disabled={currentPage === props.pageCount}
        onClick={() => props.onPageChange(currentPage + 1)}
        type="button"
      >
        <ChevronRight
          aria-hidden="true"
          focusable="false"
          size={16}
          strokeWidth={1.75}
        />
      </button>
    </nav>
  );
}

const focusableSelector = [
  "a[href]",
  "button:not([disabled])",
  "input:not([disabled])",
  "select:not([disabled])",
  "textarea:not([disabled])",
  '[tabindex]:not([tabindex="-1"])',
].join(",");

type ModalEntry = {
  id: symbol;
  container: HTMLElement;
  onCloseRef: { current: () => void };
  previouslyFocused: HTMLElement | null;
};

type ModalScrollLock = {
  document: Document;
  initialFocus: HTMLElement | null;
  bodyOverflow: string;
  scrollOwner: HTMLElement | null;
  scrollOwnerOverflow: string;
};

const modalStack: ModalEntry[] = [];
let modalScrollLock: ModalScrollLock | null = null;

function focusInitialControl(container: HTMLElement): void {
  const initialFocus =
    container.querySelector<HTMLElement>("[data-dialog-initial-focus]") ??
    container.querySelector<HTMLElement>(focusableSelector) ??
    container;
  initialFocus.focus();
}

function workspaceScrollOwner(container: HTMLElement): HTMLElement | null {
  const workspace = container.closest<HTMLElement>(".desktop-workspace");
  return (
    workspace?.querySelector<HTMLElement>(".workspace-content") ??
    container.ownerDocument.querySelector<HTMLElement>(
      ".desktop-workspace .workspace-content",
    )
  );
}

function lockModalScrolling(container: HTMLElement): void {
  if (modalScrollLock) {
    return;
  }

  const document = container.ownerDocument;
  const scrollOwner = workspaceScrollOwner(container);
  modalScrollLock = {
    document,
    initialFocus:
      document.activeElement instanceof HTMLElement
        ? document.activeElement
        : null,
    bodyOverflow: document.body.style.overflow,
    scrollOwner,
    scrollOwnerOverflow: scrollOwner?.style.overflow ?? "",
  };
  document.body.style.overflow = "hidden";
  if (scrollOwner) {
    scrollOwner.style.overflow = "hidden";
  }
  document.addEventListener("keydown", handleModalKeyDown);
}

function unlockModalScrolling(): void {
  if (!modalScrollLock) {
    return;
  }

  const lock = modalScrollLock;
  modalScrollLock = null;
  lock.document.removeEventListener("keydown", handleModalKeyDown);
  lock.document.body.style.overflow = lock.bodyOverflow;
  if (lock.scrollOwner) {
    lock.scrollOwner.style.overflow = lock.scrollOwnerOverflow;
  }
  if (lock.initialFocus?.isConnected) {
    lock.initialFocus.focus();
  }
}

function trapModalFocus(event: KeyboardEvent, container: HTMLElement): void {
  const items = Array.from(
    container.querySelectorAll<HTMLElement>(focusableSelector),
  );
  if (items.length === 0) {
    event.preventDefault();
    container.focus();
    return;
  }

  const first = items[0];
  const last = items[items.length - 1];
  const activeElement = container.ownerDocument.activeElement;
  if (
    activeElement instanceof HTMLElement &&
    container.contains(activeElement) &&
    !items.includes(activeElement)
  ) {
    event.preventDefault();
    (event.shiftKey ? last : first).focus();
    return;
  }
  if (
    event.shiftKey &&
    (activeElement === first || !container.contains(activeElement))
  ) {
    event.preventDefault();
    last.focus();
  } else if (!event.shiftKey && activeElement === last) {
    event.preventDefault();
    first.focus();
  }
}

function handleModalKeyDown(event: KeyboardEvent): void {
  const topmost = modalStack.at(-1);
  if (!topmost) {
    return;
  }
  if (event.key === "Escape") {
    event.preventDefault();
    event.stopPropagation();
    topmost.onCloseRef.current();
    return;
  }
  if (event.key === "Tab") {
    trapModalFocus(event, topmost.container);
  }
}

function registerModal(entry: ModalEntry): () => void {
  lockModalScrolling(entry.container);
  const firstContainedModal = modalStack.findIndex((candidate) =>
    entry.container.contains(candidate.container),
  );
  if (firstContainedModal === -1) {
    modalStack.push(entry);
  } else {
    modalStack.splice(firstContainedModal, 0, entry);
  }
  if (modalStack.at(-1)?.id === entry.id) {
    focusInitialControl(entry.container);
  }

  return () => {
    const index = modalStack.findIndex(
      (candidate) => candidate.id === entry.id,
    );
    if (index === -1) {
      return;
    }
    const wasTopmost = index === modalStack.length - 1;
    modalStack.splice(index, 1);

    if (modalStack.length === 0) {
      unlockModalScrolling();
      return;
    }
    if (!wasTopmost) {
      return;
    }

    const newTopmost = modalStack.at(-1);
    if (!newTopmost) {
      return;
    }
    if (
      entry.previouslyFocused?.isConnected &&
      newTopmost.container.contains(entry.previouslyFocused)
    ) {
      entry.previouslyFocused.focus();
    } else {
      focusInitialControl(newTopmost.container);
    }
  };
}

function useModalBehavior(
  open: boolean,
  onClose: () => void,
  containerRef: RefObject<HTMLElement | null>,
): void {
  const modalIdRef = useRef(Symbol("page-modal"));
  const onCloseRef = useRef(onClose);
  onCloseRef.current = onClose;

  useEffect(() => {
    const container = containerRef.current;
    if (!open || !container) {
      return;
    }

    const previouslyFocused =
      document.activeElement instanceof HTMLElement
        ? document.activeElement
        : null;
    return registerModal({
      id: modalIdRef.current,
      container,
      onCloseRef,
      previouslyFocused,
    });
  }, [containerRef, open]);
}

type ModalSurfaceProps = {
  open: boolean;
  title: string;
  description?: string;
  closeDisabled?: boolean;
  closeLabel?: string;
  children: ReactNode;
  footer?: ReactNode;
  onClose: () => void;
};

export function Dialog(props: ModalSurfaceProps): JSX.Element | null {
  const titleId = useId();
  const descriptionId = useId();
  const dialogRef = useRef<HTMLElement>(null);
  useModalBehavior(props.open, props.onClose, dialogRef);

  if (!props.open) {
    return null;
  }

  return (
    <div className="page-modal-backdrop" role="presentation">
      <section
        aria-describedby={props.description ? descriptionId : undefined}
        aria-labelledby={titleId}
        aria-modal="true"
        className="page-dialog"
        ref={dialogRef}
        role="dialog"
        tabIndex={-1}
      >
        <header className="page-modal-header">
          <div>
            <h2 id={titleId}>{props.title}</h2>
            {props.description ? (
              <p id={descriptionId}>{props.description}</p>
            ) : null}
          </div>
          <button
            aria-busy={props.closeDisabled || undefined}
            aria-label={props.closeLabel ?? "Close dialog"}
            className="page-icon-button"
            disabled={props.closeDisabled}
            onClick={props.onClose}
            type="button"
          >
            <X
              aria-hidden="true"
              focusable="false"
              size={18}
              strokeWidth={1.75}
            />
          </button>
        </header>
        <div className="page-modal-body">{props.children}</div>
        {props.footer ? (
          <footer className="page-modal-footer">{props.footer}</footer>
        ) : null}
      </section>
    </div>
  );
}

export function Drawer(props: ModalSurfaceProps): JSX.Element | null {
  const titleId = useId();
  const descriptionId = useId();
  const drawerRef = useRef<HTMLElement>(null);
  useModalBehavior(props.open, props.onClose, drawerRef);

  if (!props.open) {
    return null;
  }

  return (
    <div
      className="page-modal-backdrop page-drawer-backdrop"
      role="presentation"
    >
      <aside
        aria-describedby={props.description ? descriptionId : undefined}
        aria-label={props.title}
        aria-labelledby={titleId}
        aria-modal="true"
        className="page-drawer"
        data-page-drawer="true"
        ref={drawerRef}
        role="dialog"
        tabIndex={-1}
      >
        <header className="page-modal-header">
          <div>
            <h2 id={titleId}>{props.title}</h2>
            {props.description ? (
              <p id={descriptionId}>{props.description}</p>
            ) : null}
          </div>
          <button
            aria-busy={props.closeDisabled || undefined}
            aria-label={props.closeLabel ?? "Close drawer"}
            className="page-icon-button"
            disabled={props.closeDisabled}
            onClick={props.onClose}
            type="button"
          >
            <X
              aria-hidden="true"
              focusable="false"
              size={18}
              strokeWidth={1.75}
            />
          </button>
        </header>
        <div className="page-modal-body">{props.children}</div>
        {props.footer ? (
          <footer className="page-modal-footer">{props.footer}</footer>
        ) : null}
      </aside>
    </div>
  );
}
