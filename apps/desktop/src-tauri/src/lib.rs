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

// M4 on-device LLM scaffold (docs/architecture_blueprint.md). Feature-gated so the
// default desktop build is byte-identical to main. Phase 0 = module tree only;
// command registration + State management land in Phase 1+.
#[cfg(feature = "pocket-brain")]
mod llm;
#[cfg(feature = "pocket-brain")]
mod monitor;

// M3 Phase 0-A SQLCipher / Security.framework link probe
// (docs/m3_action_plan.md §4.1). Feature-gated so the default desktop build
// never pulls rusqlite/SQLCipher.
#[cfg(feature = "secure-vault")]
mod db;
#[cfg(feature = "secure-vault")]
mod commands_db;

use std::sync::Arc;

use engine::EngineManager;
use knowledge::NetworkPolicyStore;
#[cfg(not(mobile))]
use tauri::webview::{DownloadEvent, NewWindowResponse};
use tauri::RunEvent;
#[cfg(not(mobile))]
use tauri::WebviewWindowBuilder;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let engine = EngineManager::new();
    let engine_for_exit = Arc::clone(&engine);

    let builder = tauri::Builder::default()
        .manage(Arc::clone(&engine))
        .manage(NetworkPolicyStore::new());

    // M4 pocket-brain: spawn the LLM worker + memory monitor and register them in
    // State. Entirely feature-gated — the default desktop build is byte-identical.
    #[cfg(feature = "pocket-brain")]
    let builder = {
        let monitor = Arc::new(monitor::MemoryMonitor::new());
        builder
            .manage(Arc::clone(&monitor))
            .manage(llm::LlmHandle::spawn(Arc::clone(&monitor)))
    };

    builder
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
            #[cfg(feature = "pocket-brain")]
            llm::commands_llm::llm_load_model,
            #[cfg(feature = "pocket-brain")]
            llm::commands_llm::llm_generate,
            #[cfg(feature = "pocket-brain")]
            llm::commands_llm::llm_cancel,
            #[cfg(feature = "pocket-brain")]
            llm::commands_llm::memory_monitor_start,
            #[cfg(feature = "pocket-brain")]
            llm::commands_llm::memory_monitor_stop,
            #[cfg(feature = "secure-vault")]
            commands_db::verify_sqlcipher_link_and_keychain,
        ])
        .setup(move |app| {
            #[cfg(not(mobile))]
            {
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
            }
            #[cfg(mobile)]
            {
                // iOS: Tauri mobile runtime が config から main webview を生成する。
                // sidecar は存在しないため start() を呼ばない（engine は "not ready" のまま）。
                let _ = &engine;
                let _ = app;
            }
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
