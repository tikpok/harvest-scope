import { useEffect, useRef } from "react";
import * as echarts from "echarts/core";
import { LineChart } from "echarts/charts";
import { DataZoomComponent, GridComponent, LegendComponent, TooltipComponent } from "echarts/components";
import { CanvasRenderer } from "echarts/renderers";
import { buildMetricChartOption } from "./metricChartOptions";
import type { MetricPoint } from "../types";

echarts.use([LineChart, DataZoomComponent, GridComponent, LegendComponent, TooltipComponent, CanvasRenderer]);

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

  useEffect(() => {
    if (!ref.current) return;
    const chart = echarts.init(ref.current);
    chart.setOption(buildMetricChartOption(data, showHarvestMetrics, showSupernodeMetrics, latencyLabel));
    const resize = new ResizeObserver(() => chart.resize());
    resize.observe(ref.current);
    return () => {
      resize.disconnect();
      chart.dispose();
    };
  }, [data, showHarvestMetrics, showSupernodeMetrics, latencyLabel]);

  return <div ref={ref} className="chart" role="img" aria-label="ノードメトリクスの時系列グラフ。マウスホイールで期間を拡大縮小し、ドラッグで移動できます。" />;
}
