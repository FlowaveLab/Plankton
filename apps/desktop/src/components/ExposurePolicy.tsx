import { ChoiceGroup } from "./ChoiceGroup";
import {
  Chart as ChartJS,
  Filler,
  Legend,
  LineElement,
  PointElement,
  RadialLinearScale,
  Tooltip,
  type Chart,
  type ChartData,
  type ChartOptions,
  type Plugin,
  type RadialLinearScale as RadialScale,
} from "chart.js";
import {
  useId,
  useRef,
  useState,
  type JSX,
  type PointerEvent as ReactPointerEvent,
} from "react";
import { Radar } from "react-chartjs-2";

ChartJS.register(
  Filler,
  Legend,
  LineElement,
  PointElement,
  RadialLinearScale,
  Tooltip,
);

export type ExposureSurface =
  | "llm_context"
  | "network"
  | "local_persistence"
  | "terminal_log"
  | "process_propagation";

export type NetworkDestinationRule =
  | { kind: "exact_domain"; domain: string }
  | { kind: "subdomains_of"; domain: string; include_apex: boolean }
  | { kind: "regex"; pattern: string };

export type ExposureSurfacePolicy = {
  surface: ExposureSurface;
  max_level: number;
  network_allowlist?: NetworkDestinationRule[];
  note?: string | null;
};

export type CredentialExposurePolicy = {
  access_mode: "protected" | "direct";
  breach_action: "human_review" | "deny";
  surfaces: ExposureSurfacePolicy[];
};

export const EXPOSURE_SURFACES: ExposureSurface[] = [
  "llm_context",
  "network",
  "local_persistence",
  "terminal_log",
  "process_propagation",
];

const LABELS: Record<ExposureSurface, { zh: string; en: string }> = {
  llm_context: { zh: "LLM 回显", en: "LLM context" },
  network: { zh: "网络发送", en: "Network" },
  local_persistence: { zh: "本地持久化", en: "Local storage" },
  terminal_log: { zh: "终端 / 日志", en: "Terminal / logs" },
  process_propagation: { zh: "进程传递", en: "Process handoff" },
};

// 24-unit outline icons: model chip, globe, disk, terminal, and process handoff.
const AXIS_ICON_PATHS: Record<ExposureSurface, string> = {
  llm_context:
    "M7 5H17Q19 5 19 7V17Q19 19 17 19H7Q5 19 5 17V7Q5 5 7 5Z M9 9H15V15H9Z M9 2V5 M15 2V5 M9 19V22 M15 19V22 M2 9H5 M2 15H5 M19 9H22 M19 15H22",
  network:
    "M22 12A10 10 0 1 1 2 12A10 10 0 1 1 22 12 M2 12H22 M12 2C6 8 6 16 12 22C18 16 18 8 12 2Z",
  local_persistence:
    "M5 3H16L21 8V20Q21 21 20 21H4Q3 21 3 20V4Q3 3 5 3Z M7 3V9H16V3 M7 21V14H17V21 M13 5V7",
  terminal_log:
    "M4 4H20Q22 4 22 6V18Q22 20 20 20H4Q2 20 2 18V6Q2 4 4 4Z M6 8L10 12L6 16 M13 16H18",
  process_propagation:
    "M2 3H9V10H2Z M15 14H22V21H15Z M9 6H16Q19 6 19 9V14 M16 11L19 14L22 11 M8 21H5V14 M2 17L5 14L8 17",
};

function axisIconPlugin(
  highlighted: () => ReadonlySet<ExposureSurface>,
): Plugin<"radar"> {
  return {
    id: "exposure-axis-icons",
    afterDraw(chart) {
      const scale = chart.scales.r as RadialScale;
      const context = chart.ctx;
      EXPOSURE_SURFACES.forEach((surface, index) => {
        const label = scale.getPointLabelPosition(index);
        context.save();
        context.translate(label.left + 1, label.top + 1);
        context.scale(14 / 24, 14 / 24);
        context.strokeStyle = highlighted().has(surface)
          ? "#f2381e"
          : "#171716";
        context.lineWidth = 1.7;
        context.lineCap = "round";
        context.lineJoin = "round";
        context.stroke(new Path2D(AXIS_ICON_PATHS[surface]));
        context.restore();
      });
    },
  };
}

