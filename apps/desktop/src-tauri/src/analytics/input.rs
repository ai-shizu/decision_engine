//! Input DTOs for deterministic gap analysis (no diary/LINE loaders).

use serde::{Deserialize, Serialize};

/// Hard cap on days accepted per calculate call (Jetsam / CPU bound).
pub const MAX_DAILY_DAYS: usize = 120;
pub const MAX_TEXT_BYTES: usize = 64 * 1024;
pub const MAX_ITEMS_PER_DAY: usize = 512;
pub const MAX_LABEL_BYTES: usize = 4 * 1024;
pub const MAX_TOTAL_ITEMS: usize = 10_000;
pub const MAX_TOTAL_TEXT_BYTES: usize = 8 * 1024 * 1024;

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
    let mut total_expense = 0_i64;
    let mut total_items = 0_usize;
    let mut total_text_bytes = 0_usize;
    for day in &req.days {
        if day.date.trim().len() != 10 {
            return Err("invalid date".into());
        }
        if day.diary_text.len() > MAX_TEXT_BYTES
            || day.line_self_text.len() > MAX_TEXT_BYTES
        {
            return Err("text too large".into());
        }
        if day.consultations.len() > MAX_ITEMS_PER_DAY
            || day.transactions.len() > MAX_ITEMS_PER_DAY
            || day.calendar_events.len() > MAX_ITEMS_PER_DAY
        {
            return Err("too many daily items".into());
        }
        let daily_items = day
            .consultations
            .len()
            .checked_add(day.transactions.len())
            .and_then(|count| count.checked_add(day.calendar_events.len()))
            .ok_or_else(|| "item count overflow".to_string())?;
        total_items = total_items
            .checked_add(daily_items)
            .ok_or_else(|| "item count overflow".to_string())?;
        if total_items > MAX_TOTAL_ITEMS {
            return Err("too many total items".into());
        }

        if day
            .consultations
            .iter()
            .any(|consultation| consultation.query.len() > MAX_TEXT_BYTES)
            || day
                .calendar_events
                .iter()
                .any(|event| event.title.len() > MAX_LABEL_BYTES)
        {
            return Err("nested text too large".into());
        }
        let daily_text_bytes = day
            .diary_text
            .len()
            .checked_add(day.line_self_text.len())
            .and_then(|bytes| {
                day.consultations
                    .iter()
                    .try_fold(bytes, |sum, item| sum.checked_add(item.query.len()))
            })
            .and_then(|bytes| {
                day.calendar_events
                    .iter()
                    .try_fold(bytes, |sum, item| sum.checked_add(item.title.len()))
            })
            .and_then(|bytes| {
                day.transactions.iter().try_fold(bytes, |sum, item| {
                    sum.checked_add(item.category.len())
                })
            })
            .ok_or_else(|| "text size overflow".to_string())?;
        total_text_bytes = total_text_bytes
            .checked_add(daily_text_bytes)
            .ok_or_else(|| "text size overflow".to_string())?;
        if total_text_bytes > MAX_TOTAL_TEXT_BYTES {
            return Err("aggregate text too large".into());
        }

        for transaction in &day.transactions {
            if !matches!(transaction.tx_type.as_str(), "expense" | "income") {
                return Err("invalid transaction type".into());
            }
            if transaction.category.len() > MAX_LABEL_BYTES || transaction.amount < 0 {
                return Err("invalid transaction".into());
            }
            if transaction.tx_type == "expense" {
                total_expense = total_expense
                    .checked_add(transaction.amount)
                    .ok_or_else(|| "expense total overflow".to_string())?;
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn day_with_transactions(transactions: Vec<TransactionIn>) -> AnalyticsDailyDay {
        AnalyticsDailyDay {
            date: "2026-07-23".into(),
            diary_text: String::new(),
            consultations: Vec::new(),
            transactions,
            calendar_events: Vec::new(),
            line_self_text: String::new(),
        }
    }

    #[test]
    fn rejects_negative_expense() {
        let request = CalculateGapRequest {
            days: vec![day_with_transactions(vec![TransactionIn {
                tx_type: "expense".into(),
                category: "食費".into(),
                amount: -1,
            }])],
        };
        assert_eq!(validate_request(&request), Err("invalid transaction".into()));
    }

    #[test]
    fn rejects_expense_total_overflow() {
        let request = CalculateGapRequest {
            days: vec![day_with_transactions(vec![
                TransactionIn {
                    tx_type: "expense".into(),
                    category: "食費".into(),
                    amount: i64::MAX,
                },
                TransactionIn {
                    tx_type: "expense".into(),
                    category: "交通".into(),
                    amount: 1,
                },
            ])],
        };
        assert_eq!(
            validate_request(&request),
            Err("expense total overflow".into())
        );
    }

    #[test]
    fn rejects_aggregate_nested_text_above_process_budget() {
        let query = "x".repeat(MAX_TEXT_BYTES);
        let consultations = (0..(MAX_TOTAL_TEXT_BYTES / MAX_TEXT_BYTES + 1))
            .map(|_| ConsultationIn {
                query: query.clone(),
                is_simulated_persona: false,
            })
            .collect();
        let request = CalculateGapRequest {
            days: vec![AnalyticsDailyDay {
                date: "2026-07-23".into(),
                diary_text: String::new(),
                consultations,
                transactions: Vec::new(),
                calendar_events: Vec::new(),
                line_self_text: String::new(),
            }],
        };

        assert_eq!(
            validate_request(&request),
            Err("aggregate text too large".into())
        );
    }
}
