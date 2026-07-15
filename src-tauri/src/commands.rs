use std::{
    collections::{HashMap, HashSet},
    sync::atomic::Ordering,
    time::Duration,
};

use tauri::{AppHandle, Emitter, State};
use tauri_plugin_autostart::ManagerExt;
use tauri_plugin_dialog::DialogExt;

use crate::{
    adapters::{
        adapter_for, api_probe_candidates, normalize_api_registration_target, normalize_base_url,
        normalize_peer_endpoint, probe_endpoint,
    },
    domain::{
        AlertRecord, AppSettings, DashboardData, DiagnosticRun, HarvestEvent, MetricPoint,
        MonitorMode, NetworkEvent, NetworkInterfaceInfo, NodeConfigEntry, NodeConfigFile,
        NodeConfigImportFailure, NodeConfigImportResult, NodeDetail, NodeIdentity, NodeKind,
        NodeStatus, NodeSummary, RefreshAllResult, ScanCandidate, ScanRequest, TopologyEdge,
        TopologyNode, TopologySnapshot, UpsertNodeInput,
    },
    error::{AppError, AppResult},
    services::{desktop, diagnostic as diagnostic_service, discovery, monitor},
    state::AppState,
    storage::NodeUpsert,
};

const NODE_CONFIG_FORMAT_VERSION: u32 = 1;
const MAX_NODE_COUNT: usize = 50;
const NODE_PURGE_QUIET_PERIOD: Duration = Duration::from_secs(30);

#[derive(Debug, Default, PartialEq, Eq)]
struct DashboardStatusCounts {
    healthy: usize,
    warning: usize,
    critical: usize,
    offline: usize,
    unknown: usize,
}

impl DashboardStatusCounts {
    fn total(&self) -> usize {
        self.healthy + self.warning + self.critical + self.offline + self.unknown
    }
}

fn dashboard_status_counts<'a>(
    statuses: impl Iterator<Item = &'a NodeStatus>,
) -> DashboardStatusCounts {
    let mut counts = DashboardStatusCounts::default();
    for status in statuses {
        match status {
            NodeStatus::Healthy => counts.healthy += 1,
            NodeStatus::Warning => counts.warning += 1,
            NodeStatus::Critical => counts.critical += 1,
            NodeStatus::Offline => counts.offline += 1,
            NodeStatus::Unknown => counts.unknown += 1,
        }
    }
    counts
}

fn node_limit_reached(existing_count: usize) -> bool {
    existing_count >= MAX_NODE_COUNT
}

async fn lock_node_registration(
    state: &AppState,
    is_new: bool,
) -> AppResult<Option<tokio::sync::MutexGuard<'_, ()>>> {
    if !is_new {
        return Ok(None);
    }
    let guard = state.node_registration_lock.lock().await;
    if node_limit_reached(state.repository.list_stored_nodes(false).await?.len()) {
        return Err(AppError::Configuration(
            "監視できるノードは最大50台です".into(),
        ));
    }
    Ok(Some(guard))
}

fn deletion_purge_is_current(scheduled_generation: u64, current_generation: u64) -> bool {
    scheduled_generation == current_generation
}

pub(crate) fn schedule_deleted_node_purge(state: AppState, scheduled_generation: u64) {
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(NODE_PURGE_QUIET_PERIOD).await;
        if !deletion_purge_is_current(
            scheduled_generation,
            state.current_node_deletion_generation(),
        ) {
            return;
        }

        for attempt in 1..=3 {
            if !deletion_purge_is_current(
                scheduled_generation,
                state.current_node_deletion_generation(),
            ) {
                return;
            }
            match state.repository.purge_deleted_nodes().await {
                Ok(_) => break,
                Err(error) if attempt < 3 => {
                    tracing::warn!(%error, attempt, "deferred node purge retrying");
                    tokio::time::sleep(Duration::from_secs(1)).await;
                }
                Err(error) => {
                    tracing::warn!(%error, "deferred node purge failed");
                }
            }
        }
    });
}

#[tauri::command]
pub async fn list_nodes(state: State<'_, AppState>) -> AppResult<Vec<NodeSummary>> {
    state.repository.list_nodes().await
}

#[tauri::command]
pub async fn get_dashboard(state: State<'_, AppState>) -> AppResult<DashboardData> {
    let nodes = state.repository.list_nodes().await?;
    let counts = dashboard_status_counts(nodes.iter().map(|node| &node.status));
    Ok(DashboardData {
        total: counts.total(),
        healthy: counts.healthy,
        warning: counts.warning,
        critical: counts.critical,
        offline: counts.offline,
        unknown: counts.unknown,
        rechecking: state.is_all_nodes_rechecking(),
        nodes,
    })
}

