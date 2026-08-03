//! Phase 14.4 — immutable, replayable interview session artifacts.
//!
//! Freeze start-of-session directives + evidence *bodies* so Vacuum of live
//! vault rows cannot leave evaluation provenance dangling (F-14 / offline).

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::interviewer_context::{
    compile_interviewer_tactics, AbstractTacticSet, CognitiveFossilSnapshot,
};
use super::mentor_zpd::{r_t_from_unit_interval, COLISEUM_GENERATION_SEED};

pub const INTERVIEW_SESSION_ARTIFACT_SCHEMA: &str = "interview_session_artifact.v1";

/// Pre-allocated turn slots for deterministic per-turn seeds (Foundation+Pressure+buffer).
pub const PER_TURN_SEED_SLOTS: usize = 32;

const SUMMARY_MAX: usize = 280;

/// Frozen distortion body — text summary is owned, not a dangling vault FK.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct DistortionEvidenceSnap {
    /// Original row id at freeze time (audit only; not required for replay).
    pub source_id: String,
    pub category: String,
    /// Verbatim snippet copy at session start.
    pub text_summary: String,
    pub confidence_score: f64,
}

/// Frozen purchase body — summary is owned; amounts are banded (no yen in fingerprint text).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct PurchaseEvidenceSnap {
    pub source_id: String,
    /// Human-readable freeze of merchant + band + flags (self-contained).
    pub text_summary: String,
    pub amount_band: String,
    pub late_night: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct EvidenceSnapshot {
    pub distortions: Vec<DistortionEvidenceSnap>,
    pub purchases: Vec<PurchaseEvidenceSnap>,
    pub frozen_at_unix: i64,
}

/// Self-contained interview start freeze — evaluation must bind to this, not live vault.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct InterviewSessionArtifact {
    pub schema: String,
    pub session_id: String,
    /// Irreversible compile result from Finding 1 (frozen directives).
    pub frozen_directives: AbstractTacticSet,
    pub model_hash: String,
    pub per_turn_seeds: Vec<u32>,
    /// Quantized R(t) at session start (ZPD signal).
    pub r_t_snapshot: u8,
    pub evidence: EvidenceSnapshot,
    /// SHA-256 hex of canonical artifact body (excludes this field during hash).
    pub fingerprint: String,
}

impl InterviewSessionArtifact {
    /// Build an immutable artifact from fossils + evidence bodies (F-14).
    pub fn freeze(
        session_id: impl Into<String>,
        model_hash: impl Into<String>,
        r_unit: f64,
        fossil: CognitiveFossilSnapshot,
        evidence: EvidenceSnapshot,
    ) -> Self {
        let session_id = session_id.into();
        let model_hash = truncate_hash_label(model_hash.into());
        let r_t_snapshot = r_t_from_unit_interval(r_unit);
        let frozen_directives = compile_interviewer_tactics(fossil);
        let per_turn_seeds = allocate_per_turn_seeds(COLISEUM_GENERATION_SEED, PER_TURN_SEED_SLOTS);
        let mut art = Self {
            schema: INTERVIEW_SESSION_ARTIFACT_SCHEMA.into(),
            session_id,
            frozen_directives,
            model_hash,
            per_turn_seeds,
            r_t_snapshot,
            evidence: normalize_evidence(evidence),
            fingerprint: String::new(),
        };
        art.rehydrate_directives();
        art.fingerprint = art.generate_fingerprint();
        art
    }

