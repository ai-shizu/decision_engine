//! PromptSpec builder for on-device structured extraction (M5 Phase 1 / Phase 8 / Phase 11).
//!
//! Pure function: task_id + user text → (system_prompt, user_content).
//! No I/O, no model handle — chat-template application lives in `service`.

/// Task id for [`KakeiboEntryV1`](super::schema::KakeiboEntryV1) extraction.
pub const TASK_KAKEIBO_V1: &str = "kakeibo_v1";

/// Task id for CBT cognitive-distortion extraction (Beck 1976 / Burns 1980).
pub const TASK_COGNITIVE_DISTORTION_V1: &str = "cognitive_distortion_v1";

/// Task id for hierarchical receipt OCR extraction (Phase 11).
pub const TASK_RECEIPT_OCR_V1: &str = "receipt_ocr_v1";

/// Named grammar-constrained extraction tasks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LlmTaskId {
    KakeiboV1,
    /// Cognitive distortion fingerprint — ten Burns categories under GBNF.
    CognitiveDistortionV1,
    /// Hierarchical receipt (merchant / tax / total / lines).
    ReceiptOcrV1,
}

impl LlmTaskId {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::KakeiboV1 => TASK_KAKEIBO_V1,
            Self::CognitiveDistortionV1 => TASK_COGNITIVE_DISTORTION_V1,
            Self::ReceiptOcrV1 => TASK_RECEIPT_OCR_V1,
        }
    }

    pub fn parse(s: &str) -> Result<Self, String> {
        match s {
            TASK_KAKEIBO_V1 => Ok(Self::KakeiboV1),
            TASK_COGNITIVE_DISTORTION_V1 => Ok(Self::CognitiveDistortionV1),
            TASK_RECEIPT_OCR_V1 => Ok(Self::ReceiptOcrV1),
            other => Err(format!("unknown extraction task_id: {other}")),
        }
    }
}

const KAKEIBO_V1_SYSTEM: &str = "\
You extract a single Japanese household-ledger (家計簿) record from the user's text.
Output ONLY one JSON object with exactly these keys, in this order:
  date, amount, category, payee, memo
Rules:
- date: ISO calendar date \"YYYY-MM-DD\", or the literal \"unknown\" if not determined.
- amount: integer yen amount with no separators, or null if not determined.
- category, payee, memo: short Japanese or ASCII strings; use the literal \"unknown\" when unsure.
- Do not invent facts. Do not add extra keys. Do not wrap the JSON in markdown.";

const COGNITIVE_DISTORTION_V1_SYSTEM: &str = "\
You are a clinical-style cognitive-behavioral observer (Beck 1976; Burns 1980).
From the user's diary or chat text, detect cognitive distortions (irrational thought patterns).
Output ONLY one JSON object:
  {\"detected_distortions\":[{\"category\":...,\"snippet\":...,\"confidence_score\":...},...]}
category MUST be one of:
  all_or_nothing, overgeneralization, mental_filter, disqualifying_the_positive,
  jumping_to_conclusions, magnification_minimization, emotional_reasoning,
  should_statements, labeling, personalization
Rules:
- snippet: a short verbatim quote from the input that evidences the distortion (do not invent).
- confidence_score: number from 0 to 1.
- If none are clearly present, return {\"detected_distortions\":[]}.
- Do not add extra keys. Do not wrap the JSON in markdown. Do not diagnose disorders.";

const RECEIPT_OCR_V1_SYSTEM: &str = "\
You extract a Japanese retail receipt into a hierarchical JSON object.
Input may be Vision OCR layout text (item and amount often separated by a tab).
Output ONLY one JSON object with exactly these keys, in this order:
  merchant, occurred_at, tax, total, lines
Rules:
- merchant: store name string, or \"unknown\".
- occurred_at: ISO datetime/date string, unix epoch integer as a JSON number is also allowed via grammar string/int, or \"unknown\".
- tax, total: integer yen (no commas). tax may be 0 if not printed.
- lines: array of {item_name, unit_price, qty, amount} with integer money fields.
- Prefer amount as printed line total. If qty unknown use 1. If unit_price unknown use amount.
- Do not invent items. Do not add extra keys. Do not wrap JSON in markdown.
- Arithmetic consistency is verified later; do not retry or invent balancing tax.";

/// Build `(system_prompt, user_content)` for a named extraction task.
///
/// Returns `Err` for unknown `task_id` so callers fail closed instead of
/// silently falling back to an empty system prompt.
pub fn build_prompt(task_id: &str, input: &str) -> Result<(String, String), String> {
    match LlmTaskId::parse(task_id)? {
        LlmTaskId::KakeiboV1 => Ok((KAKEIBO_V1_SYSTEM.to_string(), input.to_string())),
        LlmTaskId::CognitiveDistortionV1 => Ok((
            COGNITIVE_DISTORTION_V1_SYSTEM.to_string(),
            input.to_string(),
        )),
        LlmTaskId::ReceiptOcrV1 => Ok((RECEIPT_OCR_V1_SYSTEM.to_string(), input.to_string())),
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
    fn cognitive_distortion_v1_mentions_burns_categories() {
        let (sys, user) = build_prompt(TASK_COGNITIVE_DISTORTION_V1, "全部ダメだ").unwrap();
        assert!(sys.contains("all_or_nothing"));
        assert!(sys.contains("Beck") || sys.contains("Burns"));
        assert_eq!(user, "全部ダメだ");
    }

    #[test]
    fn receipt_ocr_v1_mentions_hierarchy() {
        let (sys, user) = build_prompt(TASK_RECEIPT_OCR_V1, "牛乳\t120").unwrap();
        assert!(sys.contains("merchant"));
        assert!(sys.contains("lines"));
        assert!(sys.contains("tax"));
        assert_eq!(user, "牛乳\t120");
    }

    #[test]
    fn unknown_task_id_errors() {
        assert!(build_prompt("nope", "x").is_err());
    }

    #[test]
    fn llm_task_id_round_trip() {
        assert_eq!(
            LlmTaskId::parse(LlmTaskId::CognitiveDistortionV1.as_str()).unwrap(),
            LlmTaskId::CognitiveDistortionV1
        );
        assert_eq!(
            LlmTaskId::parse(LlmTaskId::ReceiptOcrV1.as_str()).unwrap(),
            LlmTaskId::ReceiptOcrV1
        );
    }
}
