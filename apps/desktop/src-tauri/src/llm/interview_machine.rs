//! Multi-stage interview state machine (M17 / Phase 14.2).
//!
//! Stages: Foundation → Pressure → Debrief → Closed.
//! Gap/Oracle are injected ONLY in Debrief (講評) — never in discussion
//! (AI_SKILLS §7.1 / I-22). Pure transition logic; I/O lives in commands.
//!
//! Phase 14.2: structural circuit breaker (short answers / withdrawal lexicon)
//! forces Pressure→Debrief. Oni intensity is tactic-driven; temperature is fixed
//! at the coliseum generation layer (not in this FSM).

use serde::{Deserialize, Serialize};

use crate::coliseum::evaluation::{
    assign_turn_ids, validate_interview_evaluation, EvaluationError, InterviewEvaluationV1,
    TranscriptTurnRef, VaultMirrorAbstract,
};
use crate::coliseum::session_artifact::InterviewSessionArtifact;
use crate::coliseum::{
    build_oni_pressure_prompt, build_oni_pressure_prompt_from_artifact, CognitiveFossilSnapshot,
    RenderGuardError,
};

pub const INTERVIEW_MACHINE_SCHEMA: &str = "interview_machine.v1";

pub const FOUNDATION_MAX_TURNS: u32 = 3;
pub const PRESSURE_MAX_TURNS: u32 = 3;

/// Candidate answers at or below this Unicode char length count as "collapsed".
pub const CIRCUIT_SHORT_ANSWER_CHARS: usize = 12;
/// Consecutive short Pressure answers that trip the breaker.
pub const CIRCUIT_SHORT_STREAK: u32 = 2;

