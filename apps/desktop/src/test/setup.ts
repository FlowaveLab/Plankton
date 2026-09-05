import React from "react";
import { vi } from "vitest";

class TestResizeObserver implements ResizeObserver {
  disconnect(): void {}

  observe(): void {}

  unobserve(): void {}
}

Object.assign(globalThis, { ResizeObserver: TestResizeObserver });

vi.mock("react-chartjs-2", () => ({
  Radar: ({
    "aria-label": ariaLabel,
    role,
    onKeyDown,
    tabIndex,
  }: {
    "aria-label"?: string;
    role?: string;
    onKeyDown?: React.KeyboardEventHandler<HTMLCanvasElement>;
    tabIndex?: number;
  }) =>
    React.createElement("canvas", {
      "aria-label": ariaLabel,
      "data-chart-library": "chart.js",
      role,
      onKeyDown,
      tabIndex,
    }),
}));
