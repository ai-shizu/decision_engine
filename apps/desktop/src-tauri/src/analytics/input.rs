//! Input DTOs for deterministic gap analysis (no diary/LINE loaders).

use serde::{Deserialize, Serialize};

/// Hard cap on days accepted per calculate call (Jetsam / CPU bound).
pub const MAX_DAILY_DAYS: usize = 120;
pub const MAX_TEXT_BYTES: usize = 64 * 1024;

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConsultationIn {
    pub query: String,
    #[serde(default)]
    pub is_simulated_persona: bool,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TransactionIn {
    /// `"expense"` or `"income"` — only expense counts toward objective money.
    #[serde(rename = "type")]
    pub tx_type: String,
    pub category: String,
    pub amount: i64,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CalendarEventIn {
    pub title: String,
}

/// One calendar day of subjective + objective evidence.
///
/// **LINE self-speech stays objective** — never merge into diary/consultations.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AnalyticsDailyDay {
    pub date: String,
    #[serde(default)]
    pub diary_text: String,
    #[serde(default)]
    pub consultations: Vec<ConsultationIn>,
    #[serde(default)]
    pub transactions: Vec<TransactionIn>,
    #[serde(default)]
    pub calendar_events: Vec<CalendarEventIn>,
    /// Objective axis only (AI_SKILLS §6.2).
    #[serde(default)]
    pub line_self_text: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CalculateGapRequest {
    pub days: Vec<AnalyticsDailyDay>,
}

pub fn validate_request(req: &CalculateGapRequest) -> Result<(), String> {
    if req.days.is_empty() {
        return Err("empty days".into());
    }
    if req.days.len() > MAX_DAILY_DAYS {
        return Err("too many days".into());
    }
    for day in &req.days {
        if day.date.trim().len() != 10 {
            return Err("invalid date".into());
        }
        if day.diary_text.len() > MAX_TEXT_BYTES
            || day.line_self_text.len() > MAX_TEXT_BYTES
        {
            return Err("text too large".into());
        }
    }
    Ok(())
}
