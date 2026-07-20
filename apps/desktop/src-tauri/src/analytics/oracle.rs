//! Hyper-Personalized Oracle — sterile payload assembly (Echo E4).
//!
//! Couples twin + coupling (+ vault provenance outside the sterile body).
//! Interventions are selected from INTERVENTION_BANK only (I-19 / no LLM rewrite).

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::analytics::coupling::{CouplingMatrix, FEATURE_LANES};
use crate::analytics::digital_twin::{TwinParams, TwinScenarioResult};

pub const ORACLE_SCHEMA: &str = "oracle_payload.v1";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct InterventionBankEntry {
    pub bank_id: &'static str,
    pub label: &'static str,
    pub target_lane: usize,
    pub template: &'static str,
}

/// I-19: target_lane ∈ FEATURE_LANES indices only.
pub const INTERVENTION_BANK: &[InterventionBankEntry] = &[
    InterventionBankEntry {
        bank_id: "iv-001",
        label: "送信クールダウン",
        target_lane: 11, // line_out_msgs
        template: "R < θ_R の間、当該 dyad への非緊急送信を保留し R 回復後に見直す",
    },
    InterventionBankEntry {
        bank_id: "iv-002",
        label: "支出クールオフ",
        target_lane: 6, // spend_hedonic
        template: "失策ハザード高位日の裁量支出に 24h の遅延を課す",
    },
    InterventionBankEntry {
        bank_id: "iv-003",
        label: "意思決定モラトリアム",
        target_lane: 18, // task_declared
        template: "R < θ_R の日に不可逆コミットメント (応募/購入/約束) をしない",
    },
    InterventionBankEntry {
        bank_id: "iv-004",
        label: "回復ブロック予約",
        target_lane: 9, // cal_private_hours
        template: "結合行列が示す回復→生産性ラグに合わせ私的時間を予定に置く",
    },
    InterventionBankEntry {
        bank_id: "iv-005",
        label: "面接前テーパリング",
        target_lane: 10, // cal_switch_count
        template: "面接前 48h のコンテキストスイッチ数を上限管理する",
    },
    InterventionBankEntry {
        bank_id: "iv-006",
        label: "深夜送信ゲート",
        target_lane: 16, // line_night_out
        template: "23:00-08:00 の下書きを朝の R 回復後レビューまで保留する",
    },
];

fn bank(id: &str) -> &'static InterventionBankEntry {
    INTERVENTION_BANK
        .iter()
        .find(|e| e.bank_id == id)
        .expect("INTERVENTION_BANK id missing")
}

fn lane_name(idx: usize) -> &'static str {
    FEATURE_LANES.get(idx).copied().unwrap_or("diary_chars")
}

