//! Phase 14.3 — two-layer evaluation schemas (construct purity).
//!
//! # Layer separation (non-negotiable)
//!
//! - [`InterviewEvaluationV1`]: transcript / turn provenance **only**.
//!   Validation APIs intentionally omit any vault handle or fossil type.
//! - [`MetacognitiveDebriefV1`]: opt-in mirror insights; may reference
//!   **abstract** vault tendencies ([`VaultMirrorAbstract`]) — never raw
//!   purchase rows, CBT free-text, or Twin scalars in the evaluation object.

use std::collections::BTreeSet;
use std::fmt;

use serde::{Deserialize, Serialize};

pub const INTERVIEW_EVALUATION_SCHEMA: &str = "interview_evaluation.v1";
pub const METACOGNITIVE_DEBRIEF_SCHEMA: &str = "metacognitive_debrief.v1";

pub const INTERVIEW_EVALUATION_V1_GBNF: &str =
    include_str!("assets/interview_evaluation_v1.gbnf");
pub const METACOGNITIVE_DEBRIEF_V1_GBNF: &str =
    include_str!("assets/metacognitive_debrief_v1.gbnf");

const UNKNOWN: &str = "unknown";
const SNIPPET_MAX: usize = 280;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EvaluationError {
    SchemaMismatch,
    EmptyProvenance,
    UnknownTurnId { turn_id: String },
    ScoreOutOfRange,
    VaultFieldLeak,
    MirrorMismatch,
    NotOptedIn,
}

impl fmt::Display for EvaluationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::SchemaMismatch => write!(f, "evaluation_schema_mismatch"),
            Self::EmptyProvenance => write!(f, "evaluation_empty_provenance"),
            Self::UnknownTurnId { turn_id } => {
                write!(f, "evaluation_unknown_turn_id:{turn_id}")
            }
            Self::ScoreOutOfRange => write!(f, "evaluation_score_out_of_range"),
            Self::VaultFieldLeak => write!(f, "evaluation_vault_field_leak"),
            Self::MirrorMismatch => write!(f, "debrief_mirror_mismatch"),
            Self::NotOptedIn => write!(f, "debrief_not_opted_in"),
        }
    }
}

impl std::error::Error for EvaluationError {}

/// Transcript turn reference for Layer-1 validation (no vault types).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TranscriptTurnRef<'a> {
    pub turn_id: &'a str,
    pub role: &'a str,
    pub text: &'a str,
}

/// Evidence tied to dialogue turns — never vault row ids.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TurnProvenance {
    pub turn_id: String,
    pub quote_snippet: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AxisScoreV1 {
    pub score: f64,
    pub provenance: Vec<TurnProvenance>,
}

/// Layer 1 — pure interview scorecard (pass/fail axes). Transcript-only.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InterviewEvaluationV1 {
    pub schema: String,
    pub mece_structure: AxisScoreV1,
    pub hypothesis_thinking: AxisScoreV1,
    pub quantitative_validity: AxisScoreV1,
    pub stress_resilience: AxisScoreV1,
    pub overall_pass: bool,
    pub summary: String,
}

impl InterviewEvaluationV1 {
    pub fn normalize(&mut self) {
        self.schema = INTERVIEW_EVALUATION_SCHEMA.into();
        normalize_axis(&mut self.mece_structure);
        normalize_axis(&mut self.hypothesis_thinking);
        normalize_axis(&mut self.quantitative_validity);
        normalize_axis(&mut self.stress_resilience);
        self.summary = truncate_snippet(self.summary.trim());
        if self.summary.is_empty() {
            self.summary = UNKNOWN.into();
        }
    }

    pub fn from_json_str(raw: &str) -> Result<Self, serde_json::Error> {
        let mut v: Self = serde_json::from_str(raw)?;
        v.normalize();
        Ok(v)
    }
}

