mod adapters;
mod commands;
mod domain;
mod error;
mod services;
mod state;
mod storage;
mod transport;

use std::{
    any::Any,
    panic::{catch_unwind, AssertUnwindSafe},
    path::{Path, PathBuf},
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

    let builder = tauri::Builder::default();
    #[cfg(not(debug_assertions))]
    let builder = builder.plugin(tauri_plugin_single_instance::init(|app, _, _| {
        if let Some(window) = app.get_webview_window("main") {
            let _ = window.show();
            let _ = window.set_focus();
        }
    }));
    #[cfg(debug_assertions)]
    let builder = builder;

    builder
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            None,
        ))
        .setup(|app| {
            let data_dir = app_database_path(app)?;
            let transport = transport::NodeTransport::new()?;
            let repository = tauri::async_runtime::block_on(storage::Repository::open(&data_dir))?;
            let state = tauri::async_runtime::block_on(AppState::new(repository, transport))?;
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
            commands::schedule_deleted_node_purge(
                state.clone(),
                state.current_node_deletion_generation(),
            );
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
            commands::list_supernode_program_nodes,
            commands::upsert_node,
            commands::delete_node,
            commands::move_node,
            commands::pause_node,
            commands::resume_node,
            commands::confirm_insecure_http,
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
            commands::delete_resolved_alerts,
            commands::run_node_diagnostic,
            commands::get_diagnostic_run,
            commands::list_node_diagnostics,
            commands::take_pending_navigation,
            commands::get_settings,
            commands::update_settings,
            commands::export_node_config,
            commands::import_node_config,
            commands::export_csv,
        ])
        .run(tauri::generate_context!())
}

fn app_database_path(app: &tauri::App) -> tauri::Result<PathBuf> {
    let database_name = if cfg!(debug_assertions) {
        "harvest-scope-dev.db"
    } else {
        "harvest-scope.db"
    };
    let data_dir = app.path().app_local_data_dir()?.join(database_name);
    if cfg!(debug_assertions) {
        return Ok(data_dir);
    }
    migrate_legacy_database(&data_dir);
    Ok(data_dir)
}

fn migrate_legacy_database(data_dir: &Path) {
    if data_dir.exists() {
        return;
    }
    let Some(legacy_data_dir) = legacy_app_data_file("node-watcher.db") else {
        return;
    };
    if !legacy_data_dir.exists() {
        return;
    }
    let result = data_dir
        .parent()
        .map(std::fs::create_dir_all)
        .transpose()
        .and_then(|_| std::fs::copy(&legacy_data_dir, data_dir).map(|_| ()));
    if let Err(error) = result {
        tracing::warn!(%error, legacy = %legacy_data_dir.display(), target = %data_dir.display(), "failed to migrate legacy database");
    }
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
    legacy_app_data_file("startup-error.log").map(|path| {
        path.parent()
            .and_then(|root| root.parent())
            .map(|root| {
                root.join("com.tikpok.harvest-scope")
                    .join("startup-error.log")
            })
            .unwrap_or(path)
    })
}

fn legacy_app_data_file(file_name: &str) -> Option<PathBuf> {
    std::env::var_os("LOCALAPPDATA").map(|root| {
        PathBuf::from(root)
            .join("com.tikpok.node-watcher")
            .join(file_name)
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
