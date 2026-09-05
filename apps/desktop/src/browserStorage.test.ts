// @vitest-environment jsdom

import { afterEach, describe, expect, it, vi } from "vitest";

import { getBrowserStorage } from "./browserStorage";

const originalLocalStorage = Object.getOwnPropertyDescriptor(
  window,
  "localStorage",
);

afterEach(() => {
  if (originalLocalStorage) {
    Object.defineProperty(window, "localStorage", originalLocalStorage);
  }
  vi.restoreAllMocks();
});

describe("getBrowserStorage", () => {
  it("falls back without blanking the app when WebKit localStorage throws", () => {
    const consoleError = vi
      .spyOn(console, "error")
      .mockImplementation(() => undefined);
    Object.defineProperty(window, "localStorage", {
      configurable: true,
      get() {
        throw new DOMException("storage unavailable", "SecurityError");
      },
    });

    const storage = getBrowserStorage();
    storage.setItem("locale", "zh-CN");

    expect(storage.getItem("locale")).toBe("zh-CN");
    expect(consoleError).toHaveBeenCalledOnce();
  });
});