#[tauri::command]
pub async fn get_topology(state: State<'_, AppState>) -> AppResult<TopologySnapshot> {
    let summaries = state
        .repository
        .list_nodes()
        .await?
        .into_iter()
        .filter(|node| node.enabled)
        .collect::<Vec<_>>();
    let stored = state.repository.list_stored_nodes(false).await?;
    let connections = state.repository.list_peer_connections().await?;
    let health_by_node = state.repository.latest_topology_health().await?;
    let websocket_by_node = state
        .runtime
        .lock()
        .await
        .iter()
        .map(|(id, runtime)| (id.clone(), runtime.websocket_connected))
        .collect::<HashMap<_, _>>();

    let stored_by_id = stored
        .iter()
        .map(|node| (node.id.clone(), node))
        .collect::<HashMap<_, _>>();
    let public_keys = stored
        .iter()
        .filter_map(|node| {
            node.public_key
                .as_ref()
                .map(|key| (key.to_ascii_lowercase(), node.id.clone()))
        })
        .collect::<HashMap<_, _>>();
    let mut host_candidates: HashMap<String, Vec<String>> = HashMap::new();
    for node in &stored {
        if let Ok(url) = url::Url::parse(&node.base_url) {
            if let Some(host) = url.host_str() {
                host_candidates
                    .entry(host.to_ascii_lowercase())
                    .or_default()
                    .push(node.id.clone());
            }
        }
    }

    let mut nodes = summaries
        .into_iter()
        .map(|node| {
            let stored = stored_by_id.get(&node.id);
            let parsed_url = stored.and_then(|stored| url::Url::parse(&stored.base_url).ok());
            let host = parsed_url
                .as_ref()
                .and_then(|url| url.host_str().map(str::to_owned));
            let port = parsed_url
                .as_ref()
                .and_then(url::Url::port_or_known_default);
            let health = health_by_node.get(&node.id).cloned().unwrap_or_default();
            TopologyNode {
                websocket_connected: websocket_by_node.get(&node.id).copied().unwrap_or(false),
                version: stored.and_then(|stored| stored.version.clone()),
                roles: if node.roles.is_empty() {
                    node.observed_roles.clone()
                } else {
                    node.roles.clone()
                },
                id: node.id,
                label: node.display_name,
                kind: node.kind,
                monitor_mode: node.monitor_mode,
                network: node.network,
                status: node.status,
                registered: true,
                enabled: node.enabled,
                host,
                port,
                public_key: stored.and_then(|stored| stored.public_key.clone()),
                peer_status: None,
                api_status: health.api_status,
                db_status: health.db_status,
                nis_status_code: health.nis_status_code,
                height: node.height,
                finalized_height: node.finalized_height,
                lag: node.lag,
                show_harvest_metrics: node.show_harvest_metrics,
                delegated_harvester_count: node.delegated_harvester_count,
                latest_harvest_at: node.latest_harvest_at,
            }
        })
        .collect::<Vec<_>>();
    let mut external_nodes = HashSet::new();
    let mut edge_ids = HashSet::new();
    let mut edges = Vec::new();

    for (source, peer) in connections {
        let Some(source_node) = stored_by_id.get(&source) else {
            continue;
        };
        if !source_node.enabled {
            continue;
        }
        let target = public_keys
            .get(&peer.key.to_ascii_lowercase())
            .cloned()
            .or_else(|| {
                peer.host.as_ref().and_then(|host| {
                    let candidates = host_candidates.get(&host.to_ascii_lowercase())?;
                    (candidates.len() == 1).then(|| candidates[0].clone())
                })
            })
            .unwrap_or_else(|| {
                format!(
                    "external:{}:{}",
                    source_node.kind.as_str(),
                    peer.key.to_ascii_lowercase()
                )
            });
        if target == source {
            continue;
        }
        if target.starts_with("external:") && external_nodes.insert(target.clone()) {
            let label = peer
                .name
                .clone()
                .or_else(|| peer.host.clone())
                .unwrap_or_else(|| peer.key.chars().take(12).collect());
            nodes.push(TopologyNode {
                id: target.clone(),
                label,
                kind: source_node.kind,
                monitor_mode: MonitorMode::Peer,
                network: source_node.network.clone(),
                status: NodeStatus::Unknown,
                registered: false,
                enabled: true,
                host: peer.host.clone(),
                port: peer.port,
                public_key: Some(peer.key.clone()),
                version: peer.version.clone(),
                roles: peer.roles.clone(),
                peer_status: Some(peer.status.clone()),
                websocket_connected: false,
                api_status: None,
                db_status: None,
                nis_status_code: None,
                height: None,
                finalized_height: None,
                lag: None,
                show_harvest_metrics: false,
                delegated_harvester_count: None,
                latest_harvest_at: None,
            });
        }
        let (left, right) = if source <= target {
            (source.clone(), target.clone())
        } else {
            (target.clone(), source.clone())
        };
        let edge_id = format!("{left}|{right}");
        if edge_ids.insert(edge_id.clone()) {
            edges.push(TopologyEdge {
                id: edge_id,
                source: left,
                target: right,
                status: peer.status,
            });
        }
    }

    Ok(TopologySnapshot {
        nodes,
        edges,
        observed_at: chrono::Utc::now().timestamp_millis(),
    })
}

#[tauri::command]
pub async fn get_recent_network_events(
    state: State<'_, AppState>,
    limit: Option<usize>,
) -> AppResult<Vec<NetworkEvent>> {
    let events = state.network_events.lock().await;
    let limit = limit.unwrap_or(50).min(200);
    Ok(events.iter().rev().take(limit).cloned().collect())
}

#[tauri::command]
pub async fn get_recent_harvest_events(
    state: State<'_, AppState>,
    limit: Option<usize>,
) -> AppResult<Vec<HarvestEvent>> {
    state
        .repository
        .recent_harvest_events(limit.unwrap_or(50))
        .await
}

#[tauri::command]
pub async fn upsert_node(
    app: AppHandle,
    state: State<'_, AppState>,
    input: UpsertNodeInput,
) -> AppResult<NodeSummary> {
    upsert_node_from_input(app, state.inner().clone(), input).await
}

