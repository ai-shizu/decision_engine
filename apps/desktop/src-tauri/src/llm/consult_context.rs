//! Mentor consult context: Gap (M14) + Oracle (M16) sections for prompt injection.
//!
//! Fail-safe: missing / ungated vault rows produce explicit "unavailable" notes
//! rather than errors. Never invent scores. Discussion-phase interview must NOT
//! call this (I-22); debrief/consult only.

use serde_json::Value;

use crate::analytics::oracle::render_oracle_consult;
use crate::db::{VaultErrorCode, VaultHandle};

const MENTOR_PREAMBLE: &str = "\
あなたは司令官の意思決定を支える冷徹なメンターである。\
一般論でごまかすな。下記の「主観×客観ギャップ」と「Oracle予測」に定量根拠がある場合は\
それを優先し、助言の自己検証を行い、行動可能な次手を1〜3個に絞れ。\
データが不足と明示されている場合は推測で埋めず、観測継続を促せ。";

const GAP_SECTION_BUDGET: usize = 2_500;
const ORACLE_SECTION_BUDGET: usize = 1_500;

#[derive(Debug, Clone, Default)]
pub struct MentorContextSections {
    pub gap_block: String,
    pub oracle_block: String,
    pub gap_available: bool,
    pub oracle_available: bool,
    pub gap_run_id: Option<String>,
    pub oracle_run_id: Option<String>,
}

fn map_vault(err: VaultErrorCode) -> String {
    format!("{err:?}").to_ascii_lowercase()
}

fn truncate(s: &str, budget: usize) -> String {
    if s.len() <= budget {
        return s.to_string();
    }
    let mut end = budget;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}…", &s[..end])
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
    truncate(&out, GAP_SECTION_BUDGET)
}

/// Load latest gap + oracle from vault. Errors only on vault transport failure;
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
                sections.oracle_block = truncate(&rendered, ORACLE_SECTION_BUDGET);
                sections.oracle_available = true;
            }
        }
        None => {
            sections.oracle_block =
                "（Oracle: Vault にペイロードなし — 未来予測に依存するな）\n".into();
        }
    }

    Ok(sections)
}

/// Mentor consult prompt: preamble + gap + oracle + optional RAG + user message.
pub fn build_consult_with_oracle_prompt(
    message: &str,
    mentor: &MentorContextSections,
    rag_block: &str,
) -> String {
    let mut out = String::with_capacity(message.len() + 2048);
    out.push_str(MENTOR_PREAMBLE);
    out.push_str("\n\n## 主観×客観ギャップ（決定論・Vault）\n");
    out.push_str(&mentor.gap_block);
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
        out.push_str("\n## Oracle予測（決定論・Vault）\n");
        out.push_str(&mentor.oracle_block);
        out.push('\n');
        out.push_str(&base_prompt[idx..]);
    } else {
        out.push_str(base_prompt);
        out.push_str("\n\n## 主観×客観ギャップ（決定論・Vault）\n");
        out.push_str(&mentor.gap_block);
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
            ..Default::default()
        };
        let p = append_mentor_sections(base, &mentor);
        let g = p.find("主観×客観ギャップ").unwrap();
        let u = p.find("ユーザーの質問").unwrap();
        assert!(g < u);
        assert!(p.contains("gap-line"));
    }
}
