import { useEffect, useRef, useState } from "react";
import * as echarts from "echarts/core";
import { LineChart } from "echarts/charts";
import { DataZoomComponent, GridComponent, LegendComponent, TooltipComponent } from "echarts/components";
import { CanvasRenderer } from "echarts/renderers";
import type { MetricPoint } from "../types";

echarts.use([LineChart, DataZoomComponent, GridComponent, LegendComponent, TooltipComponent, CanvasRenderer]);

function microXymToWholeNumber(value: string | null) {
  if (!value) return null;
  try {
    return Number(BigInt(value) / 1_000_000n);
  } catch {
    const micro = Number(value);
    return Number.isFinite(micro) ? Math.trunc(micro / 1_000_000) : null;
  }
}

function cssVar(styles: CSSStyleDeclaration, name: string, fallback: string) {
  return styles.getPropertyValue(name).trim() || fallback;
}

function readChartTheme() {
  const styles = getComputedStyle(document.documentElement);
  return {
    latency: cssVar(styles, "--chart-latency", "#55d6be"),
    peer: cssVar(styles, "--chart-peer", "#73a9ff"),
    lag: cssVar(styles, "--chart-lag", "#ffca5c"),
    harvest: cssVar(styles, "--chart-harvest", "#d58cff"),
    balance: cssVar(styles, "--chart-balance", "#ff8fb3"),
    supernode: cssVar(styles, "--chart-supernode", "#8de5a1"),
    axis: cssVar(styles, "--chart-axis", "#91a3b8"),
    grid: cssVar(styles, "--chart-grid", "#1d2b3d"),
    dataZoom: cssVar(styles, "--chart-data-zoom", "#2a3d55"),
    dataZoomFill: cssVar(styles, "--chart-data-zoom-fill", "rgba(115,169,255,.16)"),
  };
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
  const ref = useRef<HTMLDivElement>(null);
  const [themeRevision, setThemeRevision] = useState(0);

  useEffect(() => {
    const updateThemeRevision = () => setThemeRevision((value) => value + 1);
    const observer = new MutationObserver(updateThemeRevision);
    observer.observe(document.documentElement, { attributes: true, attributeFilter: ["data-theme"] });
    const lightScheme = window.matchMedia("(prefers-color-scheme: light)");
    lightScheme.addEventListener("change", updateThemeRevision);
    return () => {
      observer.disconnect();
      lightScheme.removeEventListener("change", updateThemeRevision);
    };
  }, []);

  useEffect(() => {
    if (!ref.current) return;
    const chart = echarts.init(ref.current);
    const theme = readChartTheme();
    const harvestLegend = showHarvestMetrics ? ["委任者", "合計XYM"] : [];
    const supernodeLegend = showSupernodeMetrics ? ["Supernode合否"] : [];
    const harvestSeries = showHarvestMetrics
      ? [
          {
            name: "委任者",
            type: "line",
            yAxisIndex: 2,
            showSymbol: true,
            symbolSize: 4,
            lineStyle: { color: theme.harvest, width: 3 },
            data: data.map((point) => [point.observedAt, point.delegatedHarvesterCount]),
          },
          {
            name: "合計XYM",
            type: "line",
            yAxisIndex: 3,
            showSymbol: false,
            lineStyle: { color: theme.balance, width: 2 },
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
            lineStyle: { color: theme.supernode, width: 2 },
            data: data.map((point) => [point.observedAt, point.supernodeChecksPassed == null ? null : point.supernodeChecksPassed ? 1 : 0]),
          },
        ]
      : [];
    chart.setOption({
      backgroundColor: "transparent",
      color: [theme.latency, theme.peer, theme.lag, theme.harvest, theme.balance, theme.supernode],
      animationDuration: 350,
      tooltip: { trigger: "axis" },
      legend: { textStyle: { color: theme.axis }, data: [latencyLabel, "ピア数", "同期遅延", ...harvestLegend, ...supernodeLegend] },
      grid: { left: 52, right: showHarvestMetrics || showSupernodeMetrics ? 82 : 32, top: 46, bottom: 42 },
      xAxis: {
        type: "time",
        axisLabel: { color: theme.axis },
        splitLine: { show: false },
      },
      yAxis: [
        { type: "value", name: "ms", axisLabel: { color: theme.axis }, splitLine: { lineStyle: { color: theme.grid } } },
        { type: "value", axisLabel: { color: theme.axis }, splitLine: { show: false } },
        ...(showHarvestMetrics ? [
          {
            type: "value",
            name: "委任者",
            position: "right",
            scale: true,
            minInterval: 1,
            interval: 1,
            min: (value: { min: number }) => Math.max(0, Math.floor(value.min - 1)),
            max: (value: { max: number }) => Math.ceil(value.max + 1),
            axisLabel: { color: theme.harvest, formatter: (value: number) => Number.isInteger(value) ? String(value) : "" },
            splitLine: { show: false },
          },
          {
            type: "value",
            show: false,
            scale: true,
            splitLine: { show: false },
          },
        ] : []),
        ...(showSupernodeMetrics ? [
          {
            type: "value",
            name: "SN",
            position: "right",
            min: 0,
            max: 1,
            interval: 1,
            axisLabel: { color: theme.supernode, formatter: (value: number) => value === 1 ? "OK" : "NG" },
            splitLine: { show: false },
          },
        ] : []),
      ],
      dataZoom: [
        { type: "inside" },
        {
          type: "slider",
          height: 18,
          bottom: 5,
          borderColor: theme.grid,
          dataBackground: { lineStyle: { color: theme.dataZoom }, areaStyle: { color: theme.dataZoomFill } },
          selectedDataBackground: { lineStyle: { color: theme.peer }, areaStyle: { color: theme.dataZoomFill } },
          textStyle: { color: theme.axis },
          fillerColor: theme.dataZoomFill,
          handleStyle: { color: theme.peer, borderColor: theme.peer },
          moveHandleStyle: { color: theme.dataZoom },
        },
      ],
      series: [
        {
          name: latencyLabel,
          type: "line",
          showSymbol: false,
          smooth: true,
          lineStyle: { color: theme.latency, width: 2 },
          data: data.map((point) => [point.observedAt, point.latencyMs]),
        },
        {
          name: "ピア数",
          type: "line",
          yAxisIndex: 1,
          showSymbol: false,
          lineStyle: { color: theme.peer, width: 2 },
          data: data.map((point) => [point.observedAt, point.peerCount]),
        },
        {
          name: "同期遅延",
          type: "line",
          yAxisIndex: 1,
          showSymbol: false,
          lineStyle: { color: theme.lag, width: 2 },
          data: data.map((point) => [point.observedAt, point.lag ? Number(point.lag) : null]),
        },
        ...harvestSeries,
        ...supernodeSeries,
      ],
    });
    const resize = new ResizeObserver(() => chart.resize());
    resize.observe(ref.current);
    return () => {
      resize.disconnect();
      chart.dispose();
    };
  }, [data, showHarvestMetrics, showSupernodeMetrics, latencyLabel, themeRevision]);

  return <div ref={ref} className="chart" role="img" aria-label="ノードメトリクスの時系列グラフ" />;
}
