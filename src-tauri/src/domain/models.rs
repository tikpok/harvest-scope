use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum NodeKind {
    Symbol,
    Nis1,
}

impl NodeKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Symbol => "symbol",
            Self::Nis1 => "nis1",
        }
    }
}

impl TryFrom<&str> for NodeKind {
    type Error = String;
    fn try_from(value: &str) -> Result<Self, Self::Error> {
        match value {
            "symbol" => Ok(Self::Symbol),
            "nis1" => Ok(Self::Nis1),
            other => Err(format!("unknown node kind: {other}")),
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum NodeStatus {
    Unknown,
    Healthy,
    Warning,
    Critical,
    Offline,
}

impl NodeStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Unknown => "unknown",
            Self::Healthy => "healthy",
            Self::Warning => "warning",
            Self::Critical => "critical",
            Self::Offline => "offline",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NodeIdentity {
    pub kind: NodeKind,
    pub name: Option<String>,
    pub network: Option<String>,
    pub version: Option<String>,
    pub public_key: Option<String>,
    pub roles: Vec<String>,
    pub rest_version: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HealthSnapshot {
    pub reachable: bool,
    pub api_status: Option<String>,
    pub db_status: Option<String>,
    pub nis_status_code: Option<i64>,
    pub healthy: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ChainSnapshot {
    pub height: Option<u64>,
    pub finalized_height: Option<u64>,
    pub network_height: Option<u64>,
    pub chain_score: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PeerInfo {
    pub key: String,
    pub name: Option<String>,
    pub host: Option<String>,
    pub port: Option<u16>,
    pub status: String,
    pub version: Option<String>,
}

#[derive(Debug, Clone)]
pub struct WebSocketSpec {
    pub url: String,
    pub topics: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NodeSummary {
    pub id: String,
    pub display_name: String,
    pub kind: NodeKind,
    pub base_url: String,
    pub network: Option<String>,
    pub version: Option<String>,
    pub enabled: bool,
    pub favorite: bool,
    pub status: NodeStatus,
    pub height: Option<String>,
    pub finalized_height: Option<String>,
    pub lag: Option<String>,
    pub peer_count: Option<i64>,
    pub latency_ms: Option<i64>,
    pub last_seen_at: Option<i64>,
    pub last_error: Option<String>,
}

#[derive(Debug, Clone)]
pub struct StoredNode {
    pub id: String,
    pub display_name: String,
    pub kind: NodeKind,
    pub base_url: String,
    pub network: Option<String>,
    pub version: Option<String>,
    pub public_key: Option<String>,
    pub roles: Vec<String>,
    pub rest_version: Option<String>,
    pub enabled: bool,
    pub favorite: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NodeDetail {
    pub node: NodeSummary,
    pub public_key: Option<String>,
    pub roles: Vec<String>,
    pub rest_version: Option<String>,
    pub chain_score: Option<String>,
    pub api_status: Option<String>,
    pub db_status: Option<String>,
    pub nis_status_code: Option<i64>,
    pub peers: Vec<PeerInfo>,
    pub websocket_connected: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NodeSample {
    pub observed_at: i64,
    pub reachable: bool,
    pub status: NodeStatus,
    pub latency_ms: Option<i64>,
    pub height: Option<u64>,
    pub finalized_height: Option<u64>,
    pub network_height: Option<u64>,
    pub lag: Option<u64>,
    pub peer_count: Option<i64>,
    pub chain_score: Option<String>,
    pub api_status: Option<String>,
    pub db_status: Option<String>,
    pub nis_status_code: Option<i64>,
    pub last_error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MetricPoint {
    pub observed_at: i64,
    pub height: Option<String>,
    pub finalized_height: Option<String>,
    pub lag: Option<String>,
    pub peer_count: Option<i64>,
    pub latency_ms: Option<i64>,
    pub reachable: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DashboardData {
    pub total: usize,
    pub healthy: usize,
    pub warning: usize,
    pub critical: usize,
    pub offline: usize,
    pub nodes: Vec<NodeSummary>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpsertNodeInput {
    pub id: Option<String>,
    pub display_name: Option<String>,
    pub base_url: String,
    pub kind: Option<NodeKind>,
    pub enabled: Option<bool>,
    pub favorite: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanRequest {
    pub cidr: String,
    pub ports: Vec<u16>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanCandidate {
    pub base_url: String,
    pub kind: NodeKind,
    pub name: Option<String>,
    pub network: Option<String>,
    pub version: Option<String>,
    pub latency_ms: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NetworkInterfaceInfo {
    pub name: String,
    pub address: String,
    pub cidr: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppSettings {
    pub health_interval_seconds: u64,
    pub chain_interval_seconds: u64,
    pub peer_interval_seconds: u64,
    pub raw_retention_days: u64,
    pub aggregate_retention_days: u64,
    pub lag_warning_blocks: u64,
    pub lag_critical_blocks: u64,
    pub latency_warning_ms: u64,
    pub notification_enabled: bool,
    pub background_monitoring: bool,
    pub autostart_enabled: bool,
    pub theme: String,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            health_interval_seconds: 10,
            chain_interval_seconds: 30,
            peer_interval_seconds: 60,
            raw_retention_days: 30,
            aggregate_retention_days: 365,
            lag_warning_blocks: 5,
            lag_critical_blocks: 20,
            latency_warning_ms: 2_000,
            notification_enabled: true,
            background_monitoring: false,
            autostart_enabled: false,
            theme: "system".into(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AlertRecord {
    pub id: String,
    pub node_id: String,
    pub node_name: String,
    pub alert_type: String,
    pub severity: String,
    pub state: String,
    pub message: String,
    pub raised_at: i64,
    pub updated_at: i64,
    pub acknowledged_at: Option<i64>,
    pub resolved_at: Option<i64>,
    pub occurrence_count: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanProgress {
    pub scanned: usize,
    pub total: usize,
    pub found: usize,
}
