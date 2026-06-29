export type NodeKind = "symbol" | "nis1";
export type NodeStatus = "unknown" | "healthy" | "warning" | "critical" | "offline";
export type AlertSeverity = "warning" | "critical";
export type AlertState = "open" | "acknowledged" | "resolved";

export interface NodeSummary {
  id: string;
  displayName: string;
  kind: NodeKind;
  baseUrl: string;
  network: string | null;
  version: string | null;
  enabled: boolean;
  favorite: boolean;
  status: NodeStatus;
  height: string | null;
  finalizedHeight: string | null;
  lag: string | null;
  peerCount: number | null;
  latencyMs: number | null;
  lastSeenAt: number | null;
  lastError: string | null;
}

export interface DashboardData {
  total: number;
  healthy: number;
  warning: number;
  critical: number;
  offline: number;
  nodes: NodeSummary[];
}

export interface PeerInfo {
  key: string;
  name: string | null;
  host: string | null;
  port: number | null;
  status: string;
  version: string | null;
}

export interface NodeDetail {
  node: NodeSummary;
  publicKey: string | null;
  roles: string[];
  restVersion: string | null;
  chainScore: string | null;
  apiStatus: string | null;
  dbStatus: string | null;
  nisStatusCode: number | null;
  peers: PeerInfo[];
  websocketConnected: boolean;
}

export interface MetricPoint {
  observedAt: number;
  height: string | null;
  finalizedHeight: string | null;
  lag: string | null;
  peerCount: number | null;
  latencyMs: number | null;
  reachable: boolean;
}

export interface AlertRecord {
  id: string;
  nodeId: string;
  nodeName: string;
  alertType: string;
  severity: AlertSeverity;
  state: AlertState;
  message: string;
  raisedAt: number;
  updatedAt: number;
  acknowledgedAt: number | null;
  resolvedAt: number | null;
  occurrenceCount: number;
}

export interface AppSettings {
  healthIntervalSeconds: number;
  chainIntervalSeconds: number;
  peerIntervalSeconds: number;
  rawRetentionDays: number;
  aggregateRetentionDays: number;
  lagWarningBlocks: number;
  lagCriticalBlocks: number;
  latencyWarningMs: number;
  notificationEnabled: boolean;
  backgroundMonitoring: boolean;
  autostartEnabled: boolean;
  theme: "system" | "dark" | "light";
}

export interface NetworkInterfaceInfo {
  name: string;
  address: string;
  cidr: string;
}

export interface ScanCandidate {
  baseUrl: string;
  kind: NodeKind;
  name: string | null;
  network: string | null;
  version: string | null;
  latencyMs: number;
}

export interface ScanRequest {
  cidr: string;
  ports: number[];
}

export interface UpsertNodeInput {
  id?: string;
  displayName?: string;
  baseUrl: string;
  kind?: NodeKind;
  enabled?: boolean;
  favorite?: boolean;
}