fn normalized_roles(input: Option<Vec<String>>, monitor_mode: MonitorMode) -> Vec<String> {
    let mut roles = input
        .unwrap_or_else(|| {
            if monitor_mode == MonitorMode::Peer {
                vec!["Peer".into()]
            } else {
                Vec::new()
            }
        })
        .into_iter()
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
        .collect::<Vec<_>>();
    if monitor_mode == MonitorMode::Peer
        && !roles.iter().any(|role| role.eq_ignore_ascii_case("Peer"))
    {
        roles.insert(0, "Peer".into());
    }
    roles.sort_by_key(|value| value.to_ascii_lowercase());
    roles.dedup_by(|left, right| left.eq_ignore_ascii_case(right));
    roles
}

fn provisional_identity(input: &UpsertNodeInput, monitor_mode: MonitorMode) -> NodeIdentity {
    let roles = normalized_roles(input.roles.clone(), monitor_mode);
    NodeIdentity {
        kind: input.kind.unwrap_or(NodeKind::Symbol),
        name: input
            .display_name
            .clone()
            .filter(|value| !value.trim().is_empty()),
        network: input
            .network
            .clone()
            .filter(|value| !value.trim().is_empty()),
        version: input
            .version
            .clone()
            .filter(|value| !value.trim().is_empty()),
        public_key: input
            .public_key
            .clone()
            .map(|value| value.trim().to_ascii_uppercase())
            .filter(|value| !value.is_empty()),
        roles,
        rest_version: None,
    }
}

async fn complete_node_registration(
    app: AppHandle,
    state: AppState,
    node_id: String,
    kind_hint: Option<NodeKind>,
) {
    let mut node = match state.repository.get_stored_node(&node_id).await {
        Ok(node) => node,
        Err(error) => {
            tracing::warn!(%error, node_id, "registered node disappeared before initial monitor");
            return;
        }
    };
    if node.monitor_mode == MonitorMode::Api {
        let mut discovered = None;
        for candidate in api_probe_candidates(&node.base_url).unwrap_or_default() {
            let identity_result = match kind_hint {
                Some(kind) => {
                    adapter_for(kind, candidate.clone(), state.client.clone())
                        .probe()
                        .await
                }
                None => probe_endpoint(&candidate, state.client.clone()).await,
            };
            if let Ok(identity) = identity_result {
                discovered = Some((candidate, identity));
                break;
            }
        }
        if let Some((discovered_base_url, identity)) = discovered {
            let display_name = if node.display_name == node.base_url {
                identity
                    .name
                    .clone()
                    .filter(|value| !value.trim().is_empty())
                    .unwrap_or_else(|| node.display_name.clone())
            } else {
                node.display_name.clone()
            };
            if let Err(error) = state
                .repository
                .upsert_node_with_mode(NodeUpsert {
                    id: &node.id,
                    base_url: &discovered_base_url,
                    display_name: &display_name,
                    identity: &identity,
                    monitor_mode: node.monitor_mode,
                    enabled: node.enabled,
                    favorite: node.favorite,
                })
                .await
            {
                tracing::warn!(%error, node_id = %node.id, "registered node identity update failed");
            } else if let Ok(updated) = state.repository.get_stored_node(&node.id).await {
                node = updated;
                let _ = app.emit("node-updated", &node.id);
                let _ = app.emit("topology-updated", &node.id);
            }
        }
    }
    if let Err(error) = monitor::monitor_node(app, state, node, true).await {
        tracing::warn!(%error, node_id, "initial node monitor failed");
    }
}

async fn upsert_node_from_input(
    app: AppHandle,
    state: AppState,
    input: UpsertNodeInput,
) -> AppResult<NodeSummary> {
    let monitor_mode = input.monitor_mode.unwrap_or(MonitorMode::Api);
    let base_url = match monitor_mode {
        MonitorMode::Api => normalize_api_registration_target(&input.base_url)?,
        MonitorMode::Peer => normalize_peer_endpoint(&input.base_url)?,
    };
    let is_new = input.id.is_none();
    let _registration_guard = lock_node_registration(&state, is_new).await?;
    if monitor_mode == MonitorMode::Peer
        && input
            .roles
            .as_ref()
            .is_some_and(|roles| roles.iter().any(|role| role.eq_ignore_ascii_case("Voting")))
        && input
            .public_key
            .as_ref()
            .map_or(true, |value| value.trim().is_empty())
    {
        return Err(AppError::Configuration(
            "Votingノードの登録には公開鍵が必要です".into(),
        ));
    }
    let identity = provisional_identity(&input, monitor_mode);
    let id = input.id.unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
    let display_name = input
        .display_name
        .filter(|value| !value.trim().is_empty())
        .or(identity.name.clone())
        .unwrap_or_else(|| base_url.clone());

    state
        .repository
        .upsert_node_with_mode(NodeUpsert {
            id: &id,
            base_url: &base_url,
            display_name: &display_name,
            identity: &identity,
            monitor_mode,
            enabled: input.enabled.unwrap_or(true),
            favorite: input.favorite.unwrap_or(false),
        })
        .await?;
    let app_for_monitor = app.clone();
    let state_for_monitor = state.clone();
    let id_for_monitor = id.clone();
    let kind_hint = input.kind;
    tauri::async_runtime::spawn(async move {
        complete_node_registration(
            app_for_monitor,
            state_for_monitor,
            id_for_monitor,
            kind_hint,
        )
        .await;
    });
    let summary = state.repository.get_node_summary(&id).await?;
    let _ = app.emit("node-updated", &summary.id);
    let _ = app.emit("topology-updated", &summary.id);
    Ok(summary)
}

