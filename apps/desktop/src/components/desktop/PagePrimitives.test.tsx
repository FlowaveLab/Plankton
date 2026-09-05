// @vitest-environment jsdom

import { act, StrictMode, type ReactNode } from "react";
import ReactDOM from "react-dom/client";
import { afterEach, describe, expect, it, vi } from "vitest";

import {
  Dialog,
  Drawer,
  EmptyState,
  ErrorState,
  PageHeader,
  Pagination,
  SplitPane,
} from "./PagePrimitives";

type RenderHarness = {
  container: HTMLDivElement;
  rerender: (node: ReactNode) => void;
  unmount: () => void;
};

Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });

function render(node: ReactNode): RenderHarness {
  const container = document.createElement("div");
  document.body.appendChild(container);
  const root = ReactDOM.createRoot(container);
  act(() => root.render(node));
  return {
    container,
    rerender(nextNode) {
      act(() => root.render(nextNode));
    },
    unmount() {
      act(() => root.unmount());
      container.remove();
    },
  };
}

function buttonWithText(
  container: ParentNode,
  text: string,
): HTMLButtonElement | undefined {
  return Array.from(container.querySelectorAll("button")).find(
    (button) => button.textContent === text,
  );
}

afterEach(() => {
  document.body.innerHTML = "";
  document.body.style.overflow = "";
});

function createWorkspaceScrollOwner(): HTMLDivElement {
  const workspace = document.createElement("main");
  workspace.className = "desktop-workspace";
  const scrollOwner = document.createElement("div");
  scrollOwner.className = "workspace-content";
  workspace.appendChild(scrollOwner);
  document.body.appendChild(workspace);
  return scrollOwner;
}