fn dominant_load_axis(params: &TwinParams) -> &'static str {
    let cands = [
        ("beta1", params.beta1),
        ("beta2", params.beta2),
        ("gamma", params.gamma),
    ];
    cands
        .into_iter()
        .max_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal))
        .map(|(n, _)| n)
        .unwrap_or("beta1")
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct OracleFinding {
    pub rule_id: String,
    pub severity: f64,
    pub metrics: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct OracleIntervention {
    pub bank_id: String,
    pub trigger_rule: String,
    pub target_lane: usize,
    pub params: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct OracleProvenance {
    pub gap_run_id: Option<String>,
    pub tensor_run_id: Option<String>,
    pub pulse_run_id: Option<String>,
    pub rasch_run_id: Option<String>,
    pub twin_run_id: Option<String>,
    pub rag_hit_count: Option<u32>,
    pub interview_session_id: Option<String>,
}

fn evaluate_rules(
    params: &TwinParams,
    coup: &CouplingMatrix,
    forecast_r0: Option<f64>,
) -> Vec<OracleFinding> {
    let mut findings = Vec::new();
    if params.gate_passed {
        if let (Some(theta_r), Some(r0)) = (params.theta_r, forecast_r0) {
            if r0 < theta_r {
                let severity = ((theta_r - r0) / theta_r.max(1e-6)).clamp(0.0, 1.0);
                findings.push(OracleFinding {
                    rule_id: "R-GATE-01".into(),
                    severity: round4(severity),
                    metrics: json!({"r_now": round4(r0), "theta_r": round4(theta_r)}),
                });
            }
        }
        let total = params.beta1 + params.beta2 + params.gamma;
        if total > 1e-9 {
            match dominant_load_axis(params) {
                "beta1" => findings.push(OracleFinding {
                    rule_id: "R-SWITCH-01".into(),
                    severity: round4(params.beta1 / total),
                    metrics: json!({"beta1": round4(params.beta1), "total": round4(total)}),
                }),
                "beta2" => findings.push(OracleFinding {
                    rule_id: "R-VOL-01".into(),
                    severity: round4(params.beta2 / total),
                    metrics: json!({"beta2": round4(params.beta2), "total": round4(total)}),
                }),
                _ => {}
            }
        }
    }

    for pair in &coup.pairs {
        if !pair.sig {
            continue;
        }
        let src = lane_name(pair.src);
        let dst = lane_name(pair.dst);
        if src == "line_night_out" || dst == "line_night_out" {
            findings.push(OracleFinding {
                rule_id: "R-NIGHT-01".into(),
                severity: round4(pair.rho.abs()),
                metrics: json!({"lag_days": pair.lag, "rho": round4(pair.rho)}),
            });
        }
        if pair.rho > 0.0 && (src == "cal_private_hours" || dst == "cal_private_hours") {
            findings.push(OracleFinding {
                rule_id: "R-RECOVERY-01".into(),
                severity: round4(pair.rho.abs()),
                metrics: json!({"lag_days": pair.lag, "rho": round4(pair.rho)}),
            });
        }
        if src == "spend_hedonic" || dst == "spend_hedonic" {
            findings.push(OracleFinding {
                rule_id: "R-SPEND-01".into(),
                severity: round4(pair.rho.abs()),
                metrics: json!({"lag_days": pair.lag, "rho": round4(pair.rho)}),
            });
        }
    }

    // Dedup by rule_id keeping max severity
    let mut best: Vec<OracleFinding> = Vec::new();
    for f in findings {
        if let Some(existing) = best.iter_mut().find(|e| e.rule_id == f.rule_id) {
            if f.severity > existing.severity {
                *existing = f;
            }
        } else {
            best.push(f);
        }
    }
    best.sort_by(|a, b| a.rule_id.cmp(&b.rule_id));
    best
}

fn select_interventions(findings: &[OracleFinding]) -> Vec<OracleIntervention> {
    let map = [
        ("R-GATE-01", "iv-003"),
        ("R-SWITCH-01", "iv-005"),
        ("R-VOL-01", "iv-001"),
        ("R-NIGHT-01", "iv-006"),
        ("R-RECOVERY-01", "iv-004"),
        ("R-SPEND-01", "iv-002"),
    ];
    let mut out = Vec::new();
    let mut seen = BTreeSet::new();
    for f in findings {
        if let Some((_, bank_id)) = map.iter().find(|(r, _)| *r == f.rule_id) {
            if seen.insert(*bank_id) {
                let e = bank(bank_id);
                out.push(OracleIntervention {
                    bank_id: (*bank_id).into(),
                    trigger_rule: f.rule_id.clone(),
                    target_lane: e.target_lane,
                    params: json!({}),
                });
            }
        }
    }
    out
}

fn round4(x: f64) -> f64 {
    (x * 10_000.0).round() / 10_000.0
}

fn is_iso_date(s: &str) -> bool {
    if s.len() != 10 {
        return false;
    }
    let b = s.as_bytes();
    b[4] == b'-'
        && b[7] == b'-'
        && b[0..4].iter().all(u8::is_ascii_digit)
        && b[5..7].iter().all(u8::is_ascii_digit)
        && b[8..10].iter().all(u8::is_ascii_digit)
}

fn is_sterile_token(s: &str) -> bool {
    const KEYWORDS: &[&str] = &[
        "global",
        "dyad",
        "oracle_payload.v1",
        "null_insufficient",
        "schema",
        "generated",
        "scope",
        "kind",
        "alias",
        "sufficiency",
        "days_observed",
        "coverage",
        "dead_lanes",
        "twin_bss",
        "n_lapse_test",
        "gate_passed",
        "state",
        "r_now",
        "r_trend_7d",
        "oii_ema",
        "oii_streak_days",
        "couplings",
        "src",
        "dst",
        "lag_days",
        "rho",
        "n_eff",
        "null_q99",
        "sig",
        "forecast",
        "horizon_days",
        "r_q10",
        "r_q50",
        "r_q90",
        "p_lapse",
        "critical_days",
        "findings",
        "rule_id",
        "severity",
        "metrics",
        "interventions",
        "bank_id",
        "trigger_rule",
        "target_lane",
        "params",
        "theta_r",
        "beta1",
        "beta2",
        "gamma",
        "total",
    ];
    if KEYWORDS.contains(&s) || FEATURE_LANES.contains(&s) {
        return true;
    }
    if is_iso_date(s) {
        return true;
    }
    if s.len() == 6 && s.starts_with("iv-") && s[3..].bytes().all(|c| c.is_ascii_digit()) {
        return true;
    }
    if s.starts_with("R-") {
        let rest = &s[2..];
        if let Some((alpha, num)) = rest.split_once('-') {
            if !alpha.is_empty()
                && alpha.bytes().all(|c| c.is_ascii_uppercase())
                && (2..=3).contains(&num.len())
                && num.bytes().all(|c| c.is_ascii_digit())
            {
                return true;
            }
        }
    }
    if s.len() == 66 && s.starts_with("C-") && s[2..].bytes().all(|c| c.is_ascii_hexdigit()) {
        return true;
    }
    if s.len() == 15 && s.starts_with("C~") && s[2..].bytes().all(|c| c.is_ascii_hexdigit()) {
        return true;
    }
    false
}

/// Runtime sterility guard (I-22 companion). Dictionary keys are inspected too.
pub fn assert_sterile(payload: &Value) -> Result<(), String> {
    fn walk(v: &Value, path: &str) -> Result<(), String> {
        match v {
            Value::Object(map) => {
                for (k, child) in map {
                    if !is_sterile_token(k) {
                        return Err(format!("sterile key fail at {path}.{k}"));
                    }
                    walk(child, &format!("{path}.{k}"))?;
                }
                Ok(())
            }
            Value::Array(arr) => {
                for (i, child) in arr.iter().enumerate() {
                    walk(child, &format!("{path}[{i}]"))?;
                }
                Ok(())
            }
            Value::String(s) => {
                if is_sterile_token(s) {
                    Ok(())
                } else {
                    Err(format!("sterile string fail at {path} len={}", s.len()))
                }
            }
            _ => Ok(()),
        }
    }
    walk(payload, "$")
}

pub fn empty_oracle_payload(generated: &str, scope_kind: &str) -> Value {
    json!({
        "schema": ORACLE_SCHEMA,
        "generated": generated,
        "scope": { "kind": scope_kind, "alias": Value::Null },
        "sufficiency": {
            "days_observed": 0,
            "coverage": 0.0,
            "dead_lanes": Value::Array(vec![]),
            "twin_bss": 0.0,
            "n_lapse_test": 0,
            "gate_passed": false
        },
        "state": {
            "r_now": Value::Null,
            "r_trend_7d": Value::Null,
            "oii_ema": Value::Null,
            "oii_streak_days": 0
        },
        "couplings": [],
        "forecast": {
            "horizon_days": 0,
            "r_q10": [],
            "r_q50": [],
            "r_q90": [],
            "p_lapse": [],
            "critical_days": []
        },
        "findings": [],
        "interventions": []
    })
}

/// Assemble sterile `oracle_payload.v1` from twin + optional coupling.
pub fn generate_oracle_payload(
    generated: &str,
    twin: &TwinScenarioResult,
    coup: Option<&CouplingMatrix>,
    days_observed: i32,
    coverage: f64,
) -> Result<Value, String> {
    let coup_empty = CouplingMatrix {
        pairs: vec![],
        n_rows: 0,
        max_lag: 14,
        n_lanes: FEATURE_LANES.len(),
    };
    let coup = coup.unwrap_or(&coup_empty);

    if !twin.params.gate_passed {
        let mut payload = empty_oracle_payload(generated, "global");
        if let Some(obj) = payload.as_object_mut() {
            obj.insert(
                "sufficiency".into(),
                json!({
                    "days_observed": days_observed,
                    "coverage": round4(coverage),
                    "dead_lanes": Value::Array(vec![]),
                    "twin_bss": twin.params.bss,
                    "n_lapse_test": twin.params.n_lapse_test,
                    "gate_passed": false
                }),
            );
            obj.insert(
                "state".into(),
                json!({
                    "r_now": twin.state.r_now,
                    "r_trend_7d": Value::Null,
                    "oii_ema": Value::Null,
                    "oii_streak_days": 0
                }),
            );
        }
        assert_sterile(&payload)?;
        return Ok(payload);
    }

    let r0 = twin.forecast.r_q50.first().copied();
    let r_trend = if twin.forecast.r_q50.len() > 6 {
        Some(round4(twin.forecast.r_q50[6] - twin.forecast.r_q50[0]))
    } else {
        None
    };
    let findings = evaluate_rules(&twin.params, coup, r0);
    let interventions = select_interventions(&findings);

    let couplings_out: Vec<Value> = coup
        .pairs
        .iter()
        .filter(|p| p.sig)
        .map(|p| {
            json!({
                "src": lane_name(p.src),
                "dst": lane_name(p.dst),
                "lag_days": p.lag,
                "rho": p.rho,
                "n_eff": p.n_eff,
                "null_q99": p.null_q99.unwrap_or(0.0),
                "sig": true
            })
        })
        .collect();

    let findings_json: Vec<Value> = findings
        .iter()
        .map(|f| {
            json!({
                "rule_id": f.rule_id,
                "severity": f.severity,
                "metrics": f.metrics
            })
        })
        .collect();
    let interventions_json: Vec<Value> = interventions
        .iter()
        .map(|i| {
            json!({
                "bank_id": i.bank_id,
                "trigger_rule": i.trigger_rule,
                "target_lane": i.target_lane,
                "params": i.params
            })
        })
        .collect();

    let payload = json!({
        "schema": ORACLE_SCHEMA,
        "generated": generated,
        "scope": { "kind": "global", "alias": Value::Null },
        "sufficiency": {
            "days_observed": days_observed,
            "coverage": round4(coverage),
            "dead_lanes": Value::Array(vec![]),
            "twin_bss": twin.params.bss,
            "n_lapse_test": twin.params.n_lapse_test,
            "gate_passed": true
        },
        "state": {
            "r_now": r0.map(round4),
            "r_trend_7d": r_trend,
            "oii_ema": Value::Null,
            "oii_streak_days": 0
        },
        "couplings": couplings_out,
        "forecast": {
            "horizon_days": twin.forecast.horizon_days,
            "r_q10": twin.forecast.r_q10,
            "r_q50": twin.forecast.r_q50,
            "r_q90": twin.forecast.r_q90,
            "p_lapse": twin.forecast.p_lapse,
            "critical_days": twin.forecast.critical_days
        },
        "findings": findings_json,
        "interventions": interventions_json
    });
    assert_sterile(&payload)?;
    Ok(payload)
}

/// Languageization material only (no LLM call).
pub fn render_oracle_consult(payload: &Value) -> String {
    let gate = payload
        .pointer("/sufficiency/gate_passed")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    if !gate {
        return "(Echo: データ不足または R-失策関係が未確認のため予測・介入は非表示。観測を継続すること)".into();
    }
    let mut lines = Vec::new();
    if let Some(r) = payload.pointer("/state/r_now").and_then(|v| v.as_f64()) {
        let bss = payload
            .pointer("/sufficiency/twin_bss")
            .and_then(|v| v.as_f64())
            .unwrap_or(0.0);
        lines.push(format!(
            "- 現在の認知リソース推定 R={r:.2} (snapshot BSS={bss:.3})"
        ));
    }
    if let Some(arr) = payload.get("couplings").and_then(|v| v.as_array()) {
        for c in arr.iter().take(4) {
            lines.push(format!(
                "- 結合: {} → {} (ラグ{:+}日, ρ={:+.2}, n={})",
                c.get("src").and_then(|v| v.as_str()).unwrap_or("?"),
                c.get("dst").and_then(|v| v.as_str()).unwrap_or("?"),
                c.get("lag_days").and_then(|v| v.as_i64()).unwrap_or(0),
                c.get("rho").and_then(|v| v.as_f64()).unwrap_or(0.0),
                c.get("n_eff").and_then(|v| v.as_i64()).unwrap_or(0),
            ));
        }
    }
    if let Some(arr) = payload.get("interventions").and_then(|v| v.as_array()) {
        for iv in arr.iter().take(3) {
            let id = iv.get("bank_id").and_then(|v| v.as_str()).unwrap_or("");
            if let Some(e) = INTERVENTION_BANK.iter().find(|b| b.bank_id == id) {
                lines.push(format!("- 介入候補: {} ({})", e.label, e.bank_id));
            }
        }
    }
    if lines.is_empty() {
        "(Echo: gate_passed だが findings なし)".into()
    } else {
        lines.join("\n")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::analytics::digital_twin::{
        evaluate_digital_twin_scenario, ScenarioModifiers, TwinSnapshotInput,
    };

    #[test]
    fn intervention_lanes_in_features() {
        for e in INTERVENTION_BANK {
            assert!(e.target_lane < FEATURE_LANES.len());
        }
    }

    #[test]
    fn empty_payload_is_sterile() {
        let p = empty_oracle_payload("2026-07-20", "global");
        assert!(assert_sterile(&p).is_ok());
    }

    #[test]
    fn generate_fail_closed_when_ungated() {
        let twin = evaluate_digital_twin_scenario(TwinSnapshotInput {
            tensor: None,
            pulse_affinity: None,
            rasch_posterior: None,
            gap_data_sufficiency: None,
            gap_count: None,
            today: "2026-07-20".into(),
            horizon_days: 7,
            scenario: ScenarioModifiers::default(),
        });
        let payload = generate_oracle_payload("2026-07-20", &twin, None, 0, 0.0).unwrap();
        assert_eq!(
            payload.pointer("/sufficiency/gate_passed").and_then(|v| v.as_bool()),
            Some(false)
        );
    }
}