fn config_entry_from_node(node: crate::domain::StoredNode) -> NodeConfigEntry {
    NodeConfigEntry {
        display_name: Some(node.display_name),
        kind: Some(node.kind),
        monitor_mode: Some(node.monitor_mode),
        base_url: node.base_url,
        network: node.network,
        version: node.version,
        public_key: node.public_key,
        roles: Some(node.roles),
        enabled: Some(node.enabled),
        favorite: Some(node.favorite),
        show_harvest_metrics: Some(node.show_harvest_metrics),
    }
}

fn node_config_file_from_nodes(nodes: Vec<crate::domain::StoredNode>) -> NodeConfigFile {
    NodeConfigFile {
        format_version: NODE_CONFIG_FORMAT_VERSION,
        exported_at: chrono::Utc::now().to_rfc3339(),
        app_version: env!("CARGO_PKG_VERSION").into(),
        nodes: nodes.into_iter().map(config_entry_from_node).collect(),
    }
}

#[tauri::command]
pub async fn export_node_config(
    app: AppHandle,
    state: State<'_, AppState>,
) -> AppResult<Option<String>> {
    let file = node_config_file_from_nodes(state.repository.list_stored_nodes(false).await?);
    let bytes = serde_json::to_vec_pretty(&file)?;
    let file_name = format!(
        "harvest-scope-config-{}.json",
        chrono::Local::now().format("%Y%m%d-%H%M%S")
    );
    let path = app
        .dialog()
        .file()
        .add_filter("JSON", &["json"])
        .set_file_name(file_name)
        .blocking_save_file();
    let Some(path) = path else {
        return Ok(None);
    };
    let path = path
        .into_path()
        .map_err(|error| AppError::Io(std::io::Error::other(error)))?;
    std::fs::write(&path, bytes)?;
    Ok(Some(path.to_string_lossy().into_owned()))
}

#[tauri::command]
pub async fn import_node_config(
    app: AppHandle,
    state: State<'_, AppState>,
) -> AppResult<Option<NodeConfigImportResult>> {
    let path = app
        .dialog()
        .file()
        .add_filter("JSON", &["json"])
        .blocking_pick_file();
    let Some(path) = path else {
        return Ok(None);
    };
    let path = path
        .into_path()
        .map_err(|error| AppError::Io(std::io::Error::other(error)))?;
    let bytes = std::fs::read(&path)?;
    let config: NodeConfigFile = serde_json::from_slice(&bytes)?;
    if config.format_version != NODE_CONFIG_FORMAT_VERSION {
        return Err(AppError::Configuration(format!(
            "未対応のノード設定形式です: {}",
            config.format_version
        )));
    }
    import_node_config_entries(app, state.inner().clone(), config.nodes).await
}

async fn import_node_config_entries(
    app: AppHandle,
    state: AppState,
    entries: Vec<NodeConfigEntry>,
) -> AppResult<Option<NodeConfigImportResult>> {
    let mut result = NodeConfigImportResult::default();
    let mut existing = state.repository.list_stored_nodes(false).await?;
    let mut existing_by_url = existing
        .iter()
        .filter_map(|node| {
            match node.monitor_mode {
                MonitorMode::Api => normalize_base_url(&node.base_url),
                MonitorMode::Peer => normalize_peer_endpoint(&node.base_url),
            }
            .ok()
            .map(|url| (url, node.clone()))
        })
        .collect::<HashMap<_, _>>();
    let mut total_nodes = existing.len();

    for entry in entries {
        let monitor_mode = entry.monitor_mode.unwrap_or(MonitorMode::Api);
        let normalized = match match monitor_mode {
            MonitorMode::Api => normalize_base_url(&entry.base_url),
            MonitorMode::Peer => normalize_peer_endpoint(&entry.base_url),
        } {
            Ok(value) => value,
            Err(error) => {
                result.failed += 1;
                result.failures.push(NodeConfigImportFailure {
                    base_url: Some(entry.base_url),
                    display_name: entry.display_name,
                    reason: error.to_string(),
                });
                continue;
            }
        };
        let existing_node = existing_by_url.get(&normalized).cloned();
        if existing_node.is_none() && total_nodes >= MAX_NODE_COUNT {
            result.failed += 1;
            result.failures.push(NodeConfigImportFailure {
                base_url: Some(normalized),
                display_name: entry.display_name,
                reason: "監視できるノードは最大50台です".into(),
            });
            continue;
        }
        let existing_id = existing_node.as_ref().map(|node| node.id.clone());
        let show_harvest_metrics = entry.show_harvest_metrics.unwrap_or_else(|| {
            existing_node
                .as_ref()
                .is_some_and(|node| node.show_harvest_metrics)
        });
        let input = UpsertNodeInput {
            id: existing_id.clone(),
            display_name: entry.display_name.clone(),
            base_url: normalized.clone(),
            kind: entry.kind,
            monitor_mode: Some(monitor_mode),
            network: entry.network,
            version: entry.version,
            public_key: entry.public_key,
            roles: entry.roles,
            enabled: entry
                .enabled
                .or_else(|| existing_node.as_ref().map(|node| node.enabled)),
            favorite: entry
                .favorite
                .or_else(|| existing_node.as_ref().map(|node| node.favorite)),
        };
        match upsert_node_from_input(app.clone(), state.clone(), input).await {
            Ok(node) => {
                if let Err(error) = state
                    .repository
                    .set_harvest_metrics_visible(&node.id, show_harvest_metrics)
                    .await
                {
                    result.failed += 1;
                    result.failures.push(NodeConfigImportFailure {
                        base_url: Some(normalized),
                        display_name: Some(node.display_name),
                        reason: error.to_string(),
                    });
                    continue;
                }
                if existing_id.is_some() {
                    result.updated += 1;
                } else {
                    result.added += 1;
                }
            }
            Err(error) => {
                result.failed += 1;
                result.failures.push(NodeConfigImportFailure {
                    base_url: Some(normalized),
                    display_name: entry.display_name,
                    reason: error.to_string(),
                });
            }
        }
        existing = state.repository.list_stored_nodes(false).await?;
        existing_by_url = existing
            .iter()
            .filter_map(|node| {
                match node.monitor_mode {
                    MonitorMode::Api => normalize_base_url(&node.base_url),
                    MonitorMode::Peer => normalize_peer_endpoint(&node.base_url),
                }
                .ok()
                .map(|url| (url, node.clone()))
            })
            .collect();
        total_nodes = existing.len();
    }
    Ok(Some(result))
}

