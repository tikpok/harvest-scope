use std::{
    collections::HashMap,
    time::{Duration, Instant},
};

use rand::Rng;
use tauri::{AppHandle, Emitter};

use crate::{
    adapters::adapter_for,
    domain::{NetworkEvent, NodeKind, NodeSample, NodeStatus, StoredNode},
    error::{AppError, AppResult},
    state::AppState,
};

use super::{desktop, websocket};

const RESUME_LATE_THRESHOLD_MS: i64 = 15_000;
const RESUME_GRACE_MS: i64 = 90_000;
const SUSPENDED_REQUEST_THRESHOLD: Duration = Duration::from_secs(10);

fn wake_was_late(expected_at: i64, actual_at: i64) -> bool {
    actual_at.saturating_sub(expected_at) >= RESUME_LATE_THRESHOLD_MS
}

fn latency_sample_is_slow(health_succeeded: bool, latency_ms: i64, warning_ms: u64) -> bool {
    health_succeeded && latency_ms as u64 >= warning_ms
}

async fn enter_resume_grace(state: &AppState, now: i64, reason: &str) {
    if !state.begin_resume_grace(now, RESUME_GRACE_MS) {
        return;
    }

    for runtime in state.runtime.lock().await.values_mut() {
        runtime.reset_after_resume(now);
    }
    state.state_hash_mismatches.lock().await.clear();
    tracing::info!(
        reason,
        "system resume detected; alert evaluation is temporarily paused"
    );
}

async fn update_alert(
    app: &AppHandle,
    state: &AppState,
    node: &StoredNode,
    active: bool,
    alert_type: &str,
    severity: &str,
    message: &str,
) -> AppResult<()> {
    let mutation = if active {
        let mutation = state
            .repository
            .raise_alert(&node.id, alert_type, severity, message)
            .await?;
        mutation.changed.then_some(mutation.alert_id)
    } else {
        state.repository.resolve_alert(&node.id, alert_type).await?
    };
    if let Some(alert_id) = mutation {
        let title = if active {
            format!(
                "{}: {}",
                if severity == "critical" {
                    "重大"
                } else {
                    "警告"
                },
                node.display_name
            )
        } else {
            format!("復旧: {}", node.display_name)
        };
        desktop::notify_alert(app.clone(), state.clone(), &title, message, alert_id).await;
        let _ = app.emit("alert-changed", &node.id);
        desktop::refresh_tray(app, state).await;
    }
    Ok(())
}

