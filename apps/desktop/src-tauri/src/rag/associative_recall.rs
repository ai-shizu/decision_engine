//! Associative recall engine: hybrid retrieval + temporal rerank (Phase 7).
//!
//! # Academic grounding (must not be diluted)
//!
//! 1. **Dense Passage Retrieval (DPR)** — Karpukhin et al. (2020): rank passages by
//!    semantic similarity in a shared dense embedding space (here: sqlite-vec KNN
//!    over 384-d vectors when a true embedder is loaded).
//! 2. **Reciprocal Rank Fusion (RRF)** — Cormack, Clarke & Büttcher (2009): fuse
//!    heterogeneous ranked lists without score calibration:
//!    `Score(d) = Σ_r 1 / (k + rank_r(d))` with standard `k = 60`.
//! 3. **Forgetting curve** — Ebbinghaus (1885): retention decays exponentially with
//!    elapsed time, `S = exp(−Δt / τ)`. Interval effects motivate time-weighted
//!    recall so recent memories surface preferentially while strong semantic matches
//!    can still “flash back.”
//!
//! All steps are **deterministic** (F-14): no RNG, no egress. Clocks are Unix UTC.

use std::collections::HashMap;

/// Cormack et al. (2009) default RRF constant.
pub const RRF_K: f64 = 60.0;

/// Nominal half-life (days) used to set τ via `τ = T½ / ln(2)`.
pub const HALF_LIFE_DAYS: f64 = 30.0;

/// Seconds per day (UTC calendar-free; deterministic Δt).
pub const SECS_PER_DAY: f64 = 86_400.0;

/// Decay constant τ (days) from half-life: `W(T½) = ½ = exp(−T½/τ)`.
pub fn tau_days_from_half_life(half_life_days: f64) -> f64 {
    if half_life_days <= 0.0 {
        return HALF_LIFE_DAYS / std::f64::consts::LN_2;
    }
    half_life_days / std::f64::consts::LN_2
}

/// Default τ used by the recall pipeline (~43.28 days for 30-day half-life).
pub fn default_tau_days() -> f64 {
    tau_days_from_half_life(HALF_LIFE_DAYS)
}

/// One document in a channel ranking (best-first ⇒ rank 1).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RankedDoc {
    pub id: String,
}

/// RRF fusion output before temporal weighting.
#[derive(Debug, Clone, PartialEq)]
pub struct RrfHit {
    pub id: String,
    pub rrf_score: f64,
}

/// Candidate carrying vault metadata for Ebbinghaus rerank.
#[derive(Debug, Clone)]
pub struct RecallCandidate {
    pub id: String,
    pub text_content: String,
    /// Best (minimum) channel distance when known; informational only.
    pub distance: f64,
    /// Chunk creation time as Unix UTC seconds.
    pub created_at: i64,
    pub rrf_score: f64,
}

/// Final associative-recall hit after `recall = rrf × exp(−Δt/τ)`.
#[derive(Debug, Clone)]
pub struct RecallHit {
    pub id: String,
    pub text_content: String,
    pub distance: f64,
    pub created_at: i64,
    pub recall_score: f64,
}

/// Abstraction over a single ranked retrieval channel (lexical hash or DPR).
///
/// Concrete lists are usually prefetched via sqlite-vec; this trait keeps the
/// fusion layer independent of which embedder produced the ranking so a future
/// `multilingual-e5-small` (or similar) dense path plugs in without changing RRF.
pub trait RankedRetriever {
    fn ranked_ids(&self) -> &[String];
}

/// Borrowed static ranking (already ordered best → worst).
#[derive(Debug, Clone, Copy)]
pub struct StaticRankedList<'a> {
    pub ids: &'a [String],
}

impl RankedRetriever for StaticRankedList<'_> {
    fn ranked_ids(&self) -> &[String] {
        self.ids
    }
}