#[tauri::command]
pub async fn delete_node(
    app: AppHandle,
    state: State<'_, AppState>,
    node_id: String,
) -> AppResult<()> {
    state.repository.mark_node_for_deletion(&node_id).await?;
    let deletion_generation = state.note_node_deletion();
    state.runtime.lock().await.remove(&node_id);
    let _ = app.emit("node-updated", &node_id);
    let _ = app.emit("topology-updated", &node_id);
    schedule_deleted_node_purge(state.inner().clone(), deletion_generation);
    let app_for_tray = app.clone();
    let state_for_tray = state.inner().clone();
    tauri::async_runtime::spawn(async move {
        desktop::refresh_tray(&app_for_tray, &state_for_tray).await;
    });
    Ok(())
}

#[tauri::command]
pub async fn move_node(
    state: State<'_, AppState>,
    node_id: String,
    direction: String,
) -> AppResult<()> {
    let offset = match direction.as_str() {
        "up" => -1,
        "down" => 1,
        _ => {
            return Err(AppError::Configuration(
                "並び替え方向はupまたはdownを指定してください".into(),
            ))
        }
    };
    state.repository.move_node(&node_id, offset).await
}

#[tauri::command]
pub async fn pause_node(
    app: AppHandle,
    state: State<'_, AppState>,
    node_id: String,
) -> AppResult<()> {
    state.repository.set_enabled(&node_id, false).await?;
    let _ = app.emit("node-updated", &node_id);
    let _ = app.emit("topology-updated", &node_id);
    let app_for_tray = app.clone();
    let state_for_tray = state.inner().clone();
    tauri::async_runtime::spawn(async move {
        desktop::refresh_tray(&app_for_tray, &state_for_tray).await;
    });
    Ok(())
}

#[tauri::command]
pub async fn resume_node(
    app: AppHandle,
    state: State<'_, AppState>,
    node_id: String,
) -> AppResult<()> {
    state.repository.set_enabled(&node_id, true).await?;
    let node = state.repository.get_stored_node(&node_id).await?;
    let _ = app.emit("node-updated", &node_id);
    let _ = app.emit("topology-updated", &node_id);
    let state = state.inner().clone();
    let app_for_tray = app.clone();
    let state_for_tray = state.clone();
    tauri::async_runtime::spawn(async move {
        desktop::refresh_tray(&app_for_tray, &state_for_tray).await;
    });
    tauri::async_runtime::spawn(async move {
        let _ = monitor::monitor_node(app, state, node, true).await;
    });
    Ok(())
}

#[tauri::command]
pub async fn set_node_harvest_metrics_visible(
    app: AppHandle,
    state: State<'_, AppState>,
    node_id: String,
    visible: bool,
) -> AppResult<()> {
    state
        .repository
        .set_harvest_metrics_visible(&node_id, visible)
        .await?;
    let _ = app.emit("node-updated", &node_id);
    let _ = app.emit("topology-updated", &node_id);
    Ok(())
}

#[tauri::command]
pub async fn refresh_node(
    app: AppHandle,
    state: State<'_, AppState>,
    node_id: String,
) -> AppResult<()> {
    let node = state.repository.get_stored_node(&node_id).await?;
    monitor::monitor_node(app, state.inner().clone(), node, true).await
}

#[tauri::command]
pub async fn refresh_all_nodes(
    app: AppHandle,
    state: State<'_, AppState>,
) -> AppResult<RefreshAllResult> {
    // A user clicking this button is definitive proof that Windows is awake.
    // This also recovers if a resume notification was missed by the OS.
    state.set_system_suspended(false);
    monitor::refresh_all_nodes(app, state.inner().clone()).await
}

#[tauri::command]
pub async fn get_node_detail(state: State<'_, AppState>, node_id: String) -> AppResult<NodeDetail> {
    let connected = state
        .runtime
        .lock()
        .await
        .get(&node_id)
        .map(|runtime| runtime.websocket_connected)
        .unwrap_or(false);
    state.repository.node_detail(&node_id, connected).await
}

#[tauri::command]
pub async fn query_metrics(
    state: State<'_, AppState>,
    node_id: String,
    from: i64,
    to: i64,
) -> AppResult<Vec<MetricPoint>> {
    if from > to {
        return Err(AppError::Configuration("期間指定が正しくありません".into()));
    }
    state.repository.query_metrics(&node_id, from, to).await
}

#[tauri::command]
pub fn list_network_interfaces() -> Vec<NetworkInterfaceInfo> {
    discovery::network_interfaces()
}

#[tauri::command]
pub async fn scan_subnet(
    app: AppHandle,
    state: State<'_, AppState>,
    request: ScanRequest,
) -> AppResult<Vec<ScanCandidate>> {
    discovery::scan(app, state.inner().clone(), request).await
}

#[tauri::command]
pub fn cancel_scan(state: State<'_, AppState>) {
    state.scan_cancelled.store(true, Ordering::Relaxed);
}

