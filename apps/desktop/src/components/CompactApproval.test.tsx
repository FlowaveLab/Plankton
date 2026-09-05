// @vitest-environment jsdom

import { act, type ReactNode } from "react";
import ReactDOM from "react-dom/client";
import { afterEach, describe, expect, it, vi } from "vitest";

import {
  CompactApproval,
  type CompactApprovalApi,
  type CompactApprovalRequest,
} from "./CompactApproval";

type RenderHarness = {
  container: HTMLDivElement;
  unmount: () => void;
};

Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });

const firstRequest: CompactApprovalRequest = {
  id: "request-1",
  resource: "plankton://field/deploy/token",
  requested_by: "release-agent",
  reason: "Deploy the production service",
  context: "/workspace/scripts/deploy.sh",
  call_chain: [
    { process_name: "launchd", executable_path: "/sbin/launchd" },
    {
      process_name: "codex",
      executable_path: "/opt/homebrew/bin/codex",
      argv: ["codex", "exec"],
    },
    {
      process_name: "zsh",
      executable_path: "/bin/zsh",
      argv: [
        "zsh",
        "-lc",
        "python3 <<'PY'\nfor item in range(2):\n    print(item)\nPY",
      ],
    },
  ],
  suggestion: "Approve after confirming the production target.",
  suggested_decision: "allow",
  risk_score: 28,
  approval_status: "pending",
  evaluation_state: "completed",
};

const secondRequest: CompactApprovalRequest = {
  id: "request-2",
  resource: "plankton://field/backup/key",
  requested_by: "backup-agent",
  reason: "Create the encrypted backup",
  context: "No script or call-chain context provided.",
  call_chain: [],
  suggestion: "Review the destination before approving.",
  suggested_decision: "escalate",
  risk_score: 61,
  approval_status: "pending",
  evaluation_state: "completed",
};

function createApi(
  initialRequests: CompactApprovalRequest[],
): CompactApprovalApi & {
  requests: CompactApprovalRequest[];
} {
  const api: CompactApprovalApi & {
    requests: CompactApprovalRequest[];
  } = {
    requests: [...initialRequests],
    close: vi.fn(async () => {}),
    decide: vi.fn(async (requestId) => {
      api.requests = api.requests.filter((request) => request.id !== requestId);
    }),
    loadRequests: vi.fn(async () => [...api.requests]),
    openFullDetails: vi.fn(async () => {}),
    subscribe: vi.fn(async () => () => {}),
  };
  return api;
}

function render(node: ReactNode): RenderHarness {
  const container = document.createElement("div");
  document.body.appendChild(container);
  const root = ReactDOM.createRoot(container);
  act(() => root.render(node));
  return {
    container,
    unmount() {
      act(() => root.unmount());
      container.remove();
    },
  };
}

async function flush(): Promise<void> {
  await act(async () => {
    await Promise.resolve();
    await Promise.resolve();
  });
}

function button(container: HTMLElement, name: string): HTMLButtonElement {
  const found = Array.from(container.querySelectorAll("button")).find(
    (candidate) => candidate.textContent?.trim() === name,
  );
  if (!(found instanceof HTMLButtonElement)) {
    throw new Error(`Missing button: ${name}`);
  }
  return found;
}

afterEach(() => {
  document.body.innerHTML = "";
});

