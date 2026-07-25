//! Mentor consult context: Gap (M14) + Oracle (M16) + Tensor (M14) for prompt injection.
//!
//! Fail-safe: missing / ungated vault rows produce explicit "unavailable" notes
//! rather than errors. Never invent scores. Discussion-phase interview must NOT
//! call this (I-22); debrief/consult only.
//!
//! # Adaptive mentor preamble (Phase 6 / ZPD)
//!
//! The former fixed `MENTOR_PREAMBLE` is replaced by a level-selected preamble from
//! [`crate::llm::mentor_zpd`], grounded in:
//! 1. **ZPD** — Vygotsky (1978): scaffolding intensity tracks current capability.
//! 2. **Yerkes–Dodson** (1908): inverted-U arousal → Neutral analytic load at mid R.
//! 3. **Desirable Difficulties** — Bjork (1994): high R + low p_lapse → Devil's Advocate.
//!
//! Mapping is deterministic (F-14): Twin `R(t)` / `p_lapse` only; no RNG / egress.

use std::collections::BTreeMap;

use serde_json::Value;

use crate::analytics::oracle::render_oracle_consult;
use crate::db::{VaultErrorCode, VaultHandle};
use crate::llm::mentor_zpd::MentorZpdSignal;

use crate::llm::context_budget::truncate_to_token_budget;

/// Token budgets for mentor sections (Phase 9; replaces char-tail truncate).
const GAP_SECTION_TOKEN_BUDGET: usize = 625;
const ORACLE_SECTION_TOKEN_BUDGET: usize = 375;
const TENSOR_SECTION_TOKEN_BUDGET: usize = 300;
const PROFILE_SECTION_TOKEN_BUDGET: usize = 220;

/// Known SETTINGS fixed-profile keys in canonical display order (mirrors the
/// frontend `fixed_fields` in `settingsLocalCache.ts`). Unknown keys the FE
/// sends still render, appended after these in key order.
const PROFILE_FIELD_LABELS: &[(&str, &str)] = &[
    ("birthday", "誕生日"),
    ("gender", "性別"),
    ("height", "身長(cm)"),
    ("weight", "体重(kg)"),
    ("address", "住所"),
    ("occupation", "勤務先/学校"),
];

#[derive(Debug, Clone, Default)]
pub struct MentorContextSections {
    pub gap_block: String,
    pub oracle_block: String,
    pub tensor_block: String,
    pub gap_available: bool,
    pub oracle_available: bool,
    pub tensor_available: bool,
    pub gap_run_id: Option<String>,
    pub oracle_run_id: Option<String>,
    pub tensor_run_id: Option<String>,
}

fn map_vault(err: VaultErrorCode) -> String {
    format!("{err:?}").to_ascii_lowercase()
}

fn truncate(s: &str, token_budget: usize) -> String {
    truncate_to_token_budget(s, token_budget)
}

fn format_gap_payload(payload: &Value, data_sufficiency: f64) -> String {
    let mut out = String::new();
    out.push_str(&format!("data_sufficiency: {data_sufficiency:.3}\n"));
    match payload.get("gaps").and_then(|v| v.as_array()) {
        None => {
            out.push_str("(検出ギャップなし — 一般論に頼らず観測継続を促せ)\n");
        }
        Some(gaps) if gaps.is_empty() => {
            out.push_str("(検出ギャップなし — 一般論に頼らず観測継続を促せ)\n");
        }
        Some(gaps) => {
            for (i, g) in gaps.iter().take(6).enumerate() {
                let ty = g.get("type").and_then(|v| v.as_str()).unwrap_or("?");
                let theme = g.get("theme").and_then(|v| v.as_str()).unwrap_or("?");
                let gap = g.get("gap").and_then(|v| v.as_f64()).unwrap_or(0.0);
                let insight = g.get("insight").and_then(|v| v.as_str()).unwrap_or("");
                out.push_str(&format!(
                    "{}. type={ty} theme={theme} gap={gap:.3} insight={insight}\n",
                    i + 1
                ));
            }
        }
    }
    truncate(&out, GAP_SECTION_TOKEN_BUDGET)
}