// Reserve the icon width inside Chart.js label layout, including narrow charts.
const iconLabel = (label: string): string => `\u2003\u2003${label}`;

const SURFACE_HELP: Record<ExposureSurface, { zh: string; en: string }> = {
  llm_context: {
    zh: "凭据是否可以进入模型输入、输出或上下文。",
    en: "Whether the credential may enter model input, output, or context.",
  },
  network: {
    zh: "凭据是否可以发送到远端服务；受控使用必须填写白名单。",
    en: "Whether the credential may be sent to a remote service; controlled use requires an allowlist.",
  },
  local_persistence: {
    zh: "凭据是否可以写入本地文件、缓存或数据库。",
    en: "Whether the credential may be written to local files, caches, or databases.",
  },
  terminal_log: {
    zh: "凭据是否可以出现在终端输出、调试信息或日志中。",
    en: "Whether the credential may appear in terminal output, diagnostics, or logs.",
  },
  process_propagation: {
    zh: "凭据是否可以交给声明的本地消费进程。",
    en: "Whether the credential may be passed to the declared local consumer process.",
  },
};

function maximumPolicyLevel(surface: ExposureSurface): number {
  return surface === "network" ? 2 : 1;
}

export function defaultExposurePolicy(): CredentialExposurePolicy {
  return {
    access_mode: "protected",
    breach_action: "human_review",
    surfaces: [
      { surface: "llm_context", max_level: 0 },
      { surface: "network", max_level: 0, network_allowlist: [] },
      { surface: "local_persistence", max_level: 0 },
      { surface: "terminal_log", max_level: 0 },
      {
        surface: "process_propagation",
        max_level: 1,
        note: "Only pass to the declared local consumer process.",
      },
    ],
  };
}

export function normalizeExposurePolicy(
  value?: CredentialExposurePolicy | null,
): CredentialExposurePolicy {
  const fallback = defaultExposurePolicy();
  if (!value) return fallback;
  return {
    access_mode: value.access_mode ?? "protected",
    breach_action: value.breach_action ?? "human_review",
    surfaces: EXPOSURE_SURFACES.map(
      (surface) =>
        value.surfaces?.find((entry) => entry.surface === surface) ??
        fallback.surfaces.find((entry) => entry.surface === surface)!,
    ),
  };
}

export function exposurePolicyNeedsNetworkAllowlist(
  value?: CredentialExposurePolicy | null,
): boolean {
  const network = normalizeExposurePolicy(value).surfaces.find(
    (entry) => entry.surface === "network",
  );
  return (
    network?.max_level === 1 && (network.network_allowlist?.length ?? 0) === 0
  );
}

type ExposureRadarProps = {
  compact?: boolean;
  primary: CredentialExposurePolicy;
  secondary?: CredentialExposurePolicy | null;
  locale?: string;
  primaryLabel?: string;
  secondaryLabel?: string;
  breachedSurfaces?: ExposureSurface[];
  attentionLabel?: string;
};

