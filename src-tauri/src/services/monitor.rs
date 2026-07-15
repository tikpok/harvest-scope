use std::{
    collections::HashMap,
    time::{Duration, Instant},
};

use futures_util::StreamExt;
use rand::Rng;
use tauri::{AppHandle, Emitter};

use crate::{
    adapters::{adapter_for, probe_tcp_endpoint, Nis1Adapter, SymbolAdapter},
    domain::{
        CertificateSnapshot, MonitorMode, NetworkEvent, NodeKind, NodeSample, NodeStatus,
        RefreshAllFailure, RefreshAllProgress, RefreshAllResult, StoredNode, SupernodeProgramNode,
    },
    error::{AppError, AppResult},
    state::AppState,
    storage::NodeUpsert,
};

use super::{desktop, websocket};

const RESUME_LATE_THRESHOLD_MS: i64 = 15_000;
const RESUME_GRACE_MS: i64 = 90_000;
const OFFLINE_CONFIRMATION_DELAY_MS: i64 = 90_000;
const OFFLINE_CONFIRMATION_FAILURES: u32 = 3;
const SUSPENDED_REQUEST_THRESHOLD: Duration = Duration::from_secs(10);
const SUPERNODE_INTERVAL_MS: i64 = 15 * 60 * 1_000;
const SUPERNODE_CACHE_MS: i64 = 10 * 60 * 1_000;
const BACKGROUND_MONITOR_CONCURRENCY: usize = 2;
const MANUAL_REFRESH_CONCURRENCY: usize = 8;

#[derive(Debug)]
enum RefreshNodeOutcome {
    Succeeded,
    Failed {
        node_id: String,
        node_name: String,
        message: String,
    },
}

fn summarize_refresh_outcomes(outcomes: Vec<RefreshNodeOutcome>) -> RefreshAllResult {
    let total = outcomes.len();
    let failures = outcomes
        .into_iter()
        .filter_map(|outcome| match outcome {
            RefreshNodeOutcome::Succeeded => None,
            RefreshNodeOutcome::Failed {
                node_id,
                node_name,
                message,
            } => Some(RefreshAllFailure {
                node_id,
                node_name,
                message,
            }),
        })
        .collect::<Vec<_>>();
    let failed = failures.len();
    RefreshAllResult {
        total,
        succeeded: total.saturating_sub(failed),
        failed,
        failures,
    }
}

struct RecheckingReset(AppState);

impl Drop for RecheckingReset {
    fn drop(&mut self) {
        self.0.set_all_nodes_rechecking(false);
    }
}

fn wake_was_late(expected_at: i64, actual_at: i64) -> bool {
    actual_at.saturating_sub(expected_at) >= RESUME_LATE_THRESHOLD_MS
}

fn latency_sample_is_slow(health_succeeded: bool, latency_ms: i64, warning_ms: u64) -> bool {
    health_succeeded && latency_ms as u64 >= warning_ms
}

fn status_from_signals(
    resume_grace_missing_health: bool,
    offline: bool,
    critical: bool,
    warning: bool,
) -> NodeStatus {
    if resume_grace_missing_health {
        NodeStatus::Unknown
    } else if offline {
        NodeStatus::Offline
    } else if critical {
        NodeStatus::Critical
    } else if warning {
        NodeStatus::Warning
    } else {
        NodeStatus::Healthy
    }
}