async fn monitor_node_inner(
    app: AppHandle,
    state: AppState,
    node: StoredNode,
    force: bool,
) -> AppResult<()> {
    let settings = state.settings.read().await.clone();
    let network_nodes = state
        .repository
        .list_stored_nodes(true)
        .await
        .unwrap_or_default();
    let now = chrono::Utc::now().timestamp_millis();
    let adapter = adapter_for(node.kind, node.base_url.clone(), state.client.clone());
    if let Some(spec) = adapter.websocket_capability() {
        websocket::ensure_started(app.clone(), state.clone(), node.id.clone(), spec).await;
    }
    let started = Instant::now();
    let health_result = adapter.fetch_health().await;
    let request_elapsed = started.elapsed();
    let latency_ms = request_elapsed.as_millis() as i64;
    let evaluation_now = chrono::Utc::now().timestamp_millis();
    if request_elapsed >= SUSPENDED_REQUEST_THRESHOLD {
        enter_resume_grace(&state, evaluation_now, "health request was suspended").await;
    }
    let resume_grace = state.in_resume_grace(evaluation_now);
    let mut poll_event = None;
    let mut topology_changed = false;
    let mut runtime_map = state.runtime.lock().await;
    let monitored_max = node.network.as_deref().and_then(|network| {
        network_nodes
            .iter()
            .filter(|candidate| candidate.network.as_deref() == Some(network))
            .filter_map(|candidate| {
                runtime_map
                    .get(&candidate.id)
                    .and_then(|value| value.chain.height)
            })
            .max()
    });
    let runtime = runtime_map.entry(node.id.clone()).or_default();
    let mut error = None;
    let health = match health_result {
        Ok(health) => {
            if resume_grace {
                runtime.consecutive_successes = 0;
                runtime.consecutive_failures = 0;
            } else {
                runtime.consecutive_successes += 1;
                runtime.consecutive_failures = 0;
            }
            Some(health)
        }
        Err(err) => {
            if resume_grace {
                runtime.consecutive_failures = 0;
                runtime.consecutive_successes = 0;
            } else {
                runtime.consecutive_failures += 1;
                runtime.consecutive_successes = 0;
            }
            error = Some(err.to_string());
            None
        }
    };
    let chain_due =
        force || now - runtime.last_chain_poll >= settings.chain_interval_seconds as i64 * 1_000;
    if health.is_some() && chain_due {
        match adapter.fetch_chain().await {
            Ok(chain) => {
                if runtime.chain.height.is_some()
                    && runtime.chain.height != chain.height
                    && (node.kind == NodeKind::Nis1 || !runtime.websocket_connected)
                {
                    poll_event = Some(NetworkEvent {
                        id: uuid::Uuid::new_v4().to_string(),
                        node_id: node.id.clone(),
                        kind: "pollUpdate".into(),
                        height: chain.height.map(|height| height.to_string()),
                        hash: None,
                        observed_at: now,
                        source: "polling".into(),
                    });
                }
                if chain.finalized_height != runtime.last_finalized_height {
                    runtime.last_finalized_height = chain.finalized_height;
                    runtime.finalized_changed_at = Some(now);
                    runtime.height_at_finalization_change = chain.height;
                }
                runtime.chain = chain;
                runtime.last_chain_poll = now;
            }
            Err(err) => error = Some(err.to_string()),
        }
    }
    let peers_due =
        force || now - runtime.last_peer_poll >= settings.peer_interval_seconds as i64 * 1_000;
    if health.is_some() && peers_due {
        match adapter.fetch_peers().await {
            Ok(peers) => match state.repository.replace_peers(&node.id, &peers, now).await {
                Ok(()) => {
                    topology_changed = runtime.peers != peers;
                    runtime.peers = peers;
                    runtime.last_peer_poll = now;
                }
                Err(err) => error = Some(err.to_string()),
            },
            Err(err) => error = Some(err.to_string()),
        }
    }
    let static_due = force || now - runtime.last_static_poll >= 15 * 60 * 1_000;
    if health.is_some() && static_due {
        if let Ok(identity) = adapter.fetch_static_info().await {
            match state
                .repository
                .upsert_node(
                    &node.id,
                    &node.base_url,
                    &node.display_name,
                    &identity,
                    node.enabled,
                    node.favorite,
                )
                .await
            {
                Ok(()) => runtime.last_static_poll = now,
                Err(err) => error = Some(err.to_string()),
            }
        }
    }
    runtime.chain.network_height = runtime
        .chain
        .network_height
        .into_iter()
        .chain(monitored_max)
        .max();
    let reachable = health.as_ref().is_some_and(|health| health.reachable);
    let lag = runtime
        .chain
        .network_height
        .zip(runtime.chain.height)
        .map(|(network, local)| network.saturating_sub(local));
    if lag.is_some_and(|value| value >= settings.lag_warning_blocks) {
        runtime.lag_warning_since.get_or_insert(now);
    } else {
        runtime.lag_warning_since = None;
    }
    if lag.is_some_and(|value| value >= settings.lag_critical_blocks) {
        runtime.lag_critical_since.get_or_insert(now);
    } else {
        runtime.lag_critical_since = None;
    }
    let lag_warning_active = runtime
        .lag_warning_since
        .is_some_and(|since| now - since >= 3 * 60 * 1_000);
    let lag_critical_active = runtime
        .lag_critical_since
        .is_some_and(|since| now - since >= 10 * 60 * 1_000);
    let health_degraded = health.as_ref().is_some_and(|health| !health.healthy);
    let status = if runtime.consecutive_failures >= 3 {
        NodeStatus::Offline
    } else if runtime.consecutive_failures > 0 || health_degraded {
        NodeStatus::Warning
    } else if lag_critical_active {
        NodeStatus::Critical
    } else if lag_warning_active || latency_ms as u64 >= settings.latency_warning_ms {
        NodeStatus::Warning
    } else {
        NodeStatus::Healthy
    };
    if resume_grace || health.is_none() {
        runtime.latency_failures = 0;
        runtime.latency_successes = 0;
    } else if latency_sample_is_slow(health.is_some(), latency_ms, settings.latency_warning_ms) {
        runtime.latency_failures += 1;
        runtime.latency_successes = 0;
    } else if latency_ms < 1_500 {
        runtime.latency_successes += 1;
        runtime.latency_failures = 0;
    } else {
        runtime.latency_failures = 0;
        runtime.latency_successes = 0;
    }
    if runtime
        .peers
        .iter()
        .filter(|peer| peer.status == "active")
        .count()
        == 0
    {
        runtime.zero_peers_since.get_or_insert(now);
    } else {
        runtime.zero_peers_since = None;
    }
    let offline = runtime.consecutive_failures >= 3;
    let recovered = runtime.consecutive_successes >= 2;
    let latency_alert = runtime.latency_failures >= 3;
    let latency_recovered = runtime.latency_successes >= 3;
    let zero_peers = runtime
        .zero_peers_since
        .is_some_and(|since| now - since >= 5 * 60 * 1_000);
    let finalization_stalled_for = runtime
        .finalized_changed_at
        .map(|changed| now - changed)
        .unwrap_or_default();
    let chain_advanced_since_finalization = runtime
        .chain
        .height
        .zip(runtime.height_at_finalization_change)
        .map(|(current, previous)| current.saturating_sub(previous))
        .unwrap_or_default();
    let finalization_stalled = node.kind == crate::domain::NodeKind::Symbol
        && chain_due
        && finalization_stalled_for >= 30 * 60 * 1_000
        && chain_advanced_since_finalization >= 20;
    let chain = runtime.chain.clone();
    let peer_count = runtime
        .peers
        .iter()
        .filter(|peer| peer.status == "active")
        .count() as i64;
    let api_status = health.as_ref().and_then(|health| health.api_status.clone());
    let db_status = health.as_ref().and_then(|health| health.db_status.clone());
    let nis_status_code = health.as_ref().and_then(|health| health.nis_status_code);
    drop(runtime_map);

    if let Some(event) = poll_event {
        websocket::publish_event(&app, &state, event).await;
    }
    if topology_changed {
        let _ = app.emit("topology-updated", &node.id);
    }

    let sample = NodeSample {
        observed_at: now,
        reachable,
        status,
        latency_ms: Some(latency_ms),
        height: chain.height,
        finalized_height: chain.finalized_height,
        network_height: chain.network_height,
        lag,
        peer_count: Some(peer_count),
        chain_score: chain.chain_score,
        api_status,
        db_status,
        nis_status_code,
        last_error: error,
    };
    state.repository.save_sample(&node.id, &sample).await?;
    if !resume_grace && offline {
        update_alert(
            &app,
            &state,
            &node,
            true,
            "offline",
            "critical",
            "3回連続でノードへ接続できません。",
        )
        .await?;
    } else if !resume_grace && recovered {
        update_alert(
            &app,
            &state,
            &node,
            false,
            "offline",
            "critical",
            "ノードへの接続が復旧しました。",
        )
        .await?;
    }
    if !resume_grace && latency_alert {
        update_alert(
            &app,
            &state,
            &node,
            true,
            "latency",
            "warning",
            "REST APIの応答遅延が継続しています。",
        )
        .await?;
    } else if !resume_grace && latency_recovered {
        update_alert(
            &app,
            &state,
            &node,
            false,
            "latency",
            "warning",
            "REST APIの応答時間が正常に戻りました。",
        )
        .await?;
    }
    if resume_grace {
        let _ = app.emit("node-updated", &node.id);
        desktop::refresh_tray(&app, &state).await;
        return Ok(());
    }
    update_alert(
        &app,
        &state,
        &node,
        zero_peers,
        "peers",
        "warning",
        "アクティブピアが0の状態が5分以上継続しています。",
    )
    .await?;
    update_alert(
        &app,
        &state,
        &node,
        finalization_stalled,
        "finalization_stall",
        if finalization_stalled_for >= 60 * 60 * 1_000 {
            "critical"
        } else {
            "warning"
        },
        "Symbolのファイナライズ高が30分以上更新されていません。",
    )
    .await?;
    if let Some(lag) = lag {
        let active = lag_warning_active;
        let severity = if lag_critical_active {
            "critical"
        } else {
            "warning"
        };
        update_alert(
            &app,
            &state,
            &node,
            active,
            "sync_lag",
            severity,
            &format!("ネットワーク基準高から{lag}ブロック遅れています。"),
        )
        .await?;
    }
    let _ = app.emit("node-updated", &node.id);
    desktop::refresh_tray(&app, &state).await;
    Ok(())
}

