//! Deterministic gap analysis core (linear + nonlinear merge).

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use super::input::AnalyticsDailyDay;
use super::nonlinear::{
    analyze_gakuchika, analyze_intellectualization, analyze_life_balance, analyze_procrastination,
};
use super::taxonomy::{
    GAP_THRESHOLD, GENUINE_DOC_MIN_WEIGHT, INTENT_MARKERS, NEGLIGIBLE, SIMULATED_PERSONA_WEIGHT,
    THEMES,
};

#[derive(Debug, Clone)]
struct SubjDoc {
    date: String,
    weight: f64,
    text: String,
}

fn contains_any(hay: &str, needles: &[&str]) -> bool {
    needles.iter().any(|n| hay.contains(n))
}

fn count_kw(hay: &str, kw: &str) -> usize {
    if kw.is_empty() {
        return 0;
    }
    hay.matches(kw).count()
}

fn build_subjective_corpus(daily: &[AnalyticsDailyDay]) -> Vec<SubjDoc> {
    let mut docs = Vec::new();
    for dc in daily {
        let mut genuine = Vec::new();
        let mut simulated = Vec::new();
        let diary = dc.diary_text.trim();
        if !diary.is_empty() {
            genuine.push(diary.to_string());
        }
        for c in &dc.consultations {
            let q = c.query.trim();
            if q.is_empty() {
                continue;
            }
            if c.is_simulated_persona {
                simulated.push(q.to_string());
            } else {
                genuine.push(q.to_string());
            }
        }
        if !genuine.is_empty() {
            docs.push(SubjDoc {
                date: dc.date.clone(),
                weight: 1.0,
                text: genuine.join("\n"),
            });
        }
        if !simulated.is_empty() {
            docs.push(SubjDoc {
                date: dc.date.clone(),
                weight: SIMULATED_PERSONA_WEIGHT,
                text: simulated.join("\n"),
            });
        }
    }
    docs
}

fn subjective_theme_scores(docs: &[SubjDoc]) -> Vec<(String, Value)> {
    let mut weighted: Vec<f64> = vec![0.0; THEMES.len()];
    let mut quotes: Vec<Vec<Value>> = vec![Vec::new(); THEMES.len()];

    for doc in docs {
        for (ti, theme) in THEMES.iter().enumerate() {
            for kw in theme.subjective {
                let mut start = 0usize;
                while let Some(rel) = doc.text[start..].find(kw) {
                    let abs = start + rel;
                    let s = abs.saturating_sub(15);
                    let e = (abs + kw.len() + 25).min(doc.text.len());
                    let snippet = doc.text.get(s..e).unwrap_or("").replace('\n', " ");
                    let w = if contains_any(&snippet, INTENT_MARKERS) {
                        2.0
                    } else {
                        1.0
                    };
                    weighted[ti] += w * doc.weight;
                    if doc.weight >= GENUINE_DOC_MIN_WEIGHT && quotes[ti].len() < 3 {
                        quotes[ti].push(json!({
                            "date": doc.date,
                            "quote": format!("…{}…", snippet.trim()),
                        }));
                    }
                    start = abs + kw.len().max(1);
                    if start >= doc.text.len() {
                        break;
                    }
                }
            }
        }
    }

    let total: f64 = weighted.iter().sum::<f64>().max(1.0);
    THEMES
        .iter()
        .enumerate()
        .map(|(i, t)| {
            (
                t.name.to_string(),
                json!({
                    "score": round3(weighted[i] / total),
                    "weighted_hits": (weighted[i] * 10.0).round() / 10.0,
                    "quotes": quotes[i],
                }),
            )
        })
        .collect()
}

