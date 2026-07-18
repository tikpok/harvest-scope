use std::{sync::atomic::Ordering, time::Duration};

use tauri::{AppHandle, Emitter, State};
use tauri_plugin_autostart::ManagerExt;

use crate::{
    adapters::{require_insecure_http_approval, Nis1Adapter},
    domain::{
        AlertRecord, AppSettings, DashboardData, DiagnosticRun, HarvestEvent, MetricPoint,
        NetworkEvent, NetworkInterfaceInfo, NodeConfigImportResult, NodeDetail, NodeStatus,
        NodeSummary, RefreshAllResult, ScanCandidate, ScanRequest, TopologySnapshot,
        UpsertNodeInput,
    },
    error::{AppError, AppResult},
    services::{
        csv_export, desktop, diagnostic as diagnostic_service, discovery, monitor, node_config,
        node_registration, topology,
    },
    state::AppState,
};

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
    topology::snapshot(&state).await
}

#[tauri::command]
pub async fn get_recent_network_events(
    state: State<'_, AppState>,
    limit: Option<usize>,
) -> AppResult<Vec<NetworkEvent>> {
    let events = state.network_events.lock().await;
    let limit = limit.unwrap_or(50).min(200);
    Ok(events.recent(limit))
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
pub async fn list_supernode_program_nodes(
    state: State<'_, AppState>,
) -> AppResult<Vec<crate::domain::SupernodeProgramNode>> {
    Nis1Adapter::fetch_supernode_nodes(&state.transport).await
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
    node_config::export(app, &state).await
}

#[tauri::command]
pub async fn import_node_config(
    app: AppHandle,
    state: State<'_, AppState>,
) -> AppResult<Option<NodeConfigImportResult>> {
    node_config::import(app, &state).await
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
    let node = state.repository.get_stored_node(&node_id).await?;
    require_insecure_http_approval(&node.base_url, node.allow_insecure_http)?;
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
pub async fn confirm_insecure_http(
    app: AppHandle,
    state: State<'_, AppState>,
    node_id: String,
) -> AppResult<()> {
    state.repository.confirm_insecure_http(&node_id).await?;
    let _ = app.emit("node-updated", &node_id);
    let _ = app.emit("topology-updated", &node_id);
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
    let runtime = state
        .runtime
        .lock()
        .await
        .get(&node_id)
        .cloned()
        .unwrap_or_default();
    let mut detail = state
        .repository
        .node_detail(&node_id, runtime.websocket_connected)
        .await?;
    detail.transport_rejected_count = runtime.transport_rejected_count;
    detail.last_transport_rejection_at = runtime.last_transport_rejection_at;
    detail.last_transport_rejection_reason = runtime.last_transport_rejection_reason;
    Ok(detail)
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

#[tauri::command]
pub async fn export_csv(
    app: AppHandle,
    state: State<'_, AppState>,
    dataset: String,
    node_id: Option<String>,
) -> AppResult<Option<String>> {
    csv_export::export(app, &state, dataset, node_id).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{NodeIdentity, NodeKind};

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
        let _guard = node_registration::lock_node_registration(&state, true).await?;
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
        assert!(!node_registration::node_limit_reached(49));
        assert!(node_registration::node_limit_reached(50));
        assert!(node_registration::node_limit_reached(51));
    }

    #[tokio::test]
    async fn concurrent_registrations_cannot_exceed_fifty_nodes() {
        let directory = tempfile::tempdir().expect("temporary directory should be created");
        let repository = crate::storage::Repository::open(&directory.path().join("nodes.db"))
            .await
            .expect("repository should open");
        let state = AppState::new(repository, crate::transport::NodeTransport::new().unwrap())
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
        let state = AppState::new(repository, crate::transport::NodeTransport::new().unwrap())
            .await
            .expect("state should initialize");
        for index in 0..51 {
            insert_test_node(&state, index)
                .await
                .expect("fixture node should be inserted");
        }

        assert!(node_registration::lock_node_registration(&state, true)
            .await
            .is_err());
        state
            .repository
            .mark_node_for_deletion("node-0")
            .await
            .expect("first node should be removed from monitoring");
        assert!(node_registration::lock_node_registration(&state, true)
            .await
            .is_err());
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
        assert!(node_registration::lock_node_registration(&state, true)
            .await
            .is_err());
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
}