fn format_tensor_payload(payload: &Value, model_hash: &str) -> String {
    let mut out = String::new();
    out.push_str(&format!("model_hash: {model_hash}\n"));
    let dims = payload
        .get("dimensions")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    if dims.is_empty() {
        out.push_str("(Tensor次元なし — スコアを捏造するな)\n");
    } else {
        for d in dims.iter().take(8) {
            let id = d
                .get("dimension_id")
                .and_then(|v| v.as_str())
                .unwrap_or("?");
            let axis = d
                .get("calculus_axis")
                .and_then(|v| v.as_str())
                .unwrap_or("?");
            let score = d.get("score");
            let score_s = match score {
                Some(Value::Null) | None => "N/A".to_string(),
                Some(v) => v
                    .as_f64()
                    .map(|f| format!("{f:.3}"))
                    .unwrap_or_else(|| "N/A".into()),
            };
            let conf = d.get("confidence").and_then(|v| v.as_f64()).unwrap_or(0.0);
            out.push_str(&format!(
                "- {id} ({axis}): score={score_s} conf={conf:.2}\n"
            ));
        }
        out.push_str(
            "N/A の軸は未観測として扱い、自己PRの美辞麗句で埋めさせない。矛盾があれば指摘せよ。\n",
        );
    }
    truncate(&out, TENSOR_SECTION_TOKEN_BUDGET)
}

/// Load latest gap + oracle + tensor from vault. Errors only on vault transport failure;
/// empty/ungated analytics become soft unavailable sections.
pub fn load_mentor_context(vault: &VaultHandle) -> Result<MentorContextSections, String> {
    let mut sections = MentorContextSections::default();

    match vault.gap_analysis_latest().map_err(map_vault)? {
        Some(row) => {
            sections.gap_run_id = Some(row.id.clone());
            let payload: Value =
                serde_json::from_str(&row.payload_json).unwrap_or(Value::Null);
            if row.data_sufficiency < 0.15 {
                sections.gap_block =
                    "（Gap: data_sufficiency が低すぎるため注入を抑制。確度注記のみ。）\n"
                        .into();
                sections.gap_available = false;
            } else {
                sections.gap_block = format_gap_payload(&payload, row.data_sufficiency);
                sections.gap_available = true;
            }
        }
        None => {
            sections.gap_block =
                "（Gap: Vault に分析結果なし — 助言は一般原則に留めよ）\n".into();
        }
    }

    match vault.oracle_run_latest().map_err(map_vault)? {
        Some(row) => {
            sections.oracle_run_id = Some(row.id.clone());
            let payload: Value =
                serde_json::from_str(&row.payload_json).unwrap_or(Value::Null);
            let gated = payload
                .pointer("/sufficiency/gate_passed")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            if !gated {
                sections.oracle_block =
                    "（Oracle: gate_passed=false — 予測・介入は非表示。観測継続）\n".into();
                sections.oracle_available = false;
            } else {
                let rendered = render_oracle_consult(&payload);
                sections.oracle_block = truncate(&rendered, ORACLE_SECTION_TOKEN_BUDGET);
                sections.oracle_available = true;
            }
        }
        None => {
            sections.oracle_block =
                "（Oracle: Vault にペイロードなし — 未来予測に依存するな）\n".into();
        }
    }

    match vault.tensor_profile_latest().map_err(map_vault)? {
        Some(row) => {
            sections.tensor_run_id = Some(row.id.clone());
            let payload: Value =
                serde_json::from_str(&row.payload_json).unwrap_or(Value::Null);
            sections.tensor_block = format_tensor_payload(&payload, &row.model_hash);
            sections.tensor_available = true;
        }
        None => {
            sections.tensor_block =
                "（Tensor: Vault にプロファイルなし — 能力スコアを推測で埋めない）\n".into();
        }
    }

    Ok(sections)
}

