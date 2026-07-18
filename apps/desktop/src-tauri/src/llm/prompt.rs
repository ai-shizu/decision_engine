//! PromptSpec builder for on-device structured extraction (M5 Phase 1).
//!
//! Pure function: task_id + user text → (system_prompt, user_content).
//! No I/O, no model handle — chat-template application lives in `service`.

/// Task id for [`KakeiboEntryV1`](super::schema::KakeiboEntryV1) extraction.
pub const TASK_KAKEIBO_V1: &str = "kakeibo_v1";

const KAKEIBO_V1_SYSTEM: &str = "\
You extract a single Japanese household-ledger (家計簿) record from the user's text.
Output ONLY one JSON object with exactly these keys, in this order:
  date, amount, category, payee, memo
Rules:
- date: ISO calendar date \"YYYY-MM-DD\", or the literal \"unknown\" if not determined.
- amount: integer yen amount with no separators, or null if not determined.
- category, payee, memo: short Japanese or ASCII strings; use the literal \"unknown\" when unsure.
- Do not invent facts. Do not add extra keys. Do not wrap the JSON in markdown.";

/// Build `(system_prompt, user_content)` for a named extraction task.
///
/// Returns `Err` for unknown `task_id` so callers fail closed instead of
/// silently falling back to an empty system prompt.
pub fn build_prompt(task_id: &str, input: &str) -> Result<(String, String), String> {
    match task_id {
        TASK_KAKEIBO_V1 => Ok((KAKEIBO_V1_SYSTEM.to_string(), input.to_string())),
        other => Err(format!("unknown extraction task_id: {other}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kakeibo_v1_returns_system_and_user() {
        let (sys, user) = build_prompt(TASK_KAKEIBO_V1, "昨日スーパーで1200円").unwrap();
        assert!(sys.contains("家計簿") || sys.contains("household-ledger"));
        assert!(sys.contains("unknown"));
        assert!(sys.contains("YYYY-MM-DD"));
        assert_eq!(user, "昨日スーパーで1200円");
    }

    #[test]
    fn unknown_task_id_errors() {
        assert!(build_prompt("nope", "x").is_err());
    }
}
