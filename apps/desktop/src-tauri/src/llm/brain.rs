//! Phase 15 — The Brain (on-device inference IPC surface).
//!
//! ## Crate selection
//! **`llama-cpp-2` (=0.1.151, Metal + common)** — not Candle.
//! Reasons: (1) GGUF is the on-disk format we already import as `pocket-brain.gguf`;
//! (2) Metal-backed static link already targets `aarch64-apple-ios-sim` /
//! device under `pocket-brain`; (3) Channel token streaming + Jetsam governor
//! already exist in [`super::service`]. Candle would duplicate weight formats
//! and forfeit the as-built iOS link path.
//!
//! ## IPC
//! Prefer these Phase-15 names from new FE code; they delegate to the same
//! worker as `llm_load_model` / `llm_generate` (no second runtime).

use tauri::ipc::Channel;
use tauri::{AppHandle, State};

use super::model_path::{internal_model_present, resolve_loadable_model_path};
use super::params::{GenerationParams, LoadParams};
use super::service::{LlmHandle, TokenEvent};

/// Load the on-device GGUF (bundled `$RESOURCE` on iOS, else AppData import).
/// Never accepts an external / picker path.
#[tauri::command]
pub async fn brain_load_gguf(
    app: AppHandle,
    handle: State<'_, LlmHandle>,
    params: LoadParams,
) -> Result<(), String> {
    if !internal_model_present(&app)? {
        return Err("内部モデルが未配置です。先にローカル GGUF を取り込んでください。".into());
    }
    let path = resolve_loadable_model_path(&app)?;
    let handle = handle.inner().clone();
    tauri::async_runtime::spawn_blocking(move || handle.load(path, params))
        .await
        .map_err(|_| "brain load join failed".to_string())?
}

/// Stream generation tokens to the frontend (`Channel<TokenEvent>` chunks).
#[tauri::command]
pub async fn brain_generate_stream(
    handle: State<'_, LlmHandle>,
    params: GenerationParams,
    task_id: Option<String>,
    on_token: Channel<TokenEvent>,
) -> Result<(), String> {
    handle.generate(params, task_id, on_token).await
}

/// Soft probe — true when a GGUF is resident (post-Jetsam / purge aware).
#[tauri::command]
pub async fn brain_is_ready(handle: State<'_, LlmHandle>) -> Result<bool, String> {
    let handle = handle.inner().clone();
    tauri::async_runtime::spawn_blocking(move || handle.is_loaded())
        .await
        .map_err(|_| "brain ready probe join failed".to_string())?
}
