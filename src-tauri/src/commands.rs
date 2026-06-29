use std::{sync::atomic::Ordering, time::Instant};

use tauri::{AppHandle, State};
use tauri_plugin_autostart::ManagerExt;
use tauri_plugin_dialog::DialogExt;

use crate::{
    adapters::{adapter_for, normalize_base_url, probe_endpoint},
    domain::{
        AlertRecord, AppSettings, DashboardData, MetricPoint, NetworkInterfaceInfo, NodeDetail,
        NodeSample, NodeStatus, NodeSummary, ScanCandidate, ScanRequest, UpsertNodeInput,
    },
    error::{AppError, AppResult},
    services::{discovery, monitor},
    state::AppState,
};

#[tauri::command]
pub async fn list_nodes(state: State<'_, AppState>) -> AppResult<Vec<NodeSummary>> {
    state.repository.list_nodes().await
}

#[tauri::command]
pub async fn get_dashboard(state: State<'_, AppState>) -> AppResult<DashboardData> {
    let nodes = state.repository.list_nodes().await?;
    Ok(DashboardData {
        total: nodes.len(),
        healthy: nodes
            .iter()
            .filter(|node| node.status == NodeStatus::Healthy)
            .count(),
        warning: nodes
            .iter()
            .filter(|node| node.status == NodeStatus::Warning)
            .count(),
        critical: nodes
            .iter()
            .filter(|node| node.status == NodeStatus::Critical)
            .count(),
        offline: nodes
            .iter()
            .filter(|node| node.status == NodeStatus::Offline)
            .count(),
        nodes,
    })
}

#[tauri::command]
pub async fn upsert_node(
    app: AppHandle,
    state: State<'_, AppState>,
    input: UpsertNodeInput,
) -> AppResult<NodeSummary> {
    let base_url = normalize_base_url(&input.base_url)?;
    let existing = state.repository.list_stored_nodes(false).await?;
    if input.id.is_none() && existing.len() >= 50 {
        return Err(AppError::Configuration(
            "監視できるノードは最大50台です".into(),
        ));
    }
    let identity = match input.kind {
        Some(kind) => {
            adapter_for(kind, base_url.clone(), state.client.clone())
                .probe()
                .await?
        }
        None => probe_endpoint(&base_url, state.client.clone()).await?,
    };
    let id = input.id.unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
    let display_name = input
        .display_name
        .filter(|value| !value.trim().is_empty())
        .or(identity.name.clone())
        .unwrap_or_else(|| base_url.clone());

    // Registration and monitoring used to be separate operations. If the
    // background monitor was delayed or failed before saving its first sample,
    // a successfully probed node remained "unknown" indefinitely. Capture a
    // lightweight health sample as part of registration so every registered
    // node immediately has an observable state.
    let health_adapter = adapter_for(identity.kind, base_url.clone(), state.client.clone());
    let health_started = Instant::now();
    let health_result = health_adapter.fetch_health().await;
    let health_latency_ms = health_started.elapsed().as_millis() as i64;
    let latency_warning_ms = state.settings.read().await.latency_warning_ms;
    let observed_at = chrono::Utc::now().timestamp_millis();
    let initial_sample = match health_result {
        Ok(health) => NodeSample {
            observed_at,
            reachable: health.reachable,
            status: if !health.healthy || health_latency_ms as u64 >= latency_warning_ms {
                NodeStatus::Warning
            } else {
                NodeStatus::Healthy
            },
            latency_ms: Some(health_latency_ms),
            height: None,
            finalized_height: None,
            network_height: None,
            lag: None,
            peer_count: None,
            chain_score: None,
            api_status: health.api_status,
            db_status: health.db_status,
            nis_status_code: health.nis_status_code,
            last_error: None,
        },
        Err(error) => NodeSample {
            observed_at,
            reachable: false,
            status: NodeStatus::Warning,
            latency_ms: Some(health_latency_ms),
            height: None,
            finalized_height: None,
            network_height: None,
            lag: None,
            peer_count: None,
            chain_score: None,
            api_status: None,
            db_status: None,
            nis_status_code: None,
            last_error: Some(error.to_string()),
        },
    };
    state
        .repository
        .upsert_node(
            &id,
            &base_url,
            &display_name,
            &identity,
            input.enabled.unwrap_or(true),
            input.favorite.unwrap_or(false),
        )
        .await?;
    state.repository.save_sample(&id, &initial_sample).await?;
    let node = state.repository.get_stored_node(&id).await?;
    let app_for_monitor = app.clone();
    let state_for_monitor = state.inner().clone();
    tauri::async_runtime::spawn(async move {
        if let Err(error) =
            monitor::monitor_node(app_for_monitor, state_for_monitor, node, true).await
        {
            tracing::warn!(%error, "initial node monitor failed");
        }
    });
    state
        .repository
        .list_nodes()
        .await?
        .into_iter()
        .find(|node| node.id == id)
        .ok_or_else(|| AppError::NotFound(id))
}

#[tauri::command]
pub async fn delete_node(state: State<'_, AppState>, node_id: String) -> AppResult<()> {
    state.repository.delete_node(&node_id).await?;
    state.runtime.lock().await.remove(&node_id);
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
pub async fn pause_node(state: State<'_, AppState>, node_id: String) -> AppResult<()> {
    state.repository.set_enabled(&node_id, false).await
}

#[tauri::command]
pub async fn resume_node(
    app: AppHandle,
    state: State<'_, AppState>,
    node_id: String,
) -> AppResult<()> {
    state.repository.set_enabled(&node_id, true).await?;
    let node = state.repository.get_stored_node(&node_id).await?;
    let state = state.inner().clone();
    tauri::async_runtime::spawn(async move {
        let _ = monitor::monitor_node(app, state, node, true).await;
    });
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
pub async fn acknowledge_alert(state: State<'_, AppState>, alert_id: String) -> AppResult<()> {
    state.repository.acknowledge_alert(&alert_id).await
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
        || !(1..=90).contains(&settings.raw_retention_days)
        || settings.aggregate_retention_days < 30
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
                "REST応答ms".into(),
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
                "最終化高".into(),
                "同期遅延".into(),
                "ピア数".into(),
                "REST応答ms".into(),
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
        "node-watcher-{dataset}-{}.csv",
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
    #[test]
    fn rejects_invalid_settings() {
        let settings = AppSettings {
            health_interval_seconds: 1,
            ..AppSettings::default()
        };
        assert!(validate_settings(&settings).is_err());
    }
}