export function exposureRadarChartModel(props: ExposureRadarProps): {
  data: ChartData<"radar", number[], string>;
  options: ChartOptions<"radar">;
} {
  const zh = props.locale === "zh-CN";
  const primary = normalizeExposurePolicy(props.primary);
  const secondary = props.secondary
    ? normalizeExposurePolicy(props.secondary)
    : null;
  const breached = new Set(props.breachedSurfaces ?? []);
  const levels = (policy: CredentialExposurePolicy) =>
    EXPOSURE_SURFACES.map(
      (surface) =>
        policy.surfaces.find((entry) => entry.surface === surface)?.max_level ??
        0,
    );
  const primaryLevels = levels(primary);
  const secondaryLevels = secondary ? levels(secondary) : null;
  const valueLabel = (surface: ExposureSurface): string => {
    const index = EXPOSURE_SURFACES.indexOf(surface);
    const current = primaryLevels[index] ?? 0;
    const before = secondaryLevels?.[index];
    return before === undefined ? String(current) : `${before} → ${current}`;
  };
  const labels = EXPOSURE_SURFACES.map((surface) =>
    zh ? LABELS[surface].zh : LABELS[surface].en,
  );
  const allowedLabel =
    props.secondaryLabel ?? (zh ? "修改前" : "Before change");
  const primaryLabel = props.primaryLabel ?? (zh ? "当前" : "Current");
  const datasets: ChartData<"radar", number[], string>["datasets"] = [];
  if (secondaryLevels) {
    datasets.push({
      label: allowedLabel,
      data: secondaryLevels,
      fill: false,
      borderColor: "#706d67",
      borderDash: [5, 5],
      borderWidth: 1.5,
      pointRadius: 0,
      pointHitRadius: 0,
    });
    if (breached.size > 0) {
      datasets.push({
        label:
          props.attentionLabel ?? (zh ? "超出或放宽" : "Exceeded or widened"),
        data: primaryLevels,
        fill: 0,
        backgroundColor: "rgba(239, 59, 39, 0.58)",
        borderColor: "rgba(239, 59, 39, 0)",
        borderWidth: 0,
        pointRadius: 0,
        pointHitRadius: 0,
      });
    }
  }
  datasets.push({
    label: primaryLabel,
    data: primaryLevels,
    fill: false,
    borderColor: "#171716",
    borderWidth: 2.25,
    pointRadius: 0,
    pointHitRadius: 0,
  });

  return {
    data: { labels, datasets },
    options: {
      responsive: true,
      maintainAspectRatio: false,
      animation: false,
      interaction: { intersect: false, mode: "nearest" },
      layout: {
        padding: props.compact
          ? 4
          : { top: 18, right: 38, bottom: 18, left: 38 },
      },
      plugins: {
        legend: { display: false },
        tooltip: {
          callbacks: {
            title: (items) => items[0]?.label ?? "",
            label: (item) => `${item.dataset.label}: ${item.formattedValue}`,
          },
        },
        filler: { propagate: false },
      },
      scales: {
        r: {
          min: -1,
          max: 2,
          beginAtZero: false,
          angleLines: { color: "#cfcac1", lineWidth: 1 },
          grid: { circular: false, color: "#cfcac1", lineWidth: 1 },
          ticks: { display: false, stepSize: 1 },
          pointLabels: {
            padding: props.compact ? 6 : 14,
            color: (context) =>
              breached.has(EXPOSURE_SURFACES[context.index])
                ? "#f2381e"
                : "#171716",
            font: {
              family:
                '-apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif',
              size: props.compact ? 10 : 12,
              weight: 650,
              lineHeight: 1.45,
            },
            callback: (label, index) => [
              iconLabel(String(label)),
              valueLabel(EXPOSURE_SURFACES[index]),
            ],
          },
        },
      },
    },
  };
}

export function ExposureRadar(props: ExposureRadarProps): JSX.Element {
  const zh = props.locale === "zh-CN";
  const highlighted = useRef(new Set(props.breachedSurfaces ?? []));
  highlighted.current = new Set(props.breachedSurfaces ?? []);
  const icons = useRef(axisIconPlugin(() => highlighted.current)).current;
  const chart = exposureRadarChartModel(props);
  const primary = normalizeExposurePolicy(props.primary);
  const secondary = props.secondary
    ? normalizeExposurePolicy(props.secondary)
    : null;
  const valueLabel = (surface: ExposureSurface): string => {
    const current =
      primary.surfaces.find((entry) => entry.surface === surface)?.max_level ??
      0;
    const before = secondary?.surfaces.find(
      (entry) => entry.surface === surface,
    )?.max_level;
    return before === undefined ? String(current) : `${before} → ${current}`;
  };
  return (
    <figure
      className="exposure-radar"
      aria-label={zh ? "凭据暴露面雷达图" : "Credential exposure radar"}
    >
      <div className="exposure-radar__plot">
        <Radar
          aria-label={
            zh
              ? "五个凭据暴露方向及其等级"
              : "Five credential exposure axes and levels"
          }
          data={chart.data}
          plugins={[icons]}
          options={chart.options}
          role="img"
        />
      </div>
      <figcaption>
        <div className="exposure-radar-legend">
          <span className="is-primary">
            {props.primaryLabel ?? (zh ? "当前" : "Current")}
          </span>
          {secondary ? (
            <span className="is-secondary">
              {props.secondaryLabel ?? (zh ? "修改前" : "Before")}
            </span>
          ) : null}
          {(props.breachedSurfaces?.length ?? 0) > 0 ? (
            <span className="is-attention">
              {props.attentionLabel ??
                (zh ? "红色色块：超出或放宽" : "Red area: exceeded or widened")}
            </span>
          ) : null}
        </div>
        <span className="exposure-radar-sr-only">
          {EXPOSURE_SURFACES.map(
            (surface) =>
              `${zh ? LABELS[surface].zh : LABELS[surface].en}: ${valueLabel(surface)}`,
          ).join("; ")}
        </span>
      </figcaption>
    </figure>
  );
}

