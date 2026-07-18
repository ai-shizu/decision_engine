//! [C] Tauri command bindings — M4 Phase 0 stubs (docs/architecture_blueprint.md §3.7).
//!
//! **STUBS ONLY — NO LOGIC.** Bodies return `Ok(())`. These commands are defined
//! now to verify signature/type resolution against Tauri (`State`, `Channel`,
//! `AppHandle`); they are NOT yet registered in the `invoke_handler` — registration
//! lands with the real logic (Phase 1: memory monitor, Phase 2/3: LLM).

use tauri::ipc::Channel;
use tauri::{AppHandle, State};

use super::{GenerationParams, LlmHandle, LoadParams, TokenEvent};
use crate::monitor::{MemSample, MemoryMonitor};
use std::sync::Arc;

/// Phase 2: resolve the App-Container model path and load the GGUF.
#[tauri::command]
pub async fn llm_load_model(
    _app: AppHandle,
    _handle: State<'_, LlmHandle>,
    _params: LoadParams,
) -> Result<(), String> {
    Ok(())
}

/// Phase 3: start a generation, streaming `TokenEvent`s over `on_token`.
#[tauri::command]
pub async fn llm_generate(
    _handle: State<'_, LlmHandle>,
    _params: GenerationParams,
    _on_token: Channel<TokenEvent>,
) -> Result<(), String> {
    Ok(())
}

/// Phase 3: cancel an in-flight generation.
#[tauri::command]
pub async fn llm_cancel(_handle: State<'_, LlmHandle>) -> Result<(), String> {
    Ok(())
}

/// Phase 1: start the Jetsam monitor, streaming `MemSample`s over `on_sample`.
#[tauri::command]
pub async fn memory_monitor_start(
    _monitor: State<'_, Arc<MemoryMonitor>>,
    _on_sample: Channel<MemSample>,
    _interval_ms: u64,
    _threshold_bytes: u64,
) -> Result<(), String> {
    Ok(())
}
