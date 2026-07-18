//! Kakeibo V1 extraction schema + deterministic normalization (M5 Phase 1).
//!
//! GBNF guarantees syntactically valid JSON; this module guarantees semantic
//! cleanup (fullwidth digits, thousands separators) and gap-safety (`"unknown"`).

use serde::de::{self, Deserializer, Visitor};
use serde::{Deserialize, Serialize};
use std::fmt;
use unicode_normalization::UnicodeNormalization;

/// Sentinel the model must emit when a string field cannot be determined.
pub const UNKNOWN: &str = "unknown";

/// Exact-schema kakeibo extraction record (G0-C.2 / M5).
///
/// Unknown string slots are the literal `"unknown"` (not empty, not null).
/// `amount` is `null` when unknown (Option::None).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KakeiboEntryV1 {
    pub date: String,
    #[serde(deserialize_with = "deserialize_amount")]
    pub amount: Option<i64>,
    pub category: String,
    pub payee: String,
    pub memo: String,
}

impl KakeiboEntryV1 {
    /// Deterministic post-parse cleanup. Safe to call after serde (and after a
    /// GBNF-constrained decode that already emitted numbers).
    pub fn normalize(&mut self) {
        self.date = normalize_date_field(&self.date);
        self.category = normalize_unknown_string(&self.category);
        self.payee = normalize_unknown_string(&self.payee);
        self.memo = normalize_unknown_string(&self.memo);
        // Amount is already Option<i64> after deserialize; re-parse is a no-op
        // unless a future path injects a string-shaped value through another
        // constructor. Keep amount as-is when present.
    }

    /// Parse JSON then normalize. Primary entry for the extraction pipeline.
    pub fn from_json_str(raw: &str) -> Result<Self, serde_json::Error> {
        let mut entry: Self = serde_json::from_str(raw)?;
        entry.normalize();
        Ok(entry)
    }
}

/// NFKC + strip thousands separators / currency marks, then parse as i64.
/// Returns `None` for empty / `"unknown"` / `"null"` / unparseable input.
pub fn parse_amount_token(raw: &str) -> Option<i64> {
    let nfkc: String = raw.nfkc().collect();
    let cleaned: String = nfkc
        .chars()
        .filter(|c| {
            !c.is_whitespace()
                && *c != ','
                && *c != '，'
                && *c != '¥'
                && *c != '￥'
                && *c != '円'
        })
        .collect();
    if cleaned.is_empty() {
        return None;
    }
    let lower = cleaned.to_ascii_lowercase();
    if lower == "unknown" || lower == "null" {
        return None;
    }
    cleaned.parse::<i64>().ok()
}

fn normalize_unknown_string(raw: &str) -> String {
    let nfkc: String = raw.nfkc().collect();
    let trimmed = nfkc.trim();
    if trimmed.is_empty() || trimmed.eq_ignore_ascii_case(UNKNOWN) {
        UNKNOWN.to_string()
    } else {
        trimmed.to_string()
    }
}

fn normalize_date_field(raw: &str) -> String {
    let nfkc: String = raw.nfkc().collect();
    let trimmed = nfkc.trim();
    if trimmed.is_empty() || trimmed.eq_ignore_ascii_case(UNKNOWN) {
        return UNKNOWN.to_string();
    }
    if is_iso_date(trimmed) {
        trimmed.to_string()
    } else {
        // Do not invent a date from free-form text — gap safety.
        UNKNOWN.to_string()
    }
}

fn is_iso_date(s: &str) -> bool {
    // Strict YYYY-MM-DD ASCII after NFKC.
    let b = s.as_bytes();
    if b.len() != 10 {
        return false;
    }
    b[4] == b'-' && b[7] == b'-'
        && b[0].is_ascii_digit()
        && b[1].is_ascii_digit()
        && b[2].is_ascii_digit()
        && b[3].is_ascii_digit()
        && b[5].is_ascii_digit()
        && b[6].is_ascii_digit()
        && b[8].is_ascii_digit()
        && b[9].is_ascii_digit()
}

fn deserialize_amount<'de, D>(deserializer: D) -> Result<Option<i64>, D::Error>
where
    D: Deserializer<'de>,
{
    struct AmountVisitor;

    impl<'de> Visitor<'de> for AmountVisitor {
        type Value = Option<i64>;

        fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
            f.write_str("null, an integer, or a numeric string")
        }

        fn visit_unit<E: de::Error>(self) -> Result<Self::Value, E> {
            Ok(None)
        }

        fn visit_none<E: de::Error>(self) -> Result<Self::Value, E> {
            Ok(None)
        }

        fn visit_some<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
        where
            D: Deserializer<'de>,
        {
            deserializer.deserialize_any(AmountVisitor)
        }

        fn visit_i64<E: de::Error>(self, v: i64) -> Result<Self::Value, E> {
            Ok(Some(v))
        }

        fn visit_u64<E: de::Error>(self, v: u64) -> Result<Self::Value, E> {
            i64::try_from(v)
                .map(Some)
                .map_err(|_| E::custom("amount exceeds i64 range"))
        }

        fn visit_str<E: de::Error>(self, v: &str) -> Result<Self::Value, E> {
            Ok(parse_amount_token(v))
        }

        fn visit_string<E: de::Error>(self, v: String) -> Result<Self::Value, E> {
            Ok(parse_amount_token(&v))
        }
    }

    deserializer.deserialize_any(AmountVisitor)
}

