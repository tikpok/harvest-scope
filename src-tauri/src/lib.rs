mod adapters;
mod commands;
mod domain;
mod error;
mod services;
mod state;
mod storage;

use std::{
    any::Any,
    panic::{catch_unwind, AssertUnwindSafe},
    path::PathBuf,
    time::Duration,
};

use state::AppState;
use tauri::{Manager, WindowEvent};

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let result = catch_unwind(AssertUnwindSafe(run_app));
    let failure = match result {
        Ok(Ok(())) => return,
        Ok(Err(error)) => error.to_string(),
        Err(payload) => panic_message(payload.as_ref()),
    };
    report_startup_failure(&failure);
    std::process::exit(1);
}

fn run_app() -> tauri::Result<()> {
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
            match services::power::register(app.handle().clone(), state.clone()) {
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
            commands::get_recent_harvest_events,
            commands::upsert_node,
            commands::delete_node,
            commands::move_node,
            commands::pause_node,
            commands::resume_node,
            commands::set_node_harvest_metrics_visible,
            commands::refresh_node,
            commands::refresh_all_nodes,
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
}

fn panic_message(payload: &(dyn Any + Send)) -> String {
    payload
        .downcast_ref::<String>()
        .cloned()
        .or_else(|| {
            payload
                .downcast_ref::<&str>()
                .map(|value| (*value).to_owned())
        })
        .unwrap_or_else(|| "不明な起動エラーが発生しました".to_owned())
}

fn report_startup_failure(message: &str) {
    let log_path = startup_error_log_path();
    let log_contents = format!(
        "{} Harvest Scope startup failure\n{}\n",
        chrono::Utc::now().to_rfc3339(),
        message
    );
    let saved_path = log_path.as_ref().and_then(|path| {
        path.parent()
            .and_then(|parent| std::fs::create_dir_all(parent).ok())?;
        std::fs::write(path, log_contents.as_bytes()).ok()?;
        Some(path.display().to_string())
    });
    let details = match saved_path {
        Some(path) => {
            format!("Harvest Scopeの起動に失敗しました。\n\n{message}\n\n診断ログ: {path}")
        }
        None => format!("Harvest Scopeの起動に失敗しました。\n\n{message}"),
    };
    show_startup_error(&details);
}

fn startup_error_log_path() -> Option<PathBuf> {
    std::env::var_os("LOCALAPPDATA").map(|root| {
        PathBuf::from(root)
            .join("com.tikpok.node-watcher")
            .join("startup-error.log")
    })
}

#[cfg(target_os = "windows")]
fn show_startup_error(message: &str) {
    use windows_sys::Win32::UI::WindowsAndMessaging::{MessageBoxW, MB_ICONERROR, MB_OK};

    let title = "Harvest Scope - 起動エラー"
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect::<Vec<_>>();
    let body = message
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect::<Vec<_>>();
    // SAFETY: both UTF-16 buffers are NUL-terminated and live for the call.
    unsafe {
        MessageBoxW(
            std::ptr::null_mut(),
            body.as_ptr(),
            title.as_ptr(),
            MB_OK | MB_ICONERROR,
        );
    }
}

#[cfg(not(target_os = "windows"))]
fn show_startup_error(message: &str) {
    eprintln!("{message}");
}
