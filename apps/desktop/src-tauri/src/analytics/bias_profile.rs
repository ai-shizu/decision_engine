//! Cognitive bias fingerprint aggregation (Phase 8).
//!
//! # Academic grounding
//!
//! **Cognitive Behavioral Therapy (CBT)** — Beck (1976) & Burns (1980):
//! depression and anxiety are maintained by habitual *cognitive distortions*
//! (irrational thought patterns). This module never invents labels; it only
//! aggregates vault rows whose categories were extracted under GBNF into the
//! canonical ten Burns categories.
//!
//! Aggregation is **fully deterministic** (F-14): fixed category order, no RNG,
//! no egress. Time windows use Unix UTC seconds.

use serde::Serialize;

use crate::db::DistortionTagRow;

/// Seconds in 30 calendar-free days (UTC).
pub const WINDOW_30D_SECS: i64 = 30 * 86_400;

/// Burns (1980) ten distortions in canonical order (must match GBNF / schema).
pub const BURNS_CATEGORIES: [&str; 10] = [
    "all_or_nothing",
    "overgeneralization",
    "mental_filter",
    "disqualifying_the_positive",
    "jumping_to_conclusions",
    "magnification_minimization",
    "emotional_reasoning",
    "should_statements",
    "labeling",
    "personalization",
];

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct CategoryBiasScore {
    pub category: String,
    pub count: u64,
    pub mean_confidence: f64,
    pub recent_count_30d: u64,
    /// Share of total tags in \[0, 1\]. Sum across categories ≈ 1 when total > 0.
    pub share: f64,
    /// Radar plot value: blend of share and mean confidence in \[0, 1\].
    pub score: f64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct CognitiveBiasProfile {
    pub schema: String,
    pub total_tags: u64,
    pub run_count: u64,
    pub categories: Vec<CategoryBiasScore>,
    pub as_of_unix: i64,
}

pub const BIAS_PROFILE_SCHEMA: &str = "cognitive_bias_profile.v1";

/// Deterministic category frequencies + 30d trend counts.
pub fn aggregate_bias_profile(rows: &[DistortionTagRow], now_unix: i64) -> CognitiveBiasProfile {
    let cutoff = now_unix.saturating_sub(WINDOW_30D_SECS);
    let total = rows.len() as u64;
    let mut run_ids: Vec<&str> = rows.iter().map(|r| r.run_id.as_str()).collect();
    run_ids.sort_unstable();
    run_ids.dedup();
    let run_count = run_ids.len() as u64;

    let mut categories = Vec::with_capacity(10);
    for key in BURNS_CATEGORIES {
        let mut count = 0u64;
        let mut conf_sum = 0.0f64;
        let mut recent = 0u64;
        for row in rows {
            if row.category != key {
                continue;
            }
            count += 1;
            let c = if row.confidence_score.is_finite() {
                row.confidence_score.clamp(0.0, 1.0)
            } else {
                0.0
            };
            conf_sum += c;
            if row.created_at >= cutoff {
                recent += 1;
            }
        }
        let mean_confidence = if count > 0 {
            conf_sum / count as f64
        } else {
            0.0
        };
        let share = if total > 0 {
            count as f64 / total as f64
        } else {
            0.0
        };
        // Equal blend keeps rare-but-high-confidence axes visible on radar.
        let score = (0.5 * share + 0.5 * mean_confidence).clamp(0.0, 1.0);
        categories.push(CategoryBiasScore {
            category: key.to_string(),
            count,
            mean_confidence,
            recent_count_30d: recent,
            share,
            score,
        });
    }

    CognitiveBiasProfile {
        schema: BIAS_PROFILE_SCHEMA.to_string(),
        total_tags: total,
        run_count,
        categories,
        as_of_unix: now_unix,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(cat: &str, conf: f64, created_at: i64, run_id: &str) -> DistortionTagRow {
        DistortionTagRow {
            id: format!("{run_id}-{cat}-{created_at}"),
            created_at,
            category: cat.to_string(),
            snippet: "x".into(),
            confidence_score: conf,
            source_kind: "diary".into(),
            source_id: "d1".into(),
            run_id: run_id.to_string(),
        }
    }

    #[test]
    fn empty_profile_has_ten_zero_axes() {
        let p = aggregate_bias_profile(&[], 1_700_000_000);
        assert_eq!(p.categories.len(), 10);
        assert_eq!(p.total_tags, 0);
        assert!(p.categories.iter().all(|c| c.count == 0 && c.score == 0.0));
    }

    #[test]
    fn shares_sum_to_one_and_order_is_burns() {
        let now = 1_700_000_000_i64;
        let rows = vec![
            row("should_statements", 1.0, now, "r1"),
            row("should_statements", 0.5, now, "r1"),
            row("all_or_nothing", 1.0, now, "r2"),
        ];
        let p = aggregate_bias_profile(&rows, now);
        assert_eq!(p.categories[0].category, "all_or_nothing");
        assert_eq!(p.categories[0].count, 1);
        assert_eq!(p.categories[7].category, "should_statements");
        assert_eq!(p.categories[7].count, 2);
        let share_sum: f64 = p.categories.iter().map(|c| c.share).sum();
        assert!((share_sum - 1.0).abs() < 1e-12);
        assert_eq!(p.run_count, 2);
    }

    #[test]
    fn recent_window_excludes_old_tags() {
        let now = 1_700_000_000_i64;
        let rows = vec![
            row("labeling", 1.0, now - WINDOW_30D_SECS - 1, "r1"),
            row("labeling", 1.0, now, "r1"),
        ];
        let p = aggregate_bias_profile(&rows, now);
        let labeling = p
            .categories
            .iter()
            .find(|c| c.category == "labeling")
            .unwrap();
        assert_eq!(labeling.count, 2);
        assert_eq!(labeling.recent_count_30d, 1);
    }
}