/// Render the user's self-reported SETTINGS basics into a compact prompt section.
///
/// Returns `""` when nothing is set, so the caller omits the section entirely
/// rather than emitting an empty header. Empty / whitespace-only values are
/// skipped. These are **declared** facts (settings), NOT measured Tensor/Gap
/// values — the trailing note keeps the model from laundering self-report into
/// authoritative scores (F-19 / LLM-authority boundary).
pub fn format_profile_block(profile: &BTreeMap<String, String>) -> String {
    let mut lines: Vec<String> = Vec::new();
    let mut seen: Vec<&str> = Vec::new();
    for (key, label) in PROFILE_FIELD_LABELS {
        if let Some(v) = profile.get(*key) {
            let v = v.trim();
            if !v.is_empty() {
                lines.push(format!("- {label}: {v}"));
                seen.push(*key);
            }
        }
    }
    // Any extra non-empty keys the FE sent that are outside the known set
    // (BTreeMap iteration is key-sorted → deterministic order).
    for (key, value) in profile {
        if seen.contains(&key.as_str()) {
            continue;
        }
        let v = value.trim();
        if !v.is_empty() {
            lines.push(format!("- {key}: {v}"));
        }
    }
    if lines.is_empty() {
        return String::new();
    }
    let mut out = String::from("## ユーザー基本情報（自己申告・参考）\n");
    out.push_str(&lines.join("\n"));
    out.push('\n');
    out.push_str("※自己申告データ。測定済みTensor/Gapと混同せず、事実確認の手がかりとして扱え。\n");
    truncate(&out, PROFILE_SECTION_TOKEN_BUDGET)
}

/// Mentor consult prompt: ZPD preamble + profile + gap + tensor + oracle + optional RAG + user message.
///
/// `zpd` selects the Vygotsky / Yerkes–Dodson / Bjork preamble (see `mentor_zpd`).
/// `profile_block` is the pre-formatted [`format_profile_block`] output (or `""`).
pub fn build_consult_with_oracle_prompt(
    message: &str,
    mentor: &MentorContextSections,
    rag_block: &str,
    zpd: &MentorZpdSignal,
    profile_block: &str,
) -> String {
    let mut out = String::with_capacity(message.len() + 2048);
    out.push_str(zpd.level.preamble());
    if !profile_block.is_empty() {
        out.push_str("\n\n");
        out.push_str(profile_block.trim_end());
    }
    out.push_str("\n\n## 主観×客観ギャップ（決定論・Vault）\n");
    out.push_str(&mentor.gap_block);
    out.push_str("\n## Tensorプロファイル（決定論・Vault）\n");
    out.push_str(&mentor.tensor_block);
    out.push_str("\n## Oracle予測（決定論・Vault）\n");
    out.push_str(&mentor.oracle_block);
    if !rag_block.is_empty() {
        out.push_str(
            "\n（参考情報には、過去セッションから抽出された記憶と日常記録が混在する。\n\
一体の人物像として解釈し、記録に無いことを補完するな。）\n",
        );
        out.push_str("\n");
        out.push_str(rag_block);
    }
    out.push_str("\n## ユーザーの相談\n");
    out.push_str(message.trim());
    out.push('\n');
    out
}

