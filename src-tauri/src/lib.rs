mod adapters;
mod commands;
mod domain;
mod error;
mod services;
mod state;
mod storage;

use std::time::Duration;

use state::AppState;
use tauri::{Manager, WindowEvent};

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let _ = tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .try_init();

    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _, _| {
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.show();
                let _ = window.set_focus();
            }
        }))
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            None,
        ))
        .setup(|app| {
            let data_dir = app.path().app_local_data_dir()?.join("node-watcher.db");
            let client = reqwest::Client::builder()
                .connect_timeout(Duration::from_secs(2))
                .timeout(Duration::from_secs(5))
                .pool_max_idle_per_host(2)
                .user_agent("Node-Watcher/0.1")
                .build()?;
            let repository = tauri::async_runtime::block_on(storage::Repository::open(&data_dir))?;
            let state = tauri::async_runtime::block_on(AppState::new(repository, client))?;
            app.manage(state.clone());
            #[cfg(target_os = "windows")]
            match services::power::register(state.clone()) {
                Ok(listener) => {
                    app.manage(listener);
                }
                Err(error) => {
                    tracing::warn!(%error, "failed to register Windows suspend/resume notifications");
                }
            }
            services::desktop::setup(app, state.clone())?;
            services::monitor::start(app.handle().clone(), state.clone());
            let handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                services::desktop::refresh_tray(&handle, &state).await;
            });
            Ok(())
        })
        .on_window_event(|window, event| {
            if let WindowEvent::CloseRequested { api, .. } = event {
                let state = window.state::<AppState>();
                if state
                    .background_monitoring
                    .load(std::sync::atomic::Ordering::Relaxed)
                {
                    api.prevent_close();
                    let _ = window.hide();
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            commands::list_nodes,
            commands::get_dashboard,
            commands::get_topology,
            commands::get_recent_network_events,
            commands::upsert_node,
            commands::delete_node,
            commands::move_node,
            commands::pause_node,
            commands::resume_node,
            commands::refresh_node,
            commands::get_node_detail,
            commands::query_metrics,
            commands::list_network_interfaces,
            commands::scan_subnet,
            commands::cancel_scan,
            commands::list_alerts,
            commands::get_alert,
            commands::acknowledge_alert,
            commands::run_node_diagnostic,
            commands::get_diagnostic_run,
            commands::list_node_diagnostics,
            commands::take_pending_navigation,
            commands::get_settings,
            commands::update_settings,
            commands::export_csv,
        ])
        .run(tauri::generate_context!())
        .expect("Node Watcherの起動に失敗しました");
}