    /// Deterministic SHA-256 over canonical fields (excludes `fingerprint`).
    pub fn generate_fingerprint(&self) -> String {
        let mut hasher = Sha256::new();
        hasher.update(INTERVIEW_SESSION_ARTIFACT_SCHEMA.as_bytes());
        hasher.update(b"\0");
        hasher.update(self.session_id.as_bytes());
        hasher.update(b"\0");
        hasher.update(self.model_hash.as_bytes());
        hasher.update(b"\0");
        hasher.update([self.r_t_snapshot]);
        hasher.update(b"\0");
        for seed in &self.per_turn_seeds {
            hasher.update(seed.to_le_bytes());
        }
        hasher.update(b"\0");
        for t in self.frozen_directives.tactics() {
            let key = t.sort_key();
            hasher.update([key.0, key.1]);
        }
        for term in self.frozen_directives.blacklist_terms() {
            hasher.update(term.as_bytes());
            hasher.update(b"\0");
        }
        hasher.update(self.evidence.frozen_at_unix.to_le_bytes());
        for d in &self.evidence.distortions {
            hasher.update(d.source_id.as_bytes());
            hasher.update(b"\0");
            hasher.update(d.category.as_bytes());
            hasher.update(b"\0");
            hasher.update(d.text_summary.as_bytes());
            hasher.update(b"\0");
            hasher.update(d.confidence_score.to_bits().to_le_bytes());
        }
        for p in &self.evidence.purchases {
            hasher.update(p.source_id.as_bytes());
            hasher.update(b"\0");
            hasher.update(p.text_summary.as_bytes());
            hasher.update(b"\0");
            hasher.update(p.amount_band.as_bytes());
            hasher.update(b"\0");
            hasher.update([u8::from(p.late_night)]);
        }
        hex::encode(hasher.finalize())
    }

    /// Fail closed if stored fingerprint diverges from recomputed body hash.
    pub fn verify_fingerprint(&self) -> bool {
        self.fingerprint == self.generate_fingerprint()
    }

    /// Seed for turn index `i` (deterministic; clamps to last slot).
    pub fn seed_for_turn(&self, turn_index: usize) -> u32 {
        if self.per_turn_seeds.is_empty() {
            return COLISEUM_GENERATION_SEED.wrapping_add(turn_index as u32);
        }
        let idx = turn_index.min(self.per_turn_seeds.len() - 1);
        self.per_turn_seeds[idx]
    }

    /// Rehydrate directives after serde (normalize intensities / blacklist).
    pub fn rehydrate_directives(&mut self) {
        self.frozen_directives = AbstractTacticSet::from_frozen(
            self.frozen_directives.tactics().to_vec(),
            self.frozen_directives.blacklist_terms().to_vec(),
        );
    }
}

/// Allocate F-14 per-turn seeds from the coliseum base seed.
pub fn allocate_per_turn_seeds(base: u32, n: usize) -> Vec<u32> {
    (0..n)
        .map(|i| base.wrapping_mul(1_000_003).wrapping_add(i as u32))
        .collect()
}

/// Band yen totals without retaining exact amounts in the band label set.
pub fn amount_band(yen: i64) -> &'static str {
    match yen {
        i64::MIN..=0 => "none",
        1..=999 => "micro",
        1_000..=4_999 => "low",
        5_000..=19_999 => "mid",
        20_000..=99_999 => "high",
        _ => "extreme",
    }
}

/// JST hour from unix seconds (deterministic; no chrono TZ DB).
pub fn is_late_night_jst(occurred_at_unix: i64) -> bool {
    let jst = occurred_at_unix.saturating_add(9 * 3600);
    let tod = ((jst % 86_400) + 86_400) % 86_400;
    let hour = (tod / 3600) as u32;
    hour >= 23 || hour < 5
}

pub fn purchase_text_summary(merchant_norm: &str, yen: i64, late_night: bool) -> String {
    let band = amount_band(yen);
    let night = if late_night { "late_night" } else { "day" };
    truncate_summary(&format!(
        "merchant={} band={} when={}",
        merchant_norm.trim(),
        band,
        night
    ))
}

