//! Knowledge-index embedding: model-384 or hashed-ngram fallback (M20-J).
//!
//! Phase 7 splits channels for hybrid recall:
//! - **Lexical (hash):** always [`lexical_hash_embed`] — stable offline axis.
//! - **Dense (DPR):** [`try_dense_passage_embed`] — only when the loaded GGUF is
//!   truly 384-d (Karpukhin-style dense channel). Otherwise `None` so RRF
//!   degrades to lexical-only without inventing a duplicate ranking.

use crate::llm::embed::{require_knowledge_embedding_dims, EMBEDDING_DIMENSIONS, EMBED_DEFAULT_N_CTX};
use crate::llm::hashed_embed::hashed_ngram_embed_384;
use crate::llm::LlmHandle;

fn l2_normalize_owned(mut v: Vec<f32>) -> Vec<f32> {
    let mut sum = 0.0f32;
    for x in &v {
        sum += x * x;
    }
    let norm = sum.sqrt();
    if norm > 0.0 {
        for x in &mut v {
            *x /= norm;
        }
    }
    v
}

/// Always returns a 384-d vector suitable for `knowledge_chunks`.
///
/// Prefers the loaded GGUF when `n_embd == 384`; otherwise (Qwen 1536-d chat
/// models, unloaded model, cancel) uses deterministic hashed-ngram-384 so
/// ingest/search never surface `embedding dim mismatch`.
pub fn embed_for_knowledge(llm: &LlmHandle, text: &str) -> Result<Vec<f32>, String> {
    if text.trim().is_empty() {
        return Err("embed_for_knowledge: empty input".into());
    }
    let embedding = match llm.embed(text.to_string(), EMBED_DEFAULT_N_CTX) {
        Ok(v) if v.len() == EMBEDDING_DIMENSIONS => l2_normalize_owned(v),
        Ok(_wrong_dim) => hashed_ngram_embed_384(text),
        Err(_) => hashed_ngram_embed_384(text),
    };
    require_knowledge_embedding_dims(&embedding)?;
    Ok(embedding)
}

/// Lexical / hashed channel embedding (always available, offline, deterministic).
pub fn lexical_hash_embed(text: &str) -> Result<Vec<f32>, String> {
    if text.trim().is_empty() {
        return Err("lexical_hash_embed: empty input".into());
    }
    let embedding = hashed_ngram_embed_384(text);
    require_knowledge_embedding_dims(&embedding)?;
    Ok(embedding)
}

/// Dense Passage Retrieval channel (Karpukhin et al., 2020).
///
/// Returns `Some(384-d L2 vector)` only when the loaded model produces exact
/// knowledge dims. Wrong-width / unloaded / error ⇒ `None` (no hashed twin —
/// that would duplicate the lexical channel and collapse RRF).
pub fn try_dense_passage_embed(llm: &LlmHandle, text: &str) -> Option<Vec<f32>> {
    if text.trim().is_empty() {
        return None;
    }
    match llm.embed(text.to_string(), EMBED_DEFAULT_N_CTX) {
        Ok(v) if v.len() == EMBEDDING_DIMENSIONS => Some(l2_normalize_owned(v)),
        _ => None,
    }
}
