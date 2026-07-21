//! [C] Tauri command bindings (docs/architecture_blueprint.md §3.7).
//!
// M5 Phase 2: Maintains the existing invoke_handler registration from M4, only adding the task_id argument.

use std::sync::Arc;

use tauri::ipc::Channel;
use tauri::{AppHandle, State};

use super::model_path::resolve_model_path;
use super::params::{GenerationParams, LoadParams};
use super::service::{LlmHandle, LlmLifecycleEvent, TokenEvent};
use crate::monitor::{MemSample, MemoryMonitor};

/// Resolve the App-Container model path and load the GGUF (blocking on the worker).
#[tauri::command]
pub async fn llm_load_model(
    app: AppHandle,
    handle: State<'_, LlmHandle>,
    params: LoadParams,
) -> Result<(), String> {
    let path = resolve_model_path(&app)?;
    handle.load(path, params)
}

/// Start a generation, streaming `TokenEvent`s over `on_token`.
/// `task_id` selects extraction mode (`kakeibo_v1`) or plain chat (`None`).
#[tauri::command]
pub async fn llm_generate(
    handle: State<'_, LlmHandle>,
    params: GenerationParams,
    task_id: Option<String>,
    on_token: Channel<TokenEvent>,
) -> Result<(), String> {
    handle.generate(params, task_id, on_token)
}

/// Cancel an in-flight generation.
#[tauri::command]
pub async fn llm_cancel(handle: State<'_, LlmHandle>) -> Result<(), String> {
    handle.cancel();
    Ok(())
}

/// Register a persistent frontend sink for LLM lifecycle events (e.g. an
/// out-of-band memory purge). One sink; a later call replaces it. Mirrors M6's
/// `vault_events`.
#[tauri::command]
pub async fn llm_events(
    handle: State<'_, LlmHandle>,
    on_event: Channel<LlmLifecycleEvent>,
) -> Result<(), String> {
    handle.register_events(on_event)
}

/// Start the Jetsam monitor, streaming `MemSample`s over `on_sample`.
/// Rising-edge `over_threshold` lock-free-signals the LLM governor to purge.
#[tauri::command]
pub async fn memory_monitor_start(
    monitor: State<'_, Arc<MemoryMonitor>>,
    handle: State<'_, LlmHandle>,
    on_sample: Channel<MemSample>,
    interval_ms: u64,
    threshold_bytes: u64,
) -> Result<(), String> {
    let governor = handle.governor();
    let hook: crate::monitor::OverThresholdHook = Arc::new(move || {
        governor.request_purge();
    });
    monitor.start(
        on_sample,
        interval_ms,
        threshold_bytes,
        Some(hook),
    );
    Ok(())
}

/// Stop the Jetsam monitor sampler thread.
#[tauri::command]
pub async fn memory_monitor_stop(monitor: State<'_, Arc<MemoryMonitor>>) -> Result<(), String> {
    monitor.stop();
    Ok(())
}
