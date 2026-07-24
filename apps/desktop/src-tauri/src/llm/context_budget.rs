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
#[derive(Debug, Clone, Default)]
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
        return BudgetedContext::default();
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

    let mut kept: Vec<(usize, BudgetChunk)> = Vec::new();
    let mut overflow_idx = Vec::new();
    let mut used = 0usize;

    for &(_, i) in &ranked {
        let c = &chunks[i];
        let need = chunk_tokens(c);
        if need <= token_budget.saturating_sub(used) {
            used += need;
            kept.push((i, c.clone()));
        } else {
            overflow_idx.push(i);
        }
    }

    // If nothing fits whole, keep a truncated copy of the top-salience chunk
    // so the window is never empty when candidates exist (MemGPT: compress, don't blank).
    if kept.is_empty() && !ranked.is_empty() {
        let top_index = ranked[0].1;
        let top = &chunks[top_index];
        let truncated = truncate_to_token_budget(&top.text, token_budget);
        if !truncated.is_empty() {
            let mut c = top.clone();
            c.text = truncated;
            c.token_estimate = Some(estimate_tokens(&c.text));
            used = chunk_tokens(&c);
            kept.push((top_index, c));
            overflow_idx.retain(|&i| i != top_index);
        }
    }

    // Preserve original retrieval order without reconstructing from IDs. The
    // stored clone may be a truncated top chunk and must not be replaced by
    // its original full text; indexes also keep duplicate IDs unambiguous.
    kept.sort_by_key(|(index, _)| *index);
    let kept_order: Vec<BudgetChunk> = kept.into_iter().map(|(_, chunk)| chunk).collect();

    let mut compressed = Vec::new();
    let mut stub_used = 0usize;
    let residual = token_budget.saturating_sub(used);
    for i in overflow_idx {
        let mut stub = stub_for(&chunks[i], now_unix);
        let remaining = residual.saturating_sub(stub_used);
        if remaining == 0 {
            break;
        }
        if estimate_tokens(&stub.stub_text) > remaining {
            // Shrink stub text to metadata id only so the tier still fits.
            stub.stub_text = format!("[compressed id={}]", stub.id);
        }
        let need = estimate_tokens(&stub.stub_text);
        if need > remaining {
            continue;
        }
        stub_used += need;
        compressed.push(stub);
    }

    BudgetedContext {
        kept: kept_order,
        compressed,
        tokens_kept: used,
        tokens_compressed_stubs: stub_used,
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
    let mut cjk = 0usize;
    let mut other = 0usize;
    for ch in text.chars() {
        let (next_cjk, next_other) = if ch.is_whitespace() {
            (cjk, other)
        } else if is_cjk(ch) {
            (cjk.saturating_add(1), other)
        } else {
            (cjk, other.saturating_add(1))
        };
        // U+2026 is counted as one non-CJK code point by `estimate_tokens`.
        // Reserve it before accepting the next body character, so the final
        // string — not merely its body — stays within the caller's budget.
        let with_ellipsis =
            next_cjk.saturating_add(next_other.saturating_add(1).div_ceil(4));
        if with_ellipsis > token_budget {
            break;
        }
        cjk = next_cjk;
        other = next_other;
        acc.push(ch);
    }
    while acc.ends_with(char::is_whitespace) {
        acc.pop();
    }
    if acc.is_empty() {
        return if estimate_tokens("…") <= token_budget {
            "…".to_string()
        } else {
            String::new()
        };
    }
    format!("{acc}…")
}