#[tauri::command]
pub async fn list_alerts(state: State<'_, AppState>) -> AppResult<Vec<AlertRecord>> {
    state.repository.list_alerts().await
}

#[tauri::command]
pub async fn get_alert(state: State<'_, AppState>, alert_id: String) -> AppResult<AlertRecord> {
    state.repository.get_alert(&alert_id).await
}

#[tauri::command]
pub async fn acknowledge_alert(state: State<'_, AppState>, alert_id: String) -> AppResult<()> {
    state.repository.acknowledge_alert(&alert_id).await
}

#[tauri::command]
pub async fn delete_resolved_alerts(state: State<'_, AppState>) -> AppResult<u64> {
    state.repository.delete_resolved_alerts().await
}

#[tauri::command]
pub async fn run_node_diagnostic(
    state: State<'_, AppState>,
    node_id: String,
    alert_id: Option<String>,
) -> AppResult<DiagnosticRun> {
    diagnostic_service::run(state.inner().clone(), node_id, alert_id).await
}

#[tauri::command]
pub async fn get_diagnostic_run(
    state: State<'_, AppState>,
    run_id: String,
) -> AppResult<DiagnosticRun> {
    state.repository.get_diagnostic_run(&run_id).await
}

#[tauri::command]
pub async fn list_node_diagnostics(
    state: State<'_, AppState>,
    node_id: String,
    limit: Option<usize>,
) -> AppResult<Vec<DiagnosticRun>> {
    state
        .repository
        .list_node_diagnostics(&node_id, limit.unwrap_or(20))
        .await
}

#[tauri::command]
pub async fn take_pending_navigation(state: State<'_, AppState>) -> AppResult<Option<String>> {
    Ok(state.pending_navigation.lock().await.take())
}

#[tauri::command]
pub async fn get_settings(state: State<'_, AppState>) -> AppResult<AppSettings> {
    Ok(state.settings.read().await.clone())
}

fn validate_settings(settings: &AppSettings) -> AppResult<()> {
    if !(5..=300).contains(&settings.health_interval_seconds)
        || !(10..=600).contains(&settings.chain_interval_seconds)
        || !(30..=1800).contains(&settings.peer_interval_seconds)
        || settings.lag_warning_blocks == 0
        || settings.lag_critical_blocks <= settings.lag_warning_blocks
        || !(250..=30_000).contains(&settings.latency_warning_ms)
        || !(1..=90).contains(&settings.raw_retention_days)
        || !(30..=1_825).contains(&settings.aggregate_retention_days)
        || !matches!(settings.theme.as_str(), "system" | "light" | "dark")
    {
        return Err(AppError::Configuration("設定値が許容範囲外です".into()));
    }
    Ok(())
}

#[tauri::command]
pub async fn update_settings(
    app: AppHandle,
    state: State<'_, AppState>,
    settings: AppSettings,
) -> AppResult<AppSettings> {
    validate_settings(&settings)?;
    if settings.autostart_enabled {
        app.autolaunch()
            .enable()
            .map_err(|error| AppError::Configuration(error.to_string()))?;
    } else {
        app.autolaunch()
            .disable()
            .map_err(|error| AppError::Configuration(error.to_string()))?;
    }
    state.repository.save_settings(&settings).await?;
    state.set_background_monitoring(settings.background_monitoring);
    *state.settings.write().await = settings.clone();
    desktop::refresh_tray(&app, &state).await;
    Ok(settings)
}

fn csv_bytes(records: Vec<Vec<String>>) -> AppResult<Vec<u8>> {
    let mut bytes = vec![0xEF, 0xBB, 0xBF];
    {
        let mut writer = csv::Writer::from_writer(&mut bytes);
        for record in records {
            writer.write_record(record)?;
        }
        writer.flush()?;
    }
    Ok(bytes)
}

