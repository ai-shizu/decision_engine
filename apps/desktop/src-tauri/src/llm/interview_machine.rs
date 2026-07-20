//! Multi-stage interview state machine (M17).
//!
//! Stages: Foundation → Pressure → Debrief → Closed.
//! Gap/Oracle are injected ONLY in Debrief (講評) — never in discussion
//! (AI_SKILLS §7.1 / I-22). Pure transition logic; I/O lives in commands.

use serde::{Deserialize, Serialize};

pub const INTERVIEW_MACHINE_SCHEMA: &str = "interview_machine.v1";

pub const FOUNDATION_MAX_TURNS: u32 = 3;
pub const PRESSURE_MAX_TURNS: u32 = 3;

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
        }
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
        text: quote,
        stage: stage_name,
    });
    session.total_turns = session.total_turns.saturating_add(1);
    session.turn_in_stage = session.turn_in_stage.saturating_add(1);

    match session.stage {
        InterviewStage::Foundation => {
            if session.turn_in_stage >= FOUNDATION_MAX_TURNS {
                session.stage = InterviewStage::Pressure;
                session.turn_in_stage = 0;
                Ok(AdvanceOutcome::EnteredStage(InterviewStage::Pressure))
            } else {
                Ok(AdvanceOutcome::ContinueQuestion)
            }
        }
        InterviewStage::Pressure => {
            if session.turn_in_stage >= PRESSURE_MAX_TURNS {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn foundation_to_pressure_after_max_turns() {
        let mut s = InterviewSession::new("i1".into(), "Co".into(), "{}".into());
        for _ in 0..FOUNDATION_MAX_TURNS {
            let o = apply_candidate_answer(&mut s, "答え").unwrap();
            if s.stage == InterviewStage::Pressure {
                assert_eq!(o, AdvanceOutcome::EnteredStage(InterviewStage::Pressure));
                return;
            }
        }
        panic!("should have advanced");
    }
}
