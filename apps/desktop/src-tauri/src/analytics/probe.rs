//! PROBE funnel engine — question bank, session state, next/answer.
//!
//! Ports `probe_funnel.py` core without interpersonal targeting or LLM.
//! Axis scores may be supplied by the caller (HumanSourceCode); defaults
//! yield coverage-driven priority only.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub const PROBE_STORE_SCHEMA: &str = "probe_store.v1";
pub const QUESTION_SCHEMA: &str = "probe_question.v1";
pub const STATUS_SCHEMA: &str = "probe_status.v1";
pub const ANSWER_RESULT_SCHEMA: &str = "probe_answer_result.v1";

pub const AXIS_ORDER: &[&str] = &[
    "decision_threshold",
    "reward_bias",
    "locus_of_control",
    "unlearning_rate",
    "friction_energy_ledger",
];

pub const STAGE_ORDER: &[&str] = &["FACT", "CONTEXT", "EMOTION", "MEANING"];

#[derive(Debug, Clone, Copy)]
pub struct ProbeQuestionBankEntry {
    pub id: &'static str,
    pub axis: &'static str,
    pub stage: &'static str,
    pub text: &'static str,
}

pub const PROBE_QUESTION_BANK: &[ProbeQuestionBankEntry] = &[
    ProbeQuestionBankEntry {
        id: "pq-decision_threshold-FACT-01",
        axis: "decision_threshold",
        stage: "FACT",
        text: "直近で先延ばししたタスクを一つ、事実だけで書いてください。",
    },
    ProbeQuestionBankEntry {
        id: "pq-decision_threshold-CONTEXT-01",
        axis: "decision_threshold",
        stage: "CONTEXT",
        text: "そのタスクを始める前に、何を確認しようとしていましたか。",
    },
    ProbeQuestionBankEntry {
        id: "pq-decision_threshold-EMOTION-01",
        axis: "decision_threshold",
        stage: "EMOTION",
        text: "その時点で自分が書ける感覚を、本人の言葉で書いてください。",
    },
    ProbeQuestionBankEntry {
        id: "pq-decision_threshold-MEANING-01",
        axis: "decision_threshold",
        stage: "MEANING",
        text: "いま振り返ると、その先延ばしは何を守ろうとしていましたか。",
    },
    ProbeQuestionBankEntry {
        id: "pq-reward_bias-FACT-01",
        axis: "reward_bias",
        stage: "FACT",
        text: "直近の支出または報酬に関する出来事を、事実だけで一つ書いてください。",
    },
    ProbeQuestionBankEntry {
        id: "pq-reward_bias-CONTEXT-01",
        axis: "reward_bias",
        stage: "CONTEXT",
        text: "その選択の直前に、何と比較しようとしていましたか。",
    },
    ProbeQuestionBankEntry {
        id: "pq-reward_bias-EMOTION-01",
        axis: "reward_bias",
        stage: "EMOTION",
        text: "その時点で自分が書ける感覚を、本人の言葉で書いてください。",
    },
    ProbeQuestionBankEntry {
        id: "pq-reward_bias-MEANING-01",
        axis: "reward_bias",
        stage: "MEANING",
        text: "いま振り返ると、その選択は何を優先しようとしていましたか。",
    },
    ProbeQuestionBankEntry {
        id: "pq-locus_of_control-FACT-01",
        axis: "locus_of_control",
        stage: "FACT",
        text: "直近で結果が想定とずれた出来事を、事実だけで一つ書いてください。",
    },
    ProbeQuestionBankEntry {
        id: "pq-locus_of_control-CONTEXT-01",
        axis: "locus_of_control",
        stage: "CONTEXT",
        text: "その出来事の原因を考え始める前に、何を確認していましたか。",
    },
    ProbeQuestionBankEntry {
        id: "pq-locus_of_control-EMOTION-01",
        axis: "locus_of_control",
        stage: "EMOTION",
        text: "その時点で自分が書ける感覚を、本人の言葉で書いてください。",
    },
    ProbeQuestionBankEntry {
        id: "pq-locus_of_control-MEANING-01",
        axis: "locus_of_control",
        stage: "MEANING",
        text: "いま振り返ると、その出来事をどう受け止めようとしていましたか。",
    },
    ProbeQuestionBankEntry {
        id: "pq-unlearning_rate-FACT-01",
        axis: "unlearning_rate",
        stage: "FACT",
        text: "直近で方針や前提を変えた出来事を、事実だけで一つ書いてください。",
    },
    ProbeQuestionBankEntry {
        id: "pq-unlearning_rate-CONTEXT-01",
        axis: "unlearning_rate",
        stage: "CONTEXT",
        text: "その変更を検討し始める前に、何を確認していましたか。",
    },
    ProbeQuestionBankEntry {
        id: "pq-unlearning_rate-EMOTION-01",
        axis: "unlearning_rate",
        stage: "EMOTION",
        text: "その時点で自分が書ける感覚を、本人の言葉で書いてください。",
    },
    ProbeQuestionBankEntry {
        id: "pq-unlearning_rate-MEANING-01",
        axis: "unlearning_rate",
        stage: "MEANING",
        text: "いま振り返ると、その変更は何を更新しようとしていましたか。",
    },
    ProbeQuestionBankEntry {
        id: "pq-friction_energy_ledger-FACT-01",
        axis: "friction_energy_ledger",
        stage: "FACT",
        text: "直近で摩擦や消耗を感じた出来事を、事実だけで一つ書いてください。",
    },
    ProbeQuestionBankEntry {
        id: "pq-friction_energy_ledger-CONTEXT-01",
        axis: "friction_energy_ledger",
        stage: "CONTEXT",
        text: "その出来事の前後で、何を優先しようとしていましたか。",
    },
    ProbeQuestionBankEntry {
        id: "pq-friction_energy_ledger-EMOTION-01",
        axis: "friction_energy_ledger",
        stage: "EMOTION",
        text: "その時点で自分が書ける感覚を、本人の言葉で書いてください。",
    },
    ProbeQuestionBankEntry {
        id: "pq-friction_energy_ledger-MEANING-01",
        axis: "friction_energy_ledger",
        stage: "MEANING",
        text: "いま振り返ると、その摩擦は何を守ろうとしていましたか。",
    },
];

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProbeQuestionOut {
    pub id: String,
    pub axis: String,
    pub stage: String,
    pub text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AxisScoreHint {
    pub axis: String,
    pub score: Option<f64>,
    pub confidence: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ProbeStore {
    pub schema: String,
    pub sessions: Vec<ProbeSession>,
    pub answers: Vec<ProbeAnswerRecord>,
    pub nodes: Vec<HistoricalNode>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProbeSession {
    pub id: String,
    pub date: String,
    pub stage: String,
    pub target_axis: String,
    pub questions_asked: Vec<String>,
    pub nodes_created: Vec<String>,
    pub status: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProbeAnswerRecord {
    pub session_id: String,
    pub question_id: String,
    pub stage: String,
    pub node_id: String,
    pub date: String,
    pub text_quote: String,
    pub subjective_weight: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoricalNode {
    pub id: String,
    pub date_range: String,
    pub fact_text: String,
    pub source: String,
    pub is_trusted: bool,
    pub weight: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct ProbeQuestionV1 {
    pub schema: String,
    pub session_id: String,
    pub question_id: String,
    pub axis: String,
    pub stage: String,
    pub question: String,
    pub priority: f64,
}

fn id_hex(domain: &str, payload: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(domain.as_bytes());
    hasher.update([0]);
    hasher.update(payload.as_bytes());
    let full = hex::encode(hasher.finalize());
    full.get(..12).unwrap_or(&full).to_string()
}

pub fn empty_store() -> ProbeStore {
    ProbeStore {
        schema: PROBE_STORE_SCHEMA.into(),
        sessions: Vec::new(),
        answers: Vec::new(),
        nodes: Vec::new(),
    }
}

pub fn get_probe_questions() -> Vec<ProbeQuestionOut> {
    PROBE_QUESTION_BANK
        .iter()
        .map(|q| ProbeQuestionOut {
            id: q.id.into(),
            axis: q.axis.into(),
            stage: q.stage.into(),
            text: q.text.into(),
        })
        .collect()
}

fn bank_lookup(id: &str) -> Option<&'static ProbeQuestionBankEntry> {
    PROBE_QUESTION_BANK.iter().find(|q| q.id == id)
}

fn axis_index(axis: &str) -> usize {
    AXIS_ORDER.iter().position(|a| *a == axis).unwrap_or(99)
}

fn stage_index(stage: &str) -> usize {
    STAGE_ORDER.iter().position(|s| *s == stage).unwrap_or(99)
}

fn next_stage_for_axis(store: &ProbeStore, axis: &str) -> &'static str {
    for stage in STAGE_ORDER {
        let qid = format!("pq-{axis}-{stage}-01");
        let done = store.answers.iter().any(|a| {
            a.question_id == qid
                && store
                    .nodes
                    .iter()
                    .any(|n| n.id == a.node_id && n.is_trusted)
        });
        if !done {
            return stage;
        }
    }
    "MEANING"
}

fn node_count(store: &ProbeStore, axis: &str) -> usize {
    store
        .nodes
        .iter()
        .filter(|n| n.source == axis && n.is_trusted)
        .count()
}

fn priority_for(store: &ProbeStore, axis: &str, hint: Option<&AxisScoreHint>) -> f64 {
    let confidence = hint.map(|h| h.confidence.clamp(0.0, 1.0)).unwrap_or(0.0);
    let score = hint.and_then(|h| h.score);
    let coverage = (node_count(store, axis) as f64 / 4.0).min(1.0);
    let coverage_gap = 1.0 - coverage;
    let extremity = score.map(|s| (s - 0.5).abs() * 2.0).unwrap_or(0.0);
    let raw = 0.60 * (1.0 - confidence) + 0.25 * coverage_gap + 0.15 * extremity;
    (raw.clamp(0.0, 1.0) * 1000.0).round() / 1000.0
}

fn active_session(store: &ProbeStore) -> Option<&ProbeSession> {
    for axis in AXIS_ORDER {
        if let Some(s) = store
            .sessions
            .iter()
            .find(|s| s.status == "active" && s.target_axis == *axis)
        {
            return Some(s);
        }
    }
    None
}

fn question_for(axis: &str, stage: &str) -> Option<&'static ProbeQuestionBankEntry> {
    PROBE_QUESTION_BANK
        .iter()
        .find(|q| q.axis == axis && q.stage == stage)
}

fn sanitize_answer(text: &str) -> String {
    let collapsed: String = text
        .chars()
        .map(|c| if c == '\n' || c == '\r' { ' ' } else { c })
        .collect();
    collapsed.trim().chars().take(120).collect()
}

pub fn probe_next(
    store: &mut ProbeStore,
    today: &str,
    axis_hints: &[AxisScoreHint],
) -> Result<ProbeQuestionV1, String> {
    if let Some(active) = active_session(store).cloned() {
        if active.questions_asked.len() >= 5 {
            return Err("session question cap".into());
        }
        let q = question_for(&active.target_axis, &active.stage).ok_or("question missing")?;
        let hint = axis_hints.iter().find(|h| h.axis == active.target_axis);
        let priority = priority_for(store, &active.target_axis, hint);
        return Ok(ProbeQuestionV1 {
            schema: QUESTION_SCHEMA.into(),
            session_id: active.id,
            question_id: q.id.into(),
            axis: active.target_axis,
            stage: active.stage,
            question: q.text.into(),
            priority,
        });
    }

    let mut candidates: Vec<(f64, usize, usize, String, String)> = Vec::new();
    for axis in AXIS_ORDER {
        let stage = next_stage_for_axis(store, axis);
        let hint = axis_hints.iter().find(|h| h.axis == *axis);
        let priority = priority_for(store, axis, hint);
        candidates.push((
            priority,
            axis_index(axis),
            stage_index(stage),
            (*axis).to_string(),
            stage.to_string(),
        ));
    }
    candidates.sort_by(|a, b| {
        b.0.partial_cmp(&a.0)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.1.cmp(&b.1))
            .then_with(|| a.2.cmp(&b.2))
    });
    let (_p, _, _, axis, stage) = candidates
        .into_iter()
        .next()
        .ok_or_else(|| "no candidates".to_string())?;
    let q = question_for(&axis, &stage).ok_or("question missing")?;

    let session_id = if let Some(existing) = store
        .sessions
        .iter()
        .find(|s| s.date == today && s.target_axis == axis)
        .map(|s| s.id.clone())
    {
        if let Some(s) = store.sessions.iter_mut().find(|s| s.id == existing) {
            s.status = "active".into();
            s.stage = stage.clone();
        }
        existing
    } else {
        let count = store.sessions.len();
        let id = format!(
            "ps-{}",
            id_hex(
                "decision-engine/probe-id/v2",
                &format!("{today}\0{axis}\0{stage}\0{count}")
            )
        );
        store.sessions.push(ProbeSession {
            id: id.clone(),
            date: today.into(),
            stage: stage.clone(),
            target_axis: axis.clone(),
            questions_asked: Vec::new(),
            nodes_created: Vec::new(),
            status: "active".into(),
        });
        id
    };

    let hint = axis_hints.iter().find(|h| h.axis == axis);
    let priority = priority_for(store, &axis, hint);
    Ok(ProbeQuestionV1 {
        schema: QUESTION_SCHEMA.into(),
        session_id,
        question_id: q.id.into(),
        axis,
        stage,
        question: q.text.into(),
        priority,
    })
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct ProbeAnswerResultV1 {
    pub schema: String,
    pub saved: bool,
    pub node_id: String,
    pub session_status: String,
    pub next_question: Option<ProbeQuestionV1>,
}

pub fn probe_answer(
    store: &mut ProbeStore,
    session_id: &str,
    question_id: &str,
    answer: &str,
    today: &str,
    axis_hints: &[AxisScoreHint],
) -> Result<ProbeAnswerResultV1, String> {
    let q = bank_lookup(question_id).ok_or("unknown question")?;
    let session_idx = store
        .sessions
        .iter()
        .position(|s| s.id == session_id)
        .ok_or("unknown session")?;
    {
        let session = &store.sessions[session_idx];
        if session.status != "active" {
            return Err("session not active".into());
        }
        if session.target_axis != q.axis || session.stage != q.stage {
            return Err("question mismatch".into());
        }
        if session.questions_asked.len() >= 5 {
            return Err("session question cap".into());
        }
    }

    let quote = sanitize_answer(answer);
    if quote.is_empty() {
        return Err("empty answer".into());
    }
    let node_id = format!(
        "hn-{}",
        id_hex(
            "decision-engine/historical-node/v2",
            &format!("{today}\0{quote}\0{}", q.axis)
        )
    );
    let weight = if q.stage == "EMOTION" || q.stage == "MEANING" {
        1.0
    } else {
        0.0
    };
    store.nodes.push(HistoricalNode {
        id: node_id.clone(),
        date_range: today.into(),
        fact_text: quote.clone(),
        source: q.axis.into(),
        is_trusted: true,
        weight,
    });
    store.answers.push(ProbeAnswerRecord {
        session_id: session_id.into(),
        question_id: question_id.into(),
        stage: q.stage.into(),
        node_id: node_id.clone(),
        date: today.into(),
        text_quote: quote,
        subjective_weight: weight,
    });

    let session = &mut store.sessions[session_idx];
    session.questions_asked.push(question_id.into());
    session.nodes_created.push(node_id.clone());

    let session_status = if q.stage == "MEANING" {
        session.status = "closed".into();
        "closed".to_string()
    } else {
        let next_stage = STAGE_ORDER
            .iter()
            .position(|s| *s == q.stage)
            .and_then(|i| STAGE_ORDER.get(i + 1))
            .copied()
            .unwrap_or("MEANING");
        session.stage = next_stage.into();
        "active".to_string()
    };

    let next_question = if session_status == "active" {
        Some(probe_next(store, today, axis_hints)?)
    } else {
        None
    };

    Ok(ProbeAnswerResultV1 {
        schema: ANSWER_RESULT_SCHEMA.into(),
        saved: true,
        node_id,
        session_status,
        next_question,
    })
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct ProbeStatusV1 {
    pub schema: String,
    pub today: String,
    pub progress_percent: f64,
    pub completed_stages: usize,
    pub total_stages: usize,
}

pub fn probe_status(store: &ProbeStore, today: &str) -> ProbeStatusV1 {
    let completed = store
        .answers
        .iter()
        .map(|a| a.question_id.clone())
        .collect::<std::collections::BTreeSet<_>>()
        .len();
    let total = PROBE_QUESTION_BANK.len();
    let percent = if total == 0 {
        0.0
    } else {
        ((completed as f64 / total as f64) * 1000.0).round() / 10.0
    };
    ProbeStatusV1 {
        schema: STATUS_SCHEMA.into(),
        today: today.into(),
        progress_percent: percent,
        completed_stages: completed,
        total_stages: total,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bank_has_twenty() {
        assert_eq!(PROBE_QUESTION_BANK.len(), 20);
    }

    #[test]
    fn next_then_answer_advances() {
        let mut store = empty_store();
        let q = probe_next(&mut store, "2026-07-20", &[]).unwrap();
        assert!(q.question_id.contains("FACT"));
        let res = probe_answer(
            &mut store,
            &q.session_id,
            &q.question_id,
            "先延ばししたタスクは報告書",
            "2026-07-20",
            &[],
        )
        .unwrap();
        assert!(res.saved);
        assert_eq!(res.session_status, "active");
    }
}
