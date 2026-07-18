use tauri::{
    image::Image,
    menu::{Menu, MenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    AppHandle, Emitter, Manager,
};
use tauri_plugin_notification::NotificationExt;

use crate::{
    domain::{NodeStatus, NodeSummary},
    services::monitor,
    state::AppState,
};

const TRAY_ID: &str = "monitor-center";
static TRAY_REFRESH_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

struct TrayMenuState {
    status: MenuItem<tauri::Wry>,
}

struct TrayMenu {
    menu: Menu<tauri::Wry>,
    status: MenuItem<tauri::Wry>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrayStatus {
    Healthy,
    Warning,
    Critical,
    Inactive,
}

pub fn aggregate_status(nodes: &[NodeSummary]) -> TrayStatus {
    let enabled = nodes.iter().filter(|node| node.enabled).collect::<Vec<_>>();
    if enabled.is_empty() {
        return TrayStatus::Inactive;
    }
    if enabled
        .iter()
        .any(|node| matches!(node.status, NodeStatus::Critical | NodeStatus::Offline))
    {
        TrayStatus::Critical
    } else if enabled
        .iter()
        .any(|node| node.status == NodeStatus::Warning)
    {
        TrayStatus::Warning
    } else if enabled
        .iter()
        .all(|node| node.status == NodeStatus::Healthy)
    {
        TrayStatus::Healthy
    } else {
        TrayStatus::Inactive
    }
}

fn icon(status: TrayStatus) -> Image<'static> {
    let color = match status {
        TrayStatus::Healthy => [85, 214, 190, 255],
        TrayStatus::Warning => [255, 202, 92, 255],
        TrayStatus::Critical => [255, 107, 134, 255],
        TrayStatus::Inactive => [132, 151, 174, 255],
    };
    let mut rgba = vec![0u8; 32 * 32 * 4];
    for y in 0..32 {
        for x in 0..32 {
            let dx = x as i32 - 15;
            let dy = y as i32 - 15;
            let distance = dx * dx + dy * dy;
            if distance <= 13 * 13 {
                let offset = (y * 32 + x) * 4;
                let pixel = if distance >= 11 * 11 {
                    [240, 245, 250, 255]
                } else {
                    color
                };
                rgba[offset..offset + 4].copy_from_slice(&pixel);
            }
        }
    }
    Image::new_owned(rgba, 32, 32)
}

fn status_text(nodes: &[NodeSummary]) -> String {
    let enabled = nodes.iter().filter(|node| node.enabled).count();
    let warnings = nodes
        .iter()
        .filter(|node| node.enabled && node.status == NodeStatus::Warning)
        .count();
    let critical = nodes
        .iter()
        .filter(|node| {
            node.enabled && matches!(node.status, NodeStatus::Critical | NodeStatus::Offline)
        })
        .count();
    match aggregate_status(nodes) {
        TrayStatus::Healthy => format!("全{enabled}ノード正常"),
        TrayStatus::Warning => format!("警告 {warnings}件"),
        TrayStatus::Critical => format!("重大・オフライン {critical}件"),
        TrayStatus::Inactive if enabled == 0 => "監視対象なし".into(),
        TrayStatus::Inactive => "状態を確認中".into(),
    }
}

fn show_window(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
}

pub async fn request_navigation(app: AppHandle, state: AppState, path: String) {
    *state.pending_navigation.lock().await = Some(path);
    show_window(&app);
    let _ = app.emit("navigate-requested", ());
}

async fn navigate_latest_alert(app: AppHandle, state: AppState) {
    let path = state
        .repository
        .list_alerts()
        .await
        .ok()
        .and_then(|alerts| alerts.into_iter().find(|alert| alert.state != "resolved"))
        .map(|alert| format!("/alerts/{}", alert.id))
        .unwrap_or_else(|| "/alerts".into());
    request_navigation(app, state, path).await;
}

async fn refresh_all(app: AppHandle, state: AppState) {
    if let Ok(nodes) = state.repository.list_stored_nodes(true).await {
        for node in nodes {
            let app = app.clone();
            let state = state.clone();
            tauri::async_runtime::spawn(async move {
                let _ = monitor::monitor_node(app, state, node, true).await;
            });
        }
    }
}

fn build_menu(app: &AppHandle, summary: &str) -> tauri::Result<TrayMenu> {
    let status = MenuItem::with_id(app, "status", summary, false, None::<&str>)?;
    let latest = MenuItem::with_id(app, "latest-alert", "最新の障害を表示", true, None::<&str>)?;
    let refresh = MenuItem::with_id(app, "refresh-all", "全ノードを更新", true, None::<&str>)?;
    let open = MenuItem::with_id(app, "open", "Harvest Scopeを開く", true, None::<&str>)?;
    let settings = MenuItem::with_id(app, "settings", "設定", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "終了", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&status, &latest, &refresh, &open, &settings, &quit])?;
    Ok(TrayMenu { menu, status })
}