/// Compact blocks for appending onto an existing RAG prompt (fail-safe).
pub fn append_mentor_sections(base_prompt: &str, mentor: &MentorContextSections) -> String {
    let mut out = String::with_capacity(base_prompt.len() + 1024);
    // Insert before "## ユーザーの質問" when present.
    if let Some(idx) = base_prompt.find("## ユーザーの質問") {
        out.push_str(&base_prompt[..idx]);
        out.push_str("## 主観×客観ギャップ（決定論・Vault）\n");
        out.push_str(&mentor.gap_block);
        out.push_str("\n## Tensorプロファイル（決定論・Vault）\n");
        out.push_str(&mentor.tensor_block);
        out.push_str("\n## Oracle予測（決定論・Vault）\n");
        out.push_str(&mentor.oracle_block);
        out.push('\n');
        out.push_str(&base_prompt[idx..]);
    } else {
        out.push_str(base_prompt);
        out.push_str("\n\n## 主観×客観ギャップ（決定論・Vault）\n");
        out.push_str(&mentor.gap_block);
        out.push_str("\n## Tensorプロファイル（決定論・Vault）\n");
        out.push_str(&mentor.tensor_block);
        out.push_str("\n## Oracle予測（決定論・Vault）\n");
        out.push_str(&mentor.oracle_block);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn append_inserts_before_user_question() {
        let base = "preamble\n\n## ユーザーの質問\nhello\n";
        let mentor = MentorContextSections {
            gap_block: "gap-line\n".into(),
            oracle_block: "oracle-line\n".into(),
            tensor_block: "tensor-line\n".into(),
            ..Default::default()
        };
        let p = append_mentor_sections(base, &mentor);
        let g = p.find("主観×客観ギャップ").unwrap();
        let t = p.find("Tensorプロファイル").unwrap();
        let u = p.find("ユーザーの質問").unwrap();
        assert!(g < t && t < u);
        assert!(p.contains("gap-line"));
        assert!(p.contains("tensor-line"));
    }

    fn profile(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    #[test]
    fn profile_block_empty_when_all_blank_or_missing() {
        assert_eq!(format_profile_block(&BTreeMap::new()), "");
        // present keys but only whitespace → still omitted
        let p = profile(&[("birthday", "  "), ("gender", "")]);
        assert_eq!(format_profile_block(&p), "");
    }

    #[test]
    fn profile_block_labels_and_skips_empty() {
        let p = profile(&[
            ("birthday", "1995-04-01"),
            ("gender", ""),
            ("occupation", " Acme Inc. "),
        ]);
        let block = format_profile_block(&p);
        assert!(block.contains("## ユーザー基本情報"));
        assert!(block.contains("- 誕生日: 1995-04-01"));
        assert!(block.contains("- 勤務先/学校: Acme Inc.")); // trimmed
        assert!(!block.contains("性別")); // empty value skipped
        assert!(block.contains("自己申告")); // authority-boundary note present
    }

    #[test]
    fn profile_block_renders_unknown_keys_after_known() {
        let p = profile(&[("nickname", "テスト"), ("birthday", "2000-01-01")]);
        let block = format_profile_block(&p);
        let known = block.find("誕生日").unwrap();
        let unknown = block.find("nickname").unwrap();
        assert!(known < unknown, "known fields precede extra keys");
    }

    #[test]
    fn consult_prompt_injects_profile_after_preamble_before_gap() {
        let mentor = MentorContextSections {
            gap_block: "gap-line\n".into(),
            ..Default::default()
        };
        let zpd = MentorZpdSignal::neutral_default();
        let block = format_profile_block(&profile(&[("birthday", "1990-12-31")]));
        let prompt =
            build_consult_with_oracle_prompt("相談内容", &mentor, "", &zpd, &block);
        // NOTE: the ZPD preamble prose itself mentions 「主観×客観ギャップ」 in
        // quotes, so match on the "## " section header (unique to the actual
        // block), not the bare phrase.
        let prof = prompt.find("ユーザー基本情報").expect("profile present");
        let gap = prompt
            .find("## 主観×客観ギャップ")
            .expect("gap section present");
        let msg = prompt.rfind("相談内容").expect("message present");
        assert!(prof < gap && gap < msg);
        assert!(prompt.contains("1990-12-31"));
    }

    #[test]
    fn consult_prompt_omits_profile_section_when_empty() {
        let mentor = MentorContextSections::default();
        let zpd = MentorZpdSignal::neutral_default();
        let prompt = build_consult_with_oracle_prompt("q", &mentor, "", &zpd, "");
        assert!(!prompt.contains("ユーザー基本情報"));
    }
}
