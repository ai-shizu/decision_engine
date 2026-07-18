//! LLM load / generation parameters (docs/architecture_blueprint.md §3.4).
//!
//! Deserialized from the frontend command payload. No hardcoded model name /
//! temperature — the caller supplies them (§ model strategy discipline).

use serde::Deserialize;

#[derive(Clone, Deserialize)]
pub struct LoadParams {
    /// Layers to offload to Metal. 999 = all.
    pub n_gpu_layers: u32,
    /// Keep weights as clean, file-backed pages (not counted against jetsam dirty).
    pub use_mmap: bool,
}

#[derive(Clone, Deserialize)]
pub struct GenerationParams {
    pub prompt: String,
    /// KV cache context window. 0 → llama.cpp default.
    pub n_ctx: u32,
    pub max_tokens: u32,
    /// <= 0.0 selects greedy decoding.
    pub temp: f32,
    pub top_k: i32,
    pub top_p: f32,
    pub seed: u32,
}
