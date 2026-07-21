//! Irreversible compiler: Vault fossils → abstract tactics (Phase 14 / I-22 / F-14).
//!
//! Concrete fields (dates, amounts, IDs, free text) are consumed inside
//! [`compile_interviewer_tactics`] and must never appear in the return type.

use serde::{Deserialize, Serialize};

use super::render_guard::{seal_oni_payload, RenderGuardError};
use super::tactics::InterviewerTactic;

/// Owned fossil bundle for one compile pass. Dropped at end of compile — not Clone
/// into the LLM path.
#[derive(Debug)]
pub struct CognitiveFossilSnapshot {
    /// Twin R(t) at decision time, if known.
    pub r_at_decision: Option<f64>,
    /// Burns-category keys only (e.g. `overgeneralization`). Free-text snippets forbidden.
    pub distortion_categories: Vec<String>,
    /// Recent purchase totals (yen integers). Used only for magnitude bands; never echoed.
    pub purchase_amounts: Vec<i64>,
    /// Count of late-night (JST) purchases in the window.
    pub late_night_purchase_count: u32,
    /// Count of unverified / impulse-flagged purchases.
    pub impulse_purchase_count: u32,
    /// Opaque ids / merchants / dates for leak blacklists only — never copied into tactics.
    pub blacklist_seed_terms: Vec<String>,
}

/// Sealed tactic set for 鬼モード. Constructible only via compile (or test helper).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct AbstractTacticSet {
    tactics: Vec<InterviewerTactic>,
    /// Deterministic leak lexicon derived during compile (owned copies of seed terms).
    blacklist: Vec<String>,
}

impl AbstractTacticSet {
    pub fn blacklist_terms(&self) -> &[String] {
        &self.blacklist
    }

    pub fn tactics(&self) -> &[InterviewerTactic] {
        &self.tactics
    }

    /// Rehydrate a previously frozen directive set (Phase 14.4 artifact thaw).
    pub fn from_frozen(tactics: Vec<InterviewerTactic>, blacklist: Vec<String>) -> Self {
        Self {
            tactics: normalize_tactics(tactics),
            blacklist: normalize_blacklist(blacklist),
        }
    }

    /// Join tactic instructions for the oni system prompt (no fossils).
    pub fn to_system_instructions(&self) -> String {
        let mut parts: Vec<String> = self.tactics.iter().map(|t| t.to_instruction()).collect();
        parts.push(
            "【I-22】候補の個人財務・CBT原文・Twin数値・固有の私的事象をプロンプトに\
再現・引用・推測してはならない。抽象的な思考品質のみを評価せよ。".into(),
        );
        parts.join("\n")
    }

}

/// Typestate marker: oni interview stream. No method takes a vault handle.
#[derive(Debug, Default, Clone, Copy)]
pub struct OniModePrompt;

impl OniModePrompt {
    /// Build prompt text solely from abstract tactics (+ optional public job brief).
    pub fn render(tactics: &AbstractTacticSet, public_brief: &str) -> String {
        let brief = public_brief.trim();
        if brief.is_empty() {
            tactics.to_system_instructions()
        } else {
            format!(
                "{}\n【公開ブリーフ】\n{}",
                tactics.to_system_instructions(),
                brief
            )
        }
    }

    /// Render then leak-gate against the compile-time blacklist (I-22 hard gate).
    pub fn render_sealed(
        tactics: &AbstractTacticSet,
        public_brief: &str,
    ) -> Result<String, RenderGuardError> {
        let text = Self::render(tactics, public_brief);
        seal_oni_payload(&text, tactics.blacklist_terms())
    }
}

