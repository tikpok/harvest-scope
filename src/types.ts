export type NodeKind = "symbol" | "nis1";
export type NodeStatus = "unknown" | "healthy" | "warning" | "critical" | "offline";
export type AlertSeverity = "warning" | "critical";
export type AlertState = "open" | "acknowledged" | "resolved";
export type DiagnosticStatus = "passed" | "warning" | "failed";
export type DiagnosticCheckStatus = "pass" | "warning" | "fail" | "skipped";

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
  showHarvestMetrics: boolean;
  delegatedHarvesterCount: number | null;
  delegatedHarvesterDelta: number | null;
  totalImportance: string | null;
  totalBalanceMicroXym: string | null;
  harvestMetricError: string | null;
  latestHarvestAt: number | null;
  nodeCertificateNotAfter: number | null;
  nodeCertificateRemainingDays: number | null;
  votingKeyStartEpoch: number | null;
  votingKeyEndEpoch: number | null;
  finalizationEpoch: number | null;
  votingKeyRemainingEpochs: number | null;
  certificateMetricError: string | null;
  supernodeId: number | null;
  supernodeStatus: string | null;
  supernodeLastRound: number | null;
  supernodeLastRoundAt: number | null;
  supernodeChecksPassed: boolean | null;
  supernodeFailedChecks: string[];
  supernodeMetricError: string | null;
}

export interface DashboardData {
  total: number;
  healthy: number;
  warning: number;
  critical: number;
  offline: number;
  rechecking: boolean;
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

export interface TopologyNode {
  id: string;
  label: string;
  kind: NodeKind;
  network: string | null;
  status: NodeStatus;
  registered: boolean;
  enabled: boolean;
  host: string | null;
  port: number | null;
  version: string | null;
  roles: string[];
  peerStatus: string | null;
  websocketConnected: boolean;
  apiStatus: string | null;
  dbStatus: string | null;
  nisStatusCode: number | null;
  height: string | null;
  finalizedHeight: string | null;
  lag: string | null;
  showHarvestMetrics: boolean;
  delegatedHarvesterCount: number | null;
  latestHarvestAt: number | null;
}

export interface TopologyEdge {
  id: string;
  source: string;
  target: string;
  status: string;
}

export interface TopologySnapshot {
  nodes: TopologyNode[];
  edges: TopologyEdge[];
  observedAt: number;
}

export type NetworkEventKind = "block" | "finalizedBlock" | "pollUpdate" | "harvestedBlock";

export interface NetworkEvent {
  id: string;
  nodeId: string;
  kind: NetworkEventKind;
  height: string | null;
  hash: string | null;
  signerPublicKey: string | null;
  harvesterPublicKey: string | null;
  harvestedNodeId: string | null;
  observerNodeId: string | null;
  observedAt: number;
  source: "websocket" | "polling";
}

export interface HarvestEvent {
  id: string;
  nodeId: string;
  observerNodeId: string;
  harvesterPublicKey: string;
  height: string | null;
  hash: string | null;
  observedAt: number;
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
  recentHarvests: HarvestEvent[];
}

export interface MetricPoint {
  observedAt: number;
  height: string | null;
  finalizedHeight: string | null;
  lag: string | null;
  peerCount: number | null;
  latencyMs: number | null;
  reachable: boolean;
  delegatedHarvesterCount: number | null;
  delegatedHarvesterDelta: number | null;
  totalImportance: string | null;
  totalBalanceMicroXym: string | null;
  nodeCertificateRemainingDays: number | null;
  votingKeyRemainingEpochs: number | null;
  supernodeChecksPassed: boolean | null;
  supernodeLastRound: number | null;
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

export interface DiagnosticCheck {
  key: string;
  label: string;
  status: DiagnosticCheckStatus;
  message: string;
  recommendation: string | null;
  durationMs: number;
}

export interface DiagnosticRun {
  id: string;
  nodeId: string;
  alertId: string | null;
  startedAt: number;
  completedAt: number;
  status: DiagnosticStatus;
  summary: string;
  checks: DiagnosticCheck[];
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
  onboardingCompleted: boolean;
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

export interface NodeConfigImportFailure {
  baseUrl: string | null;
  displayName: string | null;
  reason: string;
}

export interface NodeConfigImportResult {
  added: number;
  updated: number;
  failed: number;
  failures: NodeConfigImportFailure[];
}
