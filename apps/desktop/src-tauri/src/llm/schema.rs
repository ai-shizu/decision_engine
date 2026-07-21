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

/// Static GBNF grammar for [`CognitiveDistortionReportV1`].
pub const COGNITIVE_DISTORTION_V1_GBNF: &str =
    include_str!("assets/cognitive_distortion_v1.gbnf");

/// Static GBNF grammar for [`ReceiptOcrV1`].
pub const RECEIPT_OCR_V1_GBNF: &str = include_str!("assets/receipt_ocr_v1.gbnf");


// ─── Phase 8: Cognitive distortion extraction (Beck 1976 / Burns 1980) ───────

/// Burns (1980) / Beck (1976) ten cognitive distortions.
///
/// CBT holds that depression and anxiety are maintained by habitual irrational
/// thought patterns ("cognitive distortions"). These ten labels are the
/// canonical inventory used for fingerprinting — detection is LLM-assisted
/// under GBNF; aggregation in `bias_profile` remains deterministic.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DistortionCategory {
    /// All-or-Nothing — 全か無か思考
    AllOrNothing,
    /// Overgeneralization — 過度の一般化
    Overgeneralization,
    /// Mental Filter — 心のフィルター
    MentalFilter,
    /// Disqualifying the Positive — マイナス化思考
    DisqualifyingThePositive,
    /// Jumping to Conclusions — 結論の飛躍（心の読みすぎ / 先読み）
    JumpingToConclusions,
    /// Magnification / Minimization — 拡大解釈・過小評価（破局視含む）
    MagnificationMinimization,
    /// Emotional Reasoning — 感情的決めつけ
    EmotionalReasoning,
    /// Should Statements — すべき思考
    ShouldStatements,
    /// Labeling — レッテル貼り
    Labeling,
    /// Personalization — 自己関連づけ
    Personalization,
}

impl DistortionCategory {
    /// Fixed Burns order (radar / table axes). Deterministic — no HashMap iteration order.
    pub const ALL: [Self; 10] = [
        Self::AllOrNothing,
        Self::Overgeneralization,
        Self::MentalFilter,
        Self::DisqualifyingThePositive,
        Self::JumpingToConclusions,
        Self::MagnificationMinimization,
        Self::EmotionalReasoning,
        Self::ShouldStatements,
        Self::Labeling,
        Self::Personalization,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::AllOrNothing => "all_or_nothing",
            Self::Overgeneralization => "overgeneralization",
            Self::MentalFilter => "mental_filter",
            Self::DisqualifyingThePositive => "disqualifying_the_positive",
            Self::JumpingToConclusions => "jumping_to_conclusions",
            Self::MagnificationMinimization => "magnification_minimization",
            Self::EmotionalReasoning => "emotional_reasoning",
            Self::ShouldStatements => "should_statements",
            Self::Labeling => "labeling",
            Self::Personalization => "personalization",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "all_or_nothing" => Some(Self::AllOrNothing),
            "overgeneralization" => Some(Self::Overgeneralization),
            "mental_filter" => Some(Self::MentalFilter),
            "disqualifying_the_positive" => Some(Self::DisqualifyingThePositive),
            "jumping_to_conclusions" => Some(Self::JumpingToConclusions),
            "magnification_minimization" => Some(Self::MagnificationMinimization),
            "emotional_reasoning" => Some(Self::EmotionalReasoning),
            "should_statements" => Some(Self::ShouldStatements),
            "labeling" => Some(Self::Labeling),
            "personalization" => Some(Self::Personalization),
            _ => None,
        }
    }
}

/// One detected distortion span from constrained extraction.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DistortionDetectionV1 {
    pub category: DistortionCategory,
    pub snippet: String,
    pub confidence_score: f64,
}

/// Grammar-constrained CBT extraction report.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CognitiveDistortionReportV1 {
    pub detected_distortions: Vec<DistortionDetectionV1>,
}

impl CognitiveDistortionReportV1 {
    pub fn normalize(&mut self) {
        // Keep Burns inventory live in non-test builds (Zero Warnings / no allow).
        let _burns_n = DistortionCategory::ALL.len();
        debug_assert_eq!(_burns_n, 10);
        for d in &mut self.detected_distortions {
            let label = d.category.as_str();
            let _roundtrip = DistortionCategory::parse(label);
            debug_assert_eq!(_roundtrip, Some(d.category));
            d.snippet = truncate_snippet(d.snippet.trim());
            if !d.confidence_score.is_finite() {
                d.confidence_score = 0.0;
            }
            d.confidence_score = d.confidence_score.clamp(0.0, 1.0);
        }
        // Drop empty snippets (gap safety — do not invent quotes).
        self.detected_distortions
            .retain(|d| !d.snippet.is_empty() && d.snippet != UNKNOWN);
    }

    pub fn from_json_str(raw: &str) -> Result<Self, serde_json::Error> {
        let mut report: Self = serde_json::from_str(raw)?;
        report.normalize();
        Ok(report)
    }
}