#[tauri::command]
pub async fn export_csv(
    app: AppHandle,
    state: State<'_, AppState>,
    dataset: String,
    node_id: Option<String>,
) -> AppResult<Option<String>> {
    let records = match dataset.as_str() {
        "nodes" => {
            let mut rows = vec![vec![
                "名前".into(),
                "種別".into(),
                "URL".into(),
                "ネットワーク".into(),
                "状態".into(),
                "高さ".into(),
                "遅延".into(),
                "応答ms".into(),
                "委任者数".into(),
                "24時間増減".into(),
                "合計XYM(micro)".into(),
                "ノード証明書期限UTCms".into(),
                "ノード証明書残日数".into(),
                "投票キー終了epoch".into(),
                "投票キー残epoch".into(),
                "証明書取得エラー".into(),
                "Supernode ID".into(),
                "Supernode状態".into(),
                "Supernode最新ラウンド".into(),
                "Supernode最終確認UTCms".into(),
                "Supernode合格".into(),
                "Supernode失敗項目".into(),
                "Supernode取得エラー".into(),
            ]];
            rows.extend(
                state
                    .repository
                    .list_nodes()
                    .await?
                    .into_iter()
                    .map(|node| {
                        vec![
                            node.display_name,
                            node.kind.as_str().into(),
                            node.base_url,
                            node.network.unwrap_or_default(),
                            node.status.as_str().into(),
                            node.height.unwrap_or_default(),
                            node.lag.unwrap_or_default(),
                            node.latency_ms
                                .map(|value| value.to_string())
                                .unwrap_or_default(),
                            node.delegated_harvester_count
                                .map(|value| value.to_string())
                                .unwrap_or_default(),
                            node.delegated_harvester_delta
                                .map(|value| value.to_string())
                                .unwrap_or_default(),
                            node.total_balance_micro_xym.unwrap_or_default(),
                            node.node_certificate_not_after
                                .map(|value| value.to_string())
                                .unwrap_or_default(),
                            node.node_certificate_remaining_days
                                .map(|value| value.to_string())
                                .unwrap_or_default(),
                            node.voting_key_end_epoch
                                .map(|value| value.to_string())
                                .unwrap_or_default(),
                            node.voting_key_remaining_epochs
                                .map(|value| value.to_string())
                                .unwrap_or_default(),
                            node.certificate_metric_error.unwrap_or_default(),
                            node.supernode_id
                                .map(|value| value.to_string())
                                .unwrap_or_default(),
                            node.supernode_status.unwrap_or_default(),
                            node.supernode_last_round
                                .map(|value| value.to_string())
                                .unwrap_or_default(),
                            node.supernode_last_round_at
                                .map(|value| value.to_string())
                                .unwrap_or_default(),
                            node.supernode_checks_passed
                                .map(|value| value.to_string())
                                .unwrap_or_default(),
                            node.supernode_failed_checks.join("|"),
                            node.supernode_metric_error.unwrap_or_default(),
                        ]
                    }),
            );
            rows
        }
        "alerts" => {
            let mut rows = vec![vec![
                "ノード".into(),
                "種類".into(),
                "重大度".into(),
                "状態".into(),
                "メッセージ".into(),
                "発生時刻UTCms".into(),
                "更新時刻UTCms".into(),
            ]];
            rows.extend(
                state
                    .repository
                    .list_alerts()
                    .await?
                    .into_iter()
                    .map(|alert| {
                        vec![
                            alert.node_name,
                            alert.alert_type,
                            alert.severity,
                            alert.state,
                            alert.message,
                            alert.raised_at.to_string(),
                            alert.updated_at.to_string(),
                        ]
                    }),
            );
            rows
        }
        "metrics" => {
            let id = node_id.ok_or_else(|| {
                AppError::Configuration("メトリクス出力にはnodeIdが必要です".into())
            })?;
            let mut rows = vec![vec![
                "時刻UTCms".into(),
                "到達".into(),
                "高さ".into(),
                "ファイナライズ高".into(),
                "同期遅延".into(),
                "ピア数".into(),
                "応答ms".into(),
                "委任者数".into(),
                "24時間増減".into(),
                "合計XYM(micro)".into(),
                "合計importance".into(),
                "ノード証明書残日数".into(),
                "投票キー残epoch".into(),
                "Supernode合格".into(),
                "Supernode最新ラウンド".into(),
            ]];
            let metrics = state
                .repository
                .query_metrics(&id, 0, chrono::Utc::now().timestamp_millis())
                .await?;
            rows.extend(metrics.into_iter().map(|point| {
                vec![
                    point.observed_at.to_string(),
                    point.reachable.to_string(),
                    point.height.unwrap_or_default(),
                    point.finalized_height.unwrap_or_default(),
                    point.lag.unwrap_or_default(),
                    point
                        .peer_count
                        .map(|value| value.to_string())
                        .unwrap_or_default(),
                    point
                        .latency_ms
                        .map(|value| value.to_string())
                        .unwrap_or_default(),
                    point
                        .delegated_harvester_count
                        .map(|value| value.to_string())
                        .unwrap_or_default(),
                    point
                        .delegated_harvester_delta
                        .map(|value| value.to_string())
                        .unwrap_or_default(),
                    point.total_balance_micro_xym.unwrap_or_default(),
                    point.total_importance.unwrap_or_default(),
                    point
                        .node_certificate_remaining_days
                        .map(|value| value.to_string())
                        .unwrap_or_default(),
                    point
                        .voting_key_remaining_epochs
                        .map(|value| value.to_string())
                        .unwrap_or_default(),
                    point
                        .supernode_checks_passed
                        .map(|value| value.to_string())
                        .unwrap_or_default(),
                    point
                        .supernode_last_round
                        .map(|value| value.to_string())
                        .unwrap_or_default(),
                ]
            }));
            rows
        }
        _ => {
            return Err(AppError::Configuration(
                "未対応のCSVデータセットです".into(),
            ))
        }
    };
    let bytes = csv_bytes(records)?;
    let file_name = format!(
        "harvest-scope-{dataset}-{}.csv",
        chrono::Local::now().format("%Y%m%d-%H%M%S")
    );
    let path = app
        .dialog()
        .file()
        .add_filter("CSV", &["csv"])
        .set_file_name(file_name)
        .blocking_save_file();
    let Some(path) = path else {
        return Ok(None);
    };
    let path = path
        .into_path()
        .map_err(|error| AppError::Io(std::io::Error::other(error)))?;
    std::fs::write(&path, bytes)?;
    Ok(Some(path.to_string_lossy().into_owned()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_identity() -> NodeIdentity {
        NodeIdentity {
            kind: NodeKind::Symbol,
            name: None,
            network: Some("Symbol Mainnet".into()),
            version: None,
            public_key: None,
            roles: vec!["API".into()],
            rest_version: None,
        }
    }

    async fn insert_test_node(state: &AppState, index: usize) -> AppResult<()> {
        let id = format!("node-{index}");
        state
            .repository
            .upsert_node(
                &id,
                &format!("http://node-{index}.example:3000"),
                &format!("Node {index}"),
                &test_identity(),
                true,
                false,
            )
            .await
    }

    async fn register_test_node(state: AppState, index: usize) -> AppResult<()> {
        let _guard = lock_node_registration(&state, true).await?;
        insert_test_node(&state, index).await
    }

    #[test]
    fn dashboard_counts_include_unknown_nodes() {
        let counts = dashboard_status_counts(
            [
                NodeStatus::Healthy,
                NodeStatus::Healthy,
                NodeStatus::Critical,
                NodeStatus::Unknown,
                NodeStatus::Unknown,
            ]
            .iter(),
        );

        assert_eq!(counts.healthy, 2);
        assert_eq!(counts.warning, 0);
        assert_eq!(counts.critical, 1);
        assert_eq!(counts.offline, 0);
        assert_eq!(counts.unknown, 2);
        assert_eq!(counts.total(), 5);
    }

    #[test]
    fn node_limit_rejects_existing_limit_and_overage() {
        assert!(!node_limit_reached(49));
        assert!(node_limit_reached(50));
        assert!(node_limit_reached(51));
    }

    #[tokio::test]
    async fn concurrent_registrations_cannot_exceed_fifty_nodes() {
        let directory = tempfile::tempdir().expect("temporary directory should be created");
        let repository = crate::storage::Repository::open(&directory.path().join("nodes.db"))
            .await
            .expect("repository should open");
        let state = AppState::new(repository, reqwest::Client::new())
            .await
            .expect("state should initialize");
        for index in 0..49 {
            insert_test_node(&state, index)
                .await
                .expect("fixture node should be inserted");
        }

        let (first, second) = tokio::join!(
            register_test_node(state.clone(), 49),
            register_test_node(state.clone(), 50),
        );

        assert_eq!(usize::from(first.is_ok()) + usize::from(second.is_ok()), 1);
        assert_eq!(
            state
                .repository
                .list_stored_nodes(false)
                .await
                .expect("nodes should be listed")
                .len(),
            50
        );
    }

    #[tokio::test]
    async fn existing_overage_is_preserved_until_count_drops_below_limit() {
        let directory = tempfile::tempdir().expect("temporary directory should be created");
        let repository = crate::storage::Repository::open(&directory.path().join("nodes.db"))
            .await
            .expect("repository should open");
        let state = AppState::new(repository, reqwest::Client::new())
            .await
            .expect("state should initialize");
        for index in 0..51 {
            insert_test_node(&state, index)
                .await
                .expect("fixture node should be inserted");
        }

        assert!(lock_node_registration(&state, true).await.is_err());
        state
            .repository
            .mark_node_for_deletion("node-0")
            .await
            .expect("first node should be removed from monitoring");
        assert!(lock_node_registration(&state, true).await.is_err());
        state
            .repository
            .mark_node_for_deletion("node-1")
            .await
            .expect("second node should be removed from monitoring");
        register_test_node(state.clone(), 51)
            .await
            .expect("one node should fit after count drops to 49");

        assert_eq!(
            state
                .repository
                .list_stored_nodes(false)
                .await
                .expect("nodes should be listed")
                .len(),
            50
        );
        assert!(lock_node_registration(&state, true).await.is_err());
    }

    #[test]
    fn only_the_latest_deletion_generation_may_start_a_purge() {
        assert!(deletion_purge_is_current(3, 3));
        assert!(!deletion_purge_is_current(2, 3));
    }

    #[test]
    fn rejects_invalid_settings() {
        let settings = AppSettings {
            health_interval_seconds: 1,
            ..AppSettings::default()
        };
        assert!(validate_settings(&settings).is_err());
    }

    #[test]
    fn rejects_invalid_backend_only_settings() {
        let settings = AppSettings {
            latency_warning_ms: 0,
            ..AppSettings::default()
        };
        assert!(validate_settings(&settings).is_err());

        let settings = AppSettings {
            aggregate_retention_days: 2_000,
            ..AppSettings::default()
        };
        assert!(validate_settings(&settings).is_err());

        let settings = AppSettings {
            theme: "sepia".into(),
            ..AppSettings::default()
        };
        assert!(validate_settings(&settings).is_err());
    }

    #[test]
    fn exported_node_config_contains_portable_peer_metadata() {
        let file = node_config_file_from_nodes(vec![crate::domain::StoredNode {
            id: "node-a".into(),
            display_name: "Node A".into(),
            kind: crate::domain::NodeKind::Symbol,
            monitor_mode: crate::domain::MonitorMode::Api,
            base_url: "http://node-a:3000".into(),
            network: Some("Symbol Mainnet".into()),
            version: Some("1.0.0".into()),
            public_key: Some("SECRET-LIKE-PUBLIC-KEY".into()),
            roles: vec!["Peer".into(), "API".into()],
            rest_version: Some("2.4.0".into()),
            enabled: true,
            favorite: true,
            show_harvest_metrics: true,
        }]);
        let value = serde_json::to_value(file).expect("config should serialize");
        let node = &value["nodes"][0];

        assert_eq!(node["displayName"], "Node A");
        assert_eq!(node["kind"], "symbol");
        assert_eq!(node["monitorMode"], "api");
        assert_eq!(node["baseUrl"], "http://node-a:3000");
        assert_eq!(node["network"], "Symbol Mainnet");
        assert_eq!(node["version"], "1.0.0");
        assert_eq!(node["publicKey"], "SECRET-LIKE-PUBLIC-KEY");
        assert_eq!(node["roles"], serde_json::json!(["Peer", "API"]));
        assert_eq!(node["enabled"], true);
        assert_eq!(node["favorite"], true);
        assert_eq!(node["showHarvestMetrics"], true);
        assert!(node.get("id").is_none());
        assert!(node.get("restVersion").is_none());
    }
}