/// Build 1-based [`RankedDoc`] list from ordered ids (duplicates dropped, first wins).
pub fn ranked_docs_from_ids(ids: &[String]) -> Vec<RankedDoc> {
    let mut seen = HashMap::new();
    let mut out = Vec::with_capacity(ids.len());
    for id in ids {
        if seen.contains_key(id) {
            continue;
        }
        seen.insert(id.clone(), ());
        out.push(RankedDoc { id: id.clone() });
    }
    out
}

/// Reciprocal Rank Fusion (Cormack et al., 2009).
///
/// `list_a` / `list_b` are best-first. Rank is 1-based position after de-duplication
/// within each list. Docs appearing in only one list still receive that list's term.
/// Empty `list_b` (dense unavailable) ⇒ pure lexical RRF (transparent degradation).
pub fn apply_rrf(list_a: &[RankedDoc], list_b: &[RankedDoc], k: f64) -> Vec<RrfHit> {
    let k = if k > 0.0 { k } else { RRF_K };
    let mut scores: HashMap<String, f64> = HashMap::new();
    accumulate_rrf(&mut scores, list_a, k);
    accumulate_rrf(&mut scores, list_b, k);

    let mut hits: Vec<RrfHit> = scores
        .into_iter()
        .map(|(id, rrf_score)| RrfHit { id, rrf_score })
        .collect();
    // Deterministic order: score desc, then id asc.
    hits.sort_by(|a, b| {
        b.rrf_score
            .partial_cmp(&a.rrf_score)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.id.cmp(&b.id))
    });
    hits
}

/// Fuse two [`RankedRetriever`] channels (architecture hook for future DPR models).
pub fn apply_rrf_retrievers<A: RankedRetriever, B: RankedRetriever>(
    channel_a: &A,
    channel_b: &B,
    k: f64,
) -> Vec<RrfHit> {
    let a = ranked_docs_from_ids(channel_a.ranked_ids());
    let b = ranked_docs_from_ids(channel_b.ranked_ids());
    apply_rrf(&a, &b, k)
}

fn accumulate_rrf(scores: &mut HashMap<String, f64>, list: &[RankedDoc], k: f64) {
    for (idx, doc) in list.iter().enumerate() {
        let rank = (idx + 1) as f64;
        let term = 1.0 / (k + rank);
        *scores.entry(doc.id.clone()).or_insert(0.0) += term;
    }
}

/// Ebbinghaus retention `S = exp(−Δt / τ)` with `Δt` in days, `τ` in days.
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

/// Elapsed days from `created_at` to `now` (both Unix UTC seconds). Negatives → 0.
pub fn delta_days_utc(created_at_unix: i64, now_unix: i64) -> f64 {
    let delta_secs = now_unix.saturating_sub(created_at_unix).max(0) as f64;
    delta_secs / SECS_PER_DAY
}

/// RRF × Ebbinghaus: `recall_score = rrf_score × exp(−Δt/τ)`.
///
/// Strong semantic (high RRF) can outrank fresher weak matches — the flashback regime.
pub fn apply_ebbinghaus_rerank(
    candidates: &[RecallCandidate],
    now_unix_utc: i64,
    tau_days: f64,
) -> Vec<RecallHit> {
    let mut out: Vec<RecallHit> = candidates
        .iter()
        .map(|c| {
            let delta = delta_days_utc(c.created_at, now_unix_utc);
            let retention = ebbinghaus_retention(delta, tau_days);
            RecallHit {
                id: c.id.clone(),
                text_content: c.text_content.clone(),
                distance: c.distance,
                created_at: c.created_at,
                recall_score: c.rrf_score * retention,
            }
        })
        .collect();
    out.sort_by(|a, b| {
        b.recall_score
            .partial_cmp(&a.recall_score)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.id.cmp(&b.id))
    });
    out
}

