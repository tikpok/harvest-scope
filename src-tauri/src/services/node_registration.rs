use tauri::{AppHandle, Emitter};

use crate::{
    adapters::{
        adapter_for, api_probe_candidates, normalize_api_registration_target,
        normalize_peer_endpoint, probe_endpoint, require_insecure_http_approval,
    },
    domain::{MonitorMode, NodeIdentity, NodeKind, NodeSummary, UpsertNodeInput},
    error::{AppError, AppResult},
    state::AppState,
    storage::NodeUpsert,
};

use super::monitor;

const MAX_NODE_COUNT: usize = 50;

pub(crate) fn node_limit_reached(existing_count: usize) -> bool {
    existing_count >= MAX_NODE_COUNT
}

pub(crate) async fn lock_node_registration(
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
                    adapter_for(kind, candidate.clone(), state.transport.clone())
                        .probe()
                        .await
                }
                None => probe_endpoint(&candidate, state.transport.clone()).await,
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
                    allow_insecure_http: node.allow_insecure_http,
                    supernode_id: node.supernode_id,
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

pub async fn upsert_node_from_input(
    app: AppHandle,
    state: AppState,
    input: UpsertNodeInput,
    enforce_limit: bool,
) -> AppResult<NodeSummary> {
    let monitor_mode = input.monitor_mode.unwrap_or(MonitorMode::Api);
    let base_url = match monitor_mode {
        MonitorMode::Api => normalize_api_registration_target(&input.base_url)?,
        MonitorMode::Peer => normalize_peer_endpoint(&input.base_url)?,
    };
    let allow_insecure_http = input.allow_insecure_http.unwrap_or(false);
    if monitor_mode == MonitorMode::Api {
        require_insecure_http_approval(&base_url, allow_insecure_http)?;
    }
    let is_new = input.id.is_none();
    let _registration_guard = if enforce_limit {
        lock_node_registration(&state, is_new).await?
    } else {
        None
    };
    if monitor_mode == MonitorMode::Peer
        && input
            .roles
            .as_ref()
            .is_some_and(|roles| roles.iter().any(|role| role.eq_ignore_ascii_case("Voting")))
        && input
            .public_key
            .as_ref()
            .is_none_or(|value| value.trim().is_empty())
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
            allow_insecure_http,
            supernode_id: input.supernode_id,
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