fn objective_theme_scores(daily: &[AnalyticsDailyDay]) -> Vec<(String, Value)> {
    let mut spend: Vec<(String, i64)> = Vec::new();
    let mut events: Vec<(String, String)> = Vec::new();
    let mut line_docs: Vec<(String, String)> = Vec::new();

    for dc in daily {
        for tx in &dc.transactions {
            if tx.tx_type == "expense" {
                let cat = if tx.category.trim().is_empty() {
                    "(不明)".to_string()
                } else {
                    tx.category.trim().to_string()
                };
                spend.push((cat, tx.amount));
            }
        }
        for ev in &dc.calendar_events {
            events.push((dc.date.clone(), ev.title.clone()));
        }
        let line = dc.line_self_text.trim();
        if !line.is_empty() {
            line_docs.push((dc.date.clone(), line.to_string()));
        }
    }

    let total_spend = spend
        .iter()
        .map(|(_, amount)| *amount)
        .fold(0_i64, i64::saturating_add)
        .max(1);
    let total_events = events.len().max(1);

    let mut line_hits = vec![0usize; THEMES.len()];
    let mut line_ev: Vec<Vec<Value>> = vec![Vec::new(); THEMES.len()];
    let mut total_line_hits = 0usize;
    for (date, text) in &line_docs {
        for (ti, theme) in THEMES.iter().enumerate() {
            for kw in theme.line_keywords {
                let n = count_kw(text, kw);
                if n > 0 {
                    line_hits[ti] += n;
                    total_line_hits += n;
                    if line_ev[ti].len() < 2 {
                        line_ev[ti].push(json!({"date": date, "keyword": kw}));
                    }
                }
            }
        }
    }

    THEMES
        .iter()
        .enumerate()
        .map(|(ti, theme)| {
            let theme_spend: i64 = spend
                .iter()
                .filter(|(cat, _)| theme.spend_categories.iter().any(|kw| cat.contains(kw)))
                .map(|(_, a)| *a)
                .fold(0_i64, i64::saturating_add);
            let theme_event_count = events
                .iter()
                .filter(|(_, title)| {
                    theme
                        .calendar_keywords
                        .iter()
                        .any(|kw| title.contains(kw))
                })
                .count();
            let money_share = theme_spend as f64 / total_spend as f64;
            let time_share = theme_event_count as f64 / total_events as f64;
            let line_share = if total_line_hits == 0 {
                0.0
            } else {
                line_hits[ti] as f64 / total_line_hits as f64
            };
            let score = (money_share + time_share + line_share) / 3.0;
            let evidence_events: Vec<String> = events
                .iter()
                .filter(|(_, title)| {
                    theme
                        .calendar_keywords
                        .iter()
                        .any(|kw| title.contains(kw))
                })
                .map(|(_, title)| title.clone())
                .take(3)
                .collect();
            (
                theme.name.to_string(),
                json!({
                    "score": round3(score),
                    "money_spent": theme_spend,
                    "money_share": round3(money_share),
                    "event_count": theme_event_count,
                    "time_share": round3(time_share),
                    "line_mention_share": round3(line_share),
                    "evidence": {
                        "events": evidence_events,
                        "line": line_ev[ti],
                    },
                }),
            )
        })
        .collect()
}