function surfaceLevelLabel(
  surface: ExposureSurface,
  level: number,
  zh: boolean,
): string {
  if (level === 0) return zh ? "0 · 不允许" : "0 · Blocked";
  if (surface === "network" && level === 2) {
    return zh ? "2 · 白名单外" : "2 · Outside allowlist";
  }
  return zh ? "1 · 受控使用" : "1 · Controlled";
}

function networkRuleLabel(rule: NetworkDestinationRule): string {
  switch (rule.kind) {
    case "exact_domain":
      return `domain:${rule.domain}`;
    case "subdomains_of":
      return `subdomain:${rule.domain}`;
    case "regex":
      return `regex:${rule.pattern}`;
  }
}

export function exposureSurfaceForPointer(
  deltaX: number,
  deltaY: number,
): ExposureSurface {
  const fullTurn = Math.PI * 2;
  const clockwiseFromTop =
    (Math.atan2(deltaY, deltaX) + Math.PI / 2 + fullTurn) % fullTurn;
  const index =
    Math.round(clockwiseFromTop / (fullTurn / EXPOSURE_SURFACES.length)) %
    EXPOSURE_SURFACES.length;
  return EXPOSURE_SURFACES[index];
}

export function exposureLevelForDistance(
  surface: ExposureSurface,
  distance: number,
  drawingArea: number,
): number {
  if (drawingArea <= 0) return 0;
  const chartValue = (Math.max(0, distance) / drawingArea) * 3 - 1;
  return Math.max(
    0,
    Math.min(maximumPolicyLevel(surface), Math.round(chartValue)),
  );
}

export function ExposurePolicySummary(props: {
  value: CredentialExposurePolicy;
  locale?: string;
  onEdit?: () => void;
  compact?: boolean;
  collapsible?: boolean;
}): JSX.Element {
  const zh = props.locale === "zh-CN";
  const value = normalizeExposurePolicy(props.value);
  const network = value.surfaces.find((entry) => entry.surface === "network")!;
  const content = (
    <section className="exposure-policy-summary">
      <header>
        <div>
          <strong>{zh ? "暴露面控制" : "Exposure controls"}</strong>
          <span>
            {value.access_mode === "direct"
              ? zh
                ? "直接可见 · get 无需审批"
                : "Direct · get requires no approval"
              : zh
                ? `受保护 · 超限${value.breach_action === "deny" ? "拒绝" : "转人工"}`
                : `Protected · ${value.breach_action === "deny" ? "deny on breach" : "human review on breach"}`}
          </span>
        </div>
        {props.onEdit ? (
          <button onClick={props.onEdit} type="button">
            {zh ? "编辑暴露面" : "Edit exposure"}
          </button>
        ) : null}
      </header>
      {!props.compact ? (
        <ExposureRadar
          locale={props.locale}
          primary={value}
          primaryLabel={zh ? "当前允许范围" : "Current allowance"}
        />
      ) : null}
      <dl className="exposure-policy-summary-surfaces">
        {value.surfaces.map((entry) => (
          <div key={entry.surface}>
            <dt>
              <strong>
                {zh ? LABELS[entry.surface].zh : LABELS[entry.surface].en}
              </strong>
              <span>
                {surfaceLevelLabel(entry.surface, entry.max_level, zh)}
              </span>
            </dt>
            <dd>{entry.note || (zh ? "未配置备注" : "No note configured")}</dd>
          </div>
        ))}
      </dl>
      <div className="exposure-policy-summary-network">
        <strong>{zh ? "网络白名单" : "Network allowlist"}</strong>
        {(network.network_allowlist?.length ?? 0) > 0 ? (
          <ul>
            {network.network_allowlist?.map((rule, index) => (
              <li key={`${rule.kind}:${index}`}>
                <code>{networkRuleLabel(rule)}</code>
              </li>
            ))}
          </ul>
        ) : (
          <span>{zh ? "未配置" : "Not configured"}</span>
        )}
      </div>
    </section>
  );
  return props.collapsible ? (
    <details className="exposure-policy-disclosure">
      <summary>{zh ? "查看暴露面控制" : "View exposure controls"}</summary>
      {content}
    </details>
  ) : (
    content
  );
}

