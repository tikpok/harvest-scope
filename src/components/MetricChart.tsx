import { useEffect, useRef } from "react";
import * as echarts from "echarts/core";
import { LineChart } from "echarts/charts";
import { DataZoomComponent, GridComponent, LegendComponent, TooltipComponent } from "echarts/components";
import { CanvasRenderer } from "echarts/renderers";
import type { MetricPoint } from "../types";

echarts.use([LineChart, DataZoomComponent, GridComponent, LegendComponent, TooltipComponent, CanvasRenderer]);

export function MetricChart({ data }: { data: MetricPoint[] }) {
  const ref = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (!ref.current) return;
    const chart = echarts.init(ref.current);
    chart.setOption({
      backgroundColor: "transparent",
      color: ["#55d6be", "#73a9ff", "#ffca5c"],
      animationDuration: 350,
      tooltip: { trigger: "axis" },
      legend: { textStyle: { color: "#91a3b8" }, data: ["REST応答", "ピア数", "同期遅延"] },
      grid: { left: 52, right: 32, top: 46, bottom: 42 },
      xAxis: {
        type: "time",
        axisLabel: { color: "#91a3b8" },
        splitLine: { show: false },
      },
      yAxis: [
        { type: "value", name: "ms", axisLabel: { color: "#91a3b8" }, splitLine: { lineStyle: { color: "#1d2b3d" } } },
        { type: "value", axisLabel: { color: "#91a3b8" }, splitLine: { show: false } },
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
      ],
    });
    const resize = new ResizeObserver(() => chart.resize());
    resize.observe(ref.current);
    return () => {
      resize.disconnect();
      chart.dispose();
    };
  }, [data]);

  return <div ref={ref} className="chart" role="img" aria-label="ノードメトリクスの時系列グラフ" />;
}
