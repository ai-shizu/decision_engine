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

// M3 Phase 0 SQLCipher / typed objc2 Keychain link probe
// (docs/m3_action_plan.md §4.1 / §4.2.1). Feature-gated so the default desktop
// build never pulls rusqlite/SQLCipher or Apple security bindings.
#[cfg(feature = "secure-vault")]
mod db;
#[cfg(feature = "secure-vault")]
mod commands_db;

/// Phase 10: Taptic Engine IPC (soft no-op off iOS).
mod haptics;

#[cfg(all(feature = "secure-vault", target_vendor = "apple"))]
mod analytics;

#[cfg(all(feature = "secure-vault", target_vendor = "apple"))]
mod ocr;

// M10 local RAG (chunk → embed → sqlite-vec). Needs both LLM worker and vault.
#[cfg(all(
    feature = "pocket-brain",
    feature = "secure-vault",
    target_vendor = "apple"
))]
mod rag;

use std::sync::Arc;

use engine::EngineManager;
use knowledge::NetworkPolicyStore;
#[cfg(not(mobile))]
use tauri::webview::{DownloadEvent, NewWindowResponse};
use tauri::RunEvent;
#[cfg(all(feature = "secure-vault", target_vendor = "apple"))]
use tauri::Manager;
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
    // The governor clone is captured so the iOS lifecycle observer (M7) can signal
    // a memory purge. `llm_governor` is only consumed on the iOS/secure-vault
    // build; the allow keeps other feature combinations warning-clean.
    #[cfg(feature = "pocket-brain")]
    #[allow(unused_variables)]
    let (builder, llm_governor) = {
        let monitor = Arc::new(monitor::MemoryMonitor::new());
        let handle = llm::LlmHandle::spawn(Arc::clone(&monitor));
        let governor = handle.governor();
        (builder.manage(Arc::clone(&monitor)).manage(handle), governor)
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
            commands::es_list,
            commands::consult,
            commands::calendar_sync_ics,
            commands::calendar_sync_apple,
            commands::import_line_single,
            commands::import_line_batch,
            commands::import_classify,
            commands::import_document,
            commands::llm_warm,
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
            llm::commands_llm::llm_embed_binary,
            #[cfg(feature = "pocket-brain")]
            llm::commands_llm::llm_cancel,
            #[cfg(feature = "pocket-brain")]
            llm::commands_llm::llm_is_loaded,
            #[cfg(feature = "pocket-brain")]
            llm::commands_llm::llm_events,
            #[cfg(feature = "pocket-brain")]
            llm::commands_llm::memory_monitor_start,
            #[cfg(feature = "pocket-brain")]
            llm::commands_llm::memory_monitor_stop,
            haptics::haptic_feedback,
            #[cfg(feature = "pocket-brain")]
            llm::commands_model_setup::check_model_exists,
            #[cfg(feature = "pocket-brain")]
            llm::commands_model_setup::pick_local_gguf,
            #[cfg(feature = "pocket-brain")]
            llm::commands_model_setup::import_local_model,
            #[cfg(feature = "pocket-brain")]
            llm::commands_model_setup::open_recommended_model_page,
            #[cfg(all(
                feature = "pocket-brain",
                feature = "secure-vault",
                target_vendor = "apple"
            ))]
            rag::commands_rag::ingest_knowledge,
            #[cfg(all(
                feature = "pocket-brain",
                feature = "secure-vault",
                target_vendor = "apple"
            ))]
            rag::commands_rag::search_knowledge,
            #[cfg(all(
                feature = "pocket-brain",
                feature = "secure-vault",
                target_vendor = "apple"
            ))]
            rag::commands_rag::send_rag_chat,
            #[cfg(all(
                feature = "pocket-brain",
                feature = "secure-vault",
                target_vendor = "apple"
            ))]
            rag::commands_daily::sync_daily_context,
            #[cfg(all(feature = "secure-vault", target_vendor = "apple"))]
            analytics::commands::calculate_gap_analysis,
            #[cfg(all(feature = "secure-vault", target_vendor = "apple"))]
            analytics::commands::get_latest_gap_analysis,
            #[cfg(all(feature = "secure-vault", target_vendor = "apple"))]
            analytics::commands::get_latest_tensor_profile,
            #[cfg(all(feature = "secure-vault", target_vendor = "apple"))]
            analytics::commands::ensure_authoritative_tensor_profile,
            #[cfg(all(feature = "secure-vault", target_vendor = "apple"))]
            analytics::commands_psychometrics::calculate_interaction_pulse,
            #[cfg(all(feature = "secure-vault", target_vendor = "apple"))]
            analytics::commands_psychometrics::evaluate_rasch_scale,
            #[cfg(all(feature = "secure-vault", target_vendor = "apple"))]
            analytics::commands_psychometrics::rasch_select_next_item,
            #[cfg(all(feature = "secure-vault", target_vendor = "apple"))]
            analytics::commands_psychometrics::get_probe_questions,
            #[cfg(all(feature = "secure-vault", target_vendor = "apple"))]
            analytics::commands_psychometrics::probe_next_question,
            #[cfg(all(feature = "secure-vault", target_vendor = "apple"))]
            analytics::commands_psychometrics::probe_submit_answer,
            #[cfg(all(feature = "secure-vault", target_vendor = "apple"))]
            analytics::commands_psychometrics::get_probe_status,
            #[cfg(all(feature = "secure-vault", target_vendor = "apple"))]
            analytics::commands_psychometrics::get_latest_rasch_state,
            #[cfg(all(feature = "secure-vault", target_vendor = "apple"))]
            analytics::commands_oracle::evaluate_digital_twin_scenario,
            #[cfg(all(feature = "secure-vault", target_vendor = "apple"))]
            analytics::commands_oracle::get_twin_identify_status,
            #[cfg(all(feature = "secure-vault", target_vendor = "apple"))]
            analytics::commands_oracle::generate_oracle_payload,
            #[cfg(all(feature = "secure-vault", target_vendor = "apple"))]
            analytics::commands_bias::record_cognitive_distortions,
            #[cfg(all(feature = "secure-vault", target_vendor = "apple"))]
            analytics::commands_bias::get_cognitive_bias_profile,
            #[cfg(all(feature = "secure-vault", target_vendor = "apple"))]
            analytics::commands_finance::record_purchase_with_snapshot,
            #[cfg(all(feature = "secure-vault", target_vendor = "apple"))]
            ocr::commands::ocr_recognize_layout,
            #[cfg(all(
                feature = "pocket-brain",
                feature = "secure-vault",
                target_vendor = "apple"
            ))]
            llm::commands_consult::consult_with_oracle_context,
            #[cfg(all(
                feature = "pocket-brain",
                feature = "secure-vault",
                target_vendor = "apple"
            ))]
            llm::commands_sim::fetch_edinet_company_facts,
            #[cfg(all(
                feature = "pocket-brain",
                feature = "secure-vault",
                target_vendor = "apple"
            ))]
            llm::commands_sim::start_interview_session,
            #[cfg(all(
                feature = "pocket-brain",
                feature = "secure-vault",
                target_vendor = "apple"
            ))]
            llm::commands_sim::review_es_draft,
            #[cfg(all(
                feature = "pocket-brain",
                feature = "secure-vault",
                target_vendor = "apple"
            ))]
            llm::commands_sim::start_multistage_interview,
            #[cfg(all(
                feature = "pocket-brain",
                feature = "secure-vault",
                target_vendor = "apple"
            ))]
            llm::commands_sim::advance_interview_stage,
            #[cfg(all(
                feature = "pocket-brain",
                feature = "secure-vault",
                target_vendor = "apple"
            ))]
            llm::commands_sim::get_interview_session,
            #[cfg(all(feature = "secure-vault", target_vendor = "apple"))]
            commands_db::vault_status,
            #[cfg(all(feature = "secure-vault", target_vendor = "apple"))]
            commands_db::vault_unlock,
            #[cfg(all(feature = "secure-vault", target_vendor = "apple"))]
            commands_db::vault_lock,
            #[cfg(all(feature = "secure-vault", target_vendor = "apple"))]
            commands_db::check_db_health,
            #[cfg(all(feature = "secure-vault", target_vendor = "apple"))]
            commands_db::vault_chat_create,
            #[cfg(all(feature = "secure-vault", target_vendor = "apple"))]
            commands_db::vault_chat_delete,
            #[cfg(all(feature = "secure-vault", target_vendor = "apple"))]
            commands_db::vault_chats_list,
            #[cfg(all(feature = "secure-vault", target_vendor = "apple"))]
            commands_db::vault_message_append,
            #[cfg(all(feature = "secure-vault", target_vendor = "apple"))]
            commands_db::vault_messages_list,
            #[cfg(all(feature = "secure-vault", target_vendor = "apple"))]
            commands_db::vault_events,
        ])
        .setup(move |app| {
            #[cfg(all(feature = "secure-vault", target_vendor = "apple"))]
            {
                // Authentication is deliberately not started from setup. The
                // canonical contract requires an explicit `vault_unlock` user
                // action; setup registers only a locked capability handle.
                let vault = match app.path().app_data_dir() {
                    Ok(data_dir) => {
                        if std::fs::create_dir_all(&data_dir).is_ok() {
                            db::VaultHandle::spawn(data_dir.join(db::VAULT_DATABASE_FILENAME))
                        } else {
                            eprintln!("secure vault unavailable: app data directory setup failed");
                            db::VaultHandle::unavailable()
                        }
                    }
                    Err(_) => {
                        eprintln!("secure vault unavailable: app data directory resolve failed");
                        db::VaultHandle::unavailable()
                    }
                };
                // iOS only: wire background / protected-data-unavailable
                // transitions to an automatic lock before handing the vault to
                // Tauri State (docs/m3_action_plan.md §0, §8 Lifecycle row).
                #[cfg(target_os = "ios")]
                let vault_for_lifecycle = vault.clone();
                if !app.manage(vault) {
                    return Err(
                        std::io::Error::other("secure vault state registration failed").into(),
                    );
                }
                #[cfg(target_os = "ios")]
                db::lifecycle::install_auto_lock(
                    vault_for_lifecycle,
                    #[cfg(feature = "pocket-brain")]
                    llm_governor.clone(),
                );
            }

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