describe("PagePrimitives", () => {
  it("gives a page one heading, supporting copy, status, and primary action", () => {
    const view = render(
      <PageHeader
        description="Review requests that need a human decision."
        eyebrow="Approval queue"
        primaryAction={<button type="button">Create request</button>}
        status={<span>2 awaiting review</span>}
        title="Requests"
      />,
    );

    expect(view.container.querySelector("h1")?.textContent).toBe("Requests");
    expect(view.container.textContent).toContain(
      "Review requests that need a human decision.",
    );
    expect(view.container.querySelector('[role="status"]')?.textContent).toBe(
      "2 awaiting review",
    );
    expect(buttonWithText(view.container, "Create request")).toBeDefined();

    view.unmount();
  });

  it("renders guided empty and error states with a next action", () => {
    const view = render(
      <>
        <EmptyState
          action={<button type="button">Add connection</button>}
          description="Connect a password backend to begin."
          title="No connections"
        />
        <ErrorState
          action={<button type="button">Retry</button>}
          description="The daemon did not answer."
          eyebrow="需要处理"
          title="Could not load requests"
        />
      </>,
    );

    expect(
      view.container.querySelector('[data-state="empty"]')?.textContent,
    ).toContain("Add connection");
    expect(
      view.container.querySelector('[data-state="empty"] .eyebrow'),
    ).toBeNull();
    const error = view.container.querySelector('[data-state="error"]');
    expect(error?.getAttribute("role")).toBe("alert");
    expect(error?.textContent).toContain("Retry");
    expect(error?.querySelector(".eyebrow")?.textContent).toBe("需要处理");

    view.unmount();
  });

  it("gives every guided state instance its own labelled heading", () => {
    const view = render(
      <>
        <EmptyState
          action={<button type="button">Add first</button>}
          description="First empty state"
          title="First empty"
        />
        <EmptyState
          action={<button type="button">Add second</button>}
          description="Second empty state"
          title="Second empty"
        />
        <ErrorState
          action={<button type="button">Retry first</button>}
          description="First error state"
          title="First error"
        />
        <ErrorState
          action={<button type="button">Retry second</button>}
          description="Second error state"
          title="Second error"
        />
      </>,
    );

    const states = Array.from(
      view.container.querySelectorAll<HTMLElement>(".page-state"),
    );
    const titleIds = states.map((state) => {
      const titleId = state.getAttribute("aria-labelledby");
      expect(titleId).toBeTruthy();
      expect(state.querySelector("h2")?.id).toBe(titleId);
      return titleId;
    });
    expect(new Set(titleIds).size).toBe(states.length);

    view.unmount();
  });

  it("labels both full-height split panes", () => {
    const view = render(
      <SplitPane
        detail={<p>Request details</p>}
        detailLabel="Request detail"
        list={<p>Request list</p>}
        listLabel="Requests"
      />,
    );

    expect(
      view.container.querySelector('[aria-label="Requests"]')?.textContent,
    ).toContain("Request list");
    expect(
      view.container.querySelector('[aria-label="Request detail"]')
        ?.textContent,
    ).toContain("Request details");

    view.unmount();
  });

  it("can hide a single-item list and exposes a keyboard-resizable separator", () => {
    const view = render(
      <SplitPane
        detail={<p>Request details</p>}
        detailLabel="Request detail"
        list={<p>Request list</p>}
        listLabel="Requests"
        listVisible={false}
        resizable
        storageKey="test-list-width"
      />,
    );
    expect(view.container.querySelector('[aria-label="Requests"]')).toBeNull();
    expect(view.container.querySelector('[role="separator"]')).toBeNull();

    view.rerender(
      <SplitPane
        detail={<p>Request details</p>}
        detailLabel="Request detail"
        list={<p>Request list</p>}
        listLabel="Requests"
        resizable
        storageKey="test-list-width"
      />,
    );
    const separator =
      view.container.querySelector<HTMLElement>('[role="separator"]');
    expect(separator?.getAttribute("aria-orientation")).toBe("vertical");
    const before = Number(separator?.getAttribute("aria-valuenow"));
    act(() =>
      separator?.dispatchEvent(
        new KeyboardEvent("keydown", {
          bubbles: true,
          key: "ArrowRight",
        }),
      ),
    );
    expect(
      Number(
        view.container
          .querySelector('[role="separator"]')
          ?.getAttribute("aria-valuenow"),
      ),
    ).toBe(before + 24);
    view.unmount();
  });

  it("renders pagination only for multiple pages and exposes named actions", () => {
    const onPageChange = vi.fn();
    const view = render(
      <Pagination onPageChange={onPageChange} page={1} pageCount={1} />,
    );
    expect(view.container.querySelector("nav")).toBeNull();

    view.rerender(
      <Pagination
        label="Request pages"
        onPageChange={onPageChange}
        page={2}
        pageCount={3}
      />,
    );
    const pagination = view.container.querySelector(
      'nav[aria-label="Request pages"]',
    );
    expect(pagination).not.toBeNull();
    const previous = pagination?.querySelector(
      'button[aria-label="Previous page"]',
    );
    const next = pagination?.querySelector('button[aria-label="Next page"]');
    expect(previous?.querySelector('svg[aria-hidden="true"]')).not.toBeNull();
    expect(next?.querySelector('svg[aria-hidden="true"]')).not.toBeNull();
    expect(previous?.querySelector("svg")?.getAttribute("width")).toBe("16");
    expect(previous?.querySelector("svg")?.getAttribute("height")).toBe("16");
    expect(previous?.querySelector("svg")?.getAttribute("stroke-width")).toBe(
      "1.75",
    );

    act(() => (previous as HTMLButtonElement | null)?.click());
    act(() => (next as HTMLButtonElement | null)?.click());
    expect(onPageChange).toHaveBeenNthCalledWith(1, 1);
    expect(onPageChange).toHaveBeenNthCalledWith(2, 3);

    view.unmount();
  });

  it("traps dialog focus, closes on Escape, locks scroll, and restores focus", () => {
    const scrollOwner = createWorkspaceScrollOwner();
    scrollOwner.style.overflow = "auto";
    document.body.style.overflow = "clip";
    const opener = document.createElement("button");
    opener.textContent = "Open dialog";
    document.body.appendChild(opener);
    opener.focus();
    const onClose = vi.fn();
    const view = render(
      <Dialog
        footer={<button type="button">Save</button>}
        onClose={onClose}
        open
        title="Edit connection"
      >
        <button data-dialog-initial-focus type="button">
          Test connection
        </button>
      </Dialog>,
    );

    const dialog = view.container.querySelector('[role="dialog"]');
    expect(dialog?.getAttribute("aria-modal")).toBe("true");
    expect(document.body.style.overflow).toBe("hidden");
    expect(scrollOwner.style.overflow).toBe("hidden");
    expect(document.activeElement?.textContent).toBe("Test connection");
    const closeIcon = dialog?.querySelector(
      'button[aria-label="Close dialog"] svg',
    );
    expect(closeIcon).not.toBeNull();
    expect(closeIcon?.getAttribute("width")).toBe("18");
    expect(closeIcon?.getAttribute("height")).toBe("18");
    expect(closeIcon?.getAttribute("stroke-width")).toBe("1.75");

    const closeButton = dialog?.querySelector(
      'button[aria-label="Close dialog"]',
    ) as HTMLButtonElement | null;
    const saveButton = buttonWithText(dialog ?? view.container, "Save");
    act(() => {
      saveButton?.focus();
      document.dispatchEvent(
        new KeyboardEvent("keydown", { bubbles: true, key: "Tab" }),
      );
    });
    expect(document.activeElement).toBe(closeButton);
    act(() => {
      document.dispatchEvent(
        new KeyboardEvent("keydown", {
          bubbles: true,
          key: "Tab",
          shiftKey: true,
        }),
      );
    });
    expect(document.activeElement).toBe(saveButton);

    act(() => {
      document.dispatchEvent(
        new KeyboardEvent("keydown", { bubbles: true, key: "Escape" }),
      );
    });
    expect(onClose).toHaveBeenCalledTimes(1);

    view.rerender(
      <Dialog onClose={onClose} open={false} title="Edit connection">
        <p>Hidden</p>
      </Dialog>,
    );
    expect(document.body.style.overflow).toBe("clip");
    expect(scrollOwner.style.overflow).toBe("auto");
    expect(document.activeElement).toBe(opener);

    view.unmount();
    opener.remove();
  });

  it("uses the latest inline close callback without resetting focus", () => {
    const calls: string[] = [];
    const view = render(
      <Dialog onClose={() => calls.push("first")} open title="Stable dialog">
        <button data-dialog-initial-focus type="button">
          Initial
        </button>
        <button type="button">Keep focus</button>
      </Dialog>,
    );
    const keepFocus = buttonWithText(view.container, "Keep focus");
    keepFocus?.focus();

    view.rerender(
      <Dialog onClose={() => calls.push("latest")} open title="Stable dialog">
        <button data-dialog-initial-focus type="button">
          Initial updated
        </button>
        <button type="button">Keep focus</button>
      </Dialog>,
    );

    expect(document.activeElement?.textContent).toBe("Keep focus");
    act(() => {
      document.dispatchEvent(
        new KeyboardEvent("keydown", { bubbles: true, key: "Escape" }),
      );
    });
    expect(calls).toEqual(["latest"]);

    view.unmount();
  });

  it("closes only the topmost nested modal and reference-counts scroll locks", () => {
    const scrollOwner = createWorkspaceScrollOwner();
    scrollOwner.style.overflow = "auto";
    document.body.style.overflow = "clip";
    const opener = document.createElement("button");
    opener.textContent = "Open outer dialog";
    document.body.appendChild(opener);
    opener.focus();
    const closeOuter = vi.fn();
    const closeInner = vi.fn();
    const outer = (withDrawer: boolean, open = true): ReactNode => (
      <Dialog onClose={closeOuter} open={open} title="Outer dialog">
        <button data-dialog-initial-focus type="button">
          Outer action
        </button>
        {withDrawer ? (
          <Drawer onClose={closeInner} open title="Inner drawer">
            <button data-dialog-initial-focus type="button">
              Inner action
            </button>
          </Drawer>
        ) : null}
      </Dialog>
    );
    const view = render(outer(false));
    view.rerender(outer(true));

    expect(document.body.style.overflow).toBe("hidden");
    expect(scrollOwner.style.overflow).toBe("hidden");
    act(() => {
      document.dispatchEvent(
        new KeyboardEvent("keydown", { bubbles: true, key: "Escape" }),
      );
    });
    expect(closeInner).toHaveBeenCalledTimes(1);
    expect(closeOuter).not.toHaveBeenCalled();

    view.rerender(outer(false));
    expect(document.body.style.overflow).toBe("hidden");
    expect(scrollOwner.style.overflow).toBe("hidden");
    expect(document.activeElement?.textContent).toBe("Outer action");
    act(() => {
      document.dispatchEvent(
        new KeyboardEvent("keydown", { bubbles: true, key: "Escape" }),
      );
    });
    expect(closeOuter).toHaveBeenCalledTimes(1);

    view.rerender(outer(false, false));
    expect(document.body.style.overflow).toBe("clip");
    expect(scrollOwner.style.overflow).toBe("auto");
    expect(document.activeElement).toBe(opener);

    view.unmount();
    opener.remove();
  });

  it("keeps initial focus in the topmost modal when nested surfaces mount together in StrictMode", () => {
    const scrollOwner = createWorkspaceScrollOwner();
    scrollOwner.style.overflow = "auto";
    document.body.style.overflow = "clip";
    const opener = document.createElement("button");
    opener.textContent = "Open nested modals";
    document.body.appendChild(opener);
    opener.focus();
    const closeOuter = vi.fn();
    const closeInner = vi.fn();
    const view = render(
      <StrictMode>
        <Dialog onClose={closeOuter} open title="Outer dialog">
          <button data-dialog-initial-focus type="button">
            Outer initial action
          </button>
          <Drawer onClose={closeInner} open title="Inner drawer">
            <button data-dialog-initial-focus type="button">
              Inner initial action
            </button>
          </Drawer>
        </Dialog>
      </StrictMode>,
    );

    expect(document.activeElement?.textContent).toBe("Inner initial action");
    act(() => {
      document.dispatchEvent(
        new KeyboardEvent("keydown", { bubbles: true, key: "Escape" }),
      );
    });
    expect(closeInner).toHaveBeenCalledTimes(1);
    expect(closeOuter).not.toHaveBeenCalled();

    view.unmount();
    expect(document.body.style.overflow).toBe("clip");
    expect(scrollOwner.style.overflow).toBe("auto");
    expect(document.activeElement).toBe(opener);
    opener.remove();
  });

  it("traps Drawer focus, handles Escape, locks scroll, and restores focus", () => {
    const scrollOwner = createWorkspaceScrollOwner();
    const opener = document.createElement("button");
    opener.textContent = "Open drawer";
    document.body.appendChild(opener);
    opener.focus();
    const onClose = vi.fn();
    const view = render(
      <Drawer
        closeLabel="关闭筛选抽屉"
        description="筛选不会隐藏任何能力。"
        footer={<button type="button">Apply filters</button>}
        onClose={onClose}
        open
        title="Filters"
      >
        <label>
          Status
          <select data-dialog-initial-focus defaultValue="pending">
            <option value="pending">Pending</option>
          </select>
        </label>
      </Drawer>,
    );

    const drawer = view.container.querySelector(
      '[role="dialog"][data-page-drawer="true"]',
    );
    expect(drawer?.getAttribute("aria-label")).toBe("Filters");
    const descriptionId = drawer?.getAttribute("aria-describedby");
    expect(descriptionId).toBeTruthy();
    expect(document.getElementById(descriptionId ?? "")?.textContent).toBe(
      "筛选不会隐藏任何能力。",
    );
    expect(document.body.style.overflow).toBe("hidden");
    expect(scrollOwner.style.overflow).toBe("hidden");
    expect(document.activeElement?.tagName).toBe("SELECT");
    const close = drawer?.querySelector(
      'button[aria-label="关闭筛选抽屉"]',
    ) as HTMLButtonElement | null;
    const apply = buttonWithText(drawer ?? view.container, "Apply filters");
    act(() => {
      apply?.focus();
      document.dispatchEvent(
        new KeyboardEvent("keydown", { bubbles: true, key: "Tab" }),
      );
    });
    expect(document.activeElement).toBe(close);
    act(() => {
      document.dispatchEvent(
        new KeyboardEvent("keydown", {
          bubbles: true,
          key: "Tab",
          shiftKey: true,
        }),
      );
    });
    expect(document.activeElement).toBe(apply);
    act(() => {
      document.dispatchEvent(
        new KeyboardEvent("keydown", { bubbles: true, key: "Escape" }),
      );
    });
    expect(onClose).toHaveBeenCalledTimes(1);
    expect(drawer?.textContent).toContain("Apply filters");

    view.rerender(
      <Drawer onClose={onClose} open={false} title="Filters">
        <p>Hidden</p>
      </Drawer>,
    );
    expect(document.body.style.overflow).toBe("");
    expect(scrollOwner.style.overflow).toBe("");
    expect(document.activeElement).toBe(opener);

    view.unmount();
    opener.remove();
  });
});
