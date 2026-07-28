//! Frozen detection tables (F-2). Three layers — do not conflate L2 and L3.
//!
//! - **L1** is `char::is_numeric()` (Nd/Nl/No) — not stored here.
//! - **L2** Han / daiji / positional digits that are General_Category `Lo`
//!   (missed by L1). Single-character membership only.
//! - **L3** Multi-character lexical quantity tokens (longest match).
//!
//! **Never** put `無` `大` `半` `数` `両` in L2 — they are ubiquitous
//! non-quantity morphemes. That conflation is the silent-death mode of §0.

/// L2: Han numerals / daiji / positional markers (`Lo` — L1 misses these).
///
/// Does **not** include `無` `大` `半` `数` `両`.
pub(crate) const HAN_NUMERALS: &[char] = &[
    '一', '二', '三', '四', '五', '六', '七', '八', '九', '十', '百', '千', '万', '億', '兆',
    '零', //
    '壱', '壹', '弐', '貳', '参', '參', '肆', '伍', '陸', '漆', '柒', '捌', '玖', '拾', '佰',
    '仟', '萬',
];

/// L3: lexical quantity tokens. Longest-match; never single ambiguous morphemes.
///
/// Sorted longest-first so a linear scan prefers the longer terminal.
pub(crate) const LEXICAL_QUANTITY: &[&str] = &[
    "過半数", // 3
    "ひとつ",
    "ふたり",
    "ダース",
    "ハーフ",
    "大幅",
    "倍増",
    "半減",
    "数倍",
    "半分",
    "無料",
    "無人",
    "両名",
    "ゼロ",
    "ペア",
];

/// Audited exception terminals for policy edition 1 (grammar terminals).
///
/// Consumed left-to-right longest-match **before** numeral scan (shield stage).
/// Ambiguous forms (`十分` / `一部` / bare `一方` / …) are intentionally absent.
pub(crate) const AUDITED_V1: &[&str] = &[
    "一石二鳥",
    "四苦八苦",
    "三日坊主",
    "十人十色",
    "千差万別",
    "四半期",
    "万が一",
    "一気に",
    "一方で",
    "一方、",
    "一気",
];
