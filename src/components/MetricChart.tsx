import { useEffect, useRef } from "react";
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

export function MetricChart({
  data,
  showHarvestMetrics = true,
  showSupernodeMetrics = false,
}: {
  data: MetricPoint[];
  showHarvestMetrics?: boolean;
  showSupernodeMetrics?: boolean;
}) {
  const ref = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (!ref.current) return;
    const chart = echarts.init(ref.current);
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
    chart.setOption({
      backgroundColor: "transparent",
      color: ["#55d6be", "#73a9ff", "#ffca5c", "#d58cff", "#ff8fb3", "#8de5a1"],
      animationDuration: 350,
      tooltip: { trigger: "axis" },
      legend: { textStyle: { color: "#91a3b8" }, data: ["REST応答", "ピア数", "同期遅延", ...harvestLegend, ...supernodeLegend] },
      grid: { left: 52, right: showHarvestMetrics || showSupernodeMetrics ? 82 : 32, top: 46, bottom: 42 },
      xAxis: {
        type: "time",
        axisLabel: { color: "#91a3b8" },
        splitLine: { show: false },
      },
      yAxis: [
        { type: "value", name: "ms", axisLabel: { color: "#91a3b8" }, splitLine: { lineStyle: { color: "#1d2b3d" } } },
        { type: "value", axisLabel: { color: "#91a3b8" }, splitLine: { show: false } },
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
            axisLabel: { color: "#d58cff", formatter: (value: number) => Number.isInteger(value) ? String(value) : "" },
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
            axisLabel: { color: "#8de5a1", formatter: (value: number) => value === 1 ? "OK" : "NG" },
            splitLine: { show: false },
          },
        ] : []),
      ],
      dataZoom: [{ type: "inside" }, { type: "slider", height: 18, bottom: 5 }],
      series: [
        {
          name: "REST応答",
          type: "line",
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
  }, [data, showHarvestMetrics, showSupernodeMetrics]);

  return <div ref={ref} className="chart" role="img" aria-label="ノードメトリクスの時系列グラフ" />;
}
