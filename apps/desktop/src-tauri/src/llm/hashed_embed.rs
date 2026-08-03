//! Deterministic 384-d hashed n-gram embedder for vault `knowledge_chunks`.
//!
//! Pocket Brain chat GGUFs (e.g. Qwen-class `n_embd=1536`) are generation models,
//! not 384-d embedders. When `LlmHandle::embed` returns the wrong width — or the
//! model is unloaded — we fall back to this offline embedder so ingest/search
//! never fail closed on `embedding dim mismatch`.
//!
//! Algorithm mirrors Python `pipeline.HashedNgramEmbedder` (char tri-grams →
//! signed bucket counts → L2), using SHA-256 truncated to 8 bytes instead of
//! blake2b so we stay within existing Cargo deps. Vault-local consistency is
//! what matters; PKBVEC01 cross-compat is not required on the iOS RAG path.

use sha2::{Digest, Sha256};
use unicode_normalization::UnicodeNormalization;

use super::embed::EMBEDDING_DIMENSIONS;

/// NFC + whitespace canonicalize (Python `canonicalize_text` subset).
pub fn canonicalize_embed_text(text: &str) -> String {
    let normalized: String = text.nfc().collect();
    let mut out = String::with_capacity(normalized.len());
    let mut prev_space = false;
    let mut prev_nl = false;
    for ch in normalized.chars() {
        let ch = match ch {
            '\r' | '\u{0085}' | '\u{2028}' | '\u{2029}' => '\n',
            c if c.is_whitespace() && c != '\n' => ' ',
            c => c,
        };
        if ch == ' ' {
            if prev_space || prev_nl || out.is_empty() {
                continue;
            }
            prev_space = true;
            prev_nl = false;
            out.push(' ');
            continue;
        }
        if ch == '\n' {
            // trim spaces before newline
            while out.ends_with(' ') {
                out.pop();
            }
            if prev_nl || out.is_empty() {
                continue;
            }
            prev_nl = true;
            prev_space = false;
            out.push('\n');
            continue;
        }
        prev_space = false;
        prev_nl = false;
        out.push(ch);
    }
    while out.ends_with(' ') || out.ends_with('\n') {
        out.pop();
    }
    out
}

fn l2_normalize(v: &mut [f32]) {
    let mut sum = 0.0f32;
    for x in v.iter() {
        sum += x * x;
    }
    let norm = sum.sqrt();
    if norm > 0.0 {
        for x in v.iter_mut() {
            *x /= norm;
        }
    }
}

/// Char tri-gram hashed embedding → exactly [`EMBEDDING_DIMENSIONS`] floats, L2-normalized.
pub fn hashed_ngram_embed_384(text: &str) -> Vec<f32> {
    let t = canonicalize_embed_text(text);
    let s: String = format!("^{t}$");
    let chars: Vec<char> = s.chars().collect();
    let mut out = vec![0.0f32; EMBEDDING_DIMENSIONS];
    if chars.len() < 3 {
        return out;
    }
    for i in 0..chars.len() - 2 {
        let tri: String = chars[i..i + 3].iter().collect();
        let digest = Sha256::digest(tri.as_bytes());
        let mut eight = [0u8; 8];
        eight.copy_from_slice(&digest[..8]);
        let v = u64::from_le_bytes(eight);
        let idx = (v % EMBEDDING_DIMENSIONS as u64) as usize;
        if ((v >> 63) & 1) == 1 {
            out[idx] += 1.0;
        } else {
            out[idx] -= 1.0;
        }
    }
    l2_normalize(&mut out);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dims_exact_384() {
        let v = hashed_ngram_embed_384("日記: 転職を考えている");
        assert_eq!(v.len(), EMBEDDING_DIMENSIONS);
        let norm: f32 = v.iter().map(|x| x * x).sum::<f32>().sqrt();
        assert!((norm - 1.0).abs() < 1e-5, "norm={norm}");
    }

    #[test]
    fn stable_for_same_input() {
        let a = hashed_ngram_embed_384("hello world");
        let b = hashed_ngram_embed_384("hello world");
        assert_eq!(a, b);
    }

    #[test]
    fn empty_is_zero_vector() {
        let v = hashed_ngram_embed_384("");
        assert!(v.iter().all(|x| *x == 0.0));
    }
}
