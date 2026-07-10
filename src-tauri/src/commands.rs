use std::sync::atomic::Ordering;

use crate::{
    domain::{
        AlertRecord, AppSettings, DashboardData, DiagnosticRun, HarvestEvent, MetricPoint,
        NetworkEvent, NetworkInterfaceInfo, NodeConfigImportResult, NodeDetail, NodeStatus,
        NodeSummary, ScanCandidate, ScanRequest, TopologySnapshot, UpsertNodeInput,
    },
    error::{AppError, AppResult},
    services::{
        csv_export, desktop, diagnostic as diagnostic_service, discovery, monitor, node_config,
        node_registration, topology,
    },
    state::AppState,
};
use tauri::{AppHandle, Emitter, State};
use tauri_plugin_autostart::ManagerExt;

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
        rechecking: state.is_all_nodes_rechecking(),
        nodes,
    })
}

#[tauri::command]
pub async fn get_topology(state: State<'_, AppState>) -> AppResult<TopologySnapshot> {
    topology::snapshot(state.inner()).await
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
    node_registration::upsert_node_from_input(app, state.inner().clone(), input, true).await
}

#[tauri::command]
pub async fn export_node_config(
    app: AppHandle,
    state: State<'_, AppState>,
) -> AppResult<Option<String>> {
    node_config::export(app, state.inner()).await
}

#[tauri::command]
pub async fn import_node_config(
    app: AppHandle,
    state: State<'_, AppState>,
) -> AppResult<Option<NodeConfigImportResult>> {
    node_config::import(app, state.inner()).await
}

#[tauri::command]
pub async fn delete_node(
    app: AppHandle,
    state: State<'_, AppState>,
    node_id: String,
) -> AppResult<()> {
    state.repository.delete_node(&node_id).await?;
    state.runtime.lock().await.remove(&node_id);
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
pub async fn refresh_all_nodes(app: AppHandle, state: State<'_, AppState>) -> AppResult<()> {
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
    desktop::refresh_tray(&app, &state).await;
    Ok(settings)
}

#[tauri::command]
pub async fn export_csv(
    app: AppHandle,
    state: State<'_, AppState>,
    dataset: String,
    node_id: Option<String>,
) -> AppResult<Option<String>> {
    csv_export::export(app, state.inner(), dataset, node_id).await
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
