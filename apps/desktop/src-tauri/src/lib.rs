#[doc(hidden)]
pub mod artifact_auth;
mod commands;
mod engine;
#[doc(hidden)]
pub mod ipc_contract;
#[doc(hidden)]
pub mod knowledge;
#[doc(hidden)]
pub mod os_sandbox;
mod paths;
#[doc(hidden)]
pub mod webview_policy;

use std::sync::Arc;

use engine::EngineManager;
use knowledge::NetworkPolicyStore;
use tauri::webview::{DownloadEvent, NewWindowResponse};
use tauri::{RunEvent, WebviewWindowBuilder};

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let engine = EngineManager::new();
    let engine_for_exit = Arc::clone(&engine);

    tauri::Builder::default()
        .manage(Arc::clone(&engine))
        .manage(NetworkPolicyStore::new())
        .invoke_handler(tauri::generate_handler![
            commands::engine_ready,
            commands::engine_health,
            commands::record_load,
            commands::record_save,
            commands::calendar_event_dates,
            commands::import_stats,
            commands::es_view,
            commands::consult,
            commands::calendar_sync_ics,
            commands::calendar_sync_apple,
            commands::import_line_single,
            commands::import_line_batch,
            commands::import_classify,
            commands::import_document,
            commands::settings_get,
            commands::settings_save_fixed,
            commands::settings_run_profiler,
            commands::oracle_payload,
            commands::oracle_report,
            commands::twin_forecast,
            commands::tensor_rebuild,
            commands::profile_source_code,
            commands::narrative_compile,
            commands::knowledge_fetch_pending,
            commands::knowledge_research,
            commands::knowledge_policy_get,
            commands::knowledge_policy_set,
            commands::probe_status,
            commands::probe_next,
            commands::probe_answer,
            commands::context_manifest_latest,
        ])
        .setup(move |app| {
            let handle = app.handle().clone();
            let manager = Arc::clone(&engine);
            tauri::async_runtime::block_on(manager.start(handle)).map_err(std::io::Error::other)?;

            let window_config = app
                .config()
                .app
                .windows
                .first()
                .ok_or_else(|| std::io::Error::other("main window config is missing"))?;
            WebviewWindowBuilder::from_config(app.handle(), window_config)?
                .on_navigation(|url| webview_policy::current_navigation_allowed(url.as_str()))
                .on_new_window(|_, _| NewWindowResponse::Deny)
                .on_download(|_, event| !matches!(event, DownloadEvent::Requested { .. }))
                .build()?;
            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(move |_, event| {
            if matches!(event, RunEvent::Exit) {
                engine_for_exit.shutdown();
            }
        });
}