/// Irreversible map: fossils in → tactics out. F-14: sorted + deduped.
pub fn compile_interviewer_tactics(snapshot: CognitiveFossilSnapshot) -> AbstractTacticSet {
    let CognitiveFossilSnapshot {
        r_at_decision,
        distortion_categories,
        purchase_amounts,
        late_night_purchase_count,
        impulse_purchase_count,
        blacklist_seed_terms,
    } = snapshot;

    let mut tactics: Vec<InterviewerTactic> = Vec::new();

    // --- category → tactic (keys only; snippet text never read) ---
    for cat in &distortion_categories {
        let key = cat.trim();
        if key.is_empty() {
            continue;
        }
        match key {
            "overgeneralization" | "all_or_nothing" | "labeling" => {
                tactics.push(InterviewerTactic::ProbeOvergeneralization {
                    intensity: intensity_from_category_count(&distortion_categories, key),
                });
            }
            "magnification_minimization" | "mental_filter" => {
                tactics.push(InterviewerTactic::ForceNuancedTradeoff { intensity: 3 });
            }
            "should_statements" | "emotional_reasoning" => {
                tactics.push(InterviewerTactic::ForceNuancedTradeoff { intensity: 2 });
            }
            "jumping_to_conclusions" | "personalization" | "disqualifying_the_positive" => {
                tactics.push(InterviewerTactic::ProbeOvergeneralization { intensity: 2 });
            }
            _ => {}
        }
    }

    // --- R(t) band → quantitative stress (scalar never leaves as text) ---
    if let Some(r) = r_at_decision.filter(|v| v.is_finite()) {
        let r = r.clamp(0.0, 1.0);
        if r <= 0.34 {
            tactics.push(InterviewerTactic::StressTestQuantitative { intensity: 5 });
            tactics.push(InterviewerTactic::TechnicalEdgeCaseProbe { intensity: 4 });
        } else if r <= 0.67 {
            tactics.push(InterviewerTactic::StressTestQuantitative { intensity: 3 });
        } else {
            tactics.push(InterviewerTactic::TechnicalEdgeCaseProbe { intensity: 2 });
        }
    }

    // --- spend pattern bands (amounts used only as thresholds, not emitted) ---
    let n_amt = purchase_amounts.len() as u32;
    if n_amt >= 3 {
        let max_amt = purchase_amounts.iter().copied().max().unwrap_or(0);
        if max_amt >= 10_000 {
            tactics.push(InterviewerTactic::StressTestQuantitative { intensity: 4 });
        }
    }
    if late_night_purchase_count > 0 || impulse_purchase_count > 0 {
        let intens = 2u8
            .saturating_add(late_night_purchase_count.min(3) as u8)
            .saturating_add(impulse_purchase_count.min(2) as u8)
            .clamp(1, 5);
        tactics.push(InterviewerTactic::ForceNuancedTradeoff { intensity: intens });
    }

    // Default pressure if nothing mapped — still abstract.
    if tactics.is_empty() {
        tactics.push(InterviewerTactic::TechnicalEdgeCaseProbe { intensity: 2 });
        tactics.push(InterviewerTactic::ForceNuancedTradeoff { intensity: 2 });
    }

    let tactics = normalize_tactics(tactics);
    let blacklist = normalize_blacklist(blacklist_seed_terms);
    // Fossils are fully moved/consumed; only tactics + blacklist leave this scope.
    AbstractTacticSet { tactics, blacklist }
}

fn intensity_from_category_count(cats: &[String], key: &str) -> u8 {
    let n = cats.iter().filter(|c| c.trim() == key).count();
    match n {
        0 => 1,
        1 => 2,
        2 => 3,
        3 => 4,
        _ => 5,
    }
}

/// Deterministic: clamp → sort_by_key → dedup by discriminant (keep max intensity).
fn normalize_tactics(mut tactics: Vec<InterviewerTactic>) -> Vec<InterviewerTactic> {
    for t in &mut tactics {
        *t = t.clamped();
    }
    tactics.sort_by_key(|t| t.sort_key());
    // Collapse same discriminant to highest intensity (last after sort within disc).
    let mut best: Vec<InterviewerTactic> = Vec::new();
    for t in tactics {
        let disc = t.sort_key().0;
        match best.last_mut() {
            Some(prev) if prev.sort_key().0 == disc => {
                if t.sort_key().1 >= prev.sort_key().1 {
                    *prev = t;
                }
            }
            _ => best.push(t),
        }
    }
    best
}

fn normalize_blacklist(seeds: Vec<String>) -> Vec<String> {
    let mut out: Vec<String> = seeds
        .into_iter()
        .map(|s| s.trim().to_string())
        .filter(|s| s.chars().count() >= 2)
        .collect();
    out.sort();
    out.dedup();
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snap_overgen() -> CognitiveFossilSnapshot {
        CognitiveFossilSnapshot {
            r_at_decision: Some(0.2),
            distortion_categories: vec!["overgeneralization".into()],
            purchase_amounts: vec![500, 12_000],
            late_night_purchase_count: 2,
            impulse_purchase_count: 1,
            blacklist_seed_terms: vec!["山田商事".into(), "4200".into()],
        }
    }

    #[test]
    fn compile_is_deterministic_and_sorted() {
        let a = compile_interviewer_tactics(snap_overgen());
        let b = compile_interviewer_tactics(CognitiveFossilSnapshot {
            r_at_decision: Some(0.2),
            distortion_categories: vec!["overgeneralization".into()],
            purchase_amounts: vec![500, 12_000],
            late_night_purchase_count: 2,
            impulse_purchase_count: 1,
            blacklist_seed_terms: vec!["山田商事".into(), "4200".into()],
        });
        assert_eq!(a, b);
        let text_a = a.to_system_instructions();
        let text_b = b.to_system_instructions();
        assert_eq!(text_a, text_b);
        assert!(text_a.contains("戦術"));
        let bl = a.blacklist_terms().to_vec();
        let mut sorted = bl.clone();
        sorted.sort();
        assert_eq!(bl, sorted);
    }

    #[test]
    fn instructions_do_not_echo_amounts_or_names() {
        let set = compile_interviewer_tactics(snap_overgen());
        let text = OniModePrompt::render(&set, "Backend SWE");
        assert!(text.contains("戦術"));
        assert!(!text.contains("12000"));
        assert!(!text.contains("山田"));
        assert!(!text.contains("4200"));
    }

    #[test]
    fn overgeneralization_maps_to_probe() {
        let set = compile_interviewer_tactics(snap_overgen());
        assert!(set.to_system_instructions().contains("過度一般化"));
    }
}