/// Final cross-section guard for a fully-assembled prompt (2026-07-24: RAG
/// context [`DEFAULT_RAG_TOKEN_BUDGET`] and consult's mentor sections
/// (`consult_context::GAP_SECTION_TOKEN_BUDGET` + `TENSOR_...` + `ORACLE_...`)
/// are each budgeted independently, but nothing previously re-checked their
/// *sum* against the model's real available input budget before tokenizing —
/// `generate()` would then fail outright with "prompt exceeds context
/// budget", surfaced to the user as a sterile "応答を生成できませんでした").
///
/// Splits on the literal `"## ユーザーの質問"` marker (present in both
/// `rag::prompt::build_rag_prompt` and after `consult_context::
/// append_mentor_sections`) so the user's own message — the tail, from the
/// marker onward — is **never** truncated. Only the head (system preamble +
/// injected RAG/mentor context) is trimmed, front-preserved: the system
/// preamble at the very start survives longest; injected context is cut from
/// its own tail first.
///
/// When the marker is absent (an unrecognized prompt shape), fails closed via
/// the same front-preserving truncation rather than silently returning an
/// oversized prompt.
pub fn fit_prompt_to_budget(prompt: &str, available_tokens: usize) -> String {
    if estimate_tokens(prompt) <= available_tokens {
        return prompt.to_string();
    }
    let Some(idx) = prompt.find("## ユーザーの質問") else {
        return truncate_to_token_budget(prompt, available_tokens);
    };
    let tail = &prompt[idx..];
    let tail_tokens = estimate_tokens(tail);
    let head_budget = available_tokens.saturating_sub(tail_tokens);
    let head = &prompt[..idx];
    format!("{}{}", truncate_to_token_budget(head, head_budget), tail)
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
    fn overflow_never_exceeds_the_hard_budget() {
        let now = 1_700_000_000_i64;
        let a = "alpha ".repeat(80);
        let b = "bravo ".repeat(80);
        let chunks = vec![
            chunk("a", &a, 1.0, now),
            chunk("b", &b, 0.9, now),
        ];
        let fitted = fit_context_budget(&chunks, 30, now);
        assert!(fitted.tokens_kept + fitted.tokens_compressed_stubs <= 30);
        assert!(!fitted.kept.is_empty());
    }

    #[test]
    fn zero_budget_returns_no_content_or_stubs() {
        let now = 1_700_000_000_i64;
        let fitted = fit_context_budget(&[chunk("a", "alpha", 1.0, now)], 0, now);
        assert!(fitted.kept.is_empty());
        assert!(fitted.compressed.is_empty());
        assert_eq!(fitted.tokens_kept + fitted.tokens_compressed_stubs, 0);
    }

    #[test]
    fn truncated_top_chunk_is_not_reexpanded_during_ordering() {
        let now = 1_700_000_000_i64;
        let original = "あいうえおかきくけこ".repeat(10);
        let fitted = fit_context_budget(&[chunk("same", &original, 1.0, now)], 5, now);

        assert_eq!(fitted.kept.len(), 1);
        assert_ne!(fitted.kept[0].text, original);
        assert!(estimate_tokens(&fitted.kept[0].text) <= 5);
        assert_eq!(
            fitted.tokens_kept,
            estimate_tokens(&fitted.kept[0].text)
        );
    }

    #[test]
    fn residual_can_hold_a_shortened_stub_without_overrun() {
        let now = 1_700_000_000_i64;
        let mut a = chunk("a", "alpha", 1.0, now);
        let mut b = chunk("b", "bravo", 0.9, now);
        a.token_estimate = Some(20);
        b.token_estimate = Some(20);

        let fitted = fit_context_budget(&[a, b], 30, now);
        assert_eq!(fitted.kept.len(), 1);
        assert_eq!(fitted.compressed.len(), 1);
        assert!(fitted.tokens_kept + fitted.tokens_compressed_stubs <= 30);
    }

    #[test]
    fn invariant_holds_across_small_budgets() {
        let now = 1_700_000_000_i64;
        let chunks = vec![
            chunk("a", &"alpha ".repeat(20), 1.0, now),
            chunk("b", &"転職".repeat(20), 0.9, now),
            chunk("c", &"charlie ".repeat(20), 0.8, now),
        ];
        for budget in 0..=80 {
            let fitted = fit_context_budget(&chunks, budget, now);
            assert!(
                fitted.tokens_kept + fitted.tokens_compressed_stubs <= budget,
                "budget={budget}"
            );
        }
    }

    #[test]
    fn truncate_respects_budget() {
        let s = "あいうえおかきくけこ".repeat(10);
        let out = truncate_to_token_budget(&s, 5);
        assert!(estimate_tokens(&out) <= 5);
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

    #[test]
    fn fit_prompt_leaves_untouched_when_within_budget() {
        let prompt = "前置き\n## ユーザーの質問\nこんにちは\n";
        let out = fit_prompt_to_budget(prompt, 1000);
        assert_eq!(out, prompt);
    }

    #[test]
    fn fit_prompt_never_truncates_the_user_question_tail() {
        let head = "膨大な文脈。".repeat(400);
        let tail = "## ユーザーの質問\n本当に伝えたい質問文\n";
        let prompt = format!("{head}{tail}");
        // Budget far below the head's own size, but comfortably above the tail's.
        let out = fit_prompt_to_budget(&prompt, 50);
        assert!(
            out.ends_with(tail),
            "tail must survive verbatim, got: {out}"
        );
        assert!(estimate_tokens(&out) <= 50 + estimate_tokens(tail));
    }

    #[test]
    fn fit_prompt_falls_back_to_front_truncation_without_marker() {
        let prompt = "マーカーの無い長文。".repeat(200);
        let out = fit_prompt_to_budget(&prompt, 10);
        assert!(estimate_tokens(&out) <= 10);
    }

    #[test]
    fn fit_prompt_combined_rag_plus_mentor_sections_fits_real_budget() {
        // Regression for the 2026-07-24 device bug: RAG (budget 1500) +
        // Gap/Tensor/Oracle sections (625+300+375) summed to 2394 estimated
        // tokens against a real 1792-token input budget, and nothing had
        // re-checked the total before tokenizing.
        let rag_section = "検索された関連チャンク本文。".repeat(150); // ~1500 tok
        let mentor_sections = "決定論的Gap/Tensor/Oracleセクション。".repeat(90); // ~900 tok
        let tail = "## ユーザーの質問\nよろしく\n";
        let prompt = format!("{rag_section}\n{mentor_sections}\n{tail}");
        assert!(
            estimate_tokens(&prompt) > 1792,
            "fixture must reproduce the overflow"
        );
        let out = fit_prompt_to_budget(&prompt, 1792);
        assert!(estimate_tokens(&out) <= 1792);
        assert!(out.ends_with(tail));
    }
}
