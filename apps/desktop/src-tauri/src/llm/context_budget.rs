//! Hierarchical context budget (Phase 9 / Packer MemGPT-style + Baddeley chunks).
//!
//! # Academic grounding
//!
//! 1. **Working-memory chunking** — Baddeley (2000): limited capacity is managed by
//!    packing information into fewer, denser chunks rather than unbounded lists.
//! 2. **Recursive / hierarchical summarization** — Packer et al. (2023) *MemGPT*:
//!    when the context window is exhausted, demote low-salience material into a
//!    compact summary tier instead of hard-dropping the tail.
//! 3. **Forgetting curve** — Ebbinghaus (1885): salience multiplies relevance by
//!    `exp(−Δt / τ)` so stale low-relevance chunks compress first.
//!
//! Token counts use a **deterministic offline estimator** (CJK ≈ 1 tok, Latin ≈ ¼)
//! so budget math works without loading the GGUF. When a live llama tokenizer is
//! available upstream, callers may pass pre-counted `token_estimate` overrides.
//!
//! Fully deterministic (F-14): no RNG, no egress. Time is Unix UTC.

/// Half-life (days) for salience decay (aligned with Phase 7 recall τ).
pub const SALIENCE_HALF_LIFE_DAYS: f64 = 30.0;

/// Default RAG context budget in estimated tokens (~6k chars / 4).
pub const DEFAULT_RAG_TOKEN_BUDGET: usize = 1_500;

/// Seconds per day (UTC, calendar-free).
const SECS_PER_DAY: f64 = 86_400.0;

/// One candidate chunk competing for the prompt window.
#[derive(Debug, Clone)]
pub struct BudgetChunk {
    pub id: String,
    pub text: String,
    /// Semantic / retrieval relevance in \[0, 1\].
    pub relevance: f64,
    /// Chunk creation time (Unix UTC seconds). `0` ⇒ treat as “now” (no decay).
    pub created_at_unix: i64,
    /// Optional precomputed token count; `None` ⇒ [`estimate_tokens`].
    pub token_estimate: Option<usize>,
}

/// Overflow tier: metadata-only stub (MemGPT-style hierarchical demotion).
#[derive(Debug, Clone, PartialEq)]
pub struct CompressedStub {
    pub id: String,
    pub salience: f64,
    pub age_days: f64,
    pub original_tokens: usize,
    pub stub_text: String,
}

/// Result of fitting chunks into a token budget.
#[derive(Debug, Clone)]
pub struct BudgetedContext {
    pub kept: Vec<BudgetChunk>,
    pub compressed: Vec<CompressedStub>,
    pub tokens_kept: usize,
    pub tokens_compressed_stubs: usize,
}

/// Deterministic token estimator (no model required).
///
/// Heuristic mirrors typical multilingual BPE density:
/// - CJK / fullwidth ideographs ≈ 1 token each
/// - other non-whitespace ≈ 1 token / 4 chars (ceil)
/// - whitespace ignored for density (still zero-cost separators)
pub fn estimate_tokens(text: &str) -> usize {
    let mut cjk = 0usize;
    let mut other = 0usize;
    for ch in text.chars() {
        if ch.is_whitespace() {
            continue;
        }
        if is_cjk(ch) {
            cjk += 1;
        } else {
            other += 1;
        }
    }
    cjk + other.div_ceil(4).max(if other > 0 { 1 } else { 0 })
}

fn is_cjk(ch: char) -> bool {
    matches!(
        ch,
        '\u{3000}'..='\u{9FFF}'
            | '\u{F900}'..='\u{FAFF}'
            | '\u{FF00}'..='\u{FFEF}'
    )
}

fn tau_days() -> f64 {
    SALIENCE_HALF_LIFE_DAYS / std::f64::consts::LN_2
}

