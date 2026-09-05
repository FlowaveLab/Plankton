// @vitest-environment jsdom
import { act } from "react";
import ReactDOM from "react-dom/client";
import { describe, expect, it, vi } from "vitest";
import { SecretInput } from "./SecretInput";
Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });

describe("SecretInput", () => {
  it("keeps a Direct field manually hidden across refreshes and allows showing it again", async () => {
    const read = vi.fn().mockResolvedValue(true);
    const container = document.createElement("div");
    const root = ReactDOM.createRoot(container);
    const render = (resetKey: number) =>
      root.render(
        <SecretInput
          fieldName="direct"
          value="test-only"
          readOnly
          autoReveal
          onReveal={read}
          resetKey={resetKey}
        />,
      );
    await act(async () => render(0));
    expect(container.querySelector("input")?.type).toBe("text");
    expect(read).toHaveBeenCalledTimes(1);
    await act(async () => container.querySelector("button")?.click());
    await act(async () => render(1));
    expect(container.querySelector("input")?.type).toBe("password");
    expect(read).toHaveBeenCalledTimes(1);
    await act(async () => container.querySelector("button")?.click());
    expect(container.querySelector("input")?.type).toBe("text");
    expect(read).toHaveBeenCalledTimes(2);
    act(() => root.unmount());
  });

  it("reveals fields independently and automatically reveals Direct fields", async () => {
    const container = document.createElement("div");
    const root = ReactDOM.createRoot(container);
    const render = (direct: boolean) =>
      root.render(
        <>
          <SecretInput
            fieldName="first"
            value="test-first"
            readOnly
            autoReveal={direct}
          />
          <SecretInput fieldName="second" value="test-second" readOnly />
        </>,
      );
    await act(async () => render(false));
    expect(
      [...container.querySelectorAll("input")].map((input) => input.type),
    ).toEqual(["password", "password"]);
    await act(async () =>
      container
        .querySelector<HTMLButtonElement>('[aria-label="Show second"]')
        ?.click(),
    );
    expect(
      [...container.querySelectorAll("input")].map((input) => input.type),
    ).toEqual(["password", "text"]);
    await act(async () => render(true));
    expect(
      [...container.querySelectorAll("input")].map((input) => input.type),
    ).toEqual(["text", "text"]);
    await act(async () => render(false));
    expect(
      [...container.querySelectorAll("input")].map((input) => input.type),
    ).toEqual(["password", "text"]);
    act(() => root.unmount());
  });

  it("does not reopen a concealed field when an older read finishes", async () => {
    let finish: (loaded: boolean) => void = () => {};
    const read = vi.fn(
      () =>
        new Promise<boolean>((resolve) => {
          finish = resolve;
        }),
    );
    const container = document.createElement("div");
    const root = ReactDOM.createRoot(container);
    const render = (resetKey: number) =>
      root.render(
        <SecretInput
          fieldName="test"
          readOnly
          value="test-only"
          onReveal={read}
          resetKey={resetKey}
        />,
      );
    await act(async () => render(0));
    await act(async () => container.querySelector("button")?.click());
    expect(container.querySelector("button")?.disabled).toBe(true);
    await act(async () => render(1));
    await act(async () => finish(true));
    expect(container.querySelector("input")?.type).toBe("password");
    expect(container.querySelector("button")?.disabled).toBe(false);
    act(() => root.unmount());
  });
});