describe("CompactApproval", () => {
  it("uses the request locale and readable identity, rendering Markdown emphasis and inline code", async () => {
    const api = createApi([
      {
        ...firstRequest,
        locale: "zh-CN",
        resource_metadata: {
          vault: "开发集合",
          item_title: "部署服务",
          field_label: "API 令牌",
        },
        suggestion: "仅可用于 **本地验证**，不要输出 `API_TOKEN`。",
      },
    ]);
    const view = render(<CompactApproval api={api} />);
    await flush();
    expect(view.container.querySelector("h1")?.textContent).toBe(
      "需要你作出决定",
    );
    expect(
      view.container.querySelector(".compact-approval__request h2")
        ?.textContent,
    ).toBe("部署服务");
    expect(
      view.container.querySelector(".approval-markdown strong")?.textContent,
    ).toBe("本地验证");
    expect(
      view.container.querySelector(".approval-markdown code")?.textContent,
    ).toBe("API_TOKEN");
    expect(
      view.container.querySelector(".compact-approval__identifiers")
        ?.textContent,
    ).toContain(firstRequest.id);
    expect(button(view.container, "批准").disabled).toBe(false);
    view.unmount();
  });
  it("marks executable files alongside call-chain evidence", async () => {
    const view = render(
      <CompactApproval
        api={createApi([
          {
            ...firstRequest,
            locale: "zh-CN",
            exposure_report: {
              chain_summary: "local",
              node_assessments: [],
              surfaces: [
                {
                  surface: "process_propagation",
                  actual_level: 1,
                  evidence_state: "observed",
                  summary: "local",
                  annotations: [
                    {
                      target: {
                        kind: "source_file",
                        node_index: 2,
                        source_id: "file:/workspace/check.py",
                      },
                      reason: "实际执行的 **本地脚本**",
                    },
                    {
                      target: {
                        kind: "source_quote",
                        node_index: 2,
                        source_id: "file:/workspace/check.py",
                        start_line: 8,
                        end_line: 8,
                        quote: "stdout=DEVNULL",
                        occurrence: 0,
                      },
                      reason: "输出处理位置",
                    },
                  ],
                },
              ],
            },
          },
        ])}
      />,
    );
    await flush();
    const panel = view.container.querySelector(
      ".request-evidence-workbench__file",
    );
    expect(panel?.textContent).toContain("/workspace/check.py");
    expect(panel?.querySelector("mark")).not.toBeNull();
    expect(
      view.container.querySelector(".compact-approval__call-chain")
        ?.textContent,
    ).toContain("stdout=DEVNULL");
    expect(
      view.container.querySelector(
        ".compact-approval__call-chain .approval-markdown strong",
      )?.textContent,
    ).toBe("本地脚本");
    view.unmount();
  });
  it("sorts newest requests first without switching the user's selected request on refresh", async () => {
    const api = createApi([
      { ...firstRequest, created_at: "2026-09-06T01:00:00Z" },
      { ...secondRequest, created_at: "2026-09-06T02:00:00Z" },
    ]);
    const view = render(<CompactApproval api={api} />);
    await flush();
    expect(
      view.container
        .querySelector(".compact-approval__switcher button")
        ?.getAttribute("aria-pressed"),
    ).toBe("true");
    expect(
      view.container.querySelector(".compact-approval__request")?.textContent,
    ).toContain(secondRequest.reason);
    view.unmount();
  });
  it("shows item identities in the queue and allows keyboard resizing", async () => {
    const view = render(
      <CompactApproval
        api={createApi([
          {
            ...firstRequest,
            resource_metadata: {
              item_title: "发布配置",
              field_label: "API token",
              vault: "DEV",
            },
          },
          {
            ...secondRequest,
            resource_metadata: {
              item_title: "数据库配置",
              field_label: "Password",
              vault: "DEV",
            },
          },
        ])}
      />,
    );
    await flush();
    const queue = view.container.querySelector(".compact-approval__switcher");
    expect(queue?.textContent).toContain("发布配置");
    expect(queue?.textContent).toContain("数据库配置");
    expect(queue?.textContent).not.toContain("plankton://");
    const divider = view.container.querySelector('[role="separator"]')!;
    const before = Number(divider.getAttribute("aria-valuenow"));
    act(() =>
      divider.dispatchEvent(
        new KeyboardEvent("keydown", { key: "ArrowRight", bubbles: true }),
      ),
    );
    expect(Number(divider.getAttribute("aria-valuenow"))).toBeGreaterThan(
      before,
    );
    view.unmount();
  });
  it("shows only value-free request context, suggestion, risk, and queue count", async () => {
    const api = createApi([
      {
        ...firstRequest,
        value: "TOP-SECRET-MUST-NOT-RENDER",
      } as CompactApprovalRequest & { value: string },
      secondRequest,
    ]);
    const view = render(<CompactApproval api={api} />);
    await flush();

    expect(view.container.textContent).toContain(firstRequest.resource);
    expect(view.container.textContent).toContain(firstRequest.requested_by);
    expect(view.container.textContent).toContain(firstRequest.reason);
    expect(view.container.textContent).toContain(firstRequest.context);
    expect(view.container.textContent).toContain(firstRequest.suggestion);
    expect(view.container.textContent).toContain("Risk 28 / 100");
    expect(view.container.textContent).toContain("1 of 2");
    expect(
      view.container.querySelectorAll(
        '.compact-approval__switcher button[aria-pressed="true"]',
      ),
    ).toHaveLength(1);
    expect(
      view.container.querySelectorAll(
        ".compact-approval__call-chain .agent-scoped",
      ),
    ).toHaveLength(2);
    expect(
      view.container.querySelector(
        '.compact-approval__call-chain code[data-language="python heredoc"]',
      ),
    ).not.toBeNull();
    expect(view.container.textContent).not.toContain(
      "TOP-SECRET-MUST-NOT-RENDER",
    );

    view.unmount();
  });

  it("switches among aggregated approvals from the left rail", async () => {
    const api = createApi([firstRequest, secondRequest]);
    const view = render(<CompactApproval api={api} />);
    await flush();

    const second = view.container.querySelector<HTMLButtonElement>(
      '[aria-label^="Open approval 2:"]',
    );
    act(() => second?.click());

    expect(view.container.textContent).toContain(secondRequest.resource);
    expect(view.container.textContent).toContain("2 of 2");
    expect(second?.getAttribute("aria-pressed")).toBe("true");
    expect(view.container.textContent).toContain(
      "No call-chain context was captured.",
    );
    view.unmount();
  });

  it("does not render a queue switcher for one approval", async () => {
    const api = createApi([firstRequest]);
    const view = render(<CompactApproval api={api} />);
    await flush();

    expect(
      view.container.querySelector(".compact-approval__switcher"),
    ).toBeNull();
    view.unmount();
  });

  it("shows automatic guidance and detail-generation progress in the compact window", async () => {
    const api = createApi([
      {
        ...firstRequest,
        evaluation_state: "completed",
        review_progress: {
          state: "running",
          completed_units: 3,
          total_units: 8,
          updated_at: "2026-08-16T08:00:00Z",
        },
      },
    ]);
    const view = render(<CompactApproval api={api} />);
    await flush();

    expect(view.container.textContent).not.toContain(
      "Guidance ready · mapping evidence",
    );
    expect(
      view.container
        .querySelector(".compact-approval__evaluation")
        ?.getAttribute("data-has-message"),
    ).toBe("false");
    expect(view.container.querySelector('[role="progressbar"]')).not.toBeNull();
    view.unmount();
  });

  it("shows an active repair state without stopping compact progress", async () => {
    const api = createApi([
      {
        ...firstRequest,
        evaluation_state: "completed",
        review_progress: {
          state: "running",
          completed_units: 3,
          total_units: 8,
          error: "Automatic repair 1/2: invalid enrichment frame",
          updated_at: "2026-08-16T08:00:00Z",
        },
      },
    ]);
    const view = render(<CompactApproval api={api} />);
    await flush();

    expect(view.container.textContent).toContain(
      "Guidance ready · repairing evidence",
    );
    expect(view.container.textContent).toContain(
      "Automatic repair 1/2: invalid enrichment frame",
    );
    expect(
      view.container
        .querySelector(".compact-approval__evaluation")
        ?.getAttribute("data-state"),
    ).toBe("running");
    expect(view.container.querySelector('[role="progressbar"]')).not.toBeNull();
    view.unmount();
  });

  it("distinguishes partial evidence details from active generation", async () => {
    const api = createApi([
      {
        ...firstRequest,
        evaluation_state: "completed",
        review_progress: {
          state: "partial",
          completed_units: 4,
          total_units: 8,
          error: "the final detail frame was incomplete",
          updated_at: "2026-08-16T08:00:00Z",
        },
      },
    ]);
    const view = render(<CompactApproval api={api} />);
    await flush();

    expect(view.container.textContent).toContain(
      "Evidence details partially complete",
    );
    expect(
      view.container.querySelector(".compact-approval__evaluation"),
    ).not.toBeNull();
    expect(
      view.container
        .querySelector(".compact-approval__evaluation")
        ?.getAttribute("data-state"),
    ).toBe("partial");
    view.unmount();
  });

  it.each(["queued", "running"] as const)(
    "allows human decisions while AI is %s",
    async (state) => {
      for (const label of ["Approve", "Reject"]) {
        const api = createApi([{ ...firstRequest, evaluation_state: state }]);
        const view = render(<CompactApproval api={api} />);
        await flush();
        expect(
          view.container.querySelector('[role="progressbar"]'),
        ).not.toBeNull();
        expect(button(view.container, "Approve").disabled).toBe(false);
        expect(button(view.container, "Reject").disabled).toBe(false);
        await act(async () => button(view.container, label).click());
        expect(api.decide).toHaveBeenCalledWith(
          firstRequest.id,
          label === "Approve" ? "approve_request" : "reject_request",
          null,
        );
        expect(api.close).toHaveBeenCalled();
        view.unmount();
      }
    },
  );

  it("hides a decided request while evidence generation continues in the background", async () => {
    const running = {
      ...firstRequest,
      review_progress: {
        state: "running" as const,
        completed_units: 2,
        total_units: 8,
        updated_at: "2026-08-16T08:00:00Z",
      },
    };
    const api = createApi([running]);
    api.decide = vi.fn(async () => {
      api.requests = [{ ...running, approval_status: "approved" }];
    });
    const view = render(<CompactApproval api={api} />);
    await flush();

    await act(async () => button(view.container, "Approve").click());
    await flush();

    expect(view.container.textContent).not.toContain(firstRequest.resource);
    expect(view.container.textContent).not.toContain("Approval recorded.");
    expect(view.container.textContent).not.toContain(
      "Evidence generation is still running",
    );
    expect(api.close).toHaveBeenCalledTimes(1);
    view.unmount();
  });

  it("shows the same complete evidence references as the detailed review", async () => {
    const api = createApi([
      {
        ...firstRequest,
        inline_sources: [
          {
            source_id: "inline:python",
            node_index: 2,
            argument_index: 2,
            language: "python",
            lines: [
              { line: 1, text: "from urllib.request import urlopen" },
              {
                line: 2,
                text: 'response = urlopen("https://api.example.test/v1")',
              },
            ],
          },
        ],
        exposure_report: {
          chain_summary:
            "The command hands control to a network-capable script.",
          node_assessments: [
            {
              node_index: 2,
              summary: "The shell launches the inline Python program.",
              capabilities: ["subprocess launch", "network request"],
            },
          ],
          surfaces: [
            {
              surface: "process_propagation",
              actual_level: 1,
              evidence_state: "observed",
              summary: "Python receives the command argument.",
              annotations: [
                {
                  reason: "This exact argument starts the Python program.",
                  target: {
                    kind: "argument_quote",
                    node_index: 2,
                    argument_index: 2,
                    quote: "python3",
                    occurrence: 0,
                  },
                },
              ],
            },
            {
              surface: "network",
              actual_level: 1,
              evidence_state: "observed",
              summary: "The inline source performs a network request.",
              annotations: [
                {
                  reason: "This exact source expression opens the URL.",
                  target: {
                    kind: "source_quote",
                    node_index: 2,
                    source_id: "inline:python",
                    start_line: 2,
                    end_line: 2,
                    quote: "urlopen",
                    occurrence: 0,
                  },
                },
              ],
            },
          ],
        },
      },
    ]);
    const view = render(<CompactApproval api={api} />);
    await flush();

    expect(view.container.textContent).toContain("Automatic guidance");
    expect(view.container.textContent).toContain(
      "The command hands control to a network-capable script.",
    );
    expect(view.container.textContent).toContain(
      "The shell launches the inline Python program.",
    );
    expect(view.container.textContent).toContain(
      "subprocess launch · network request",
    );
    expect(
      view.container.querySelector(
        '.compact-approval__reference-scope [aria-label="Precise node evidence"]',
      ),
    ).not.toBeNull();
    expect(
      view.container.querySelector(
        '.request-evidence-workbench__command mark[data-reference-ids~="1"]',
      )?.textContent,
    ).toContain("python3");
    expect(
      view.container.querySelector(
        '.request-evidence-workbench__origin mark[data-reference-ids~="2"]',
      )?.textContent,
    ).toContain("urlopen");
    expect(
      view.container.querySelector('button[aria-label="Locate evidence 1"]'),
    ).not.toBeNull();
    expect(view.container.textContent).toContain(
      "This exact argument starts the Python program.",
    );
    expect(view.container.textContent).toContain(
      "This exact source expression opens the URL.",
    );
    view.unmount();
  });

  it("uses one reachable review scroller while keeping decision actions outside", async () => {
    const api = createApi([firstRequest]);
    const view = render(<CompactApproval api={api} />);
    await flush();

    const reviewScroll = view.container.querySelector(
      ".compact-approval__review-scroll",
    );
    const actions = view.container.querySelector(".compact-approval__actions");
    expect(reviewScroll).not.toBeNull();
    expect(actions).not.toBeNull();
    expect(reviewScroll?.contains(actions)).toBe(false);
    const styles = view.container.querySelector("style")?.textContent ?? "";
    expect(styles).toMatch(
      /\.compact-approval__review-scroll\s*\{[^}]*display: flex;[^}]*flex-direction: column;[^}]*overflow-y: auto;/s,
    );
    expect(styles).toMatch(
      /\.compact-approval__call-chain\s*\{[^}]*min-height: 220px;[^}]*flex: 0 0 auto;/s,
    );
    expect(styles).toMatch(
      /\.compact-approval__call-chain > ol\s*\{[^}]*overflow: visible;/s,
    );
    expect(styles).toMatch(
      /\.compact-approval__evaluation\s*\{[^}]*position: sticky;[^}]*top: 0;/s,
    );
    expect(styles).toMatch(
      /\.compact-approval dl div:nth-child\(n \+ 2\) dd\s*\{[^}]*overscroll-behavior: auto;/s,
    );

    view.unmount();
  });

  it("submits an edited note, approves, and advances to the next request", async () => {
    const api = createApi([firstRequest, secondRequest]);
    const view = render(<CompactApproval api={api} />);
    await flush();

    const note = view.container.querySelector(
      'textarea[aria-label="Decision note"]',
    );
    if (!(note instanceof HTMLTextAreaElement)) {
      throw new Error("Missing decision note");
    }
    act(() => {
      const valueSetter = Object.getOwnPropertyDescriptor(
        HTMLTextAreaElement.prototype,
        "value",
      )?.set;
      valueSetter?.call(note, "Confirmed production target");
      note.dispatchEvent(new Event("input", { bubbles: true }));
    });
    await act(async () => {
      button(view.container, "Approve").click();
    });
    await flush();

    expect(api.decide).toHaveBeenCalledWith(
      "request-1",
      "approve_request",
      "Confirmed production target",
    );
    expect(view.container.textContent).toContain(secondRequest.resource);
    expect(view.container.textContent).toContain("1 of 1");
    expect(note.value).toBe("");
    expect(api.close).not.toHaveBeenCalled();

    view.unmount();
  });

  it("rejects the final request and closes the compact window", async () => {
    const api = createApi([firstRequest]);
    const view = render(<CompactApproval api={api} />);
    await flush();

    await act(async () => {
      button(view.container, "Reject").click();
    });
    await flush();

    expect(api.decide).toHaveBeenCalledWith(
      "request-1",
      "reject_request",
      null,
    );
    expect(api.close).toHaveBeenCalledOnce();

    view.unmount();
  });

  it("hides the resolved window even when the post-decision refresh fails", async () => {
    const api = createApi([firstRequest]);
    let loadCount = 0;
    api.loadRequests = vi.fn(async () => {
      loadCount += 1;
      if (loadCount === 1) {
        return [firstRequest];
      }
      throw new Error("queue refresh failed");
    });
    const view = render(<CompactApproval api={api} />);
    await flush();

    await act(async () => {
      button(view.container, "Approve").click();
    });
    await flush();

    expect(api.close).toHaveBeenCalledOnce();
    expect(
      Array.from(view.container.querySelectorAll("button")).some(
        (candidate) => candidate.textContent?.trim() === "Approve",
      ),
    ).toBe(false);
    view.unmount();
  });

  it("opens full details without leaving the reused compact window disabled", async () => {
    const api = createApi([firstRequest]);
    const view = render(<CompactApproval api={api} />);
    await flush();

    await act(async () => {
      button(view.container, "Open full details").click();
    });

    expect(api.openFullDetails).toHaveBeenCalledWith("request-1");
    expect(button(view.container, "Approve").disabled).toBe(false);
    expect(button(view.container, "Reject").disabled).toBe(false);
    view.unmount();
  });

  it("hides without deciding from the close button or Escape", async () => {
    const api = createApi([firstRequest]);
    const view = render(<CompactApproval api={api} />);
    await flush();

    await act(async () => {
      button(view.container, "Close").click();
    });
    act(() => {
      window.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape" }));
    });

    expect(api.close).toHaveBeenCalledTimes(2);
    expect(api.decide).not.toHaveBeenCalled();
    view.unmount();
  });

  it("focuses the review heading and reports loading failures inline", async () => {
    const api = createApi([]);
    api.loadRequests = vi.fn(async () => {
      throw new Error("daemon unavailable");
    });
    const view = render(<CompactApproval api={api} />);
    await flush();

    const heading = view.container.querySelector("h1");
    expect(document.activeElement).toBe(heading);
    expect(
      view.container.querySelector('[role="alert"]')?.textContent,
    ).toContain("daemon unavailable");
    expect(view.container.textContent).toContain("Try again");

    view.unmount();
  });

  it("renders a directed empty state and can close it", async () => {
    const api = createApi([]);
    const view = render(<CompactApproval api={api} />);
    await flush();

    expect(view.container.textContent).toContain("No approval is waiting");
    await act(async () => {
      button(view.container, "Close").click();
    });
    expect(api.close).toHaveBeenCalledOnce();

    view.unmount();
  });
});
