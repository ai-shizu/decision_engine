//! Phase 13 — If-Then commitment proposals from FDR-robust spend↔cognition links.
//!
//! Precommitment / Ulysses-contract style soft nudges (not hard blocks).
//! Deterministic copy + condition JSON; no RNG / egress.

use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::analytics::bias_profile::BURNS_CATEGORIES;
use crate::analytics::spend_cognition::{
    robust_relations, SpendCognitionRelation, SpendCognitionReport, LOW_R_THRESHOLD,
};
use crate::db::CommitmentRow;

const CONDITION_SCHEMA: &str = "commitment_condition.v1";

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct CommitmentProposal {
    pub source_relation_id: String,
    pub treatment: String,
    pub outcome: String,
    pub contrast: f64,
    pub p_raw: f64,
    pub action_type: String,
    pub custom_prompt: String,
    pub delay_seconds: i64,
    pub condition_json: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct CommitmentView {
    pub id: String,
    pub created_at: i64,
    pub condition_json: String,
    pub action_type: String,
    pub custom_prompt: String,
    pub delay_seconds: i64,
    pub source_relation_id: String,
    pub enabled: bool,
    pub origin: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct CognitiveCommitmentsResult {
    pub analysis: SpendCognitionReport,
    pub proposals: Vec<CommitmentProposal>,
    pub commitments: Vec<CommitmentView>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct CommitmentFire {
    pub commitment_id: String,
    pub action_type: String,
    pub custom_prompt: String,
    pub delay_seconds: i64,
    pub source_relation_id: String,
}

#[derive(Debug, Clone, Deserialize)]
struct ConditionDoc {
    schema: String,
    #[serde(default)]
    all_of: Vec<ConditionAtom>,
}

#[derive(Debug, Clone, Deserialize)]
struct ConditionAtom {
    #[serde(rename = "type")]
    kind: String,
    #[serde(default)]
    category: Option<String>,
    #[serde(default)]
    max: Option<f64>,
    #[serde(default)]
    equals: Option<bool>,
}

/// Map FDR survivors → human If-Then proposals (positive contrast only).
pub fn proposals_from_report(report: &SpendCognitionReport) -> Vec<CommitmentProposal> {
    let mut out = Vec::new();
    for rel in robust_relations(report) {
        if let Some(p) = proposal_from_relation(rel) {
            out.push(p);
        }
    }
    out.sort_by(|a, b| a.source_relation_id.cmp(&b.source_relation_id));
    out
}

fn proposal_from_relation(rel: &SpendCognitionRelation) -> Option<CommitmentProposal> {
    let (action_type, delay_seconds, prompt) = action_for(rel)?;
    let condition_json = condition_json_for(rel)?;
    Some(CommitmentProposal {
        source_relation_id: rel.id.clone(),
        treatment: rel.treatment.clone(),
        outcome: rel.outcome.clone(),
        contrast: rel.contrast,
        p_raw: rel.p_raw,
        action_type: action_type.into(),
        custom_prompt: prompt,
        delay_seconds,
        condition_json,
    })
}

fn treatment_label(treatment: &str) -> String {
    match treatment {
        "low_r" => "認知資源が低い".into(),
        "all_or_nothing" => "全か無かの思考".into(),
        "overgeneralization" => "過度の一般化".into(),
        "mental_filter" => "心のフィルター".into(),
        "disqualifying_the_positive" => "マイナス化思考".into(),
        "jumping_to_conclusions" => "結論の飛躍".into(),
        "magnification_minimization" => "破局視（拡大／過小）".into(),
        "emotional_reasoning" => "感情的決めつけ".into(),
        "should_statements" => "すべき思考".into(),
        "labeling" => "レッテル貼り".into(),
        "personalization" => "自己関連づけ".into(),
        other => other.to_string(),
    }
}

fn action_for(rel: &SpendCognitionRelation) -> Option<(&'static str, i64, String)> {
    let tlab = treatment_label(&rel.treatment);
    match rel.outcome.as_str() {
        "late_night" => Some((
            "breath_pause",
            120,
            format!("{tlab}が出た日の深夜は、購入前に一呼吸置く"),
        )),
        "impulse_spend" => Some((
            "delay_minutes",
            300,
            format!("{tlab}の傾向があるときは、未検証・衝動寄りの購入前に5分待つ"),
        )),
        "ln_amount" => Some((
            "custom_prompt",
            180,
            format!("{tlab}のとき、いつもの支出水準を超えていないか金額を声に出して確認する"),
        )),
        _ => None,
    }
}

fn condition_json_for(rel: &SpendCognitionRelation) -> Option<String> {
    let mut all_of = Vec::new();
    match rel.treatment.as_str() {
        "low_r" => {
            all_of.push(json!({
                "type": "low_r",
                "max": LOW_R_THRESHOLD,
            }));
        }
        other if BURNS_CATEGORIES.contains(&other) => {
            all_of.push(json!({
                "type": "distortion",
                "category": other,
            }));
        }
        _ => return None,
    }
    match rel.outcome.as_str() {
        "late_night" => {
            all_of.push(json!({ "type": "late_night", "equals": true }));
        }
        "impulse_spend" => {
            // Fires when cognitive condition holds at purchase time;
            // impulse is the historical outcome, not a runtime gate.
        }
        "ln_amount" => {}
        _ => return None,
    }
    let doc = json!({
        "schema": CONDITION_SCHEMA,
        "all_of": all_of,
    });
    Some(doc.to_string())
}

pub fn commitment_row_from_proposal(
    proposal: &CommitmentProposal,
    now_unix: i64,
    existing_id: Option<String>,
) -> CommitmentRow {
    let id = existing_id.unwrap_or_else(|| format!("cmt-{}", proposal.source_relation_id));
    CommitmentRow {
        id,
        created_at: now_unix,
        condition_json: proposal.condition_json.clone(),
        action_type: proposal.action_type.clone(),
        custom_prompt: proposal.custom_prompt.clone(),
        delay_seconds: proposal.delay_seconds,
        source_relation_id: proposal.source_relation_id.clone(),
        enabled: 1,
        origin: "suggested".into(),
    }
}

pub fn to_commitment_view(row: &CommitmentRow) -> CommitmentView {
    CommitmentView {
        id: row.id.clone(),
        created_at: row.created_at,
        condition_json: row.condition_json.clone(),
        action_type: row.action_type.clone(),
        custom_prompt: row.custom_prompt.clone(),
        delay_seconds: row.delay_seconds,
        source_relation_id: row.source_relation_id.clone(),
        enabled: row.enabled != 0,
        origin: row.origin.clone(),
    }
}

/// Soft-match enabled commitments against the cognitive state at decision time.
pub fn evaluate_commitment_fires(
    commitments: &[CommitmentRow],
    r_at_decision: f64,
    distortions_json: &str,
    occurred_at: i64,
) -> Vec<CommitmentFire> {
    let distortions = parse_distortion_set(distortions_json);
    let late = is_jst_late_night(occurred_at);
    let mut fires = Vec::new();
    for row in commitments {
        if row.enabled == 0 {
            continue;
        }
        if condition_matches(&row.condition_json, r_at_decision, &distortions, late) {
            fires.push(CommitmentFire {
                commitment_id: row.id.clone(),
                action_type: row.action_type.clone(),
                custom_prompt: row.custom_prompt.clone(),
                delay_seconds: row.delay_seconds,
                source_relation_id: row.source_relation_id.clone(),
            });
        }
    }
    fires.sort_by(|a, b| a.commitment_id.cmp(&b.commitment_id));
    fires
}

fn parse_distortion_set(raw: &str) -> std::collections::BTreeSet<String> {
    match serde_json::from_str::<Vec<String>>(raw.trim()) {
        Ok(items) => items
            .into_iter()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect(),
        Err(_) => std::collections::BTreeSet::new(),
    }
}

fn is_jst_late_night(unix: i64) -> bool {
    let jst = unix.saturating_add(9 * 3_600);
    let hour = (jst.rem_euclid(86_400) / 3_600) as u8;
    hour >= 22 || hour < 5
}

fn condition_matches(
    condition_json: &str,
    r: f64,
    distortions: &std::collections::BTreeSet<String>,
    late_night: bool,
) -> bool {
    let doc: ConditionDoc = match serde_json::from_str(condition_json) {
        Ok(d) => d,
        Err(_) => return false,
    };
    if doc.schema != CONDITION_SCHEMA {
        return false;
    }
    if doc.all_of.is_empty() {
        return false;
    }
    for atom in &doc.all_of {
        match atom.kind.as_str() {
            "low_r" => {
                let max = atom.max.unwrap_or(LOW_R_THRESHOLD);
                if !(r.is_finite() && r <= max) {
                    return false;
                }
            }
            "distortion" => {
                let Some(cat) = atom.category.as_deref() else {
                    return false;
                };
                if !distortions.contains(cat) {
                    return false;
                }
            }
            "late_night" => {
                let expect = atom.equals.unwrap_or(true);
                if late_night != expect {
                    return false;
                }
            }
            _ => return false,
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::analytics::spend_cognition::SpendCognitionRelation;

    #[test]
    fn late_night_low_r_proposal_copy() {
        let rel = SpendCognitionRelation {
            id: "low_r__late_night".into(),
            treatment: "low_r".into(),
            outcome: "late_night".into(),
            contrast: 0.4,
            n_treat: 10,
            n_control: 10,
            p_raw: 0.01,
            rejected_bh: true,
            bh_rank: 1,
            bh_critical: 0.05,
        };
        let p = proposal_from_relation(&rel).expect("proposal");
        assert_eq!(p.action_type, "breath_pause");
        assert!(p.custom_prompt.contains("深夜"));
        assert!(p.condition_json.contains("low_r"));
        assert!(p.condition_json.contains("late_night"));
    }

    #[test]
    fn evaluate_fires_on_matching_state() {
        let row = CommitmentRow {
            id: "c1".into(),
            created_at: 1,
            condition_json: json!({
                "schema": CONDITION_SCHEMA,
                "all_of": [
                    {"type": "low_r", "max": 0.34},
                    {"type": "late_night", "equals": true}
                ]
            })
            .to_string(),
            action_type: "breath_pause".into(),
            custom_prompt: "pause".into(),
            delay_seconds: 120,
            source_relation_id: "low_r__late_night".into(),
            enabled: 1,
            origin: "suggested".into(),
        };
        // day_key 20000 @ 23:00 JST
        let jst_late = 20_000_i64 * 86_400 - 9 * 3_600 + 23 * 3_600;
        let fires = evaluate_commitment_fires(&[row], 0.2, "[]", jst_late);
        assert_eq!(fires.len(), 1);
        assert_eq!(fires[0].action_type, "breath_pause");
    }

    #[test]
    fn no_fire_when_r_high() {
        let row = CommitmentRow {
            id: "c1".into(),
            created_at: 1,
            condition_json: json!({
                "schema": CONDITION_SCHEMA,
                "all_of": [{"type": "low_r", "max": 0.34}]
            })
            .to_string(),
            action_type: "breath_pause".into(),
            custom_prompt: "pause".into(),
            delay_seconds: 120,
            source_relation_id: "x".into(),
            enabled: 1,
            origin: "suggested".into(),
        };
        let fires = evaluate_commitment_fires(&[row], 0.9, "[]", 1_720_000_000);
        assert!(fires.is_empty());
    }
}