pub fn setup(app: &tauri::App, state: AppState) -> tauri::Result<()> {
    let tray_menu = build_menu(app.handle(), "状態を確認中")?;
    TrayIconBuilder::with_id(TRAY_ID)
        .icon(icon(TrayStatus::Inactive))
        .tooltip("Harvest Scope - 状態を確認中")
        .menu(&tray_menu.menu)
        .show_menu_on_left_click(false)
        .on_tray_icon_event(|tray, event| {
            if matches!(
                event,
                TrayIconEvent::Click {
                    button: MouseButton::Left,
                    button_state: MouseButtonState::Up,
                    ..
                }
            ) {
                show_window(tray.app_handle());
            }
        })
        .on_menu_event(move |app, event| match event.id.as_ref() {
            "open" => show_window(app),
            "settings" => {
                let app = app.clone();
                let state = state.clone();
                tauri::async_runtime::spawn(request_navigation(app, state, "/settings".into()));
            }
            "latest-alert" => {
                let app = app.clone();
                let state = state.clone();
                tauri::async_runtime::spawn(navigate_latest_alert(app, state));
            }
            "refresh-all" => {
                let app = app.clone();
                let state = state.clone();
                tauri::async_runtime::spawn(refresh_all(app, state));
            }
            "quit" => app.exit(0),
            _ => {}
        })
        .build(app)?;
    app.manage(TrayMenuState {
        status: tray_menu.status,
    });
    Ok(())
}

pub async fn refresh_tray(app: &AppHandle, state: &AppState) {
    let _guard = TRAY_REFRESH_LOCK.lock().await;
    let Ok(nodes) = state.repository.list_nodes().await else {
        if let Some(tray) = app.tray_by_id(TRAY_ID) {
            let _ = tray.set_icon(Some(icon(TrayStatus::Inactive)));
            let _ = tray.set_tooltip(Some("Harvest Scope - 監視データを取得できません"));
        }
        return;
    };
    let status = aggregate_status(&nodes);
    let summary = status_text(&nodes);
    if let Some(tray) = app.tray_by_id(TRAY_ID) {
        let _ = tray.set_icon(Some(icon(status)));
        let _ = tray.set_tooltip(Some(format!("Harvest Scope - {summary}")));
        if let Some(menu) = app.try_state::<TrayMenuState>() {
            let should_update = menu
                .status
                .text()
                .map_or(true, |current| current != summary);
            if should_update {
                let _ = menu.status.set_text(&summary);
            }
        }
    }
}

pub async fn notify_alert(
    app: AppHandle,
    state: AppState,
    title: &str,
    body: &str,
    alert_id: String,
) {
    let enabled = state.settings.read().await.notification_enabled;
    if !enabled {
        return;
    }

    #[cfg(windows)]
    {
        let app_for_action = app.clone();
        let state_for_action = state.clone();
        let route = format!("/alerts/{alert_id}");
        let action = format!("open-alert:{alert_id}");
        let result = tauri_winrt_notification::Toast::new(&app.config().identifier)
            .title(title)
            .text1(body)
            .add_button("障害を表示", &action)
            .on_activated(move |_| {
                let app = app_for_action.clone();
                let state = state_for_action.clone();
                let route = route.clone();
                tauri::async_runtime::spawn(request_navigation(app, state, route));
                Ok(())
            })
            .show();
        if result.is_ok() {
            return;
        }
    }

    let _ = app.notification().builder().title(title).body(body).show();
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{MonitorMode, NodeKind};

    fn node(status: NodeStatus, enabled: bool) -> NodeSummary {
        NodeSummary {
            id: "node".into(),
            display_name: "Node".into(),
            kind: NodeKind::Symbol,
            monitor_mode: MonitorMode::Api,
            base_url: "http://127.0.0.1:3000".into(),
            network: None,
            version: None,
            roles: Vec::new(),
            observed_roles: Vec::new(),
            rest_private: false,
            allow_insecure_http: true,
            configured_supernode_id: None,
            enabled,
            favorite: false,
            status,
            height: None,
            finalized_height: None,
            lag: None,
            peer_count: None,
            latency_ms: None,
            last_seen_at: None,
            last_error: None,
            show_harvest_metrics: false,
            delegated_harvester_count: None,
            delegated_harvester_delta: None,
            total_importance: None,
            total_balance_micro_xym: None,
            harvest_metric_error: None,
            latest_harvest_at: None,
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

    #[test]
    fn tray_status_uses_highest_severity() {
        assert_eq!(aggregate_status(&[]), TrayStatus::Inactive);
        assert_eq!(
            aggregate_status(&[node(NodeStatus::Healthy, true)]),
            TrayStatus::Healthy
        );
        assert_eq!(
            aggregate_status(&[
                node(NodeStatus::Healthy, true),
                node(NodeStatus::Warning, true)
            ]),
            TrayStatus::Warning
        );
        assert_eq!(
            aggregate_status(&[
                node(NodeStatus::Warning, true),
                node(NodeStatus::Offline, true)
            ]),
            TrayStatus::Critical
        );
        assert_eq!(
            aggregate_status(&[node(NodeStatus::Critical, false)]),
            TrayStatus::Inactive
        );
    }
}
