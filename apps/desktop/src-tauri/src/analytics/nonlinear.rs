//! Nonlinear gap detectors (AI_SKILLS §6.4 — do not simplify).

use serde_json::{json, Value};

use super::input::AnalyticsDailyDay;
use super::taxonomy::{
    ABSTRACT_LEXICON, ABSTRACT_MIN_HITS, ABSTRACT_SPIKE_RATIO, AVOIDANCE_FLAG_THRESHOLD,
    DECLARE_MARKERS, DONE_MARKERS, GENUINE_DOC_MIN_WEIGHT, GUILT_MARKERS, JOBHUNT_ACTION_KEYWORDS,
    K_HYPERBOLIC, PRIVATE_TIME_KEYWORDS, PRODUCTIVITY_MARKERS, SIMULATED_PERSONA_WEIGHT,
    STABILIZER_WINDOW_DAYS, TASK_LEXICON, THEMES,
};

fn contains_any(hay: &str, needles: &[&str]) -> bool {
    needles.iter().any(|n| hay.contains(n))
}

fn count_any(hay: &str, needles: &[&str]) -> i32 {
    needles.iter().map(|n| hay.matches(n).count() as i32).sum()
}

/// Hyperbolic discount V = 1/(1 + k·D). Never replace with exponential.
pub fn hyperbolic_discount(delay_days: i64) -> f64 {
    1.0 / (1.0 + K_HYPERBOLIC * (delay_days.max(0) as f64))
}

fn parse_ymd(s: &str) -> Option<(i32, u32, u32)> {
    let s = s.trim();
    if s.len() != 10 {
        return None;
    }
    let y: i32 = s.get(0..4)?.parse().ok()?;
    let m: u32 = s.get(5..7)?.parse().ok()?;
    let d: u32 = s.get(8..10)?.parse().ok()?;
    if !(1..=12).contains(&m) || !(1..=31).contains(&d) {
        return None;
    }
    Some((y, m, d))
}

