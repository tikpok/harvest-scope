import type { EChartsCoreOption } from "echarts/core";
import type { MetricPoint } from "../types";

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

export function buildMetricChartOption(
  data: MetricPoint[],
  showHarvestMetrics = true,
  showSupernodeMetrics = false,
  latencyLabel = "REST応答",
): EChartsCoreOption {
  const harvestLegend = showHarvestMetrics ? ["委任者", "合計XYM"] : [];
  const supernodeLegend = showSupernodeMetrics ? ["Supernode合否"] : [];
  const harvestSeries = showHarvestMetrics
    ? [
        {
          name: "委任者",
          type: "line",
          yAxisIndex: 2,
          showSymbol: true,
          symbolSize: 5,
          lineStyle: { color: "#d58cff", width: 3 },
          data: data.map((point) => [point.observedAt, point.delegatedHarvesterCount]),
        },
        {
          name: "合計XYM",
          type: "line",
          yAxisIndex: 3,
          showSymbol: false,
          lineStyle: { color: "#ff8fb3", width: 2 },
          data: data.map((point) => [point.observedAt, microXymToWholeNumber(point.totalBalanceMicroXym)]),
        },
      ]
    : [];
  const supernodeAxisIndex = showHarvestMetrics ? 4 : 2;
  const supernodeSeries = showSupernodeMetrics
    ? [
        {
          name: "Supernode合否",
          type: "line",
          yAxisIndex: supernodeAxisIndex,
          step: "end",
          showSymbol: true,
          symbolSize: 6,
          lineStyle: { color: "#8de5a1", width: 2 },
          data: data.map((point) => [point.observedAt, point.supernodeChecksPassed == null ? null : point.supernodeChecksPassed ? 1 : 0]),
        },
      ]
    : [];

  return {
    backgroundColor: "transparent",
    color: ["#55d6be", "#73a9ff", "#ffca5c", "#d58cff", "#ff8fb3", "#8de5a1"],
    animationDuration: 350,
    tooltip: {
      trigger: "axis",
      backgroundColor: "rgba(10, 18, 28, 0.96)",
      borderColor: "#2b3d52",
      textStyle: { color: "#dce7f5" },
    },
    legend: {
      top: 4,
      textStyle: { color: "#91a3b8" },
      data: [latencyLabel, "ピア数", "同期遅延", ...harvestLegend, ...supernodeLegend],
    },
    grid: { left: 54, right: showHarvestMetrics || showSupernodeMetrics ? 84 : 32, top: 48, bottom: 76 },
    xAxis: {
      type: "time",
      axisLabel: { color: "#91a3b8", hideOverlap: true },
      axisLine: { lineStyle: { color: "#2b3d52" } },
      splitLine: { show: false },
    },
    yAxis: [
      {
        type: "value",
        name: "ms",
        axisLabel: { color: "#91a3b8" },
        axisLine: { show: true, lineStyle: { color: "#55d6be" } },
        splitLine: { lineStyle: { color: "#1d2b3d" } },
      },
      {
        type: "value",
        axisLabel: { color: "#91a3b8" },
        splitLine: { show: false },
      },
      ...(showHarvestMetrics
        ? [
            {
              type: "value",
              name: "委任者",
              position: "right",
              scale: true,
              minInterval: 1,
              interval: 1,
              min: (value: { min: number }) => Math.max(0, Math.floor(value.min - 1)),
              max: (value: { max: number }) => Math.ceil(value.max + 1),
              axisLabel: { color: "#d58cff", formatter: (value: number) => (Number.isInteger(value) ? String(value) : "") },
              axisLine: { show: true, lineStyle: { color: "#d58cff" } },
              splitLine: { show: false },
            },
            {
              type: "value",
              show: false,
              scale: true,
              splitLine: { show: false },
            },
          ]
        : []),
      ...(showSupernodeMetrics
        ? [
            {
              type: "value",
              name: "SN",
              position: "right",
              offset: showHarvestMetrics ? 48 : 0,
              min: 0,
              max: 1,
              interval: 1,
              axisLabel: { color: "#8de5a1", formatter: (value: number) => (value === 1 ? "OK" : "NG") },
              axisLine: { show: true, lineStyle: { color: "#8de5a1" } },
              splitLine: { show: false },
            },
          ]
        : []),
    ],
    dataZoom: [
      {
        type: "inside",
        zoomOnMouseWheel: true,
        moveOnMouseMove: true,
        moveOnMouseWheel: false,
      },
      {
        type: "slider",
        show: true,
        height: 20,
        bottom: 12,
        borderColor: "#2b3d52",
        backgroundColor: "#0d1723",
        fillerColor: "rgba(85, 214, 190, 0.18)",
        handleStyle: { color: "#55d6be", borderColor: "#87ead7" },
        moveHandleStyle: { color: "#55d6be" },
        textStyle: { color: "#91a3b8" },
      },
    ],
    series: [
      {
        name: latencyLabel,
        type: "line",
        yAxisIndex: 0,
        showSymbol: false,
        smooth: true,
        lineStyle: { color: "#55d6be", width: 2 },
        data: data.map((point) => [point.observedAt, point.latencyMs]),
      },
      {
        name: "ピア数",
        type: "line",
        yAxisIndex: 1,
        showSymbol: false,
        lineStyle: { color: "#73a9ff", width: 2 },
        data: data.map((point) => [point.observedAt, point.peerCount]),
      },
      {
        name: "同期遅延",
        type: "line",
        yAxisIndex: 1,
        showSymbol: false,
        lineStyle: { color: "#ffca5c", width: 2 },
        data: data.map((point) => [point.observedAt, numeric(point.lag)]),
      },
      ...harvestSeries,
      ...supernodeSeries,
    ],
  };
}