fn certificate_expiry_severity(remaining: Option<i64>) -> Option<&'static str> {
    remaining.and_then(|value| {
        if value <= 7 {
            Some("critical")
        } else if value <= 30 {
            Some("warning")
        } else {
            None
        }
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ExpiryAlertTransition {
    Active(&'static str),
    Resolved,
    Unchanged,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FinalizationStallTransition {
    Active(&'static str),
    Resolved,
    Unchanged,
}

fn expiry_alert_transition(
    current_remaining: Option<i64>,
    current_expiry_anchor: Option<i64>,
    previous_expiry_anchor: Option<i64>,
) -> ExpiryAlertTransition {
    if let Some(severity) = certificate_expiry_severity(current_remaining) {
        return ExpiryAlertTransition::Active(severity);
    }

    if current_expiry_anchor
        .zip(previous_expiry_anchor)
        .is_some_and(|(current, previous)| current > previous)
    {
        ExpiryAlertTransition::Resolved
    } else {
        ExpiryAlertTransition::Unchanged
    }
}

fn finalization_stall_transition(
    evaluated: bool,
    symbol_node: bool,
    stalled_for_ms: i64,
    chain_advanced_since_finalization: u64,
) -> FinalizationStallTransition {
    if !evaluated || !symbol_node {
        return FinalizationStallTransition::Unchanged;
    }

    if stalled_for_ms >= 30 * 60 * 1_000 && chain_advanced_since_finalization >= 20 {
        if stalled_for_ms >= 60 * 60 * 1_000 {
            FinalizationStallTransition::Active("critical")
        } else {
            FinalizationStallTransition::Active("warning")
        }
    } else {
        FinalizationStallTransition::Resolved
    }
}

fn node_certificate_message(days: i64) -> String {
    if days < 0 {
        "ノード証明書の期限が切れています。".into()
    } else {
        format!("ノード証明書の期限まで残り{days}日です。")
    }
}

fn voting_key_message(epochs: i64) -> String {
    if epochs == 0 {
        "投票キーの有効epochが終了しています。".into()
    } else {
        format!("投票キーの有効期限まで残り{epochs} epochです。")
    }
}

fn supernode_failure_message(round: Option<i64>, failed_checks: &[String]) -> String {
    let round = round
        .map(|value| format!("ラウンド{value}"))
        .unwrap_or_else(|| "最新ラウンド".into());
    if failed_checks.is_empty() {
        format!("NEM Supernodeの{round}でチェックに失敗しました。")
    } else {
        format!(
            "NEM Supernodeの{round}で{}に失敗しました。",
            failed_checks.join(", ")
        )
    }
}

fn empty_sample(
    observed_at: i64,
    reachable: bool,
    status: NodeStatus,
    latency_ms: Option<i64>,
    last_error: Option<String>,
) -> NodeSample {
    NodeSample {
        observed_at,
        reachable,
        status,
        latency_ms,
        height: None,
        finalized_height: None,
        network_height: None,
        lag: None,
        peer_count: None,
        chain_score: None,
        api_status: None,
        db_status: None,
        nis_status_code: None,
        last_error,
        delegated_harvester_count: None,
        delegated_harvester_delta: None,
        total_importance: None,
        total_balance_micro_xym: None,
        harvest_metric_error: None,
        node_certificate_not_after: None,
        node_certificate_remaining_days: None,
        voting_key_start_epoch: None,
        voting_key_end_epoch: None,
        finalization_epoch: None,
        voting_key_remaining_epochs: None,
        certificate_metric_error: None,
        supernode_id: None,
        supernode_status: None,
        supernode_last_round: None,
        supernode_last_round_at: None,
        supernode_checks_passed: None,
        supernode_failed_checks: Vec::new(),
        supernode_metric_error: None,
    }
}

fn roles_include(roles: &[String], role: &str) -> bool {
    roles.iter().any(|value| value.eq_ignore_ascii_case(role))
}

fn has_role(node: &StoredNode, role: &str) -> bool {
    roles_include(&node.roles, role)
}

fn rest_metric_alerts_enabled(public_key: Option<&str>, observed_roles: &[String]) -> bool {
    public_key.is_some_and(|value| !value.trim().is_empty())
        || !roles_include(observed_roles, "API")
}

fn symbol_network_match_key(value: &str) -> String {
    let normalized = value
        .trim()
        .to_ascii_lowercase()
        .replace([' ', '_'], "")
        .replace("--", "-");
    match normalized.as_str() {
        "104" | "-104" | "symbol104" | "symbol-104" | "symbolmainnet" | "symbolmain" => {
            "symbol-mainnet".into()
        }
        "152" | "symbol152" | "symbol-152" | "symboltestnet" | "symboltest" => {
            "symbol-testnet".into()
        }
        other => other.to_owned(),
    }
}

async fn fetch_peer_voting_metrics(
    state: &AppState,
    node: &StoredNode,
) -> Result<CertificateSnapshot, String> {
    if !has_role(node, "Voting") {
        return Ok(CertificateSnapshot::default());
    }
    let public_key = node
        .public_key
        .as_deref()
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| "Voting期限監視には公開鍵が必要です".to_string())?;
    let network = node
        .network
        .as_deref()
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| "Voting期限監視にはネットワークが必要です".to_string())?;
    let network_key = symbol_network_match_key(network);
    let reference = state
        .repository
        .list_stored_nodes(true)
        .await
        .map_err(|error| error.to_string())?
        .into_iter()
        .find(|candidate| {
            candidate.id != node.id
                && candidate.kind == NodeKind::Symbol
                && candidate.monitor_mode == MonitorMode::Api
                && candidate
                    .network
                    .as_deref()
                    .map(symbol_network_match_key)
                    .as_deref()
                    == Some(network_key.as_str())
        })
        .ok_or_else(|| "同一ネットワークの参照用Symbol APIノードがありません".to_string())?;
    SymbolAdapter::new(reference.base_url, state.client.clone())
        .fetch_voting_key_snapshot_for_public_key(public_key)
        .await
}

async fn monitor_peer_node_inner(
    app: AppHandle,
    state: AppState,
    node: StoredNode,
    _force: bool,
) -> AppResult<()> {
    let settings = state.settings.read().await.clone();
    let now = chrono::Utc::now().timestamp_millis();
    let started = Instant::now();
    let probe_result = probe_tcp_endpoint(&node.base_url, Duration::from_secs(10)).await;
    let latency_ms = started.elapsed().as_millis() as i64;
    let evaluation_now = chrono::Utc::now().timestamp_millis();
    let resume_grace = state.is_system_suspended() || state.in_resume_grace(evaluation_now);
    let mut runtime_map = state.runtime.lock().await;
    let runtime = runtime_map.entry(node.id.clone()).or_default();
    let mut last_error = None;
    match probe_result {
        Ok(()) => {
            runtime.record_connectivity_success(resume_grace);
        }
        Err(error) => {
            runtime.record_connectivity_failure(resume_grace, now, OFFLINE_CONFIRMATION_DELAY_MS);
            last_error = Some(error.to_string());
        }
    }
    if resume_grace || last_error.is_some() {
        runtime.latency_failures = 0;
        runtime.latency_successes = 0;
    } else if latency_sample_is_slow(true, latency_ms, settings.latency_warning_ms) {
        runtime.latency_failures += 1;
        runtime.latency_successes = 0;
    } else if latency_ms < 1_500 {
        runtime.latency_successes += 1;
        runtime.latency_failures = 0;
    } else {
        runtime.latency_failures = 0;
        runtime.latency_successes = 0;
    }
    let offline = runtime.offline_confirmed(OFFLINE_CONFIRMATION_FAILURES);
    let recovered = runtime.consecutive_successes >= 2;
    let latency_alert = runtime.latency_failures >= 3;
    let latency_recovered = runtime.latency_successes >= 3;
    let status = if resume_grace && last_error.is_some() {
        NodeStatus::Unknown
    } else if offline {
        NodeStatus::Offline
    } else if last_error.is_some() || latency_alert {
        NodeStatus::Warning
    } else {
        NodeStatus::Healthy
    };
    drop(runtime_map);

    let mut sample = empty_sample(
        now,
        last_error.is_none(),
        status,
        Some(latency_ms),
        last_error,
    );
    match fetch_peer_voting_metrics(&state, &node).await {
        Ok(certificate) => {
            sample.voting_key_start_epoch = certificate.voting_key_start_epoch;
            sample.voting_key_end_epoch = certificate.voting_key_end_epoch;
            sample.finalization_epoch = certificate.finalization_epoch;
            sample.voting_key_remaining_epochs = certificate.voting_key_remaining_epochs;
            sample.certificate_metric_error = certificate.error;
        }
        Err(error) => sample.certificate_metric_error = Some(error),
    }
    let previous_certificate = state
        .repository
        .latest_certificate_snapshot(&node.id)
        .await?;
    state.repository.save_sample(&node.id, &sample).await?;

    let evaluation_paused = resume_grace
        || state.is_system_suspended()
        || state.in_resume_grace(chrono::Utc::now().timestamp_millis());
    if !evaluation_paused && offline {
        update_alert(
            &app,
            &state,
            &node,
            true,
            "offline",
            "critical",
            "3回連続でPeerノードへ接続できません。",
        )
        .await?;
    } else if !evaluation_paused && recovered {
        update_alert(
            &app,
            &state,
            &node,
            false,
            "offline",
            "critical",
            "Peerノードへの接続が復旧しました。",
        )
        .await?;
    }
    if !evaluation_paused && latency_alert {
        update_alert(
            &app,
            &state,
            &node,
            true,
            "latency",
            "warning",
            "P2Pポートの応答遅延が継続しています。",
        )
        .await?;
    } else if !evaluation_paused && latency_recovered {
        update_alert(
            &app,
            &state,
            &node,
            false,
            "latency",
            "warning",
            "P2Pポートの応答時間が正常に戻りました。",
        )
        .await?;
    }
    if !evaluation_paused {
        let voting_key_message = sample
            .voting_key_remaining_epochs
            .map(voting_key_message)
            .unwrap_or_else(|| "投票キー期限を確認できません。".into());
        update_expiry_alert(
            &app,
            &state,
            &node,
            expiry_alert_transition(
                sample.voting_key_remaining_epochs,
                sample.voting_key_end_epoch,
                previous_certificate.voting_key_end_epoch,
            ),
            "voting_key_expiring",
            &voting_key_message,
            "投票キーの有効期限が延長されました。",
        )
        .await?;
    }
    let _ = app.emit("node-updated", &node.id);
    desktop::refresh_tray(&app, &state).await;
    Ok(())
}

async fn supernode_nodes(state: &AppState, now: i64) -> AppResult<Vec<SupernodeProgramNode>> {
    let mut cache = state.supernode_nodes_cache.lock().await;
    if let Some(cache) = cache
        .as_ref()
        .filter(|cache| now.saturating_sub(cache.observed_at) <= SUPERNODE_CACHE_MS)
    {
        return Ok(cache.nodes.clone());
    }
    let nodes = Nis1Adapter::fetch_supernode_nodes(&state.client).await?;
    *cache = Some(crate::domain::SupernodeListCache {
        observed_at: now,
        nodes: nodes.clone(),
    });
    Ok(nodes)
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

pub(crate) async fn handle_system_resume(state: &AppState, reason: &str) {
    let now = chrono::Utc::now().timestamp_millis();
    enter_resume_grace(state, now, reason).await;
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

async fn resolve_alert_silently(
    app: &AppHandle,
    state: &AppState,
    node: &StoredNode,
    alert_type: &str,
) -> AppResult<()> {
    if state
        .repository
        .resolve_alert(&node.id, alert_type)
        .await?
        .is_some()
    {
        let _ = app.emit("alert-changed", &node.id);
        desktop::refresh_tray(app, state).await;
    }
    Ok(())
}

async fn update_expiry_alert(
    app: &AppHandle,
    state: &AppState,
    node: &StoredNode,
    transition: ExpiryAlertTransition,
    alert_type: &str,
    active_message: &str,
    resolved_message: &str,
) -> AppResult<()> {
    match transition {
        ExpiryAlertTransition::Active(severity) => {
            update_alert(app, state, node, true, alert_type, severity, active_message).await
        }
        ExpiryAlertTransition::Resolved => {
            update_alert(
                app,
                state,
                node,
                false,
                alert_type,
                "warning",
                resolved_message,
            )
            .await
        }
        ExpiryAlertTransition::Unchanged => Ok(()),
    }
}

async fn monitor_node_inner(
    app: AppHandle,
    state: AppState,
    node: StoredNode,
    force: bool,
) -> AppResult<()> {
    if node.monitor_mode == MonitorMode::Peer {
        return monitor_peer_node_inner(app, state, node, force).await;
    }
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
    let resume_grace = state.is_system_suspended() || state.in_resume_grace(evaluation_now);
    let mut poll_event = None;
    let mut topology_changed = false;
    let monitored_max = {
        let runtime_map = state.runtime.lock().await;
        node.network.as_deref().and_then(|network| {
            network_nodes
                .iter()
                .filter(|candidate| candidate.network.as_deref() == Some(network))
                .filter_map(|candidate| {
                    runtime_map
                        .get(&candidate.id)
                        .and_then(|value| value.chain.height)
                })
                .max()
        })
    };
    let mut error = None;
    let health = match health_result {
        Ok(health) => Some(health),
        Err(err) => {
            error = Some(err.to_string());
            None
        }
    };
    let (chain_due, peers_due, static_due, supernode_due) = {
        let mut runtime_map = state.runtime.lock().await;
        let runtime = runtime_map.entry(node.id.clone()).or_default();
        if health.is_some() {
            runtime.record_connectivity_success(resume_grace);
        } else {
            runtime.record_connectivity_failure(resume_grace, now, OFFLINE_CONFIRMATION_DELAY_MS);
        }
        (
            force
                || now - runtime.last_chain_poll >= settings.chain_interval_seconds as i64 * 1_000,
            force || now - runtime.last_peer_poll >= settings.peer_interval_seconds as i64 * 1_000,
            force || now - runtime.last_static_poll >= 15 * 60 * 1_000,
            force || now - runtime.last_supernode_poll >= SUPERNODE_INTERVAL_MS,
        )
    };
    if health.is_some() && chain_due {
        match adapter.fetch_chain().await {
            Ok(chain) => {
                let mut runtime_map = state.runtime.lock().await;
                let runtime = runtime_map.entry(node.id.clone()).or_default();
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
                        signer_public_key: None,
                        harvester_public_key: None,
                        harvested_node_id: None,
                        observer_node_id: None,
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
    if health.is_some() && peers_due {
        match adapter.fetch_peers().await {
            Ok(peers) => match state.repository.replace_peers(&node.id, &peers, now).await {
                Ok(()) => {
                    let mut runtime_map = state.runtime.lock().await;
                    let runtime = runtime_map.entry(node.id.clone()).or_default();
                    topology_changed = runtime.peers != peers;
                    runtime.peers = peers;
                    runtime.last_peer_poll = now;
                }
                Err(err) => error = Some(err.to_string()),
            },
            Err(err) => error = Some(err.to_string()),
        }
    }
    let mut delegated_harvester_count = None;
    let mut delegated_harvester_delta = None;
    let mut total_importance = None;
    let mut total_balance_micro_xym = None;
    let mut harvest_metric_error = None;
    if node.kind == NodeKind::Symbol && health.is_some() && peers_due {
        match adapter.fetch_harvest_metrics().await {
            Ok(harvest) if harvest.supported => {
                delegated_harvester_count = harvest.delegated_harvester_count;
                total_importance = harvest.total_importance;
                total_balance_micro_xym = harvest.total_balance_micro_xym;
                harvest_metric_error = harvest.error;
                if let Some(count) = delegated_harvester_count {
                    let cutoff = now.saturating_sub(24 * 60 * 60 * 1_000);
                    if let Ok(Some(previous)) = state
                        .repository
                        .harvester_count_at_or_before(&node.id, cutoff)
                        .await
                    {
                        delegated_harvester_delta = Some(count.saturating_sub(previous));
                    }
                }
                if let Err(err) = state
                    .repository
                    .replace_harvesters(&node.id, &harvest.harvester_public_keys, now)
                    .await
                {
                    harvest_metric_error = Some(err.to_string());
                }
            }
            Ok(_) => {}
            Err(err) => harvest_metric_error = Some(err.to_string()),
        }
    }
    let mut node_certificate_not_after = None;
    let mut node_certificate_remaining_days = None;
    let mut voting_key_start_epoch = None;
    let mut voting_key_end_epoch = None;
    let mut finalization_epoch = None;
    let mut voting_key_remaining_epochs = None;
    let mut certificate_metric_error = None;
    if node.kind == NodeKind::Symbol && health.is_some() && static_due {
        match adapter.fetch_certificate_metrics().await {
            Ok(certificate) => {
                node_certificate_not_after = certificate.node_certificate_not_after;
                node_certificate_remaining_days = certificate.node_certificate_remaining_days;
                voting_key_start_epoch = certificate.voting_key_start_epoch;
                voting_key_end_epoch = certificate.voting_key_end_epoch;
                finalization_epoch = certificate.finalization_epoch;
                voting_key_remaining_epochs = certificate.voting_key_remaining_epochs;
                certificate_metric_error = certificate.error;
            }
            Err(err) => certificate_metric_error = Some(err.to_string()),
        }
    }
    let mut supernode_id = None;
    let mut supernode_status = None;
    let mut supernode_last_round = None;
    let mut supernode_last_round_at = None;
    let mut supernode_checks_passed = None;
    let mut supernode_failed_checks = Vec::new();
    let mut supernode_metric_error = None;
    let mut supernode_alert_evaluated = false;
    if node.kind == NodeKind::Nis1 && health.is_some() && supernode_due {
        match supernode_nodes(&state, now).await {
            Ok(nodes) => match adapter.fetch_supernode_metrics(&nodes).await {
                Ok(snapshot) => {
                    supernode_alert_evaluated = snapshot.error.is_none();
                    supernode_id = snapshot.supernode_id;
                    supernode_status = snapshot.supernode_status;
                    supernode_last_round = snapshot.supernode_last_round;
                    supernode_last_round_at = snapshot.supernode_last_round_at;
                    supernode_checks_passed = snapshot.supernode_checks_passed;
                    supernode_failed_checks = snapshot.supernode_failed_checks;
                    supernode_metric_error = snapshot.error;
                    state
                        .runtime
                        .lock()
                        .await
                        .entry(node.id.clone())
                        .or_default()
                        .last_supernode_poll = now;
                }
                Err(err) => supernode_metric_error = Some(err.to_string()),
            },
            Err(err) => supernode_metric_error = Some(err.to_string()),
        }
    }
    if health.is_some() && static_due {
        if let Ok(identity) = adapter.fetch_static_info().await {
            match state
                .repository
                .upsert_node_with_mode(NodeUpsert {
                    id: &node.id,
                    base_url: &node.base_url,
                    display_name: &node.display_name,
                    identity: &identity,
                    monitor_mode: node.monitor_mode,
                    enabled: node.enabled,
                    favorite: node.favorite,
                })
                .await
            {
                Ok(()) => {
                    state
                        .runtime
                        .lock()
                        .await
                        .entry(node.id.clone())
                        .or_default()
                        .last_static_poll = now;
                }
                Err(err) => error = Some(err.to_string()),
            }
        }
    }
    let mut runtime_map = state.runtime.lock().await;
    let runtime = runtime_map.entry(node.id.clone()).or_default();
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
    let status = status_from_signals(
        resume_grace && health.is_none(),
        runtime.offline_confirmed(OFFLINE_CONFIRMATION_FAILURES),
        lag_critical_active,
        runtime.consecutive_failures > 0
            || health_degraded
            || lag_warning_active
            || latency_ms as u64 >= settings.latency_warning_ms
            || supernode_checks_passed == Some(false),
    );
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
    let offline = runtime.offline_confirmed(OFFLINE_CONFIRMATION_FAILURES);
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
    let finalization_stall_alert = finalization_stall_transition(
        health.is_some() && chain_due,
        node.kind == crate::domain::NodeKind::Symbol,
        finalization_stalled_for,
        chain_advanced_since_finalization,
    );
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

    let previous_certificate = state
        .repository
        .latest_certificate_snapshot(&node.id)
        .await?;
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
        delegated_harvester_count,
        delegated_harvester_delta,
        total_importance,
        total_balance_micro_xym,
        harvest_metric_error,
        node_certificate_not_after,
        node_certificate_remaining_days,
        voting_key_start_epoch,
        voting_key_end_epoch,
        finalization_epoch,
        voting_key_remaining_epochs,
        certificate_metric_error,
        supernode_id,
        supernode_status,
        supernode_last_round,
        supernode_last_round_at,
        supernode_checks_passed,
        supernode_failed_checks: supernode_failed_checks.clone(),
        supernode_metric_error,
    };
    state.repository.save_sample(&node.id, &sample).await?;
    // Power events can arrive while a request or database write is in flight.
    // Recheck immediately before mutating alerts so a suspend transition can
    // never race a stale health result into an offline notification.
    let evaluation_paused = resume_grace
        || state.is_system_suspended()
        || state.in_resume_grace(chrono::Utc::now().timestamp_millis());
    let observed_roles = state
        .repository
        .observed_roles_for_node_url(&node.base_url)
        .await?;
    let rest_metric_alerts_allowed =
        rest_metric_alerts_enabled(node.public_key.as_deref(), &observed_roles);
    if !evaluation_paused && !rest_metric_alerts_allowed {
        resolve_alert_silently(&app, &state, &node, "offline").await?;
        resolve_alert_silently(&app, &state, &node, "latency").await?;
        resolve_alert_silently(&app, &state, &node, "peers").await?;
    } else if !evaluation_paused && offline {
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
    } else if !evaluation_paused && recovered {
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
    if rest_metric_alerts_allowed && !evaluation_paused && latency_alert {
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
    } else if rest_metric_alerts_allowed && !evaluation_paused && latency_recovered {
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
    if evaluation_paused {
        let _ = app.emit("node-updated", &node.id);
        desktop::refresh_tray(&app, &state).await;
        return Ok(());
    }
    if rest_metric_alerts_allowed {
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
    }
    match finalization_stall_alert {
        FinalizationStallTransition::Active(severity) => {
            update_alert(
                &app,
                &state,
                &node,
                true,
                "finalization_stall",
                severity,
                "Symbolのファイナライズ高が30分以上更新されていません。",
            )
            .await?;
        }
        FinalizationStallTransition::Resolved => {
            update_alert(
                &app,
                &state,
                &node,
                false,
                "finalization_stall",
                "warning",
                "Symbolのファイナライズ高が更新されました。",
            )
            .await?;
        }
        FinalizationStallTransition::Unchanged => {}
    }
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
    let node_certificate_message = node_certificate_remaining_days
        .map(node_certificate_message)
        .unwrap_or_else(|| "ノード証明書期限を確認できません。".into());
    update_expiry_alert(
        &app,
        &state,
        &node,
        expiry_alert_transition(
            node_certificate_remaining_days,
            node_certificate_not_after,
            previous_certificate.node_certificate_not_after,
        ),
        "node_certificate_expiring",
        &node_certificate_message,
        "ノード証明書の期限が延長されました。",
    )
    .await?;
    let voting_key_message = voting_key_remaining_epochs
        .map(voting_key_message)
        .unwrap_or_else(|| "投票キー期限を確認できません。".into());
    update_expiry_alert(
        &app,
        &state,
        &node,
        expiry_alert_transition(
            voting_key_remaining_epochs,
            voting_key_end_epoch,
            previous_certificate.voting_key_end_epoch,
        ),
        "voting_key_expiring",
        &voting_key_message,
        "投票キーの有効期限が延長されました。",
    )
    .await?;
    if supernode_alert_evaluated {
        let supernode_failed = supernode_checks_passed == Some(false);
        update_alert(
            &app,
            &state,
            &node,
            supernode_failed,
            "supernode_check_failed",
            "warning",
            &supernode_failure_message(supernode_last_round, &supernode_failed_checks),
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
    if state.is_system_suspended() {
        return Ok(());
    }
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

pub async fn refresh_all_nodes(app: AppHandle, state: AppState) -> AppResult<RefreshAllResult> {
    let Ok(_refresh_guard) = state.all_nodes_refresh_lock.try_lock() else {
        return Ok(RefreshAllResult::default());
    };
    state.set_all_nodes_rechecking(true);
    let _rechecking_reset = RecheckingReset(state.clone());
    let _ = app.emit("refresh-all-state-changed", true);

    let nodes = state.repository.list_stored_nodes(true).await?;
    let total = nodes.len();
    let _ = app.emit(
        "refresh-all-progress",
        RefreshAllProgress {
            completed: 0,
            total,
            failed: 0,
        },
    );
    let mut stream = futures_util::stream::iter(nodes.into_iter().map(|node| {
        let app = app.clone();
        let state = state.clone();
        async move {
            let node_id = node.id.clone();
            let node_name = node.display_name.clone();
            match monitor_node(app, state, node, true).await {
                Ok(()) => RefreshNodeOutcome::Succeeded,
                Err(error) => RefreshNodeOutcome::Failed {
                    node_id,
                    node_name,
                    message: error.to_string(),
                },
            }
        }
    }))
    .buffer_unordered(MANUAL_REFRESH_CONCURRENCY);
    let mut outcomes = Vec::with_capacity(total);
    let mut failed = 0;
    while let Some(outcome) = stream.next().await {
        if matches!(outcome, RefreshNodeOutcome::Failed { .. }) {
            failed += 1;
        }
        outcomes.push(outcome);
        let _ = app.emit(
            "refresh-all-progress",
            RefreshAllProgress {
                completed: outcomes.len(),
                total,
                failed,
            },
        );
    }
    let result = summarize_refresh_outcomes(outcomes);

    state.set_all_nodes_rechecking(false);
    let _ = app.emit("refresh-all-state-changed", false);
    Ok(result)
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
        .filter(|node| node.monitor_mode == MonitorMode::Api)
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
        let state_for_compact = state.clone();
        tauri::async_runtime::spawn(async move {
            tokio::time::sleep(Duration::from_secs(90)).await;
            let settings = state_for_compact.settings.read().await.clone();
            if let Err(error) = state_for_compact.repository.compact(&settings).await {
                tracing::warn!(%error, "delayed repository compaction failed");
            }
        });
        let mut last_state_hash_check = chrono::Utc::now().timestamp_millis();
        let mut expected_wake_at = None;
        loop {
            let cycle_started_at = chrono::Utc::now().timestamp_millis();
            if expected_wake_at.is_some_and(|expected| wake_was_late(expected, cycle_started_at)) {
                enter_resume_grace(&state, cycle_started_at, "monitor loop woke late").await;
                state.set_system_suspended(false);
                let app_for_refresh = app.clone();
                let state_for_refresh = state.clone();
                tauri::async_runtime::spawn(async move {
                    if let Err(error) = refresh_all_nodes(app_for_refresh, state_for_refresh).await
                    {
                        tracing::warn!(%error, "post-resume node refresh failed");
                    }
                });
            }
            let settings = state.settings.read().await.clone();
            let nodes = state
                .repository
                .list_stored_nodes(true)
                .await
                .unwrap_or_default();
            let app_for_cycle = app.clone();
            let state_for_cycle = state.clone();
            tauri::async_runtime::spawn(async move {
                futures_util::stream::iter(nodes.into_iter().map(|node| {
                    let app = app_for_cycle.clone();
                    let state = state_for_cycle.clone();
                    async move {
                        if let Err(error) = monitor_node(app, state, node, false).await {
                            tracing::warn!(%error, "node monitor failed");
                        }
                    }
                }))
                .buffer_unordered(BACKGROUND_MONITOR_CONCURRENCY)
                .collect::<Vec<_>>()
                .await;
            });
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
    fn refresh_summary_keeps_partial_failures_out_of_the_command_error_path() {
        let summary = summarize_refresh_outcomes(vec![
            RefreshNodeOutcome::Succeeded,
            RefreshNodeOutcome::Failed {
                node_id: "node-b".into(),
                node_name: "REST未検出B".into(),
                message: "タイムアウトしました".into(),
            },
            RefreshNodeOutcome::Succeeded,
        ]);

        assert_eq!(summary.total, 3);
        assert_eq!(summary.succeeded, 2);
        assert_eq!(summary.failed, 1);
        assert_eq!(summary.failures[0].node_id, "node-b");
    }

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

    #[test]
    fn rest_metric_alerts_are_suppressed_for_observed_only_api_nodes() {
        assert!(!rest_metric_alerts_enabled(None, &["API".to_string()]));
        assert!(rest_metric_alerts_enabled(
            Some("verified-public-key"),
            &["API".to_string()]
        ));
        assert!(rest_metric_alerts_enabled(None, &[]));
    }

    #[test]
    fn critical_status_takes_precedence_over_warning_signals() {
        assert_eq!(
            status_from_signals(false, false, true, true),
            NodeStatus::Critical
        );
        assert_eq!(
            status_from_signals(false, true, true, true),
            NodeStatus::Offline
        );
        assert_eq!(
            status_from_signals(true, true, true, true),
            NodeStatus::Unknown
        );
    }

    #[test]
    fn certificate_expiry_thresholds_are_warning_then_critical() {
        assert_eq!(certificate_expiry_severity(Some(31)), None);
        assert_eq!(certificate_expiry_severity(Some(30)), Some("warning"));
        assert_eq!(certificate_expiry_severity(Some(7)), Some("critical"));
        assert_eq!(certificate_expiry_severity(Some(-1)), Some("critical"));
        assert_eq!(certificate_expiry_severity(None), None);
    }

    #[test]
    fn expiry_alert_skips_when_current_metric_is_missing() {
        assert_eq!(
            expiry_alert_transition(None, None, Some(100)),
            ExpiryAlertTransition::Unchanged
        );
    }

    #[test]
    fn expiry_alert_resolves_only_when_expiry_anchor_extends() {
        assert_eq!(
            expiry_alert_transition(Some(31), Some(100), Some(100)),
            ExpiryAlertTransition::Unchanged
        );
        assert_eq!(
            expiry_alert_transition(Some(31), Some(101), Some(100)),
            ExpiryAlertTransition::Resolved
        );
    }

    #[test]
    fn expiry_alert_stays_active_when_extended_but_still_expiring() {
        assert_eq!(
            expiry_alert_transition(Some(7), Some(101), Some(100)),
            ExpiryAlertTransition::Active("critical")
        );
    }

    #[test]
    fn finalization_stall_alert_skips_when_chain_was_not_checked() {
        assert_eq!(
            finalization_stall_transition(false, true, 45 * 60 * 1_000, 30),
            FinalizationStallTransition::Unchanged
        );
    }

    #[test]
    fn finalization_stall_alert_resolves_only_after_chain_check() {
        assert_eq!(
            finalization_stall_transition(true, true, 5 * 60 * 1_000, 30),
            FinalizationStallTransition::Resolved
        );
    }

    #[test]
    fn finalization_stall_alert_escalates_after_one_hour() {
        assert_eq!(
            finalization_stall_transition(true, true, 30 * 60 * 1_000, 20),
            FinalizationStallTransition::Active("warning")
        );
        assert_eq!(
            finalization_stall_transition(true, true, 60 * 60 * 1_000, 20),
            FinalizationStallTransition::Active("critical")
        );
    }

    #[test]
    fn normalizes_symbol_network_names_for_reference_lookup() {
        assert_eq!(
            symbol_network_match_key("Symbol Mainnet"),
            symbol_network_match_key("Symbol-104")
        );
        assert_eq!(
            symbol_network_match_key("SYMBOL Mainnet"),
            symbol_network_match_key("-104")
        );
        assert_eq!(
            symbol_network_match_key("Symbol Testnet"),
            symbol_network_match_key("152")
        );
    }
}
