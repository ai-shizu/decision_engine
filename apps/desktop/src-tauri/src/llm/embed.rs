//! M9 embedding helpers for on-device RAG (llama-cpp-2).
//!
//! Pure functions that allocate a short-lived embeddings-enabled
//! [`LlamaContext`], decode one prompt batch, copy out the sequence embedding,
//! then drop the context. They never touch Tauri State and must only be called
//! from the pocket-brain worker thread (same Send/Sync constraints as generate).

use std::num::NonZeroU32;

use llama_cpp_2::context::params::{LlamaContextParams, LlamaPoolingType};
use llama_cpp_2::llama_backend::LlamaBackend;
use llama_cpp_2::llama_batch::LlamaBatch;
use llama_cpp_2::model::{AddBos, LlamaModel};

/// Canonical storage width for vault `knowledge_chunks.embedding` (M9).
/// Callers that ingest into the vault must ensure the returned vector length
/// matches this constant (typically via a dedicated embedding GGUF).
pub const EMBEDDING_DIMENSIONS: usize = 384;

/// Default context window for a single embedding pass (kept small to bound
/// Jetsam pressure; not a generation context).
pub const EMBED_DEFAULT_N_CTX: u32 = 512;

/// Embed `text` with a temporary embeddings context on `model`.
///
/// - Creates an embeddings-enabled context (`with_embeddings(true)` + mean pool).
/// - Does not share the generation context; Drop returns memory when done.
/// - Checks the memory governor cancel flag between tokenize and decode so an
///   in-flight purge/cancel can abort without finishing a large decode.
///
/// Returns the raw `f32` embedding (`model.n_embd()` long). Matching
/// [`EMBEDDING_DIMENSIONS`] is the caller's responsibility at ingest time.
pub fn embed_text(
    backend: &LlamaBackend,
    model: &LlamaModel,
    text: &str,
    n_ctx: u32,
    is_cancelled: impl Fn() -> bool,
) -> Result<Vec<f32>, String> {
    if text.is_empty() {
        return Err("embed_text: empty input".into());
    }
    if is_cancelled() {
        return Err("embed_text: cancelled".into());
    }

    let ctx_tokens = NonZeroU32::new(n_ctx.max(1)).ok_or("embed_text: n_ctx invalid")?;
    let mut ctx_params = LlamaContextParams::default()
        .with_n_ctx(Some(ctx_tokens))
        .with_embeddings(true)
        .with_pooling_type(LlamaPoolingType::Mean);
    // The synthetic iOS Simulator Metal device lacks the unified/shared-memory
    // capabilities required for reliable quantized inference. Model layers are
    // forced to CPU at load time; keep K/Q/V and operation offload there too.
    // `CORAXIS_FORCE_CPU=1` (G-2 oracle) uses the same context half of the
    // 4-point set.
    if cfg!(all(target_os = "ios", target_abi = "sim"))
        || crate::llm::params::force_cpu_oracle_enabled()
    {
        ctx_params = ctx_params.with_offload_kqv(false).with_op_offload(false);
    }

    let mut ctx = model
        .new_context(backend, ctx_params)
        .map_err(|error| format!("embed context: {error}"))?;

    let tokens = model
        .str_to_token(text, AddBos::Always)
        .map_err(|error| format!("embed tokenize: {error}"))?;
    if tokens.is_empty() {
        return Err("embed_text: tokenization produced no tokens".into());
    }
    if tokens.len() as u32 >= n_ctx {
        return Err(format!(
            "embed_text: input too long ({} tokens >= n_ctx {n_ctx})",
            tokens.len()
        ));
    }
    if is_cancelled() {
        return Err("embed_text: cancelled".into());
    }

    let mut batch = LlamaBatch::new(tokens.len(), 1);
    let last = tokens.len().saturating_sub(1);
    for (index, token) in tokens.iter().enumerate() {
        batch
            .add(*token, index as i32, &[0], index == last)
            .map_err(|error| format!("embed batch: {error}"))?;
    }
    ctx.decode(&mut batch)
        .map_err(|error| format!("embed decode: {error}"))?;

    if is_cancelled() {
        return Err("embed_text: cancelled".into());
    }

    let embedding = ctx
        .embeddings_seq_ith(0)
        .map_err(|error| format!("embed extract: {error}"))?;
    Ok(embedding.to_vec())
}

/// Pack `f32` embedding as little-endian bytes for Tauri raw binary IPC (Phase 9).
pub fn f32_slice_to_le_bytes(v: &[f32]) -> Vec<u8> {
    let mut out = Vec::with_capacity(v.len() * 4);
    for x in v {
        out.extend_from_slice(&x.to_le_bytes());
    }
    out
}

/// Decode little-endian f32 bytes back to a vector (tests / FE contract mirror).
pub fn le_bytes_to_f32_vec(bytes: &[u8]) -> Result<Vec<f32>, String> {
    if bytes.len() % 4 != 0 {
        return Err("embed binary length not divisible by 4".into());
    }
    let mut out = Vec::with_capacity(bytes.len() / 4);
    for chunk in bytes.chunks_exact(4) {
        let arr: [u8; 4] = chunk.try_into().map_err(|_| "embed binary chunk".to_string())?;
        out.push(f32::from_le_bytes(arr));
    }
    Ok(out)
}

/// Fail closed when a vector cannot be stored in the M9 `float[384]` column.
pub fn require_knowledge_embedding_dims(embedding: &[f32]) -> Result<(), String> {
    if embedding.len() == EMBEDDING_DIMENSIONS {
        Ok(())
    } else {
        Err(format!(
            "embedding dim mismatch: got {}, expected {EMBEDDING_DIMENSIONS}",
            embedding.len()
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn require_dims_accepts_exact_width() {
        let v = vec![0.0_f32; EMBEDDING_DIMENSIONS];
        assert!(require_knowledge_embedding_dims(&v).is_ok());
    }

    #[test]
    fn require_dims_rejects_wrong_width() {
        let v = vec![0.0_f32; 8];
        assert!(require_knowledge_embedding_dims(&v).is_err());
    }

    #[test]
    fn le_bytes_round_trip() {
        let v = vec![1.0f32, -2.5, 0.0, 384.0];
        let b = f32_slice_to_le_bytes(&v);
        assert_eq!(b.len(), 16);
        let back = le_bytes_to_f32_vec(&b).expect("decode");
        assert_eq!(back, v);
    }
}