/// Ebbinghaus retention `exp(−Δt / τ)` with `Δt` in days.
pub fn ebbinghaus_retention(delta_days: f64, tau_days: f64) -> f64 {
    if !tau_days.is_finite() || tau_days <= 0.0 {
        return 1.0;
    }
    let dt = if delta_days.is_finite() {
        delta_days.max(0.0)
    } else {
        0.0
    };
    (-dt / tau_days).exp()
}

fn age_days(created_at_unix: i64, now_unix: i64) -> f64 {
    if created_at_unix <= 0 {
        return 0.0;
    }
    let delta = now_unix.saturating_sub(created_at_unix).max(0) as f64;
    delta / SECS_PER_DAY
}

/// Salience = relevance × Ebbinghaus retention.
pub fn salience_score(relevance: f64, created_at_unix: i64, now_unix: i64) -> f64 {
    let r = if relevance.is_finite() {
        relevance.clamp(0.0, 1.0)
    } else {
        0.0
    };
    let w = ebbinghaus_retention(age_days(created_at_unix, now_unix), tau_days());
    r * w
}

fn chunk_tokens(c: &BudgetChunk) -> usize {
    c.token_estimate.unwrap_or_else(|| estimate_tokens(&c.text)).max(1)
}

fn stub_for(c: &BudgetChunk, now_unix: i64) -> CompressedStub {
    let sal = salience_score(c.relevance, c.created_at_unix, now_unix);
    let age = age_days(c.created_at_unix, now_unix);
    let original_tokens = chunk_tokens(c);
    let stub_text = format!(
        "[compressed id={} salience={:.3} age_days={:.1} tokens≈{}]",
        c.id, sal, age, original_tokens
    );
    CompressedStub {
        id: c.id.clone(),
        salience: sal,
        age_days: age,
        original_tokens,
        stub_text,
    }
}

/// Greedy keep-by-salience; overflow demoted to compressed stubs (not discarded).
///
/// Sort key: salience desc, then id asc (deterministic). Stubs are appended in
/// residual salience order and included only while the stub texts themselves fit.
pub fn fit_context_budget(
    chunks: &[BudgetChunk],
    token_budget: usize,
    now_unix: i64,
) -> BudgetedContext {
    if token_budget == 0 {
        let compressed: Vec<CompressedStub> = chunks.iter().map(|c| stub_for(c, now_unix)).collect();
        let tokens_compressed_stubs = compressed.iter().map(|s| estimate_tokens(&s.stub_text)).sum();
        return BudgetedContext {
            kept: Vec::new(),
            compressed,
            tokens_kept: 0,
            tokens_compressed_stubs,
        };
    }

    let mut ranked: Vec<(f64, usize)> = chunks
        .iter()
        .enumerate()
        .map(|(i, c)| (salience_score(c.relevance, c.created_at_unix, now_unix), i))
        .collect();
    ranked.sort_by(|a, b| {
        b.0.partial_cmp(&a.0)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| chunks[a.1].id.cmp(&chunks[b.1].id))
    });

    let mut kept = Vec::new();
    let mut overflow_idx = Vec::new();
    let mut used = 0usize;

    for &(_, i) in &ranked {
        let c = &chunks[i];
        let need = chunk_tokens(c);
        if used + need <= token_budget {
            used += need;
            kept.push(c.clone());
        } else {
            overflow_idx.push(i);
        }
    }

    // If nothing fits whole, keep a truncated copy of the top-salience chunk
    // so the window is never empty when candidates exist (MemGPT: compress, don't blank).
    if kept.is_empty() && !ranked.is_empty() {
        let top = &chunks[ranked[0].1];
        let truncated = truncate_to_token_budget(&top.text, token_budget);
        if !truncated.is_empty() {
            let mut c = top.clone();
            c.text = truncated;
            c.token_estimate = Some(estimate_tokens(&c.text));
            used = chunk_tokens(&c);
            kept.push(c);
            overflow_idx.retain(|&i| chunks[i].id != top.id);
        }
    }

    // Preserve original retrieval order among kept items for prompt readability.
    let mut kept_order: Vec<BudgetChunk> = Vec::with_capacity(kept.len());
    for c in chunks {
        if kept.iter().any(|k| k.id == c.id) {
            kept_order.push(c.clone());
        }
    }

    let mut compressed = Vec::new();
    let mut stub_used = 0usize;
    let residual = token_budget.saturating_sub(used);
    for i in overflow_idx {
        let mut stub = stub_for(&chunks[i], now_unix);
        let st = estimate_tokens(&stub.stub_text);
        if residual > 0 && stub_used + st > residual {
            // Shrink stub text to metadata id only so the tier still fits.
            stub.stub_text = format!("[compressed id={}]", stub.id);
        }
        stub_used += estimate_tokens(&stub.stub_text);
        compressed.push(stub);
    }

    let tokens_compressed_stubs = compressed.iter().map(|s| estimate_tokens(&s.stub_text)).sum();

    BudgetedContext {
        kept: kept_order,
        compressed,
        tokens_kept: used,
        tokens_compressed_stubs,
    }
}