/// Validate Layer-1 against an allowed turn-id set from the transcript.
///
/// **Type boundary:** no vault handle / fossil / purchase / distortion row
/// appears in this signature — construct purity by construction.
pub fn validate_interview_evaluation(
    eval: &InterviewEvaluationV1,
    transcript: &[TranscriptTurnRef<'_>],
) -> Result<(), EvaluationError> {
    if eval.schema != INTERVIEW_EVALUATION_SCHEMA {
        return Err(EvaluationError::SchemaMismatch);
    }
    let allowed: BTreeSet<&str> = transcript.iter().map(|t| t.turn_id).collect();
    for axis in [
        &eval.mece_structure,
        &eval.hypothesis_thinking,
        &eval.quantitative_validity,
        &eval.stress_resilience,
    ] {
        validate_axis(axis, &allowed)?;
    }
    reject_vaultish_text(&eval.summary)?;
    Ok(())
}

fn validate_axis(axis: &AxisScoreV1, allowed: &BTreeSet<&str>) -> Result<(), EvaluationError> {
    if !axis.score.is_finite() || !(0.0..=1.0).contains(&axis.score) {
        return Err(EvaluationError::ScoreOutOfRange);
    }
    if axis.provenance.is_empty() {
        return Err(EvaluationError::EmptyProvenance);
    }
    for p in &axis.provenance {
        if p.turn_id.trim().is_empty() || !allowed.contains(p.turn_id.as_str()) {
            return Err(EvaluationError::UnknownTurnId {
                turn_id: p.turn_id.clone(),
            });
        }
        if p.quote_snippet.trim().is_empty() {
            return Err(EvaluationError::EmptyProvenance);
        }
        reject_vaultish_text(&p.quote_snippet)?;
        reject_vaultish_text(&p.turn_id)?;
    }
    Ok(())
}

/// Heuristic hard-reject of vault-shaped tokens leaking into Layer-1 text.
fn reject_vaultish_text(s: &str) -> Result<(), EvaluationError> {
    let lower = s.to_ascii_lowercase();
    const FORBIDDEN: &[&str] = &[
        "purchase_id",
        "distortion_id",
        "r_at_decision",
        "vault.",
        "active_distortions",
        "total_amount",
    ];
    for tok in FORBIDDEN {
        if lower.contains(tok) {
            return Err(EvaluationError::VaultFieldLeak);
        }
    }
    Ok(())
}

fn normalize_axis(axis: &mut AxisScoreV1) {
    if !axis.score.is_finite() {
        axis.score = 0.0;
    }
    axis.score = axis.score.clamp(0.0, 1.0);
    for p in &mut axis.provenance {
        p.turn_id = p.turn_id.trim().to_string();
        p.quote_snippet = truncate_snippet(p.quote_snippet.trim());
    }
    axis.provenance
        .retain(|p| !p.turn_id.is_empty() && !p.quote_snippet.is_empty());
}

fn truncate_snippet(s: &str) -> String {
    if s.chars().count() <= SNIPPET_MAX {
        return s.to_string();
    }
    s.chars().take(SNIPPET_MAX).collect::<String>() + "…"
}

// ─── Layer 2: opt-in metacognitive debrief ───────────────────────────────────

/// Abstract vault mirror for Layer-2 only (no row ids, no yen amounts, no free text).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VaultMirrorAbstract {
    /// Burns category keys present in recent history (ids only).
    pub distortion_category_keys: Vec<String>,
    /// Coarse spend tendency band — not amounts.
    pub spend_pattern_band: String,
    pub late_night_tendency: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MetacognitiveInsightV1 {
    pub parallel_label: String,
    pub interview_turn_id: String,
    pub mirror_kind: String,
    pub distortion_category: Option<String>,
    pub note: String,
}

/// Layer 2 — opt-in self-insight. Never feeds pass/fail ([`InterviewEvaluationV1`]).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MetacognitiveDebriefV1 {
    pub schema: String,
    pub opted_in: bool,
    pub insights: Vec<MetacognitiveInsightV1>,
}

