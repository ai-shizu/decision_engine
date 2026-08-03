//! Interaction / romance pulse (`romance_analysis.v1`) — deterministic only.
//!
//! Observes speaker-sequence affinity from canonical transcript lines.
//! Does NOT infer third-party affection. No reply-latency / sentiment.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub const ROMANCE_SCHEMA: &str = "romance_analysis.v1";
pub const MAX_INPUT_CHARS: usize = 12_000;
pub const MAX_LINES: usize = 500;

const INSUFFICIENT_TENDENCY: &str = "判定に必要な観測量が不足しています";
const INSUFFICIENT_ACTION: &str = "会話履歴を追加して再分析する";

const TENDENCIES: [&str; 4] = [
    "発話数とターン切り替えは概ね均衡しています",
    "発話数に偏りがあります",
    "ターン切り替えが少ない状態です",
    "観測範囲では中程度の往復です",
];

const ACTIONS: [&str; 3] = [
    "同じ形式で履歴を追加して再分析する",
    "往復のバランスを意識して記録を続ける",
    "ターン切り替えを意識して記録を続ける",
];

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct RomanceMetrics {
    pub total: u32,
    pub self_count: u32,
    pub contact_count: u32,
    pub switches: u32,
    pub self_to_contact: u32,
    pub self_turns_with_successor: u32,
    pub balance: f64,
    pub switch_rate: f64,
    pub reply_coverage: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct RomanceAnalysisV1 {
    pub schema: String,
    pub affinity_score: Option<u8>,
    pub interaction_tendency: String,
    pub next_best_action: String,
}

fn strip_controls(s: &str) -> String {
    s.chars()
        .filter(|c| {
            let u = *c as u32;
            !matches!(u, 0x00..=0x08 | 0x0B | 0x0C | 0x0E..=0x1F | 0x7F)
        })
        .collect()
}

pub fn sanitize_input(raw: &str) -> String {
    let mut text = raw.replace("\r\n", "\n").replace('\r', "\n");
    text = strip_controls(&text);
    if text.len() > MAX_INPUT_CHARS {
        let mut end = MAX_INPUT_CHARS;
        while end > 0 && !text.is_char_boundary(end) {
            end -= 1;
        }
        text = text.get(..end).unwrap_or("").to_string();
    }
    let lines: Vec<&str> = text.lines().take(MAX_LINES).collect();
    lines.join("\n")
}

pub fn parse_canonical_speakers(sanitized: &str) -> Vec<&'static str> {
    let mut speakers = Vec::new();
    for line in sanitized.lines() {
        let stripped = line.trim();
        if stripped.is_empty() {
            continue;
        }
        let (tag, body) = if let Some(rest) = stripped.strip_prefix("[self]") {
            ("self", rest)
        } else if let Some(rest) = stripped.strip_prefix("[contact_alias]") {
            ("contact_alias", rest)
        } else {
            continue;
        };
        let body = strip_controls(body.trim());
        if body.is_empty() {
            continue;
        }
        speakers.push(if tag == "self" {
            "self"
        } else {
            "contact_alias"
        });
    }
    speakers
}

pub fn compute_metrics(speakers: &[&str]) -> RomanceMetrics {
    let total = speakers.len() as u32;
    let self_count = speakers.iter().filter(|s| **s == "self").count() as u32;
    let contact_count = speakers
        .iter()
        .filter(|s| **s == "contact_alias")
        .count() as u32;
    let mut switches = 0u32;
    let mut self_to_contact = 0u32;
    let mut self_turns_with_successor = 0u32;
    for i in 1..speakers.len() {
        if speakers[i] != speakers[i - 1] {
            switches += 1;
        }
    }
    for i in 0..speakers.len().saturating_sub(1) {
        if speakers[i] == "self" {
            self_turns_with_successor += 1;
            if speakers[i + 1] == "contact_alias" {
                self_to_contact += 1;
            }
        }
    }
    let balance = if total == 0 {
        0.0
    } else {
        1.0 - (self_count as i32 - contact_count as i32).unsigned_abs() as f64 / f64::from(total)
    };
    let switch_rate = if total <= 1 {
        0.0
    } else {
        f64::from(switches) / f64::from(total - 1)
    };
    let reply_coverage =
        f64::from(self_to_contact) / f64::from(self_turns_with_successor.max(1));
    RomanceMetrics {
        total,
        self_count,
        contact_count,
        switches,
        self_to_contact,
        self_turns_with_successor,
        balance,
        switch_rate,
        reply_coverage,
    }
}

