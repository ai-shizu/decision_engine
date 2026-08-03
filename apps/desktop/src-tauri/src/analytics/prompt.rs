//! Languageization prompt only — does NOT discover gaps (AI_SKILLS §6.2).

use serde_json::Value;

use super::gap::GapAnalysisResult;

/// Build a prompt that asks the LLM to verbalize already-computed gaps.
/// The model must not invent new gap types or scores.
pub fn build_gap_languageization_prompt(result: &GapAnalysisResult) -> String {
    let mut out = String::from(
        "あなたは主観×客観ギャップ表を言語化するアシスタントである。\
新たなギャップを創作してはならない。与えられた gaps の type/theme/gap/insight と定量証拠のみを根拠に、\
残酷だが反証可能な事実として短く述べよ。data_sufficiency が 0.5 未満なら確度が低い旨を必ず注記せよ。\n\n",
    );
    out.push_str(&format!(
        "data_sufficiency: {}\n\n## gaps\n",
        result.data_sufficiency
    ));
    if result.gaps.is_empty() {
        out.push_str("(検出ギャップなし)\n");
    } else {
        for (i, g) in result.gaps.iter().enumerate() {
            out.push_str(&format!("{}. {}\n", i + 1, compact_gap(g)));
        }
    }
    out
}

fn compact_gap(g: &Value) -> String {
    format!(
        "type={} theme={} gap={} insight={}",
        g.get("type").and_then(|v| v.as_str()).unwrap_or("?"),
        g.get("theme").and_then(|v| v.as_str()).unwrap_or("?"),
        g.get("gap").and_then(|v| v.as_f64()).unwrap_or(0.0),
        g.get("insight").and_then(|v| v.as_str()).unwrap_or(""),
    )
}
