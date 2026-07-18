use std::collections::{HashMap, HashSet};

use crate::{
    domain::{MonitorMode, NodeStatus, TopologyEdge, TopologyNode, TopologySnapshot},
    error::AppResult,
    state::AppState,
};

pub async fn snapshot(state: &AppState) -> AppResult<TopologySnapshot> {
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
