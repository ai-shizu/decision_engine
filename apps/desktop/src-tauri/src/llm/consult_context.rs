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

use serde_json::Value;

use crate::analytics::oracle::render_oracle_consult;
use crate::db::{VaultErrorCode, VaultHandle};
use crate::llm::mentor_zpd::MentorZpdSignal;

use crate::llm::context_budget::truncate_to_token_budget;

/// Token budgets for mentor sections (Phase 9; replaces char-tail truncate).
const GAP_SECTION_TOKEN_BUDGET: usize = 625;
const ORACLE_SECTION_TOKEN_BUDGET: usize = 375;
const TENSOR_SECTION_TOKEN_BUDGET: usize = 300;

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

/// Mentor consult prompt: ZPD preamble + gap + tensor + oracle + optional RAG + user message.
///
/// `zpd` selects the Vygotsky / Yerkes–Dodson / Bjork preamble (see `mentor_zpd`).
pub fn build_consult_with_oracle_prompt(
    message: &str,
    mentor: &MentorContextSections,
    rag_block: &str,
    zpd: &MentorZpdSignal,
) -> String {
    let mut out = String::with_capacity(message.len() + 2048);
    out.push_str(zpd.level.preamble());
    out.push_str("\n\n## 主観×客観ギャップ（決定論・Vault）\n");
    out.push_str(&mentor.gap_block);
    out.push_str("\n## Tensorプロファイル（決定論・Vault）\n");
    out.push_str(&mentor.tensor_block);
    out.push_str("\n## Oracle予測（決定論・Vault）\n");
    out.push_str(&mentor.oracle_block);
    if !rag_block.is_empty() {
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
}
