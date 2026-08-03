//! LLM load / generation parameters (docs/architecture_blueprint.md §3.4).
//!
//! Deserialized from the frontend command payload. No hardcoded model name /
//! temperature — the caller supplies them (§ model strategy discipline).

use serde::Deserialize;

/// Pinned llama.cpp context default (`llama-cpp-2` 0.1.151).
pub const DEFAULT_N_CTX: u32 = 512;
/// Smallest context accepted by Coraxis. Keeping this equal to llama.cpp's
/// default avoids the invalid `clamp(512, requested)` range that previously
/// panicked for `n_ctx < 512`.
pub const MIN_N_CTX: u32 = 512;
/// Application-level resource ceiling for the mobile inference runtime.
pub const MAX_N_CTX: u32 = 16_384;
pub const MAX_GENERATION_TOKENS: u32 = 4_096;
pub const MAX_PROMPT_BYTES: usize = 1024 * 1024;
pub const MAX_TOP_K: i32 = 200;
pub const MAX_TEMPERATURE: f32 = 2.0;

/// Xcode-scheme-injectable CPU oracle (Tier 3 G-2). Only `"1"` enables it.
/// Absent / any other value → current Metal path unchanged.
pub const CORAXIS_FORCE_CPU_ENV: &str = "CORAXIS_FORCE_CPU";

/// True only on iOS when `CORAXIS_FORCE_CPU=1`. Desktop always false (no
/// behavior change). Never permanently forces device Metal off.
pub fn force_cpu_oracle_enabled() -> bool {
    if !cfg!(target_os = "ios") {
        return false;
    }
    std::env::var(CORAXIS_FORCE_CPU_ENV)
        .ok()
        .as_deref()
        == Some("1")
}

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

impl GenerationParams {
    /// Resolve the public `0 → llama.cpp default` contract without ever
    /// constructing an invalid clamp range.
    pub fn resolved_n_ctx(&self) -> u32 {
        if self.n_ctx == 0 {
            DEFAULT_N_CTX
        } else {
            self.n_ctx
        }
    }

    /// Validate every caller-controlled allocation and sampling input before
    /// the request is queued to the single-owner llama worker.
    pub fn validate(&self) -> Result<(), String> {
        let n_ctx = self.resolved_n_ctx();
        if !(MIN_N_CTX..=MAX_N_CTX).contains(&n_ctx) {
            return Err(format!(
                "n_ctx must be in {MIN_N_CTX}..={MAX_N_CTX} (or 0 for default)"
            ));
        }
        if self.max_tokens == 0
            || self.max_tokens > MAX_GENERATION_TOKENS
            || self.max_tokens >= n_ctx
        {
            return Err("invalid max_tokens for context budget".into());
        }
        if self.prompt.len() > MAX_PROMPT_BYTES {
            return Err("prompt too large".into());
        }
        if !self.temp.is_finite() || !(0.0..=MAX_TEMPERATURE).contains(&self.temp) {
            return Err("invalid temperature".into());
        }
        if !(0..=MAX_TOP_K).contains(&self.top_k) {
            return Err("invalid top_k".into());
        }
        if !self.top_p.is_finite() || !(0.0 < self.top_p && self.top_p <= 1.0) {
            return Err("invalid top_p".into());
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn params() -> GenerationParams {
        GenerationParams {
            prompt: "test".into(),
            n_ctx: 2_048,
            max_tokens: 256,
            temp: 0.7,
            top_k: 40,
            top_p: 0.95,
            seed: 0,
        }
    }

    #[test]
    fn zero_context_resolves_to_pinned_llama_default() {
        let mut p = params();
        p.n_ctx = 0;
        assert_eq!(p.resolved_n_ctx(), DEFAULT_N_CTX);
        assert!(p.validate().is_ok());
    }

    #[test]
    fn rejects_context_and_output_budget_abuse() {
        let mut p = params();
        p.n_ctx = MIN_N_CTX - 1;
        assert!(p.validate().is_err());

        p = params();
        p.max_tokens = p.n_ctx;
        assert!(p.validate().is_err());

        p = params();
        p.max_tokens = MAX_GENERATION_TOKENS + 1;
        assert!(p.validate().is_err());
    }

    #[test]
    fn rejects_non_finite_or_out_of_range_sampling() {
        let mut p = params();
        p.temp = f32::NAN;
        assert!(p.validate().is_err());

        p = params();
        p.top_k = MAX_TOP_K + 1;
        assert!(p.validate().is_err());

        p = params();
        p.top_p = 0.0;
        assert!(p.validate().is_err());
    }
}
