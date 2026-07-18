//! [A] Core LLM Service — M4 Phase 0 scaffold (docs/architecture_blueprint.md §3.6).
//!
//! **STUBS ONLY — NO LOGIC.** Function bodies are `todo!()` until later phases.
//! The entire module is compiled only under the `pocket-brain` feature (wired in
//! `lib.rs`), so the default desktop build never sees it and stays byte-identical.
//!
//! Design invariant (verified in the M0 spike): `LlamaContext` is neither `Send`
//! nor `Sync` and borrows its `LlamaModel`, so it can never live in Tauri `State`.
//! The real implementation (Phase 2/3) confines backend/model/context to a single
//! worker thread; `LlmHandle` below is only the Send+Sync command sender.

pub mod commands_llm;

use serde::{Deserialize, Serialize};

/// Model load parameters (blueprint §3.4). Defaults: n_gpu_layers=999, use_mmap=true.
#[derive(Clone, Deserialize)]
pub struct LoadParams {
    pub n_gpu_layers: u32,
    pub use_mmap: bool,
}

/// Generation parameters (blueprint §3.4).
#[derive(Clone, Deserialize)]
pub struct GenerationParams {
    pub prompt: String,
    pub n_ctx: u32,
    pub max_tokens: u32,
    pub temp: f32,
    pub top_k: i32,
    pub top_p: f32,
    pub seed: u32,
}

/// One streamed token pushed to the frontend over `tauri::ipc::Channel` (blueprint §3.6).
#[derive(Clone, Serialize)]
pub struct TokenEvent {
    pub seq: u32,
    pub text: String,
    pub done: bool,
    pub error: Option<String>,
}

/// Handle placed in Tauri `State` (Send + Sync). Phase 0 stub — no worker yet.
pub struct LlmHandle;

impl LlmHandle {
    /// Phase 2: spawn the LLM worker thread, init `LlamaBackend`, enter command loop.
    pub fn spawn() -> Self {
        todo!("M4 Phase 2: spawn LLM worker thread")
    }

    /// Phase 2: mmap-load the GGUF on the worker thread.
    pub fn load(&self, _model_path: std::path::PathBuf, _params: LoadParams) -> Result<(), String> {
        todo!("M4 Phase 2: model load")
    }

    /// Phase 3: run the sampling loop, streaming tokens over the channel.
    pub fn generate(&self, _params: GenerationParams) {
        todo!("M4 Phase 3: generation loop")
    }

    /// Phase 3: cancel an in-flight generation.
    pub fn cancel(&self) {
        todo!("M4 Phase 3: cancel")
    }
}