/// End-to-end: RRF fuse two id lists + metadata map → temporally reranked hits.
pub fn fuse_and_rerank(
    lexical_ids: &[String],
    dense_ids: &[String],
    meta: &HashMap<String, (String, f64, i64)>,
    now_unix_utc: i64,
    tau_days: f64,
    rrf_k: f64,
    limit: usize,
) -> Vec<RecallHit> {
    let fused = apply_rrf_retrievers(
        &StaticRankedList { ids: lexical_ids },
        &StaticRankedList { ids: dense_ids },
        rrf_k,
    );
    let candidates: Vec<RecallCandidate> = fused
        .into_iter()
        .filter_map(|h| {
            let (text, distance, created_at) = meta.get(&h.id)?.clone();
            Some(RecallCandidate {
                id: h.id,
                text_content: text,
                distance,
                created_at,
                rrf_score: h.rrf_score,
            })
        })
        .collect();
    let mut ranked = apply_ebbinghaus_rerank(&candidates, now_unix_utc, tau_days);
    if ranked.len() > limit {
        ranked.truncate(limit);
    }
    ranked
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rrf_prefers_docs_high_in_both_lists() {
        let a = ranked_docs_from_ids(&[
            "x".into(),
            "shared".into(),
            "y".into(),
        ]);
        let b = ranked_docs_from_ids(&[
            "shared".into(),
            "z".into(),
        ]);
        let fused = apply_rrf(&a, &b, RRF_K);
        assert_eq!(fused[0].id, "shared");
        let shared = fused.iter().find(|h| h.id == "shared").unwrap();
        let only_a = fused.iter().find(|h| h.id == "x").unwrap();
        assert!(shared.rrf_score > only_a.rrf_score);
    }

    #[test]
    fn rrf_empty_second_list_is_lexical_only() {
        let a = ranked_docs_from_ids(&["a".into(), "b".into()]);
        let fused = apply_rrf(&a, &[], RRF_K);
        assert_eq!(fused.len(), 2);
        assert_eq!(fused[0].id, "a");
        assert!((fused[0].rrf_score - 1.0 / (RRF_K + 1.0)).abs() < 1e-12);
    }

    #[test]
    fn ebbinghaus_half_life_is_one_half() {
        let tau = tau_days_from_half_life(30.0);
        let w = ebbinghaus_retention(30.0, tau);
        assert!((w - 0.5).abs() < 1e-9);
    }

    #[test]
    fn flashback_strong_old_beats_weak_fresh() {
        // Short τ so a strong RRF match from ~1.5τ ago can still win
        // (Ebbinghaus flashback): recall = rrf × exp(−Δt/τ).
        let now = 1_700_000_000_i64;
        let tau = 10.0;
        let old = RecallCandidate {
            id: "old".into(),
            text_content: "flashback".into(),
            distance: 0.1,
            created_at: now - (15 * 86_400),
            rrf_score: 0.033, // dual-list top ranks
        };
        let fresh = RecallCandidate {
            id: "fresh".into(),
            text_content: "weak".into(),
            distance: 0.9,
            created_at: now - 86_400,
            rrf_score: 0.004, // deep single-list rank
        };
        let ranked = apply_ebbinghaus_rerank(&[old, fresh], now, tau);
        assert_eq!(ranked[0].id, "old");
        assert!(ranked[0].recall_score > ranked[1].recall_score);
    }

    #[test]
    fn fuse_and_rerank_is_deterministic() {
        let mut meta = HashMap::new();
        meta.insert("a".into(), ("ta".into(), 0.2, 100));
        meta.insert("b".into(), ("tb".into(), 0.3, 200));
        let now = 1_000_000_i64;
        let once = fuse_and_rerank(
            &["a".into(), "b".into()],
            &["b".into()],
            &meta,
            now,
            default_tau_days(),
            RRF_K,
            10,
        );
        let twice = fuse_and_rerank(
            &["a".into(), "b".into()],
            &["b".into()],
            &meta,
            now,
            default_tau_days(),
            RRF_K,
            10,
        );
        assert_eq!(once.len(), twice.len());
        for (x, y) in once.iter().zip(twice.iter()) {
            assert_eq!(x.id, y.id);
            assert!((x.recall_score - y.recall_score).abs() < 1e-15);
        }
    }
}
