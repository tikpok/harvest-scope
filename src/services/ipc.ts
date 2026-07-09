import { invoke } from "@tauri-apps/api/core";
import type {
  AlertRecord,
  AppSettings,
  DashboardData,
  DiagnosticRun,
  HarvestEvent,
  MetricPoint,
  NetworkEvent,
  NetworkInterfaceInfo,
  NodeDetail,
  NodeConfigImportResult,
  NodeSummary,
  ScanCandidate,
  ScanRequest,
  TopologySnapshot,
  UpsertNodeInput,
} from "../types";

export const defaultSettings: AppSettings = {
  healthIntervalSeconds: 10,
  chainIntervalSeconds: 30,
  peerIntervalSeconds: 60,
  rawRetentionDays: 30,
  aggregateRetentionDays: 365,
  lagWarningBlocks: 5,
  lagCriticalBlocks: 20,
  latencyWarningMs: 2000,
  notificationEnabled: true,
  backgroundMonitoring: true,
  autostartEnabled: false,
  onboardingCompleted: false,
  theme: "system",
};

const isTauri = typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;

function call<T>(command: string, args: Record<string, unknown> | undefined, browserFallback: T): Promise<T> {
  return isTauri ? invoke<T>(command, args) : Promise.resolve(browserFallback);
}

const emptyDashboard: DashboardData = {
  total: 0,
  healthy: 0,
  warning: 0,
  critical: 0,
  offline: 0,
  rechecking: false,
  nodes: [],
};

const emptyTopology: TopologySnapshot = { nodes: [], edges: [], observedAt: 0 };

export const api = {
  dashboard: () => call("get_dashboard", undefined, emptyDashboard),
  topology: () => call<TopologySnapshot>("get_topology", undefined, emptyTopology),
  recentNetworkEvents: (limit = 50) =>
    call<NetworkEvent[]>("get_recent_network_events", { limit }, []),
  recentHarvestEvents: (limit = 50) =>
    call<HarvestEvent[]>("get_recent_harvest_events", { limit }, []),
  nodes: () => call<NodeSummary[]>("list_nodes", undefined, []),
  nodeDetail: (nodeId: string) => call<NodeDetail | null>("get_node_detail", { nodeId }, null).then((value) => {
    if (!value) throw new Error("ノード情報はTauriアプリで確認できます");
    return value;
  }),
  metrics: (nodeId: string, from: number, to: number) =>
    call<MetricPoint[]>("query_metrics", { nodeId, from, to }, []),
  upsertNode: (input: UpsertNodeInput) => invoke<NodeSummary>("upsert_node", { input }),
  deleteNode: (nodeId: string) => invoke<void>("delete_node", { nodeId }),
  moveNode: (nodeId: string, direction: "up" | "down") =>
    invoke<void>("move_node", { nodeId, direction }),
  pauseNode: (nodeId: string) => invoke<void>("pause_node", { nodeId }),
  resumeNode: (nodeId: string) => invoke<void>("resume_node", { nodeId }),
  setNodeHarvestMetricsVisible: (nodeId: string, visible: boolean) =>
    invoke<void>("set_node_harvest_metrics_visible", { nodeId, visible }),
  refreshNode: (nodeId: string) => invoke<void>("refresh_node", { nodeId }),
  refreshAllNodes: () => invoke<void>("refresh_all_nodes"),
  interfaces: () => call<NetworkInterfaceInfo[]>("list_network_interfaces", undefined, []),
  scan: (request: ScanRequest) => invoke<ScanCandidate[]>("scan_subnet", { request }),
  cancelScan: () => invoke<void>("cancel_scan"),
  alerts: () => call<AlertRecord[]>("list_alerts", undefined, []),
  alert: (alertId: string) => invoke<AlertRecord>("get_alert", { alertId }),
  acknowledgeAlert: (alertId: string) => invoke<void>("acknowledge_alert", { alertId }),
  runNodeDiagnostic: (nodeId: string, alertId?: string) =>
    invoke<DiagnosticRun>("run_node_diagnostic", { nodeId, alertId }),
  diagnosticRun: (runId: string) => invoke<DiagnosticRun>("get_diagnostic_run", { runId }),
  nodeDiagnostics: (nodeId: string, limit = 20) =>
    call<DiagnosticRun[]>("list_node_diagnostics", { nodeId, limit }, []),
  takePendingNavigation: () => call<string | null>("take_pending_navigation", undefined, null),
  settings: () => call("get_settings", undefined, defaultSettings),
  updateSettings: (settings: AppSettings) => invoke<AppSettings>("update_settings", { settings }),
  exportNodeConfig: () => invoke<string | null>("export_node_config"),
  importNodeConfig: () => invoke<NodeConfigImportResult | null>("import_node_config"),
  exportCsv: (dataset: "nodes" | "metrics" | "alerts", nodeId?: string) =>
    invoke<string | null>("export_csv", { dataset, nodeId }),
};
