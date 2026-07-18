use std::collections::HashMap;

use tauri::AppHandle;
use tauri_plugin_dialog::DialogExt;

use crate::{
    adapters::{normalize_base_url, normalize_peer_endpoint},
    domain::{
        MonitorMode, NodeConfigEntry, NodeConfigFile, NodeConfigImportFailure,
        NodeConfigImportResult, UpsertNodeInput,
    },
    error::{AppError, AppResult},
    state::AppState,
};

use super::node_registration;

const NODE_CONFIG_FORMAT_VERSION: u32 = 1;
const MAX_NODE_COUNT: usize = 50;

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
        supernode_id: node.supernode_id,
    }
}

pub(crate) fn node_config_file_from_nodes(nodes: Vec<crate::domain::StoredNode>) -> NodeConfigFile {
    NodeConfigFile {
        format_version: NODE_CONFIG_FORMAT_VERSION,
        exported_at: chrono::Utc::now().to_rfc3339(),
        app_version: env!("CARGO_PKG_VERSION").into(),
        nodes: nodes.into_iter().map(config_entry_from_node).collect(),
    }
}

pub async fn export(app: AppHandle, state: &AppState) -> AppResult<Option<String>> {
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

pub async fn import(app: AppHandle, state: &AppState) -> AppResult<Option<NodeConfigImportResult>> {
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
    import_node_config_entries(app, state.clone(), config.nodes).await
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
            allow_insecure_http: None,
            supernode_id: entry.supernode_id,
        };
        match node_registration::upsert_node_from_input(
            app.clone(),
            state.clone(),
            input,
            existing_id.is_none(),
        )
        .await
        {
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

#[cfg(test)]
mod tests {
    use super::*;

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
            allow_insecure_http: true,
            supernode_id: Some(6021),
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
        assert_eq!(node["supernodeId"], 6021);
        assert!(node.get("allowInsecureHttp").is_none());
        assert!(node.get("id").is_none());
        assert!(node.get("restVersion").is_none());
    }
}
