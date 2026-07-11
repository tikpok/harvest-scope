import type { MetricPoint } from "../types";

interface Series {
  name: string;
  color: string;
  values: Array<number | null>;
}

const WIDTH = 920;
const HEIGHT = 370;
const PADDING = { top: 34, right: 28, bottom: 44, left: 54 };

function microXymToWholeNumber(value: string | null) {
  if (!value) return null;
  try {
    return Number(BigInt(value) / 1_000_000n);
  } catch {
    const micro = Number(value);
    return Number.isFinite(micro) ? Math.trunc(micro / 1_000_000) : null;
  }
}

function numeric(value: string | number | null) {
  if (value == null) return null;
  const parsed = Number(value);
  return Number.isFinite(parsed) ? parsed : null;
}

function extent(values: Array<number | null>) {
  const numbers = values.filter((value): value is number => value != null && Number.isFinite(value));
  if (numbers.length === 0) return null;
  let min = Math.min(...numbers);
  let max = Math.max(...numbers);
  if (min === max) {
    min -= 1;
    max += 1;
  }
  return { min, max };
}

function formatTime(value: number) {
  return new Intl.DateTimeFormat("ja-JP", { month: "numeric", day: "numeric", hour: "2-digit", minute: "2-digit" }).format(value);
}

function formatRange(values: Array<number | null>) {
  const range = extent(values);
  if (!range) return "データなし";
  return `${Math.round(range.min).toLocaleString("ja-JP")} - ${Math.round(range.max).toLocaleString("ja-JP")}`;
}

function linePaths(series: Series, timestamps: number[]) {
  const range = extent(series.values);
  if (!range || timestamps.length === 0) return [];
  const minTime = Math.min(...timestamps);
  const maxTime = Math.max(...timestamps);
  const timeSpan = Math.max(1, maxTime - minTime);
  const plotWidth = WIDTH - PADDING.left - PADDING.right;
  const plotHeight = HEIGHT - PADDING.top - PADDING.bottom;
  const paths: string[] = [];
  let current = "";

  series.values.forEach((value, index) => {
    if (value == null) {
      if (current) paths.push(current);
      current = "";
      return;
    }
    const x = PADDING.left + ((timestamps[index] - minTime) / timeSpan) * plotWidth;
    const y = PADDING.top + (1 - (value - range.min) / (range.max - range.min)) * plotHeight;
    current += `${current ? " L" : "M"} ${x.toFixed(1)} ${y.toFixed(1)}`;
  });
  if (current) paths.push(current);
  return paths;
}

export function MetricChart({
  data,
  showHarvestMetrics = true,
  showSupernodeMetrics = false,
  latencyLabel = "REST応答",
}: {
  data: MetricPoint[];
  showHarvestMetrics?: boolean;
  showSupernodeMetrics?: boolean;
  latencyLabel?: string;
}) {
  const timestamps = data.map((point) => point.observedAt);
  const series: Series[] = [
    { name: latencyLabel, color: "#55d6be", values: data.map((point) => point.latencyMs) },
    { name: "ピア数", color: "#73a9ff", values: data.map((point) => point.peerCount) },
    { name: "同期遅延", color: "#ffca5c", values: data.map((point) => numeric(point.lag)) },
    ...(showHarvestMetrics
      ? [
          { name: "委任者", color: "#d58cff", values: data.map((point) => point.delegatedHarvesterCount) },
          { name: "合計XYM", color: "#ff8fb3", values: data.map((point) => microXymToWholeNumber(point.totalBalanceMicroXym)) },
        ]
      : []),
    ...(showSupernodeMetrics
      ? [
          {
            name: "Supernode合否",
            color: "#8de5a1",
            values: data.map((point) => point.supernodeChecksPassed == null ? null : point.supernodeChecksPassed ? 1 : 0),
          },
        ]
      : []),
  ];
  const minTime = Math.min(...timestamps);
  const maxTime = Math.max(...timestamps);
  const midTime = minTime + (maxTime - minTime) / 2;
  const gridLines = [0, 1, 2, 3, 4].map((index) => PADDING.top + index * ((HEIGHT - PADDING.top - PADDING.bottom) / 4));

  return (
    <div className="chart" role="img" aria-label="ノードメトリクスの時系列グラフ">
      <svg viewBox={`0 0 ${WIDTH} ${HEIGHT}`} width="100%" height="100%" preserveAspectRatio="none">
        <rect width={WIDTH} height={HEIGHT} rx="10" fill="transparent" />
        {gridLines.map((y) => (
          <line key={y} x1={PADDING.left} x2={WIDTH - PADDING.right} y1={y} y2={y} stroke="#1d2b3d" strokeWidth="1" />
        ))}
        {[minTime, midTime, maxTime].map((value, index) => (
          <text key={`${value}-${index}`} x={index === 0 ? PADDING.left : index === 1 ? WIDTH / 2 : WIDTH - PADDING.right} y={HEIGHT - 16} textAnchor={index === 0 ? "start" : index === 1 ? "middle" : "end"} fill="#91a3b8" fontSize="12">
            {formatTime(value)}
          </text>
        ))}
        {series.map((item) => (
          <g key={item.name}>
            {linePaths(item, timestamps).map((path, index) => (
              <path key={`${item.name}-${index}`} d={path} fill="none" stroke={item.color} strokeWidth="2.5" strokeLinecap="round" strokeLinejoin="round" vectorEffect="non-scaling-stroke" />
            ))}
          </g>
        ))}
        <g transform={`translate(${PADDING.left} 18)`}>
          {series.map((item, index) => (
            <g key={item.name} transform={`translate(${index * 136} 0)`}>
              <circle r="5" fill={item.color} />
              <text x="10" y="4" fill="#c8d5e5" fontSize="12">{item.name}</text>
              <title>{`${item.name}: ${formatRange(item.values)}`}</title>
            </g>
          ))}
        </g>
      </svg>
    </div>
  );
}