/// Days since civil epoch (Howard Hinnant) for date diffs.
fn civil_days(y: i32, m: u32, d: u32) -> i64 {
    let y = y as i64;
    let m = m as i64;
    let d = d as i64;
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = if m > 2 { m - 3 } else { m + 9 };
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

fn day_diff(a: &str, b: &str) -> Option<i64> {
    let (ay, am, ad) = parse_ymd(a)?;
    let (by, bm, bd) = parse_ymd(b)?;
    Some(civil_days(by, bm, bd) - civil_days(ay, am, ad))
}

fn iso_week(y: i32, m: u32, d: u32) -> (i32, u8) {
    let days = civil_days(y, m, d);
    // 1970-01-01 was Thursday → Mon=0 via (days + 3) rem 7.
    let wd = (days + 3).rem_euclid(7);
    let thursday = days - wd + 3;
    let mut year = y;
    if thursday < civil_days(y, 1, 1) {
        year = y - 1;
    } else if thursday >= civil_days(y + 1, 1, 1) {
        year = y + 1;
    }
    let jan4 = civil_days(year, 1, 4);
    let wd4 = (jan4 + 3).rem_euclid(7);
    let week1_thu = jan4 - wd4 + 3;
    let week = ((thursday - week1_thu) / 7 + 1) as u8;
    (year, week.max(1))
}

struct SubjDoc {
    date: String,
    weight: f64,
    text: String,
}

fn subjective_docs(daily: &[AnalyticsDailyDay]) -> Vec<SubjDoc> {
    let mut docs = Vec::new();
    for dc in daily {
        let mut genuine = Vec::new();
        let mut simulated = Vec::new();
        if !dc.diary_text.trim().is_empty() {
            genuine.push(dc.diary_text.trim().to_string());
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

/// task_avoidance via hyperbolic discount of declare→execute delay.
pub fn analyze_procrastination(daily: &[AnalyticsDailyDay]) -> Value {
    let docs = subjective_docs(daily);
    let mut declarations: Vec<(String, String, String)> = Vec::new(); // task, date, quote
    let mut seen = std::collections::BTreeSet::new();

    for doc in &docs {
        if doc.weight < GENUINE_DOC_MIN_WEIGHT {
            continue;
        }
        for (task, kws) in TASK_LEXICON {
            if !contains_any(&doc.text, kws) {
                continue;
            }
            if !contains_any(&doc.text, DECLARE_MARKERS) {
                continue;
            }
            if contains_any(&doc.text, DONE_MARKERS) {
                continue;
            }
            let key = (task.to_string(), doc.date.clone());
            if seen.insert(key.clone()) {
                let quote = doc.text.chars().take(80).collect::<String>();
                declarations.push((task.to_string(), doc.date.clone(), quote));
            }
        }
    }

    let mut records = Vec::new();
    let mut values = Vec::new();
    for (task, decl_date, quote) in &declarations {
        let kws = TASK_LEXICON
            .iter()
            .find(|(t, _)| t == task)
            .map(|(_, k)| *k)
            .unwrap_or(&[]);
        let mut exec_date: Option<String> = None;
        for dc in daily {
            if day_diff(decl_date, &dc.date).unwrap_or(-1) < 0 {
                continue;
            }
            let cal_hit = dc
                .calendar_events
                .iter()
                .any(|e| contains_any(&e.title, kws));
            let line_hit = contains_any(&dc.line_self_text, kws)
                && contains_any(&dc.line_self_text, DONE_MARKERS);
            let diary_hit =
                contains_any(&dc.diary_text, kws) && contains_any(&dc.diary_text, DONE_MARKERS);
            if cal_hit || line_hit || diary_hit {
                exec_date = Some(dc.date.clone());
                break;
            }
        }
        let (delay, value, executed) = match exec_date {
            Some(ref ed) => {
                let d = day_diff(decl_date, ed).unwrap_or(0).max(0);
                (d, hyperbolic_discount(d), true)
            }
            None => (0, 0.0, false),
        };
        values.push(value);
        records.push(json!({
            "declared": task,
            "declared_date": decl_date,
            "executed": executed,
            "execution_evidence": exec_date,
            "delay_days": delay,
            "discounted_value": (value * 1000.0).round() / 1000.0,
            "quote": quote,
        }));
    }

    let avoidance = if values.is_empty() {
        0.0
    } else {
        1.0 - values.iter().sum::<f64>() / values.len() as f64
    };
    let mut flags = Vec::new();
    if avoidance >= AVOIDANCE_FLAG_THRESHOLD && !records.is_empty() {
        let task = records[0]
            .get("declared")
            .and_then(|v| v.as_str())
            .unwrap_or("タスク");
        flags.push(json!({
            "theme": format!("就活タスク: {task}"),
            "type": "task_avoidance",
            "gap": (avoidance * 1000.0).round() / 1000.0,
            "insight": format!(
                "宣言した就活タスクの双曲割引評価で回避指数 {:.0}%。遅延初期に価値が大きく毀損するパターン (V=1/(1+0.3D))。宣言の翌日実行はフラグしない対照群を維持せよ。",
                avoidance * 100.0,
            ),
            "subjective": {"quotes": records.iter().take(3).map(|r| json!({
                "date": r.get("declared_date"),
                "quote": r.get("quote"),
            })).collect::<Vec<_>>()},
            "objective": {"records": records},
        }));
    }

    json!({
        "avoidance_index": (avoidance * 1000.0).round() / 1000.0,
        "records": records,
        "flags": flags,
    })
}

/// true_gakuchika: declared subjective vs passion objective divergence.
pub fn analyze_gakuchika(subj: &[(String, Value)], obj: &[(String, Value)]) -> Value {
    let mut flags = Vec::new();
    let mut declared: Option<(String, f64)> = None;
    for (theme, v) in subj {
        let score = v.get("score").and_then(|x| x.as_f64()).unwrap_or(0.0);
        if declared.as_ref().map(|(_, s)| score > *s).unwrap_or(true) {
            declared = Some((theme.clone(), score));
        }
    }
    let Some((decl_theme, decl_score)) = declared else {
        return json!({"flags": []});
    };
    if decl_score < 0.3 {
        return json!({"flags": [], "declared_theme": decl_theme, "declared_score": decl_score});
    }

    let mut passion: Option<(String, f64, &Value)> = None;
    for (theme, v) in obj {
        if theme == &decl_theme {
            continue;
        }
        let score = v.get("score").and_then(|x| x.as_f64()).unwrap_or(0.0);
        if score >= 0.35
            && passion
                .as_ref()
                .map(|(_, s, _)| score > *s)
                .unwrap_or(true)
        {
            passion = Some((theme.clone(), score, v));
        }
    }
    let Some((passion_theme, passion_score, passion_obj)) = passion else {
        return json!({"flags": [], "declared_theme": decl_theme});
    };

    let decl_obj_score = obj
        .iter()
        .find(|(t, _)| t == &decl_theme)
        .and_then(|(_, v)| v.get("score").and_then(|x| x.as_f64()))
        .unwrap_or(0.0);

    if decl_obj_score >= passion_score * 0.5 {
        return json!({"flags": [], "aligned": true});
    }

    let money = passion_obj
        .get("money_spent")
        .and_then(|v| v.as_i64())
        .unwrap_or(0);
    let events = passion_obj
        .get("event_count")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    let gap = ((passion_score - decl_obj_score) * 1000.0).round() / 1000.0;
    flags.push(json!({
        "theme": passion_theme,
        "type": "true_gakuchika",
        "gap": gap,
        "insight": format!(
            "主観の首位は「{decl_theme}」(建前/サンクコスト候補) だが、客観資源は「{passion_theme}」に集中 (支出 {money}円 / 予定 {events}件)。語るべきガクチカは後者にある可能性",
        ),
        "subjective": {"score": decl_score, "declared_theme": decl_theme},
        "objective": {
            "score": passion_score,
            "money_spent": money,
            "event_count": events,
            "evidence": passion_obj.get("evidence").cloned().unwrap_or(json!({})),
        },
    }));

    json!({"flags": flags})
}

pub fn analyze_intellectualization(daily: &[AnalyticsDailyDay]) -> Value {
    use std::collections::BTreeMap;
    let mut weeks: BTreeMap<(i32, u8), (String, i32, i32, Vec<Value>)> = BTreeMap::new();

    for dc in daily {
        let Some((y, m, d)) = parse_ymd(&dc.date) else {
            continue;
        };
        let (iy, iw) = iso_week(y, m, d);
        let key = (iy, iw);
        let entry = weeks.entry(key).or_insert_with(|| {
            (format!("{iy}-W{iw:02}"), 0, 0, Vec::new())
        });
        let abstract_hits = count_any(&dc.diary_text, ABSTRACT_LEXICON);
        entry.1 += abstract_hits;
        if abstract_hits > 0 && entry.3.len() < 2 {
            entry.3.push(json!({
                "date": dc.date,
                "quote": dc.diary_text.chars().take(60).collect::<String>(),
            }));
        }
        let mut actions = 0;
        for ev in &dc.calendar_events {
            if contains_any(&ev.title, JOBHUNT_ACTION_KEYWORDS) {
                actions += 1;
            }
        }
        if contains_any(&dc.line_self_text, JOBHUNT_ACTION_KEYWORDS) {
            actions += 1;
        }
        entry.2 += actions;
    }

    let hit_counts: Vec<f64> = weeks.values().map(|(_, h, _, _)| *h as f64).collect();
    let baseline = if hit_counts.is_empty() {
        0.0
    } else {
        hit_counts.iter().sum::<f64>() / hit_counts.len() as f64
    };

    let mut flags = Vec::new();
    let mut week_rows = Vec::new();
    for ((_y, _w), (label, abs_hits, actions, quotes)) in &weeks {
        let spike = *abs_hits >= ABSTRACT_MIN_HITS
            && (*abs_hits as f64) >= ABSTRACT_SPIKE_RATIO * baseline.max(1.0);
        // NON-NEGOTIABLE: action_count == 0
        let flagged = spike && *actions == 0;
        week_rows.push(json!({
            "week": label,
            "abstract_hits": abs_hits,
            "action_count": actions,
            "flagged": flagged,
        }));
        if flagged {
            flags.push(json!({
                "theme": format!("知性化の疑い ({label})"),
                "type": "intellectualization_gap",
                "gap": ((*abs_hits as f64 / 6.0).min(1.0) * 1000.0).round() / 1000.0,
                "insight": format!(
                    "{label} は就活アクション 0 件なのに、日記の抽象語彙が {abs_hits} 回 (全期間平均 {baseline:.1} 回/週) と急上昇。行動が止まった不安を抽象的思考で覆い隠す防衛機制 (知性化) の疑い。"
                ),
                "subjective": {"quotes": quotes},
                "objective": {
                    "action_count": 0,
                    "abstract_hits": abs_hits,
                    "baseline_avg": (baseline * 10.0).round() / 10.0,
                },
            }));
        }
    }

    json!({
        "weeks": week_rows,
        "baseline_avg_hits": (baseline * 10.0).round() / 10.0,
        "flags": flags,
    })
}

pub fn analyze_life_balance(daily: &[AnalyticsDailyDay]) -> Value {
    let mut by_date: std::collections::BTreeMap<String, &AnalyticsDailyDay> =
        std::collections::BTreeMap::new();
    for dc in daily {
        by_date.insert(dc.date.clone(), dc);
    }

    let productivity = |dc: &AnalyticsDailyDay| -> i32 {
        count_any(&dc.diary_text, PRODUCTIVITY_MARKERS)
            + count_any(&dc.line_self_text, PRODUCTIVITY_MARKERS)
    };

    let mut flags = Vec::new();
    for dc in daily {
        let private = dc
            .calendar_events
            .iter()
            .any(|e| contains_any(&e.title, PRIVATE_TIME_KEYWORDS));
        if !private {
            continue;
        }
        let guilt_today = contains_any(&dc.diary_text, GUILT_MARKERS);
        let guilt_next = by_date
            .keys()
            .find(|d| day_diff(&dc.date, d) == Some(1))
            .and_then(|d| by_date.get(d))
            .map(|n| contains_any(&n.diary_text, GUILT_MARKERS))
            .unwrap_or(false);
        if !(guilt_today || guilt_next) {
            continue;
        }

        let mut before = 0i32;
        let mut after = 0i32;
        for (date, day) in &by_date {
            if let Some(diff) = day_diff(&dc.date, date) {
                if (-STABILIZER_WINDOW_DAYS..0).contains(&diff) {
                    before += productivity(day);
                }
                if (1..=STABILIZER_WINDOW_DAYS).contains(&diff) {
                    after += productivity(day);
                }
            }
        }
        // NON-NEGOTIABLE: after > before; sole positive flag
        if after > before {
            let gap = (((after - before) as f64 / 3.0).min(1.0) * 1000.0).round() / 1000.0;
            let private_titles: Vec<&str> = dc
                .calendar_events
                .iter()
                .filter(|e| contains_any(&e.title, PRIVATE_TIME_KEYWORDS))
                .map(|e| e.title.as_str())
                .take(3)
                .collect();
            flags.push(json!({
                "theme": "ライフバランス・スタビライザー",
                "type": "stabilizer_effect",
                "gap": gap,
                "insight": format!(
                    "{date} の私的時間の後、生産性シグナルが {before}→{after} と向上。罪悪感は Productivity_Guilt_Trap の可能性 — 充電として肯定する唯一のポジティブフラグ。",
                    date = dc.date,
                ),
                "subjective": {"quotes": [{"date": dc.date, "quote": "罪悪感マーカー検出"}]},
                "objective": {
                    "private_events": private_titles,
                    "productivity_before": before,
                    "productivity_after": after,
                },
            }));
        }
    }

    json!({"flags": flags})
}

// silence unused THEMES in this module
#[allow(dead_code)]
fn _themes_len() -> usize {
    THEMES.len()
}
