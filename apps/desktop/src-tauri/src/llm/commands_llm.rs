//! [C] Tauri command bindings (docs/architecture_blueprint.md §3.7).
//!
// M5 Phase 2: Maintains the existing invoke_handler registration from M4, only adding the task_id argument.

use std::sync::Arc;

use tauri::ipc::Channel;
use tauri::{AppHandle, State};

use super::model_path::resolve_model_path;
use super::params::{GenerationParams, LoadParams};
use super::service::{LlmHandle, TokenEvent};
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

/// Start the Jetsam monitor, streaming `MemSample`s over `on_sample`.
#[tauri::command]
pub async fn memory_monitor_start(
    monitor: State<'_, Arc<MemoryMonitor>>,
    on_sample: Channel<MemSample>,
    interval_ms: u64,
    threshold_bytes: u64,
) -> Result<(), String> {
    monitor.start(on_sample, interval_ms, threshold_bytes);
    Ok(())
}

/// Stop the Jetsam monitor sampler thread.
#[tauri::command]
pub async fn memory_monitor_stop(monitor: State<'_, Arc<MemoryMonitor>>) -> Result<(), String> {
    monitor.stop();
    Ok(())
}