/// Deterministic withdrawal lexicon (substring match, Unicode).
pub const WITHDRAWAL_LEXICON: &[&str] = &[
    "わからない",
    "分かりません",
    "わかりません",
    "パス",
    "もう無理",
    "無理です",
    "やめたい",
    "終了",
    "ギブアップ",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InterviewStage {
    Foundation,
    Pressure,
    Debrief,
    Closed,
}

impl InterviewStage {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Foundation => "foundation",
            Self::Pressure => "pressure",
            Self::Debrief => "debrief",
            Self::Closed => "closed",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct InterviewTurn {
    pub role: String, // "interviewer" | "candidate"
    pub text: String,
    pub stage: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct InterviewSession {
    pub schema: String,
    pub id: String,
    pub company_name: String,
    pub facts_json: String,
    pub stage: InterviewStage,
    pub turn_in_stage: u32,
    pub total_turns: u32,
    pub transcript: Vec<InterviewTurn>,
    pub status: String, // "active" | "closed"
    pub evaluation_notes: String,
    /// True only when oni was requested **and** ZPD eligibility passed (structural gate).
    #[serde(default)]
    pub oni_active: bool,
    /// Start-of-session immutable freeze (Phase 14.4). Evaluation binds here only.
    #[serde(default)]
    pub session_artifact: Option<InterviewSessionArtifact>,
}

impl InterviewSession {
    pub fn new(id: String, company_name: String, facts_json: String) -> Self {
        Self {
            schema: INTERVIEW_MACHINE_SCHEMA.into(),
            id,
            company_name,
            facts_json,
            stage: InterviewStage::Foundation,
            turn_in_stage: 0,
            total_turns: 0,
            transcript: Vec::new(),
            status: "active".into(),
            evaluation_notes: String::new(),
            oni_active: false,
            session_artifact: None,
        }
    }

    pub fn attach_artifact(&mut self, artifact: InterviewSessionArtifact) {
        self.session_artifact = Some(artifact);
    }
}

/// Stage-specific interviewer directive (discussion: no vault/gap; debrief: mentor OK).
pub fn stage_directive(stage: InterviewStage) -> &'static str {
    match stage {
        InterviewStage::Foundation => {
            "【Stage 1: 基礎突撃】経歴・動機・基本事実の確認。一度に1問。曖昧な回答には具体例を要求せよ。\
Vault / Gap / Tensor は参照禁止。"
        }
        InterviewStage::Pressure => {
            "【Stage 2: 圧迫・深掘り】矛盾・定量欠落・再現性を突け。助け舟を出さない。一度に1問。\
Vault / Gap / Tensor は参照禁止。本セッションの発話と企業ファクトのみ。"
        }
        InterviewStage::Debrief => {
            "【Stage 3: 最終講評・感想戦】これ以上の新規面接質問はせず、論理強度・定量根拠・企業適合を講評せよ。\
下記の Vault 参考情報・Gap・Tensor・Oracle がある場合は同意で終わらせず、\
「本当にそうか？」と過去記録との矛盾を突け。データ不足なら推測で埋めるな。"
        }
        InterviewStage::Closed => "セッションは終了している。新たな質問を生成するな。",
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AdvanceOutcome {
    /// Stay in stage; generate next interviewer question.
    ContinueQuestion,
    /// Moved to next stage; generate opening for new stage.
    EnteredStage(InterviewStage),
    /// Pressure aborted early by structural circuit breaker → Debrief.
    CircuitBreakToDebrief,
    /// Session closed after debrief.
    Closed,
}

/// Record candidate answer and decide next machine action.
pub fn apply_candidate_answer(session: &mut InterviewSession, answer: &str) -> Result<AdvanceOutcome, String> {
    if session.status != "active" || session.stage == InterviewStage::Closed {
        return Err("session_closed".into());
    }
    let quote: String = answer.trim().chars().take(4000).collect();
    if quote.is_empty() {
        return Err("empty_answer".into());
    }

    let stage_name = session.stage.as_str().to_string();
    session.transcript.push(InterviewTurn {
        role: "candidate".into(),
        text: quote.clone(),
        stage: stage_name,
    });
    session.total_turns = session.total_turns.saturating_add(1);
    session.turn_in_stage = session.turn_in_stage.saturating_add(1);

    match session.stage {
        InterviewStage::Foundation => {
            if session.turn_in_stage >= FOUNDATION_MAX_TURNS {
                // Structural ZPD downgrade: skip Pressure when oni is inactive.
                if session.oni_active {
                    session.stage = InterviewStage::Pressure;
                    session.turn_in_stage = 0;
                    Ok(AdvanceOutcome::EnteredStage(InterviewStage::Pressure))
                } else {
                    session.stage = InterviewStage::Debrief;
                    session.turn_in_stage = 0;
                    session.evaluation_notes = "zpd_downgrade_skip_pressure".into();
                    Ok(AdvanceOutcome::EnteredStage(InterviewStage::Debrief))
                }
            } else {
                Ok(AdvanceOutcome::ContinueQuestion)
            }
        }
        InterviewStage::Pressure => {
            if pressure_circuit_breaker_tripped(session, &quote) {
                session.stage = InterviewStage::Debrief;
                session.turn_in_stage = 0;
                session.evaluation_notes = "circuit_break_pressure".into();
                Ok(AdvanceOutcome::CircuitBreakToDebrief)
            } else if session.turn_in_stage >= PRESSURE_MAX_TURNS {
                session.stage = InterviewStage::Debrief;
                session.turn_in_stage = 0;
                Ok(AdvanceOutcome::EnteredStage(InterviewStage::Debrief))
            } else {
                Ok(AdvanceOutcome::ContinueQuestion)
            }
        }
        InterviewStage::Debrief => {
            session.stage = InterviewStage::Closed;
            session.status = "closed".into();
            Ok(AdvanceOutcome::Closed)
        }
        InterviewStage::Closed => Err("session_closed".into()),
    }
}

/// Deterministic Pressure safety valve (lexicon ∪ short-answer streak).
pub fn pressure_circuit_breaker_tripped(session: &InterviewSession, latest_answer: &str) -> bool {
    if contains_withdrawal_lexicon(latest_answer) {
        return true;
    }
    short_answer_streak(session, latest_answer) >= CIRCUIT_SHORT_STREAK
}

fn contains_withdrawal_lexicon(answer: &str) -> bool {
    let trimmed = answer.trim();
    if trimmed.is_empty() {
        return false;
    }
    for term in WITHDRAWAL_LEXICON {
        if trimmed.contains(term) {
            return true;
        }
    }
    false
}

fn is_short_answer(text: &str) -> bool {
    text.trim().chars().count() <= CIRCUIT_SHORT_ANSWER_CHARS
}

fn short_answer_streak(session: &InterviewSession, latest_answer: &str) -> u32 {
    if !is_short_answer(latest_answer) {
        return 0;
    }
    let mut streak = 1u32;
    // Walk prior candidate turns in Pressure (excluding the just-pushed latest).
    let prior = session.transcript.len().saturating_sub(1);
    for turn in session.transcript[..prior].iter().rev() {
        if turn.role != "candidate" || turn.stage != "pressure" {
            break;
        }
        if is_short_answer(&turn.text) {
            streak = streak.saturating_add(1);
        } else {
            break;
        }
    }
    streak
}

pub fn record_interviewer_utterance(session: &mut InterviewSession, text: &str) {
    let quote: String = text.trim().chars().take(4000).collect();
    if quote.is_empty() {
        return;
    }
    let stage_name = session.stage.as_str().to_string();
    session.transcript.push(InterviewTurn {
        role: "interviewer".into(),
        text: quote,
        stage: stage_name,
    });
}

/// Build discussion/debrief prompt layers (caller adds company + optional vault mentor).
pub fn build_stage_prompt_prefix(session: &InterviewSession, include_mentor_debrief: bool) -> String {
    let mut out = String::new();
    out.push_str(
        "あなたは外資系 / テック企業の厳格な面接官である。人格攻撃は禁止。\n",
    );
    out.push_str(stage_directive(session.stage));
    out.push('\n');
    out.push_str(&format!(
        "company={} stage={} turn_in_stage={} total_turns={}\n",
        session.company_name,
        session.stage.as_str(),
        session.turn_in_stage,
        session.total_turns
    ));
    if !session.transcript.is_empty() {
        out.push_str("\n## これまでの会話\n");
        let start = session.transcript.len().saturating_sub(12);
        for turn in session.transcript.iter().skip(start) {
            out.push_str(&format!("{}: {}\n", turn.role, turn.text));
        }
    }
    if include_mentor_debrief && session.stage == InterviewStage::Debrief {
        out.push_str(
            "\n（講評フェーズ: 呼び出し側が Vault 参考情報と Gap/Tensor/Oracle を続けて注入する）\n",
        );
    }
    out
}


/// Oni / Pressure: irreversible compile + leak gate. No `VaultHandle` in signature (I-22).
pub fn oni_pressure_prompt_from_fossils(
    snapshot: CognitiveFossilSnapshot,
    public_brief: &str,
) -> Result<String, RenderGuardError> {
    build_oni_pressure_prompt(snapshot, public_brief)
}

/// Oni / Pressure from frozen artifact directives (Phase 14.4 replay path).
pub fn oni_pressure_prompt_from_artifact(
    artifact: &InterviewSessionArtifact,
    public_brief: &str,
) -> Result<String, RenderGuardError> {
    build_oni_pressure_prompt_from_artifact(artifact, public_brief)
}

/// Require a verified start-of-session artifact — evaluation must not touch live vault.
pub fn require_session_artifact(
    session: &InterviewSession,
) -> Result<&InterviewSessionArtifact, String> {
    let art = session
        .session_artifact
        .as_ref()
        .ok_or_else(|| "session_artifact_missing".to_string())?;
    if art.session_id != session.id {
        return Err("session_artifact_id_mismatch".into());
    }
    if !art.verify_fingerprint() {
        return Err("session_artifact_fingerprint_mismatch".into());
    }
    Ok(art)
}

/// Build Layer-1 transcript binding exclusively from the session + frozen artifact.
///
/// Deliberately omits any vault handle / live fossil type from the signature.
pub fn prepare_interview_evaluation_binding(
    session: &InterviewSession,
) -> Result<(InterviewSessionArtifact, Vec<(String, String, String)>), String> {
    let art = require_session_artifact(session)?.clone();
    let pairs: Vec<(&str, &str)> = session
        .transcript
        .iter()
        .map(|t| (t.role.as_str(), t.text.as_str()))
        .collect();
    let turns = assign_turn_ids(&pairs);
    Ok((art, turns))
}

/// Validate Layer-1 scorecard against the session transcript; artifact must be present.
pub fn seal_evaluation_against_artifact(
    session: &InterviewSession,
    eval: &InterviewEvaluationV1,
) -> Result<(), String> {
    let _artifact = require_session_artifact(session)?;
    let pairs: Vec<(&str, &str)> = session
        .transcript
        .iter()
        .map(|t| (t.role.as_str(), t.text.as_str()))
        .collect();
    let owned = assign_turn_ids(&pairs);
    let refs: Vec<TranscriptTurnRef<'_>> = owned
        .iter()
        .map(|(id, role, text)| TranscriptTurnRef {
            turn_id: id.as_str(),
            role: role.as_str(),
            text: text.as_str(),
        })
        .collect();
    validate_interview_evaluation(eval, &refs).map_err(|e: EvaluationError| e.to_string())
}

/// Derive opt-in Layer-2 mirror **from frozen evidence bodies** (not live vault).
pub fn vault_mirror_from_artifact(artifact: &InterviewSessionArtifact) -> VaultMirrorAbstract {
    let mut keys: Vec<String> = artifact
        .evidence
        .distortions
        .iter()
        .map(|d| d.category.clone())
        .collect();
    keys.sort();
    keys.dedup();
    let late_night_tendency = artifact.evidence.purchases.iter().any(|p| p.late_night);
    let spend_pattern_band = artifact
        .evidence
        .purchases
        .iter()
        .map(|p| p.amount_band.as_str())
        .max_by_key(|b| band_rank(b))
        .unwrap_or("unknown")
        .to_string();
    VaultMirrorAbstract {
        distortion_category_keys: keys,
        spend_pattern_band,
        late_night_tendency,
    }
}

fn band_rank(b: &str) -> u8 {
    match b {
        "none" => 0,
        "micro" => 1,
        "low" => 2,
        "mid" => 3,
        "high" => 4,
        "extreme" => 5,
        _ => 0,
    }
}

/// Evidence block for debrief prompts — frozen text only (Vacuum-safe).
pub fn artifact_evidence_prompt_block(artifact: &InterviewSessionArtifact) -> String {
    let mut out = String::from(
        "## 開始時凍結エビデンス（Vault 生データではなくアーティファクト実体）\n",
    );
    out.push_str(&format!(
        "fingerprint={} r_t={} model_hash={}\n",
        artifact.fingerprint, artifact.r_t_snapshot, artifact.model_hash
    ));
    if artifact.evidence.distortions.is_empty() && artifact.evidence.purchases.is_empty() {
        out.push_str("（凍結エビデンスなし）\n");
        return out;
    }
    for d in &artifact.evidence.distortions {
        out.push_str(&format!(
            "- distortion category={} summary={}\n",
            d.category, d.text_summary
        ));
    }
    for p in &artifact.evidence.purchases {
        out.push_str(&format!("- purchase {}\n", p.text_summary));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::coliseum::session_artifact::{
        DistortionEvidenceSnap, EvidenceSnapshot, PurchaseEvidenceSnap,
    };

    fn attach_sample_artifact(s: &mut InterviewSession) {
        let art = InterviewSessionArtifact::freeze(
            s.id.clone(),
            "pocket-brain.gguf",
            0.8,
            CognitiveFossilSnapshot {
                r_at_decision: Some(0.8),
                distortion_categories: vec!["overgeneralization".into()],
                purchase_amounts: vec![],
                late_night_purchase_count: 0,
                impulse_purchase_count: 0,
                blacklist_seed_terms: vec![],
            },
            EvidenceSnapshot {
                distortions: vec![DistortionEvidenceSnap {
                    source_id: "d1".into(),
                    category: "overgeneralization".into(),
                    text_summary: "いつも".into(),
                    confidence_score: 0.7,
                }],
                purchases: vec![PurchaseEvidenceSnap {
                    source_id: "p1".into(),
                    text_summary: "merchant=x band=low when=day".into(),
                    amount_band: "low".into(),
                    late_night: false,
                }],
                frozen_at_unix: 1,
            },
        );
        s.attach_artifact(art);
    }

    #[test]
    fn foundation_to_pressure_after_max_turns() {
        let mut s = InterviewSession::new("i1".into(), "Co".into(), "{}".into());
        s.oni_active = true;
        for _ in 0..FOUNDATION_MAX_TURNS {
            let o = apply_candidate_answer(&mut s, "答え").unwrap();
            if s.stage == InterviewStage::Pressure {
                assert_eq!(o, AdvanceOutcome::EnteredStage(InterviewStage::Pressure));
                return;
            }
        }
        panic!("should have advanced");
    }

    #[test]
    fn oni_pressure_compiles_without_vault_handle() {
        let snap = CognitiveFossilSnapshot {
            r_at_decision: Some(0.2),
            distortion_categories: vec!["overgeneralization".into()],
            purchase_amounts: vec![12_000],
            late_night_purchase_count: 1,
            impulse_purchase_count: 0,
            blacklist_seed_terms: vec!["秘密商事".into()],
        };
        let text = oni_pressure_prompt_from_fossils(snap, "SWE").expect("sealed");
        assert!(text.contains("戦術"));
        assert!(!text.contains("秘密商事"));
        assert!(!text.contains("12000"));
    }

    #[test]
    fn evaluation_requires_artifact_not_live_vault() {
        let mut s = InterviewSession::new("i1".into(), "Co".into(), "{}".into());
        assert!(require_session_artifact(&s).is_err());
        attach_sample_artifact(&mut s);
        let art = require_session_artifact(&s).unwrap();
        assert!(art.verify_fingerprint());
        let mirror = vault_mirror_from_artifact(art);
        assert!(mirror
            .distortion_category_keys
            .iter()
            .any(|k| k == "overgeneralization"));
    }

    #[test]
    fn artifact_replay_pressure_prompt() {
        let mut s = InterviewSession::new("i1".into(), "Co".into(), "{}".into());
        attach_sample_artifact(&mut s);
        let art = s.session_artifact.as_ref().unwrap();
        let text = oni_pressure_prompt_from_artifact(art, "").unwrap();
        assert!(text.contains("戦術"));
    }

    #[test]
    fn circuit_break_on_withdrawal_lexicon() {
        let mut s = InterviewSession::new("i1".into(), "Co".into(), "{}".into());
        s.oni_active = true;
        s.stage = InterviewStage::Pressure;
        s.turn_in_stage = 0;
        let o = apply_candidate_answer(&mut s, "もう無理です").unwrap();
        assert_eq!(o, AdvanceOutcome::CircuitBreakToDebrief);
        assert_eq!(s.stage, InterviewStage::Debrief);
    }

    #[test]
    fn circuit_break_on_short_streak() {
        let mut s = InterviewSession::new("i1".into(), "Co".into(), "{}".into());
        s.oni_active = true;
        s.stage = InterviewStage::Pressure;
        s.turn_in_stage = 0;
        let o1 = apply_candidate_answer(&mut s, "はい").unwrap();
        assert_eq!(o1, AdvanceOutcome::ContinueQuestion);
        let o2 = apply_candidate_answer(&mut s, "ええ").unwrap();
        assert_eq!(o2, AdvanceOutcome::CircuitBreakToDebrief);
        assert_eq!(s.stage, InterviewStage::Debrief);
    }

    #[test]
    fn zpd_downgrade_skips_pressure() {
        let mut s = InterviewSession::new("i1".into(), "Co".into(), "{}".into());
        s.oni_active = false;
        for _ in 0..FOUNDATION_MAX_TURNS {
            let o = apply_candidate_answer(&mut s, "十分な長さのある回答です").unwrap();
            if s.stage == InterviewStage::Debrief {
                assert_eq!(o, AdvanceOutcome::EnteredStage(InterviewStage::Debrief));
                assert_eq!(s.evaluation_notes, "zpd_downgrade_skip_pressure");
                return;
            }
        }
        panic!("should skip pressure");
    }
}