/// Truncate a single string to an estimated token budget (char-boundary safe).
pub fn truncate_to_token_budget(text: &str, token_budget: usize) -> String {
    if token_budget == 0 {
        return String::new();
    }
    if estimate_tokens(text) <= token_budget {
        return text.to_string();
    }
    let mut acc = String::new();
    for ch in text.chars() {
        acc.push(ch);
        if estimate_tokens(&acc) > token_budget {
            acc.pop();
            break;
        }
    }
    while acc.ends_with(char::is_whitespace) {
        acc.pop();
    }
    if acc.is_empty() {
        return String::new();
    }
    format!("{acc}…")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chunk(id: &str, text: &str, rel: f64, created: i64) -> BudgetChunk {
        BudgetChunk {
            id: id.into(),
            text: text.into(),
            relevance: rel,
            created_at_unix: created,
            token_estimate: None,
        }
    }

    #[test]
    fn estimate_tokens_cjk_dense() {
        assert!(estimate_tokens("転職") >= 2);
        assert!(estimate_tokens("abcd") <= 2);
    }

    #[test]
    fn greedy_keeps_high_salience() {
        let now = 1_700_000_000_i64;
        let old_text = "古い低関連の長文。".repeat(40);
        let chunks = vec![
            chunk("old", &old_text, 0.2, now - 90 * 86_400),
            chunk("hot", "重要で新しい知見。", 0.95, now),
        ];
        let fitted = fit_context_budget(&chunks, 20, now);
        assert!(fitted.kept.iter().any(|c| c.id == "hot"));
        assert!(
            fitted.compressed.iter().any(|s| s.id == "old") || !fitted.kept.iter().any(|c| c.id == "old")
        );
    }

    #[test]
    fn overflow_becomes_stub_not_silent_drop() {
        let now = 1_700_000_000_i64;
        let a = "alpha ".repeat(80);
        let b = "bravo ".repeat(80);
        let chunks = vec![
            chunk("a", &a, 1.0, now),
            chunk("b", &b, 0.9, now),
        ];
        let fitted = fit_context_budget(&chunks, 30, now);
        // At least one full chunk or all accounted for via stubs.
        assert_eq!(fitted.kept.len() + fitted.compressed.len(), 2);
        assert!(!fitted.compressed.is_empty() || fitted.kept.len() == 2);
    }

    #[test]
    fn truncate_respects_budget() {
        let s = "あいうえおかきくけこ".repeat(10);
        let out = truncate_to_token_budget(&s, 5);
        assert!(estimate_tokens(&out) <= 6); // ellipsis slack
        assert!(out.ends_with('…') || estimate_tokens(&s) <= 5);
    }

    #[test]
    fn salience_decays_with_age() {
        let now = 1_700_000_000_i64;
        let fresh = salience_score(1.0, now, now);
        let old = salience_score(1.0, now - 30 * 86_400, now);
        assert!(fresh > old);
        assert!((old - 0.5).abs() < 1e-6);
    }
}