fn normalize_evidence(mut e: EvidenceSnapshot) -> EvidenceSnapshot {
    for d in &mut e.distortions {
        d.source_id = d.source_id.trim().to_string();
        d.category = d.category.trim().to_string();
        d.text_summary = truncate_summary(d.text_summary.trim());
        if !d.confidence_score.is_finite() {
            d.confidence_score = 0.0;
        }
        d.confidence_score = d.confidence_score.clamp(0.0, 1.0);
    }
    e.distortions
        .retain(|d| !d.category.is_empty() && !d.text_summary.is_empty());
    for p in &mut e.purchases {
        p.source_id = p.source_id.trim().to_string();
        p.text_summary = truncate_summary(p.text_summary.trim());
        p.amount_band = p.amount_band.trim().to_string();
        if p.amount_band.is_empty() {
            p.amount_band = "unknown".into();
        }
    }
    e.purchases
        .retain(|p| !p.source_id.is_empty() && !p.text_summary.is_empty());
    e
}

fn truncate_summary(s: &str) -> String {
    if s.chars().count() <= SUMMARY_MAX {
        return s.to_string();
    }
    s.chars().take(SUMMARY_MAX).collect::<String>() + "…"
}

fn truncate_hash_label(s: String) -> String {
    let t = s.trim();
    if t.is_empty() {
        return hex::encode(Sha256::digest(b"pocket-brain.gguf"));
    }
    if t.len() > 128 {
        return t.chars().take(128).collect();
    }
    t.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_fossil() -> CognitiveFossilSnapshot {
        CognitiveFossilSnapshot {
            r_at_decision: Some(0.55),
            distortion_categories: vec!["overgeneralization".into()],
            purchase_amounts: vec![12_000],
            late_night_purchase_count: 1,
            impulse_purchase_count: 0,
            blacklist_seed_terms: vec!["秘密商事".into()],
        }
    }

    fn sample_evidence() -> EvidenceSnapshot {
        EvidenceSnapshot {
            distortions: vec![DistortionEvidenceSnap {
                source_id: "dist-1".into(),
                category: "overgeneralization".into(),
                text_summary: "いつも失敗する".into(),
                confidence_score: 0.8,
            }],
            purchases: vec![PurchaseEvidenceSnap {
                source_id: "pur-1".into(),
                text_summary: purchase_text_summary("コンビニ", 1200, true),
                amount_band: amount_band(1200).into(),
                late_night: true,
            }],
            frozen_at_unix: 1_700_000_000,
        }
    }

    #[test]
    fn fingerprint_is_stable_and_verifies() {
        let a = InterviewSessionArtifact::freeze(
            "iv-1",
            "pocket-brain.gguf",
            0.55,
            sample_fossil(),
            sample_evidence(),
        );
        assert_eq!(a.fingerprint.len(), 64);
        assert!(a.verify_fingerprint());
        let again = a.generate_fingerprint();
        assert_eq!(a.fingerprint, again);
    }

    #[test]
    fn fingerprint_changes_when_evidence_body_changes() {
        let mut e = sample_evidence();
        let a1 = InterviewSessionArtifact::freeze(
            "iv-1",
            "m",
            0.55,
            sample_fossil(),
            e.clone(),
        );
        e.distortions[0].text_summary = "別の要約".into();
        let a2 = InterviewSessionArtifact::freeze("iv-1", "m", 0.55, sample_fossil(), e);
        assert_ne!(a1.fingerprint, a2.fingerprint);
    }

    #[test]
    fn vacuum_safe_evidence_retains_text_not_only_ids() {
        let a = InterviewSessionArtifact::freeze(
            "iv-1",
            "m",
            0.7,
            sample_fossil(),
            sample_evidence(),
        );
        assert!(a.evidence.distortions[0].text_summary.contains("失敗"));
        assert!(a.evidence.purchases[0].text_summary.contains("merchant="));
    }

    #[test]
    fn per_turn_seeds_are_deterministic() {
        let s1 = allocate_per_turn_seeds(14, 4);
        let s2 = allocate_per_turn_seeds(14, 4);
        assert_eq!(s1, s2);
        assert_ne!(s1[0], s1[1]);
    }
}