export function ExposurePolicyEditor(props: {
  value: CredentialExposurePolicy;
  onChange: (value: CredentialExposurePolicy) => void;
  locale?: string;
  compact?: boolean;
}): JSX.Element {
  const editorId = useId();
  const zh = props.locale === "zh-CN";
  const value = normalizeExposurePolicy(props.value);
  const [activeSurface, setActiveSurface] =
    useState<ExposureSurface>("llm_context");
  const activeSurfaceRef = useRef(activeSurface);
  activeSurfaceRef.current = activeSurface;
  const icons = useRef(
    axisIconPlugin(() => new Set([activeSurfaceRef.current])),
  ).current;
  const selectionPlugin = useRef<Plugin<"radar">>({
    id: "selected-exposure-axis",
    beforeDatasetsDraw(chartInstance) {
      const scale = chartInstance.scales.r as RadialScale;
      const index = EXPOSURE_SURFACES.indexOf(activeSurfaceRef.current);
      const end = scale.getPointPositionForValue(index, scale.max);
      const context = chartInstance.ctx;
      context.save();
      context.strokeStyle = "#f2381e";
      context.lineWidth = 1.5;
      context.beginPath();
      context.moveTo(scale.xCenter, scale.yCenter);
      context.lineTo(end.x, end.y);
      context.stroke();
      context.restore();
    },
    afterDraw(chartInstance) {
      const scale = chartInstance.scales.r as RadialScale;
      const index = EXPOSURE_SURFACES.indexOf(activeSurfaceRef.current);
      const label = scale.getPointLabelPosition(index);
      const context = chartInstance.ctx;
      context.save();
      context.fillStyle = "#f2381e";
      context.beginPath();
      context.arc(
        label.right + 7,
        (label.top + label.bottom) / 2,
        2.5,
        0,
        Math.PI * 2,
      );
      context.fill();
      context.restore();
    },
  }).current;
  const chartRef = useRef<Chart<"radar", number[], string> | null>(null);
  const draggingSurfaceRef = useRef<ExposureSurface | null>(null);
  const updateSurface = (
    surface: ExposureSurface,
    patch: Partial<ExposureSurfacePolicy>,
  ) => {
    props.onChange({
      ...value,
      surfaces: value.surfaces.map((entry) =>
        entry.surface === surface ? { ...entry, ...patch } : entry,
      ),
    });
  };
  const network = value.surfaces.find((entry) => entry.surface === "network")!;
  const activeEntry = value.surfaces.find(
    (entry) => entry.surface === activeSurface,
  )!;
  const allowlistText = (network.network_allowlist ?? [])
    .map((rule) =>
      rule.kind === "regex"
        ? `regex:${rule.pattern}`
        : rule.kind === "subdomains_of"
          ? `subdomain:${rule.domain}`
          : `domain:${rule.domain}`,
    )
    .join("\n");
  const chart = exposureRadarChartModel({
    locale: props.locale,
    primary: value,
    primaryLabel: zh ? "允许范围" : "Allowed exposure",
  });
  chart.options.layout = {
    padding: { top: 16, right: 16, bottom: 16, left: 16 },
  };
  const radialScale = chart.options.scales?.r;
  if (radialScale?.pointLabels) {
    radialScale.pointLabels.padding = 10;
    radialScale.pointLabels.font = {
      family: '-apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif',
      size: 11,
      weight: 500,
    };
    radialScale.pointLabels.callback = (label) => iconLabel(String(label));
    radialScale.pointLabels.color = (context) =>
      EXPOSURE_SURFACES[context.index] === activeSurface
        ? "#f2381e"
        : "#171716";
  }
  const editableDataset = chart.data.datasets.at(-1);
  if (editableDataset) {
    editableDataset.fill = true;
    editableDataset.backgroundColor = "rgba(242, 56, 30, 0.10)";
    editableDataset.borderWidth = 1.5;
    editableDataset.borderColor = "#171716";
    editableDataset.pointRadius = EXPOSURE_SURFACES.map((surface) =>
      surface === activeSurface ? 4 : 3,
    );
    editableDataset.pointHoverRadius = 5;
    editableDataset.pointHitRadius = 16;
    editableDataset.pointBackgroundColor = EXPOSURE_SURFACES.map((surface) =>
      surface === activeSurface ? "#f2381e" : "#fffefb",
    );
    editableDataset.pointBorderColor = "#171716";
    editableDataset.pointBorderWidth = 1.5;
  }

  const chartCoordinates = (
    event: ReactPointerEvent<HTMLCanvasElement>,
  ): { x: number; y: number } | null => {
    const currentChart = chartRef.current;
    if (!currentChart) return null;
    const bounds = event.currentTarget.getBoundingClientRect();
    if (bounds.width <= 0 || bounds.height <= 0) return null;
    return {
      x: (event.clientX - bounds.left) * (currentChart.width / bounds.width),
      y: (event.clientY - bounds.top) * (currentChart.height / bounds.height),
    };
  };
  const updateFromPointer = (
    surface: ExposureSurface,
    event: ReactPointerEvent<HTMLCanvasElement>,
  ): void => {
    const currentChart = chartRef.current;
    const point = chartCoordinates(event);
    if (!currentChart || !point) return;
    const scale = currentChart.scales.r as RadialScale;
    updateSurface(surface, {
      max_level: exposureLevelForDistance(
        surface,
        Math.hypot(point.x - scale.xCenter, point.y - scale.yCenter),
        scale.drawingArea,
      ),
    });
  };
  const handlePointerDown = (
    event: ReactPointerEvent<HTMLCanvasElement>,
  ): void => {
    const currentChart = chartRef.current;
    const point = chartCoordinates(event);
    if (!currentChart || !point) return;
    const scale = currentChart.scales.r as RadialScale;
    const surface = exposureSurfaceForPointer(
      point.x - scale.xCenter,
      point.y - scale.yCenter,
    );
    setActiveSurface(surface);
    const index = EXPOSURE_SURFACES.indexOf(surface);
    const level =
      value.surfaces.find((entry) => entry.surface === surface)?.max_level ?? 0;
    const handle = scale.getPointPositionForValue(index, level);
    if (Math.hypot(point.x - handle.x, point.y - handle.y) > 26) return;
    draggingSurfaceRef.current = surface;
    event.currentTarget.setPointerCapture(event.pointerId);
    event.preventDefault();
    updateFromPointer(surface, event);
  };
  const handlePointerMove = (
    event: ReactPointerEvent<HTMLCanvasElement>,
  ): void => {
    const surface = draggingSurfaceRef.current;
    if (!surface) return;
    event.preventDefault();
    updateFromPointer(surface, event);
  };
  const stopDragging = (event: ReactPointerEvent<HTMLCanvasElement>): void => {
    draggingSurfaceRef.current = null;
    if (event.currentTarget.hasPointerCapture(event.pointerId)) {
      event.currentTarget.releasePointerCapture(event.pointerId);
    }
  };
  return (
    <div
      className={`exposure-policy-editor${props.compact ? " is-compact" : ""}`}
    >
      <div className="exposure-policy-mode">
        <ChoiceGroup
          label={<>{zh ? "访问方式" : "Access mode"}</>}
          onChange={(selected) =>
            props.onChange({
              ...value,
              access_mode: selected as CredentialExposurePolicy["access_mode"],
            })
          }
          value={value.access_mode}
          options={[
            {
              value: "protected",
              label: (
                <>
                  {zh
                    ? "受保护（按暴露面审批）"
                    : "Protected (exposure review)"}
                </>
              ),
            },
            {
              value: "direct",
              label: (
                <>{zh ? "直接可见（无需审批）" : "Direct (no approval)"}</>
              ),
            },
          ]}
        />
        <ChoiceGroup
          label={<>{zh ? "超限动作" : "On breach"}</>}
          disabled={value.access_mode === "direct"}
          onChange={(selected) =>
            props.onChange({
              ...value,
              breach_action:
                selected as CredentialExposurePolicy["breach_action"],
            })
          }
          value={value.breach_action}
          options={[
            {
              value: "human_review",
              label: <>{zh ? "转人工" : "Human review"}</>,
            },
            { value: "deny", label: <>{zh ? "拒绝" : "Deny"}</> },
          ]}
        />
      </div>
      <div className="exposure-policy-workbench">
        <section className="exposure-policy-radar-editor">
          <header>
            <strong>{zh ? "允许暴露轮廓" : "Allowed exposure profile"}</strong>
            <span>
              {zh
                ? "拖动节点定级；点击轴切换编辑"
                : "Drag a point to set its level; select an axis to edit"}
            </span>
          </header>
          <div className="exposure-policy-radar-canvas">
            <Radar
              aria-label={
                zh
                  ? "可拖动的凭据暴露面雷达图"
                  : "Draggable credential exposure radar"
              }
              aria-describedby={`${editorId}-radar-help`}
              tabIndex={0}
              onKeyDown={(event) => {
                const index = EXPOSURE_SURFACES.indexOf(activeSurface);
                if (event.key === "ArrowRight" || event.key === "ArrowLeft") {
                  event.preventDefault();
                  const direction = event.key === "ArrowRight" ? 1 : -1;
                  setActiveSurface(
                    EXPOSURE_SURFACES[
                      (index + direction + EXPOSURE_SURFACES.length) %
                        EXPOSURE_SURFACES.length
                    ],
                  );
                } else if (
                  event.key === "ArrowUp" ||
                  event.key === "ArrowDown"
                ) {
                  event.preventDefault();
                  const direction = event.key === "ArrowUp" ? 1 : -1;
                  updateSurface(activeSurface, {
                    max_level: Math.max(
                      0,
                      Math.min(
                        maximumPolicyLevel(activeSurface),
                        activeEntry.max_level + direction,
                      ),
                    ),
                  });
                }
              }}
              plugins={[icons, selectionPlugin]}
              data={chart.data}
              onPointerCancel={stopDragging}
              onPointerDown={handlePointerDown}
              onPointerMove={handlePointerMove}
              onPointerUp={stopDragging}
              options={chart.options}
              ref={(instance) => {
                chartRef.current = instance ?? null;
              }}
              role="application"
            />
          </div>
          <p
            className="exposure-radar-sr-only"
            id={`${editorId}-radar-help`}
            aria-live="polite"
          >
            {zh
              ? `当前：${LABELS[activeSurface].zh}，等级 ${activeEntry.max_level}。左右方向键切换维度，上下方向键调整等级。`
              : `Selected: ${LABELS[activeSurface].en}, level ${activeEntry.max_level}. Use left and right arrow keys to select an axis, up and down to change its level.`}
          </p>
        </section>
        <section
          aria-label={
            zh
              ? `${LABELS[activeSurface].zh}设置`
              : `${LABELS[activeSurface].en} settings`
          }
          className="exposure-axis-editor"
          role="region"
        >
          <header>
            <span>{zh ? "正在编辑" : "Editing axis"}</span>
            <h4>{zh ? LABELS[activeSurface].zh : LABELS[activeSurface].en}</h4>
            <p>
              {zh
                ? SURFACE_HELP[activeSurface].zh
                : SURFACE_HELP[activeSurface].en}
            </p>
          </header>
          <fieldset>
            <legend>{zh ? "允许等级" : "Allowed level"}</legend>
            {Array.from(
              { length: maximumPolicyLevel(activeSurface) + 1 },
              (_, level) => (
                <label key={level}>
                  <input
                    checked={activeEntry.max_level === level}
                    name={`${editorId}-exposure-level-${activeSurface}`}
                    onChange={() =>
                      updateSurface(activeSurface, { max_level: level })
                    }
                    type="radio"
                  />
                  <span>{surfaceLevelLabel(activeSurface, level, zh)}</span>
                </label>
              ),
            )}
          </fieldset>
          {activeSurface === "network" ? (
            <label className="exposure-network-allowlist">
              {zh
                ? "网络白名单（每行 domain: / subdomain: / regex:）"
                : "Network allowlist (domain:, subdomain:, or regex: per line)"}
              <textarea
                aria-invalid={
                  activeEntry.max_level === 1 && allowlistText.length === 0
                }
                onChange={(event) => {
                  const rules = event.currentTarget.value
                    .split("\n")
                    .map((line) => line.trim())
                    .filter(Boolean)
                    .map<NetworkDestinationRule>((line) => {
                      if (line.startsWith("regex:"))
                        return { kind: "regex", pattern: line.slice(6).trim() };
                      if (line.startsWith("subdomain:"))
                        return {
                          kind: "subdomains_of",
                          domain: line.slice(10).trim(),
                          include_apex: false,
                        };
                      return {
                        kind: "exact_domain",
                        domain: line.replace(/^domain:/, "").trim(),
                      };
                    });
                  updateSurface("network", { network_allowlist: rules });
                }}
                rows={4}
                value={allowlistText}
              />
              {activeEntry.max_level === 1 && allowlistText.length === 0 ? (
                <small role="alert">
                  {zh
                    ? "等级 1 必须至少声明一个目标。"
                    : "Level 1 requires at least one destination."}
                </small>
              ) : null}
            </label>
          ) : null}
          <label className="exposure-axis-note">
            {zh ? "给审批 LLM 的约束或备注" : "Constraint or note for approval"}
            <textarea
              aria-label={`${activeSurface} note`}
              onChange={(event) =>
                updateSurface(activeSurface, {
                  note: event.currentTarget.value || null,
                })
              }
              placeholder={
                zh
                  ? "例如：仅传递给声明的本地进程"
                  : "For example: only pass to the declared local process"
              }
              rows={3}
              value={activeEntry.note ?? ""}
            />
          </label>
        </section>
      </div>
    </div>
  );
}