fn has_sufficient(m: &RomanceMetrics) -> bool {
    m.total >= 6 && m.self_count >= 2 && m.contact_count >= 2
}

/// Half-up rounding to integer (Decimal ROUND_HALF_UP semantics for non-neg).
fn round_half_up(x: f64) -> i32 {
    if x >= 0.0 {
        (x + 0.5).floor() as i32
    } else {
        (x - 0.5).ceil() as i32
    }
}

pub fn compute_affinity_score(m: &RomanceMetrics) -> Option<u8> {
    if !has_sufficient(m) {
        return None;
    }
    let raw = 100.0 * (0.40 * m.balance + 0.40 * m.switch_rate + 0.20 * m.reply_coverage);
    let score = round_half_up(raw).clamp(0, 100) as u8;
    Some(score)
}

pub fn deterministic_tendency(m: &RomanceMetrics) -> &'static str {
    if m.balance >= 0.75 && m.switch_rate >= 0.5 {
        TENDENCIES[0]
    } else if m.balance < 0.6 {
        TENDENCIES[1]
    } else if m.switch_rate < 0.4 {
        TENDENCIES[2]
    } else {
        TENDENCIES[3]
    }
}

pub fn deterministic_action(m: &RomanceMetrics) -> &'static str {
    if m.balance < 0.6 {
        ACTIONS[1]
    } else if m.switch_rate < 0.4 {
        ACTIONS[2]
    } else {
        ACTIONS[0]
    }
}

pub fn speakers_hash(speakers: &[&str]) -> String {
    let mut hasher = Sha256::new();
    for s in speakers {
        hasher.update(s.as_bytes());
        hasher.update([0]);
    }
    hex::encode(hasher.finalize())
}

/// Full deterministic romance_analysis.v1 (no LLM).
pub fn calculate_interaction_pulse(transcript: &str) -> (RomanceAnalysisV1, RomanceMetrics, String) {
    let sanitized = sanitize_input(transcript);
    let speakers = parse_canonical_speakers(&sanitized);
    let metrics = compute_metrics(&speakers);
    let hash = speakers_hash(&speakers);
    let score = compute_affinity_score(&metrics);
    let result = match score {
        None => RomanceAnalysisV1 {
            schema: ROMANCE_SCHEMA.into(),
            affinity_score: None,
            interaction_tendency: INSUFFICIENT_TENDENCY.into(),
            next_best_action: INSUFFICIENT_ACTION.into(),
        },
        Some(s) => RomanceAnalysisV1 {
            schema: ROMANCE_SCHEMA.into(),
            affinity_score: Some(s),
            interaction_tendency: deterministic_tendency(&metrics).into(),
            next_best_action: deterministic_action(&metrics).into(),
        },
    };
    (result, metrics, hash)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn balanced_dialogue_scores() {
        let raw = "\
[self] hi
[contact_alias] yo
[self] a
[contact_alias] b
[self] c
[contact_alias] d
";
        let (out, m, _) = calculate_interaction_pulse(raw);
        assert!(m.total >= 6);
        assert!(out.affinity_score.is_some());
        assert_eq!(out.schema, ROMANCE_SCHEMA);
    }

    #[test]
    fn insufficient_when_short() {
        let (out, _, _) = calculate_interaction_pulse("[self] only\n[contact_alias] one");
        assert!(out.affinity_score.is_none());
    }
}
