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
mod commands_db;
#[cfg(feature = "secure-vault")]
mod db;

/// Phase 10: Taptic Engine IPC (soft no-op off iOS).
mod haptics;

/// Phase 14: Inner Coliseum — irreversible tactic compile + oni leak gate (I-22).
#[cfg(feature = "pocket-brain")]
mod coliseum;

// LLM Flavor Layer — type skeleton + two-key witness seal (F-1).
// Parent: docs/SPEC_FLAVOR_LAYER.md v2. Top-level (NOT under llm/) so seal
// proofs do not require llama-cpp-2 / cmake. Opt-in only (FLV-R-4).
#[cfg(feature = "flavor-layer")]
#[doc(hidden)]
pub mod flavor;

/// Seal probe: crate-root brace construction of VerifiedFlavor → E0451.
/// Driven by `cargo rustc --features flavor-layer --lib -- --cfg flavor_seal_probe_verified`.
#[cfg(all(feature = "flavor-layer", flavor_seal_probe_verified))]
#[allow(dead_code)]
fn flavor_seal_probe_verified(checked: crate::flavor::checked::Checked) {
    let _ = crate::flavor::verified::VerifiedFlavor { checked };
}

// BLACKBOX SIMULATOR — deterministic finance-sim core
// (docs/SPEC_BLACKBOX_SIMULATOR.md). As of Phase 3 this carries the market
// kernel, the operating model, the turn driver and stimulus planting; command
// registration and FE wiring land in later adjudicated phases. Zero extra
// dependencies; default builds are byte-identical.
#[cfg(feature = "blackbox-sim")]
#[doc(hidden)]
pub mod blackbox_sim;

// Phase 5: Tauri command layer + dedicated sim worker thread wiring the
// above core to production (docs/SPEC_BLACKBOX_SIMULATOR.md §15). The only
// module besides `db::blackbox_repo` allowed to import both `blackbox_sim`
// and `db` (see `blackbox_arena::handle`'s module header).
#[cfg(feature = "blackbox-sim")]
mod blackbox_arena;

#[cfg(all(feature = "secure-vault", target_vendor = "apple"))]
mod analytics;

#[cfg(all(feature = "secure-vault", target_vendor = "apple"))]
mod ocr;

/// M20 データ連携 Part 2: EventKit calendar read bridge.
#[cfg(all(feature = "secure-vault", target_vendor = "apple"))]
mod calendar;

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
#[cfg(all(feature = "secure-vault", target_vendor = "apple"))]
use tauri::Manager;
use tauri::RunEvent;
#[cfg(not(mobile))]
use tauri::WebviewWindowBuilder;

/// Minimal `log::Log` backend writing to stderr.
///
/// 2026-07-24 iOS-device investigation: every `log::error!`/`log::info!` call
/// already scattered through this codebase (`llm/model_path.rs`,
/// `llm/service.rs`, `commands.rs`, …) was a silent no-op — the `log` facade
/// crate does nothing at all until a concrete `Log` implementation is
/// registered via `log::set_logger`, and none ever was. Every earlier device
/// log capture in this investigation only ever showed llama.cpp's own C++
/// stderr output; not one Rust-level `log::` line was ever visible, on this
/// build or any prior one.
///
/// `idevicesyslog -p Coraxis` already proven (this investigation) to capture
/// this process's raw stdout/stderr as `[stderr]`-tagged lines, so routing
/// through `eprintln!` makes every existing and future `log::` call visible
/// via the exact same capture method with no new dependency.
struct StderrLogger;

impl log::Log for StderrLogger {
    fn enabled(&self, _metadata: &log::Metadata) -> bool {
        true
    }

    fn log(&self, record: &log::Record) {
        eprintln!(
            "[{}] {}: {}",
            record.level(),
            record.target(),
            record.args()
        );
    }

    fn flush(&self) {}
}

static STDERR_LOGGER: StderrLogger = StderrLogger;