/// Static GBNF grammar for [`KakeiboEntryV1`] (embedded asset).
pub const KAKEIBO_V1_GBNF: &str = include_str!("assets/kakeibo_v1.gbnf");

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_clean_json_numeric_amount() {
        let raw = r#"{"date":"2026-07-18","amount":1500,"category":"食費","payee":"スーパー","memo":"弁当"}"#;
        let e = KakeiboEntryV1::from_json_str(raw).expect("parse");
        assert_eq!(e.date, "2026-07-18");
        assert_eq!(e.amount, Some(1500));
        assert_eq!(e.category, "食費");
        assert_eq!(e.payee, "スーパー");
        assert_eq!(e.memo, "弁当");
    }

    #[test]
    fn parse_null_amount_and_unknown_strings() {
        let raw = r#"{"date":"unknown","amount":null,"category":"unknown","payee":"unknown","memo":"unknown"}"#;
        let e = KakeiboEntryV1::from_json_str(raw).expect("parse");
        assert_eq!(e.date, UNKNOWN);
        assert_eq!(e.amount, None);
        assert_eq!(e.category, UNKNOWN);
        assert_eq!(e.payee, UNKNOWN);
        assert_eq!(e.memo, UNKNOWN);
    }

    #[test]
    fn deny_unknown_fields() {
        let raw = r#"{"date":"2026-01-01","amount":1,"category":"a","payee":"b","memo":"c","extra":true}"#;
        assert!(KakeiboEntryV1::from_json_str(raw).is_err());
    }

    #[test]
    fn amount_string_with_comma() {
        assert_eq!(parse_amount_token("1,000"), Some(1000));
        assert_eq!(parse_amount_token("12,345,678"), Some(12_345_678));
        let raw = r#"{"date":"2026-07-18","amount":"1,000","category":"食費","payee":"unknown","memo":"unknown"}"#;
        let e = KakeiboEntryV1::from_json_str(raw).expect("parse");
        assert_eq!(e.amount, Some(1000));
    }

    #[test]
    fn amount_fullwidth_digits() {
        assert_eq!(parse_amount_token("１０００"), Some(1000));
        assert_eq!(parse_amount_token("１,２３４"), Some(1234));
        let raw = r#"{"date":"2026-07-18","amount":"１０００","category":"交通","payee":"unknown","memo":"unknown"}"#;
        let e = KakeiboEntryV1::from_json_str(raw).expect("parse");
        assert_eq!(e.amount, Some(1000));
    }

    #[test]
    fn amount_yen_suffix_and_unknown() {
        assert_eq!(parse_amount_token("500円"), Some(500));
        assert_eq!(parse_amount_token("¥2,000"), Some(2000));
        assert_eq!(parse_amount_token("unknown"), None);
        assert_eq!(parse_amount_token(""), None);
    }

    #[test]
    fn normalize_blank_strings_to_unknown() {
        let mut e = KakeiboEntryV1 {
            date: "  ".into(),
            amount: None,
            category: "".into(),
            payee: "UNKNOWN".into(),
            memo: " メモ ".into(),
        };
        e.normalize();
        assert_eq!(e.date, UNKNOWN);
        assert_eq!(e.category, UNKNOWN);
        assert_eq!(e.payee, UNKNOWN);
        assert_eq!(e.memo, "メモ");
    }

    #[test]
    fn normalize_rejects_non_iso_date() {
        let mut e = KakeiboEntryV1 {
            date: "令和6年7月18日".into(),
            amount: Some(1),
            category: "食費".into(),
            payee: "a".into(),
            memo: "b".into(),
        };
        e.normalize();
        assert_eq!(e.date, UNKNOWN);
    }

    #[test]
    fn normalize_accepts_fullwidth_iso_date_via_nfkc() {
        let mut e = KakeiboEntryV1 {
            date: "２０２６-０７-１８".into(),
            amount: Some(1),
            category: "食費".into(),
            payee: "a".into(),
            memo: "b".into(),
        };
        e.normalize();
        assert_eq!(e.date, "2026-07-18");
    }

    #[test]
    fn gbnf_asset_is_non_empty_and_mentions_keys() {
        assert!(KAKEIBO_V1_GBNF.contains("date"));
        assert!(KAKEIBO_V1_GBNF.contains("amount"));
        assert!(KAKEIBO_V1_GBNF.contains("category"));
        assert!(KAKEIBO_V1_GBNF.contains("payee"));
        assert!(KAKEIBO_V1_GBNF.contains("memo"));
        assert!(KAKEIBO_V1_GBNF.contains("unknown"));
        assert!(!KAKEIBO_V1_GBNF.trim().is_empty());
    }
}
