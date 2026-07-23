//! [C] Tauri command bindings (docs/architecture_blueprint.md §3.7).
//!
// M5 Phase 2: Maintains the existing invoke_handler registration from M4, only adding the task_id argument.

use std::sync::Arc;

use tauri::ipc::Channel;
use tauri::{AppHandle, State};

use super::embed::EMBED_DEFAULT_N_CTX;
use super::model_path::{internal_model_present, resolve_model_path};
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
    if !internal_model_present(&app)? {
        return Err("内部モデルが未配置です。先にローカル GGUF を取り込んでください。".into());
    }
    let path = resolve_model_path(&app)?;
    let handle = handle.inner().clone();
    tauri::async_runtime::spawn_blocking(move || handle.load(path, params))
        .await
        .map_err(|_| "llm load join failed".to_string())?
}

/// Start a generation, streaming `TokenEvent`s over `on_token`.
/// `task_id` selects extraction mode (`kakeibo_v1`) or plain chat (`None`).
/// Tokens are micro-batched (~30ms / 8 pieces) before IPC send (Phase 9).
#[tauri::command]
pub async fn llm_generate(
    handle: State<'_, LlmHandle>,
    params: GenerationParams,
    task_id: Option<String>,
    on_token: Channel<TokenEvent>,
) -> Result<(), String> {
    handle.generate(params, task_id, on_token).await
}

/// Embed `text` and return little-endian `f32` bytes (Phase 9 binary IPC).
/// Frontend decodes with `Float32Array` / `DataView` — no JSON number array.
#[tauri::command]
pub async fn llm_embed_binary(
    handle: State<'_, LlmHandle>,
    text: String,
    n_ctx: Option<u32>,
) -> Result<Vec<u8>, String> {
    if text.trim().is_empty() {
        return Err("empty embed input".into());
    }
    let n_ctx = n_ctx.unwrap_or(EMBED_DEFAULT_N_CTX);
    let handle = handle.inner().clone();
    tauri::async_runtime::spawn_blocking(move || handle.embed_binary(text, n_ctx))
        .await
        .map_err(|_| "embed binary join failed".to_string())?
}

/// Cancel an in-flight generation.
#[tauri::command]
pub async fn llm_cancel(handle: State<'_, LlmHandle>) -> Result<(), String> {
    handle.cancel();
    Ok(())
}

/// Phase 10: probe whether the GGUF is still resident after Jetsam / purge.
#[tauri::command]
pub async fn llm_is_loaded(handle: State<'_, LlmHandle>) -> Result<bool, String> {
    let handle = handle.inner().clone();
    tauri::async_runtime::spawn_blocking(move || handle.is_loaded())
        .await
        .map_err(|_| "llm ready probe join failed".to_string())?
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

/// Start the thermal / Jetsam monitor, streaming `MemSample`s over `on_sample`.
/// Critical / over-threshold rising-edge lock-free-signals the LLM governor to purge.
#[tauri::command]
pub async fn memory_monitor_start(
    monitor: State<'_, Arc<MemoryMonitor>>,
    handle: State<'_, LlmHandle>,
    on_sample: Channel<MemSample>,
    interval_ms: u64,
    threshold_bytes: u64,
) -> Result<(), String> {
    let governor = handle.governor();
    let purge_hook: crate::monitor::OverThresholdHook = Arc::new({
        let governor = Arc::clone(&governor);
        move || {
            governor.request_purge();
        }
    });
    let degradation_hook: crate::monitor::DegradationHook = Arc::new({
        let governor = Arc::clone(&governor);
        move |level| {
            governor.set_degradation(level);
        }
    });
    let monitor = Arc::clone(monitor.inner());
    tauri::async_runtime::spawn_blocking(move || {
        monitor.start(
            on_sample,
            interval_ms,
            threshold_bytes,
            Some(purge_hook),
            Some(degradation_hook),
        )
    })
    .await
    .map_err(|_| "memory monitor start join failed".to_string())?
}

/// Stop the Jetsam monitor sampler thread.
#[tauri::command]
pub async fn memory_monitor_stop(monitor: State<'_, Arc<MemoryMonitor>>) -> Result<(), String> {
    let monitor = Arc::clone(monitor.inner());
    tauri::async_runtime::spawn_blocking(move || monitor.stop())
        .await
        .map_err(|_| "memory monitor stop join failed".to_string())?
}