impl MetacognitiveDebriefV1 {
    pub fn normalize(&mut self) {
        self.schema = METACOGNITIVE_DEBRIEF_SCHEMA.into();
        for i in &mut self.insights {
            i.parallel_label = truncate_snippet(i.parallel_label.trim());
            i.interview_turn_id = i.interview_turn_id.trim().to_string();
            i.mirror_kind = i.mirror_kind.trim().to_string();
            i.note = truncate_snippet(i.note.trim());
            if let Some(c) = i.distortion_category.as_mut() {
                *c = c.trim().to_string();
                if c.is_empty() {
                    i.distortion_category = None;
                }
            }
        }
        self.insights.retain(|i| {
            !i.interview_turn_id.is_empty() && !i.note.is_empty() && !i.mirror_kind.is_empty()
        });
    }

    pub fn from_json_str(raw: &str) -> Result<Self, serde_json::Error> {
        let mut v: Self = serde_json::from_str(raw)?;
        v.normalize();
        Ok(v)
    }
}

/// Validate Layer-2 against abstract mirror + transcript turn ids.
///
/// Accepts [`VaultMirrorAbstract`] only — not `VaultHandle` / purchase fossils.
pub fn validate_metacognitive_debrief(
    debrief: &MetacognitiveDebriefV1,
    mirror: &VaultMirrorAbstract,
    transcript: &[TranscriptTurnRef<'_>],
) -> Result<(), EvaluationError> {
    if debrief.schema != METACOGNITIVE_DEBRIEF_SCHEMA {
        return Err(EvaluationError::SchemaMismatch);
    }
    if !debrief.opted_in {
        return Err(EvaluationError::NotOptedIn);
    }
    let allowed_turns: BTreeSet<&str> = transcript.iter().map(|t| t.turn_id).collect();
    let allowed_cats: BTreeSet<&str> = mirror
        .distortion_category_keys
        .iter()
        .map(String::as_str)
        .collect();
    for insight in &debrief.insights {
        if !allowed_turns.contains(insight.interview_turn_id.as_str()) {
            return Err(EvaluationError::UnknownTurnId {
                turn_id: insight.interview_turn_id.clone(),
            });
        }
        match insight.mirror_kind.as_str() {
            "distortion_isomorphism" => {
                let Some(cat) = insight.distortion_category.as_deref() else {
                    return Err(EvaluationError::MirrorMismatch);
                };
                if !allowed_cats.contains(cat) {
                    return Err(EvaluationError::MirrorMismatch);
                }
            }
            "spend_pattern" => {
                if mirror.spend_pattern_band.is_empty()
                    || mirror.spend_pattern_band == UNKNOWN
                {
                    return Err(EvaluationError::MirrorMismatch);
                }
            }
            "late_night_tendency" => {
                if !mirror.late_night_tendency {
                    return Err(EvaluationError::MirrorMismatch);
                }
            }
            "resource_parallel" => {}
            _ => return Err(EvaluationError::MirrorMismatch),
        }
    }
    Ok(())
}

/// Assign deterministic turn ids `t-{n}` for transcript rows (F-14).
pub fn assign_turn_ids(roles_and_texts: &[(&str, &str)]) -> Vec<(String, String, String)> {
    roles_and_texts
        .iter()
        .enumerate()
        .map(|(i, (role, text))| (format!("t-{i}"), (*role).to_string(), (*text).to_string()))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_eval() -> InterviewEvaluationV1 {
        InterviewEvaluationV1 {
            schema: INTERVIEW_EVALUATION_SCHEMA.into(),
            mece_structure: AxisScoreV1 {
                score: 0.8,
                provenance: vec![TurnProvenance {
                    turn_id: "t-1".into(),
                    quote_snippet: "まず論点を3つに分けます".into(),
                }],
            },
            hypothesis_thinking: AxisScoreV1 {
                score: 0.7,
                provenance: vec![TurnProvenance {
                    turn_id: "t-2".into(),
                    quote_snippet: "仮説としては需要側です".into(),
                }],
            },
            quantitative_validity: AxisScoreV1 {
                score: 0.6,
                provenance: vec![TurnProvenance {
                    turn_id: "t-3".into(),
                    quote_snippet: "オーダーは10^7程度".into(),
                }],
            },
            stress_resilience: AxisScoreV1 {
                score: 0.75,
                provenance: vec![TurnProvenance {
                    turn_id: "t-4".into(),
                    quote_snippet: "前提を置き直します".into(),
                }],
            },
            overall_pass: true,
            summary: "構造化と定量が安定".into(),
        }
    }

    #[test]
    fn layer1_accepts_transcript_provenance() {
        let eval = sample_eval();
        let turns = [
            TranscriptTurnRef {
                turn_id: "t-1",
                role: "candidate",
                text: "まず論点を3つに分けます",
            },
            TranscriptTurnRef {
                turn_id: "t-2",
                role: "candidate",
                text: "仮説としては需要側です",
            },
            TranscriptTurnRef {
                turn_id: "t-3",
                role: "candidate",
                text: "オーダーは10^7程度",
            },
            TranscriptTurnRef {
                turn_id: "t-4",
                role: "candidate",
                text: "前提を置き直します",
            },
        ];
        assert!(validate_interview_evaluation(&eval, &turns).is_ok());
    }

    #[test]
    fn layer1_rejects_unknown_turn() {
        let eval = sample_eval();
        let turns = [TranscriptTurnRef {
            turn_id: "t-9",
            role: "candidate",
            text: "x",
        }];
        assert!(matches!(
            validate_interview_evaluation(&eval, &turns),
            Err(EvaluationError::UnknownTurnId { .. })
        ));
    }

    #[test]
    fn layer1_rejects_vaultish_leak() {
        let mut eval = sample_eval();
        eval.summary = "see purchase_id=pur-1".into();
        let turns = [
            TranscriptTurnRef {
                turn_id: "t-1",
                role: "c",
                text: "a",
            },
            TranscriptTurnRef {
                turn_id: "t-2",
                role: "c",
                text: "a",
            },
            TranscriptTurnRef {
                turn_id: "t-3",
                role: "c",
                text: "a",
            },
            TranscriptTurnRef {
                turn_id: "t-4",
                role: "c",
                text: "a",
            },
        ];
        assert_eq!(
            validate_interview_evaluation(&eval, &turns),
            Err(EvaluationError::VaultFieldLeak)
        );
    }

    #[test]
    fn layer2_opt_in_and_mirror() {
        let debrief = MetacognitiveDebriefV1 {
            schema: METACOGNITIVE_DEBRIEF_SCHEMA.into(),
            opted_in: true,
            insights: vec![MetacognitiveInsightV1 {
                parallel_label: "圧迫下の視野狭窄".into(),
                interview_turn_id: "t-1".into(),
                mirror_kind: "distortion_isomorphism".into(),
                distortion_category: Some("mental_filter".into()),
                note: "先月のパターンと同型".into(),
            }],
        };
        let mirror = VaultMirrorAbstract {
            distortion_category_keys: vec!["mental_filter".into()],
            spend_pattern_band: "baseline".into(),
            late_night_tendency: false,
        };
        let turns = [TranscriptTurnRef {
            turn_id: "t-1",
            role: "candidate",
            text: "答えに詰まった",
        }];
        assert!(validate_metacognitive_debrief(&debrief, &mirror, &turns).is_ok());
    }

    #[test]
    fn gbnf_assets_mention_axes() {
        assert!(INTERVIEW_EVALUATION_V1_GBNF.contains("mece_structure"));
        assert!(INTERVIEW_EVALUATION_V1_GBNF.contains("turn_id"));
        assert!(METACOGNITIVE_DEBRIEF_V1_GBNF.contains("opted_in"));
        assert!(METACOGNITIVE_DEBRIEF_V1_GBNF.contains("distortion_isomorphism"));
    }

    #[test]
    fn parse_round_trip_layer1() {
        let raw = serde_json::to_string(&sample_eval()).unwrap();
        let parsed = InterviewEvaluationV1::from_json_str(&raw).unwrap();
        assert_eq!(parsed.schema, INTERVIEW_EVALUATION_SCHEMA);
        assert!(parsed.mece_structure.score > 0.0);
    }
}