pub async fn monitor_node(
    app: AppHandle,
    state: AppState,
    node: StoredNode,
    force: bool,
) -> AppResult<()> {
    let node_id = node.id.clone();
    {
        let mut active = state.monitoring_nodes.lock().await;
        if !active.insert(node_id.clone()) {
            return Ok(());
        }
    }
    // Run the operation behind a join boundary so a panic cannot leave the
    // node permanently present in `monitoring_nodes`. The timeout also recovers
    // from a dependency that stops responding despite request-level timeouts.
    let task_state = state.clone();
    let mut task = tokio::spawn(async move {
        let _permit = task_state
            .request_limit
            .clone()
            .acquire_owned()
            .await
            .map_err(|error| AppError::Configuration(error.to_string()))?;
        monitor_node_inner(app, task_state, node, force).await
    });
    let result = match tokio::time::timeout(Duration::from_secs(60), &mut task).await {
        Ok(Ok(result)) => result,
        Ok(Err(error)) => Err(AppError::Configuration(format!(
            "監視処理が異常終了しました: {error}"
        ))),
        Err(_) => {
            task.abort();
            let _ = task.await;
            Err(AppError::Network(
                "監視処理が60秒以内に完了しませんでした".into(),
            ))
        }
    };
    state.monitoring_nodes.lock().await.remove(&node_id);
    result
}