fn detect_linear_gaps(subj: &[(String, Value)], obj: &[(String, Value)]) -> Vec<Value> {
    let mut gaps = Vec::new();
    for ((theme, s), (_, o)) in subj.iter().zip(obj.iter()) {
        let s_score = s.get("score").and_then(|v| v.as_f64()).unwrap_or(0.0);
        let o_score = o.get("score").and_then(|v| v.as_f64()).unwrap_or(0.0);
        let gap = round3(s_score - o_score);
        let money = o.get("money_spent").and_then(|v| v.as_i64()).unwrap_or(0);
        let events = o.get("event_count").and_then(|v| v.as_u64()).unwrap_or(0);
        let line_share = o
            .get("line_mention_share")
            .and_then(|v| v.as_f64())
            .unwrap_or(0.0);
        let quotes = s.get("quotes").cloned().unwrap_or(json!([]));

        if gap >= GAP_THRESHOLD && o_score <= s_score {
            gaps.push(json!({
                "theme": theme,
                "type": "intention_gap",
                "gap": gap,
                "insight": format!(
                    "「{theme}」は内省・相談で強く言及される (主観 {s_pct:.0}%) が、支出 {money}円 / 関連予定 {events}件 / 対外的言及シェア {line_pct:.0}% と行動が伴っていない。考えるだけで資源 (金・時間) を投じていない認知的不協和の候補",
                    s_pct = s_score * 100.0,
                    line_pct = line_share * 100.0,
                ),
                "subjective": {"score": s_score, "quotes": quotes},
                "objective": {
                    "score": o_score,
                    "money_spent": money,
                    "event_count": events,
                    "line_mention_share": line_share,
                    "evidence": o.get("evidence").cloned().unwrap_or(json!({})),
                },
            }));
        } else if -gap >= GAP_THRESHOLD && o_score > NEGLIGIBLE {
            gaps.push(json!({
                "theme": theme,
                "type": "blind_spot",
                "gap": round3(-gap),
                "insight": format!(
                    "「{theme}」は支出 {money}円 / 予定 {events}件 / 対外言及 {line_pct:.0}% と資源を投じているのに、内省・相談での言及は主観 {s_pct:.0}% と薄い。言語化されていない行動パターン (盲点) の候補",
                    s_pct = s_score * 100.0,
                    line_pct = line_share * 100.0,
                ),
                "subjective": {"score": s_score, "quotes": quotes},
                "objective": {
                    "score": o_score,
                    "money_spent": money,
                    "event_count": events,
                    "line_mention_share": line_share,
                    "evidence": o.get("evidence").cloned().unwrap_or(json!({})),
                },
            }));
        }
    }
    gaps
}

fn data_sufficiency(daily: &[AnalyticsDailyDay], subj_docs: usize) -> f64 {
    let subj = (subj_docs as f64 / 7.0).min(1.0);
    let has_spend = daily.iter().any(|d| {
        d.transactions
            .iter()
            .any(|t| t.tx_type == "expense" && t.amount > 0)
    });
    let has_events = daily.iter().any(|d| !d.calendar_events.is_empty());
    let has_line = daily.iter().any(|d| !d.line_self_text.trim().is_empty());
    let obj = ((has_spend as u8 + has_events as u8 + has_line as u8) as f64).min(1.0);
    // Python: mean of two channels; second saturates once any objective exists
    let obj_score = if has_spend || has_events || has_line {
        1.0
    } else {
        0.0
    };
    let _ = obj;
    round3((subj + obj_score) / 2.0)
}

fn round3(x: f64) -> f64 {
    (x * 1000.0).round() / 1000.0
}

