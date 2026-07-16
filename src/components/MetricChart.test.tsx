import { buildMetricChartOption } from "./metricChartOptions";
import type { MetricPoint } from "../types";

const points: MetricPoint[] = [
  {
    observedAt: 1_700_000_000_000,
    height: "100",
    finalizedHeight: "98",
    lag: "2",
    peerCount: 12,
    latencyMs: 45,
    reachable: true,
    delegatedHarvesterCount: 3,
    delegatedHarvesterDelta: 1,
    totalImportance: "1000",
    totalBalanceMicroXym: "250000000",
    nodeCertificateRemainingDays: 90,
    votingKeyRemainingEpochs: 40,
    supernodeChecksPassed: true,
    supernodeLastRound: 123,
  },
  {
    observedAt: 1_700_000_060_000,
    height: "101",
    finalizedHeight: "99",
    lag: "2",
    peerCount: 13,
    latencyMs: 52,
    reachable: true,
    delegatedHarvesterCount: 4,
    delegatedHarvesterDelta: 1,
    totalImportance: "1100",
    totalBalanceMicroXym: "275000000",
    nodeCertificateRemainingDays: 90,
    votingKeyRemainingEpochs: 40,
    supernodeChecksPassed: false,
    supernodeLastRound: 124,
  },
];

test("詳細メトリクスをスライダーとグラフ内操作で期間移動できる", () => {
  const option = buildMetricChartOption(points, true, true, "REST応答");

  expect(option).toMatchObject({
    tooltip: { trigger: "axis" },
    xAxis: { type: "time" },
    dataZoom: [
      { type: "inside", zoomOnMouseWheel: true, moveOnMouseMove: true },
      { type: "slider", show: true },
    ],
  });
});

test("単位の異なる詳細メトリクスを専用のY軸へ割り当てる", () => {
  const option = buildMetricChartOption(points, true, true, "REST応答") as {
    series: Array<{ name: string; yAxisIndex?: number; data: Array<[number, number | null]> }>;
  };

  expect(option.series.map(({ name, yAxisIndex }) => ({ name, yAxisIndex }))).toEqual([
    { name: "REST応答", yAxisIndex: 0 },
    { name: "ピア数", yAxisIndex: 1 },
    { name: "同期遅延", yAxisIndex: 1 },
    { name: "委任者", yAxisIndex: 2 },
    { name: "合計XYM", yAxisIndex: 3 },
    { name: "Supernode合否", yAxisIndex: 4 },
  ]);
  expect(option.series.find((series) => series.name === "合計XYM")?.data).toEqual([
    [1_700_000_000_000, 250],
    [1_700_000_060_000, 275],
  ]);
});