fn truncate_snippet(s: &str) -> String {
    const MAX: usize = 280;
    if s.len() <= MAX {
        return s.to_string();
    }
    let mut end = MAX;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}…", &s[..end])
}


// ─── Phase 11: Hierarchical receipt OCR extraction ───────────────────────────

/// One receipt line item — all money fields are integers (yen).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReceiptLineV1 {
    pub item_name: String,
    pub unit_price: i64,
    pub qty: i64,
    pub amount: i64,
}

/// Grammar-constrained hierarchical receipt extract.
///
/// Checksum gate (deterministic, no LLM retry):
/// `sum(line.amount) + tax == total` → verified; else fail-closed verified=0.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReceiptOcrV1 {
    pub merchant: String,
    /// ISO datetime / date string, unix integer as string, or `"unknown"`.
    pub occurred_at: String,
    pub tax: i64,
    pub total: i64,
    pub lines: Vec<ReceiptLineV1>,
}

impl ReceiptOcrV1 {
    pub fn normalize(&mut self) {
        self.merchant = truncate_snippet(self.merchant.trim());
        if self.merchant.is_empty() {
            self.merchant = UNKNOWN.to_string();
        }
        self.occurred_at = self.occurred_at.trim().to_string();
        if self.occurred_at.is_empty() {
            self.occurred_at = UNKNOWN.to_string();
        }
        if self.tax < 0 {
            self.tax = 0;
        }
        if self.total < 0 {
            self.total = 0;
        }
        for line in &mut self.lines {
            line.item_name = truncate_snippet(line.item_name.trim());
            if line.item_name.is_empty() {
                line.item_name = UNKNOWN.to_string();
            }
            if line.qty <= 0 {
                line.qty = 1;
            }
            if line.unit_price < 0 {
                line.unit_price = 0;
            }
            if line.amount < 0 {
                line.amount = 0;
            }
        }
        self.lines.retain(|l| l.item_name != UNKNOWN || l.amount > 0);
    }

    pub fn from_json_str(raw: &str) -> Result<Self, serde_json::Error> {
        let mut report: Self = serde_json::from_str(raw)?;
        report.normalize();
        Ok(report)
    }

    /// Deterministic checksum: Σ line.amount + tax == total (integer arithmetic).
    pub fn checksum_ok(&self) -> bool {
        let mut sum: i64 = 0;
        for line in &self.lines {
            match sum.checked_add(line.amount) {
                Some(v) => sum = v,
                None => return false,
            }
        }
        match sum.checked_add(self.tax) {
            Some(v) => v == self.total,
            None => false,
        }
    }
}


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

    #[test]
    fn cognitive_distortion_report_parses_and_clamps() {
        let raw = r#"{
          "detected_distortions": [
            {"category":"all_or_nothing","snippet":"いつも失敗する","confidence_score":1.5},
            {"category":"should_statements","snippet":"","confidence_score":0.8}
          ]
        }"#;
        let r = CognitiveDistortionReportV1::from_json_str(raw).expect("parse");
        assert_eq!(r.detected_distortions.len(), 1);
        assert_eq!(
            r.detected_distortions[0].category,
            DistortionCategory::AllOrNothing
        );
        assert!((r.detected_distortions[0].confidence_score - 1.0).abs() < 1e-9);
    }

    #[test]
    fn cognitive_gbnf_lists_all_ten_categories() {
        for cat in DistortionCategory::ALL {
            assert!(
                COGNITIVE_DISTORTION_V1_GBNF.contains(cat.as_str()),
                "missing {}",
                cat.as_str()
            );
        }
        assert!(COGNITIVE_DISTORTION_V1_GBNF.contains("detected_distortions"));
    }

    #[test]
    fn distortion_category_round_trip() {
        for cat in DistortionCategory::ALL {
            assert_eq!(DistortionCategory::parse(cat.as_str()), Some(cat));
        }
    }

    #[test]
    fn receipt_checksum_gate() {
        let ok = ReceiptOcrV1 {
            merchant: "店".into(),
            occurred_at: "unknown".into(),
            tax: 100,
            total: 1100,
            lines: vec![ReceiptLineV1 {
                item_name: "牛乳".into(),
                unit_price: 500,
                qty: 2,
                amount: 1000,
            }],
        };
        assert!(ok.checksum_ok());
        let bad = ReceiptOcrV1 {
            total: 999,
            ..ok.clone()
        };
        assert!(!bad.checksum_ok());
    }

    #[test]
    fn receipt_gbnf_mentions_hierarchy() {
        assert!(RECEIPT_OCR_V1_GBNF.contains("merchant"));
        assert!(RECEIPT_OCR_V1_GBNF.contains("lines"));
        assert!(RECEIPT_OCR_V1_GBNF.contains("unit_price"));
        assert!(RECEIPT_OCR_V1_GBNF.contains("tax"));
        assert!(RECEIPT_OCR_V1_GBNF.contains("total"));
    }

}