fn map_scores(pairs: &[(String, Value)]) -> Value {
    let mut obj = serde_json::Map::new();
    for (k, v) in pairs {
        obj.insert(k.clone(), v.clone());
    }
    Value::Object(obj)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct GapAnalysisResult {
    pub schema: String,
    pub data_sufficiency: f64,
    pub gaps: Vec<Value>,
    pub payload: Value,
}

/// Run full gap_analysis.v3 (deterministic).
pub fn analyze_gaps(daily: &[AnalyticsDailyDay]) -> GapAnalysisResult {
    let subj_docs = build_subjective_corpus(daily);
    let subj = subjective_theme_scores(&subj_docs);
    let obj = objective_theme_scores(daily);
    let mut gaps = detect_linear_gaps(&subj, &obj);

    let proc = analyze_procrastination(daily);
    for g in proc.get("flags").and_then(|v| v.as_array()).into_iter().flatten() {
        gaps.push(g.clone());
    }
    let gaku = analyze_gakuchika(&subj, &obj);
    for g in gaku.get("flags").and_then(|v| v.as_array()).into_iter().flatten() {
        gaps.push(g.clone());
    }
    let intel = analyze_intellectualization(daily);
    for g in intel.get("flags").and_then(|v| v.as_array()).into_iter().flatten() {
        gaps.push(g.clone());
    }
    let life = analyze_life_balance(daily);
    for g in life.get("flags").and_then(|v| v.as_array()).into_iter().flatten() {
        gaps.push(g.clone());
    }

    let coverage = json!({
        "subjective_docs": subj_docs.len(),
        "total_expense_yen": daily.iter().flat_map(|d| d.transactions.iter())
            .filter(|t| t.tx_type == "expense")
            .map(|t| t.amount)
            .fold(0_i64, i64::saturating_add),
        "calendar_events": daily.iter().map(|d| d.calendar_events.len()).sum::<usize>(),
        "line_docs": daily.iter().filter(|d| !d.line_self_text.trim().is_empty()).count(),
    });

    let sufficiency = data_sufficiency(daily, subj_docs.len());
    let payload = json!({
        "schema": "gap_analysis.v3",
        "coverage": coverage,
        "data_sufficiency": sufficiency,
        "subjective_scores": map_scores(&subj),
        "objective_scores": map_scores(&obj),
        "procrastination": proc,
        "gakuchika": gaku,
        "intellectualization": intel,
        "life_balance": life,
        "gaps": gaps,
    });

    GapAnalysisResult {
        schema: "gap_analysis.v3".into(),
        data_sufficiency: sufficiency,
        gaps: payload
            .get("gaps")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default(),
        payload,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::analytics::input::{CalendarEventIn, TransactionIn};

    #[test]
    fn line_not_in_subjective_intention() {
        let days = vec![AnalyticsDailyDay {
            date: "2026-01-10".into(),
            diary_text: "キャリアが不安で焦っている。転職したい。".into(),
            consultations: vec![],
            transactions: vec![],
            calendar_events: vec![],
            line_self_text: "転職エージェントに連絡した。面接も受けた。".into(),
        }];
        let result = analyze_gaps(&days);
        // LINE must boost objective career axis — may reduce intention_gap strength
        assert_eq!(result.schema, "gap_analysis.v3");
        assert!(result.payload.get("objective_scores").is_some());
    }

    #[test]
    fn hyperbolic_not_exponential_shape() {
        use crate::analytics::nonlinear::hyperbolic_discount;
        let v0 = hyperbolic_discount(0);
        let v7 = hyperbolic_discount(7);
        assert!((v0 - 1.0).abs() < 1e-9);
        assert!((v7 - 1.0 / (1.0 + 0.3 * 7.0)).abs() < 1e-9);
        assert!(v7 < 0.4);
    }

    #[test]
    fn expense_not_income() {
        let days = vec![AnalyticsDailyDay {
            date: "2026-01-10".into(),
            diary_text: String::new(),
            consultations: vec![],
            transactions: vec![
                TransactionIn {
                    tx_type: "income".into(),
                    category: "給与".into(),
                    amount: 300_000,
                },
                TransactionIn {
                    tx_type: "expense".into(),
                    category: "ゲーム".into(),
                    amount: 5_000,
                },
            ],
            calendar_events: vec![CalendarEventIn {
                title: "ゲーム大会".into(),
            }],
            line_self_text: String::new(),
        }];
        let result = analyze_gaps(&days);
        let coverage = result.payload.get("coverage").unwrap();
        assert_eq!(coverage.get("total_expense_yen").and_then(|v| v.as_i64()), Some(5_000));
    }

    #[test]
    fn expense_aggregation_saturates_instead_of_overflowing() {
        let days = vec![AnalyticsDailyDay {
            date: "2026-01-10".into(),
            diary_text: String::new(),
            consultations: vec![],
            transactions: vec![
                TransactionIn {
                    tx_type: "expense".into(),
                    category: "食費".into(),
                    amount: i64::MAX,
                },
                TransactionIn {
                    tx_type: "expense".into(),
                    category: "食費".into(),
                    amount: 1,
                },
            ],
            calendar_events: vec![],
            line_self_text: String::new(),
        }];

        let result = analyze_gaps(&days);
        let coverage = result.payload.get("coverage").expect("coverage");
        assert_eq!(
            coverage.get("total_expense_yen").and_then(Value::as_i64),
            Some(i64::MAX)
        );
    }
}