export function parseExposurePolicy(
  value?: string | null,
): CredentialExposurePolicy | null {
  if (!value) return null;
  try {
    return normalizeExposurePolicy(
      JSON.parse(value) as CredentialExposurePolicy,
    );
  } catch {
    return null;
  }
}

export function CollectionExposurePolicyEditor(props: {
  value: CredentialExposurePolicy;
  onChange: (value: CredentialExposurePolicy) => void;
  locale?: string;
}): JSX.Element {
  const zh = props.locale === "zh-CN";
  return (
    <details className="collection-exposure-profile">
      <summary>{zh ? "默认暴露面配置" : "Default exposure profile"}</summary>
      <p>
        {zh
          ? "所有继承项随此配置更新；自定义项保持独立。"
          : "Inherited fields follow this profile. Custom fields keep their own settings."}
      </p>
      <ExposurePolicyEditor compact {...props} />
    </details>
  );
}

export function FieldExposurePolicyEditor(props: {
  defaultPolicy: CredentialExposurePolicy;
  customPolicy?: CredentialExposurePolicy | null;
  onChange: (value: CredentialExposurePolicy | null) => void;
  locale?: string;
}): JSX.Element {
  const zh = props.locale === "zh-CN";
  return (
    <div className="field-exposure-profile">
      <ChoiceGroup
        label={zh ? "暴露面配置来源" : "Exposure profile source"}
        value={props.customPolicy ? "custom" : "inherit"}
        onChange={(mode) =>
          props.onChange(
            mode === "custom"
              ? structuredClone(normalizeExposurePolicy(props.defaultPolicy))
              : null,
          )
        }
        options={[
          {
            value: "inherit",
            label: zh ? "继承默认" : "Inherit defaults",
          },
          { value: "custom", label: zh ? "自定义" : "Custom" },
        ]}
      />
      {props.customPolicy ? (
        <ExposurePolicyEditor
          compact
          locale={props.locale}
          value={props.customPolicy}
          onChange={props.onChange}
        />
      ) : (
        <p className="field-exposure-inherited-hint">
          {zh
            ? "使用默认配置。选择自定义会复制当前配置，之后可独立调整。"
            : "Uses the default profile. Choose Custom to copy it and adjust this field independently."}
        </p>
      )}
    </div>
  );
}