async fn compare_state_hashes(app: AppHandle, state: AppState) -> AppResult<()> {
    if state.in_resume_grace(chrono::Utc::now().timestamp_millis()) {
        return Ok(());
    }
    let nodes = state.repository.list_stored_nodes(true).await?;
    let runtime = state.runtime.lock().await;
    let mut groups: HashMap<String, Vec<(StoredNode, u64)>> = HashMap::new();
    for node in nodes
        .into_iter()
        .filter(|node| node.kind == crate::domain::NodeKind::Symbol)
    {
        if let (Some(network), Some(height)) = (
            node.network.clone(),
            runtime
                .get(&node.id)
                .and_then(|value| value.chain.finalized_height),
        ) {
            groups.entry(network).or_default().push((node, height));
        }
    }
    drop(runtime);
    for (network, members) in groups.into_iter().filter(|(_, members)| members.len() >= 2) {
        let height = members
            .iter()
            .map(|(_, height)| *height)
            .min()
            .unwrap_or_default();
        let mut hashes: HashMap<String, Vec<StoredNode>> = HashMap::new();
        for (node, _) in &members {
            let adapter = adapter_for(node.kind, node.base_url.clone(), state.client.clone());
            if let Ok(Some(hash)) = adapter.fetch_state_hash(height).await {
                hashes.entry(hash).or_default().push(node.clone());
            }
        }
        let mismatch = hashes.len() > 1;
        let consecutive = {
            let mut counters = state.state_hash_mismatches.lock().await;
            let counter = counters.entry(network.clone()).or_default();
            *counter = if mismatch { *counter + 1 } else { 0 };
            *counter
        };
        for (node, _) in members {
            update_alert(
                &app,
                &state,
                &node,
                mismatch && consecutive >= 2,
                "state_hash_mismatch",
                "critical",
                &format!("ファイナライズ高{height}のStateHashが同一ネットワーク内で一致しません。"),
            )
            .await?;
        }
    }
    Ok(())
}

pub fn start(app: AppHandle, state: AppState) {
    tauri::async_runtime::spawn(async move {
        let initial_settings = state.settings.read().await.clone();
        let _ = state.repository.compact(&initial_settings).await;
        let mut last_state_hash_check = 0i64;
        let mut expected_wake_at = None;
        loop {
            let cycle_started_at = chrono::Utc::now().timestamp_millis();
            if expected_wake_at.is_some_and(|expected| wake_was_late(expected, cycle_started_at)) {
                enter_resume_grace(&state, cycle_started_at, "monitor loop woke late").await;
            }
            let settings = state.settings.read().await.clone();
            let nodes = state
                .repository
                .list_stored_nodes(true)
                .await
                .unwrap_or_default();
            for node in nodes {
                let app = app.clone();
                let state = state.clone();
                tauri::async_runtime::spawn(async move {
                    if let Err(error) = monitor_node(app, state, node, false).await {
                        tracing::warn!(%error, "node monitor failed");
                    }
                });
            }
            let now = chrono::Utc::now().timestamp_millis();
            if !state.in_resume_grace(now) && now - last_state_hash_check >= 120_000 {
                last_state_hash_check = now;
                let app_for_hash = app.clone();
                let state_for_hash = state.clone();
                tauri::async_runtime::spawn(async move {
                    let _ = compare_state_hashes(app_for_hash, state_for_hash).await;
                });
            }
            let jitter = rand::rng().random_range(90..=110) as u64;
            let sleep_ms = settings.health_interval_seconds * 10 * jitter;
            expected_wake_at = Some(
                chrono::Utc::now()
                    .timestamp_millis()
                    .saturating_add(sleep_ms as i64),
            );
            tokio::time::sleep(Duration::from_millis(sleep_ms)).await;
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_a_monitor_loop_delayed_by_suspend() {
        assert!(!wake_was_late(100_000, 114_999));
        assert!(wake_was_late(100_000, 115_000));
    }

    #[test]
    fn connection_failures_are_not_latency_samples() {
        assert!(!latency_sample_is_slow(false, 5_000, 2_000));
        assert!(latency_sample_is_slow(true, 5_000, 2_000));
    }
}
