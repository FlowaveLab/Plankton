// @vitest-environment jsdom

import { act, useState, type JSX } from "react";
import ReactDOM from "react-dom/client";
import { afterEach, describe, expect, it } from "vitest";

import {
  defaultExposurePolicy,
  exposureLevelForDistance,
  exposurePolicyNeedsNetworkAllowlist,
  ExposurePolicyEditor,
  FieldExposurePolicyEditor,
  ExposureRadar,
  exposureRadarChartModel,
  EXPOSURE_SURFACES,
  exposureSurfaceForPointer,
  type CredentialExposurePolicy,
} from "./ExposurePolicy";

Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });

afterEach(() => {
  document.body.innerHTML = "";
});

describe("ExposureRadar", () => {
  it("builds a standard Chart.js radar with aligned labels and dataset fill", () => {
    const before = defaultExposurePolicy();
    const after = {
      ...before,
      access_mode: "direct" as const,
      surfaces: before.surfaces.map((entry) => ({
        ...entry,
        max_level: entry.surface === "network" ? 2 : 1,
      })),
    };
    const props = {
      attentionLabel: "新增暴露范围",
      breachedSurfaces: EXPOSURE_SURFACES.slice(0, 4),
      locale: "zh-CN",
      primary: after,
      primaryLabel: "修改后",
      secondary: before,
      secondaryLabel: "修改前",
    };
    const model = exposureRadarChartModel(props);

    expect(model.data.labels).toEqual([
      "LLM 回显",
      "网络发送",
      "本地持久化",
      "终端 / 日志",
      "进程传递",
    ]);
    expect(model.data.datasets).toHaveLength(3);
    expect(model.data.datasets[0]?.label).toBe("修改前");
    expect(model.data.datasets[0]?.data).toEqual([0, 0, 0, 0, 1]);
    expect(model.data.datasets[1]?.label).toBe("新增暴露范围");
    expect(model.data.datasets[1]?.fill).toBe(0);
    expect(model.data.datasets[1]?.data).toEqual([1, 2, 1, 1, 1]);
    expect(model.data.datasets[2]?.label).toBe("修改后");
    expect(model.options.scales?.r?.min).toBe(-1);
    expect(model.options.scales?.r?.max).toBe(2);

    const container = document.createElement("div");
    document.body.appendChild(container);
    const root = ReactDOM.createRoot(container);
    act(() => root.render(<ExposureRadar {...props} />));

    expect(
      container.querySelector('canvas[data-chart-library="chart.js"]'),
    ).not.toBeNull();
    expect(container.querySelector("svg")).toBeNull();
    expect(container.textContent).toContain("LLM 回显: 0 → 1");
    expect(container.textContent).toContain("网络发送: 0 → 2");
    expect(container.textContent).toContain("进程传递: 1 → 1");
    expect(container.textContent).toContain("新增暴露范围");

    act(() => root.unmount());
  });

  it("represents a 1 to 2 process breach as library-managed fill between datasets", () => {
    const allowed = defaultExposurePolicy();
    const actual = {
      ...allowed,
      surfaces: allowed.surfaces.map((entry) => ({
        ...entry,
        max_level: entry.surface === "process_propagation" ? 2 : 0,
      })),
    };
    const model = exposureRadarChartModel({
      breachedSurfaces: ["process_propagation"],
      locale: "zh-CN",
      primary: actual,
      secondary: allowed,
    });
    const allowedDataset = model.data.datasets[0];
    const breachDataset = model.data.datasets[1];

    expect(allowedDataset?.data).toEqual([0, 0, 0, 0, 1]);
    expect(breachDataset?.data).toEqual([0, 0, 0, 0, 2]);
    expect(breachDataset?.fill).toBe(0);
    expect(breachDataset?.backgroundColor).toBe("rgba(239, 59, 39, 0.58)");
    expect(model.options.plugins?.filler?.propagate).toBe(false);
  });
});