fn install_stderr_logger() {
    // `set_logger` errors only if a logger was already installed (e.g. a
    // second `run()` call in a test) — never fatal, so ignore and proceed;
    // `set_max_level` still runs for the already-installed case too, if this
    // is ever reached from a fresh process it always succeeds first.
    if log::set_logger(&STDERR_LOGGER).is_ok() {
        log::set_max_level(log::LevelFilter::Debug);
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    install_stderr_logger();

    // `egress-live` builds reqwest with `rustls-no-provider`, so rustls has NO
    // compiled-in default CryptoProvider. Any TLS client constructed before one
    // is installed panics ("no process-level CryptoProvider available"). On iOS
    // that panic happens inside the `extern "C"` UIApplicationDelegate callback
    // (`did_finish_launching`), where it cannot unwind → `panic_cannot_unwind`
    // → SIGABRT at launch (2026-07-25 device crash). Install once, here, before
    // anything can build a client. `ReqwestTransport::new`'s own call then
    // becomes a no-op (it already discards the Result).
    #[cfg(feature = "egress-live")]
    {
        let _ = rustls::crypto::ring::default_provider().install_default();
    }

    let engine = EngineManager::new();
    let engine_for_exit = Arc::clone(&engine);

    let builder = tauri::Builder::default()
        .plugin(tauri_plugin_fs::init())
        .plugin(tauri_plugin_dialog::init())
        .manage(Arc::clone(&engine))
        .manage(NetworkPolicyStore::new());

    // BLACKBOX SIMULATOR sim worker: always constructible (play with no
    // back-end is a hard requirement), independent of `secure-vault`/Apple.
    // The vault capability, when the build has one, is attached below inside
    // `.setup()` once the vault itself exists.
    #[cfg(feature = "blackbox-sim")]
    let builder = builder.manage(blackbox_arena::handle::BlackboxSimHandle::spawn());

    // M4 pocket-brain: spawn the LLM worker + memory monitor and register them in
    // State. Entirely feature-gated — the default desktop build is byte-identical.
    // The governor clone is captured so the iOS lifecycle observer (M7) can signal
    // a memory purge. `llm_governor` is only consumed on the iOS/secure-vault
    // build; the allow keeps other feature combinations warning-clean.
    #[cfg(feature = "pocket-brain")]
    #[allow(unused_variables)]
    let (builder, llm_governor) = {
        let monitor = Arc::new(monitor::MemoryMonitor::new());
        let handle = llm::LlmHandle::spawn(Arc::clone(&monitor)).unwrap_or_else(|error| {
            eprintln!("Coraxis LLM worker unavailable: {error}");
            llm::LlmHandle::unavailable(error)
        });
        let governor = handle.governor();
        (
            builder.manage(Arc::clone(&monitor)).manage(handle),
            governor,
        )
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
            llm::brain::brain_load_gguf,
            #[cfg(feature = "pocket-brain")]
            llm::brain::brain_generate_stream,
            #[cfg(feature = "pocket-brain")]
            llm::brain::brain_is_ready,
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
            llm::commands_model_setup::prepare_model_import_dest,
            #[cfg(feature = "pocket-brain")]
            llm::commands_model_setup::confirm_model_imported,
            #[cfg(feature = "pocket-brain")]
            llm::commands_model_setup::open_recommended_model_page,
            #[cfg(feature = "pocket-brain")]
            coliseum::commands::assign_interview_turn_ids,
            #[cfg(feature = "pocket-brain")]
            coliseum::commands::seal_interview_evaluation,
            #[cfg(feature = "pocket-brain")]
            coliseum::commands::seal_metacognitive_debrief,
            #[cfg(all(
                feature = "pocket-brain",
                feature = "secure-vault",
                target_vendor = "apple"
            ))]
            coliseum::commands::seal_interview_evaluation_from_session,
            #[cfg(all(
                feature = "pocket-brain",
                feature = "secure-vault",
                target_vendor = "apple"
            ))]
            coliseum::commands::seal_metacognitive_debrief_from_session,
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
            rag::commands_rag::ingest_company_knowledge,
            #[cfg(all(
                feature = "pocket-brain",
                feature = "secure-vault",
                target_vendor = "apple"
            ))]
            rag::commands_rag::ingest_line_history,
            #[cfg(all(
                feature = "pocket-brain",
                feature = "secure-vault",
                target_vendor = "apple"
            ))]
            rag::commands_rag::ingest_line_history_path,
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
            analytics::commands_finance::get_cognitive_month_view,
            #[cfg(all(feature = "secure-vault", target_vendor = "apple"))]
            analytics::commands_finance::get_cognitive_commitments,
            #[cfg(all(feature = "secure-vault", target_vendor = "apple"))]
            ocr::commands::ocr_recognize_layout,
            #[cfg(all(feature = "secure-vault", target_vendor = "apple"))]
            calendar::commands::fetch_apple_calendar_events,
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
            llm::commands_sim::enrich_company_facts_from_edinet,
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
            #[cfg(all(
                feature = "pocket-brain",
                feature = "secure-vault",
                target_vendor = "apple"
            ))]
            llm::commands_sim::analyze_company_knowledge,
            #[cfg(all(
                feature = "pocket-brain",
                feature = "secure-vault",
                target_vendor = "apple"
            ))]
            llm::commands_sim::ingest_session_memory,
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
            #[cfg(feature = "blackbox-sim")]
            blackbox_arena::commands::bxs_start_campaign,
            #[cfg(feature = "blackbox-sim")]
            blackbox_arena::commands::bxs_get_view,
            #[cfg(feature = "blackbox-sim")]
            blackbox_arena::commands::bxs_submit_decision,
            #[cfg(feature = "blackbox-sim")]
            blackbox_arena::commands::bxs_advance,
            #[cfg(feature = "blackbox-sim")]
            blackbox_arena::commands::bxs_abort,
            #[cfg(feature = "blackbox-sim")]
            blackbox_arena::commands::bxs_take_flavor,
            #[cfg(feature = "blackbox-sim")]
            blackbox_arena::commands::bxs_load_generation,
            #[cfg(feature = "blackbox-sim")]
            blackbox_arena::commands::bxs_estimate_profile,
            #[cfg(all(
                feature = "blackbox-sim",
                feature = "secure-vault",
                target_vendor = "apple"
            ))]
            blackbox_arena::commands::bxs_latest_profile,
            #[cfg(all(
                feature = "blackbox-sim",
                feature = "secure-vault",
                target_vendor = "apple"
            ))]
            blackbox_arena::commands::bxs_list_profiles,
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
                // BLACKBOX SIMULATOR: hand the sim worker a copy of the vault
                // capability so its settle-time flush can reach storage. Best
                // effort by construction — `unavailable()` vaults clone and
                // attach the same as a real one, and every flush already
                // tolerates a non-`Unlocked` vault via `VaultErrorCode`.
                #[cfg(feature = "blackbox-sim")]
                app.state::<blackbox_arena::handle::BlackboxSimHandle>()
                    .attach_vault(vault.clone());
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
                tauri::async_runtime::block_on(manager.start(handle))
                    .map_err(std::io::Error::other)?;

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