describe("ExposurePolicyEditor", () => {
  it("maps pointer direction and distance onto the five legal policy axes", () => {
    expect(exposureSurfaceForPointer(0, -1)).toBe("llm_context");
    expect(exposureSurfaceForPointer(1, -0.3)).toBe("network");
    expect(exposureSurfaceForPointer(0.6, 0.8)).toBe("local_persistence");
    expect(exposureSurfaceForPointer(-0.6, 0.8)).toBe("terminal_log");
    expect(exposureSurfaceForPointer(-1, -0.3)).toBe("process_propagation");

    expect(exposureLevelForDistance("network", 30, 90)).toBe(0);
    expect(exposureLevelForDistance("network", 60, 90)).toBe(1);
    expect(exposureLevelForDistance("network", 90, 90)).toBe(2);
    expect(exposureLevelForDistance("terminal_log", 90, 90)).toBe(1);
    expect(exposureLevelForDistance("network", 10, 0)).toBe(0);

    const incompleteNetwork = defaultExposurePolicy();
    incompleteNetwork.surfaces.find(
      (surface) => surface.surface === "network",
    )!.max_level = 1;
    expect(exposurePolicyNeedsNetworkAllowlist(incompleteNetwork)).toBe(true);
    incompleteNetwork.surfaces.find(
      (surface) => surface.surface === "network",
    )!.network_allowlist = [
      { kind: "exact_domain", domain: "api.example.com" },
    ];
    expect(exposurePolicyNeedsNetworkAllowlist(incompleteNetwork)).toBe(false);
  });

  it.each([false, true])(
    "selects a radar axis and validates its allowlist (compact=%s)",
    (compact) => {
      function Harness(): JSX.Element {
        const [policy, setPolicy] = useState<CredentialExposurePolicy>(
          defaultExposurePolicy(),
        );
        return (
          <ExposurePolicyEditor
            compact={compact}
            locale="zh-CN"
            onChange={setPolicy}
            value={policy}
          />
        );
      }

      const container = document.createElement("div");
      document.body.appendChild(container);
      const root = ReactDOM.createRoot(container);
      act(() => root.render(<Harness />));

      expect(
        container.querySelector('[aria-label="可拖动的凭据暴露面雷达图"]'),
      ).not.toBeNull();
      expect(container.querySelector('[role="tablist"]')).toBeNull();
      const radar = container.querySelector<HTMLCanvasElement>("canvas");
      expect(radar?.tabIndex).toBe(0);
      act(() =>
        radar?.dispatchEvent(
          new KeyboardEvent("keydown", { key: "ArrowRight", bubbles: true }),
        ),
      );
      expect(
        container.querySelector('[aria-label="网络发送设置"]'),
      ).not.toBeNull();
      expect(container.textContent).toContain("发送到远端服务");

      const controlled = Array.from(
        container.querySelectorAll<HTMLInputElement>('input[type="radio"]'),
      ).find((input) =>
        input.parentElement?.textContent?.includes("1 · 受控使用"),
      );
      act(() => controlled?.click());
      expect(container.textContent).toContain("等级 1 必须至少声明一个目标");
      expect(
        container.querySelector('textarea[aria-invalid="true"]'),
      ).not.toBeNull();

      act(() => root.unmount());
    },
  );
});

describe("FieldExposurePolicyEditor inheritance", () => {
  it("copies the current default deeply and can return to inheritance", async () => {
    const container = document.createElement("div");
    document.body.appendChild(container);
    const root = ReactDOM.createRoot(container);
    let defaultPolicy = defaultExposurePolicy();
    let custom: CredentialExposurePolicy | null = null;
    const render = () =>
      root.render(
        <FieldExposurePolicyEditor
          defaultPolicy={defaultPolicy}
          customPolicy={custom}
          onChange={(value) => {
            custom = value;
          }}
          locale="en"
        />,
      );
    await act(async () => render());
    expect(container.querySelector("canvas")).toBeNull();
    await act(async () =>
      container
        .querySelector<HTMLInputElement>('input[value="custom"]')
        ?.click(),
    );
    const copy = custom as CredentialExposurePolicy | null;
    expect(copy).toEqual(defaultPolicy);
    expect(copy).not.toBe(defaultPolicy);
    expect(copy?.surfaces[1].network_allowlist).not.toBe(
      defaultPolicy.surfaces[1].network_allowlist,
    );
    defaultPolicy = { ...defaultPolicy, access_mode: "direct" };
    await act(async () => render());
    expect((custom as CredentialExposurePolicy | null)?.access_mode).toBe(
      "protected",
    );
    expect(container.querySelector("canvas")).not.toBeNull();
    await act(async () =>
      container
        .querySelector<HTMLInputElement>('input[value="inherit"]')
        ?.click(),
    );
    expect(custom).toBeNull();
    await act(async () => render());
    expect(container.querySelector("canvas")).toBeNull();
    await act(async () =>
      container
        .querySelector<HTMLInputElement>('input[value="custom"]')
        ?.click(),
    );
    expect((custom as CredentialExposurePolicy | null)?.access_mode).toBe(
      "direct",
    );
    act(() => root.unmount());
  });
});
