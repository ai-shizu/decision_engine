//! Frozen detection tables (F-2 / F-2b). Three layers — do not conflate L2 and L3.
//!
//! - **L1** is `char::is_numeric()` (Nd/Nl/No) — not stored here.
//! - **L2** Han / daiji / positional digits that are General_Category `Lo`
//!   (missed by L1). Single-character membership only.
//! - **L3** Multi-character lexical quantity tokens (longest match) plus the
//!   limited `数`+助数詞 generative rule (F-2b Plan B).
//!
//! **Never** put `無` `大` `半` `数` `両` in L2 — they are ubiquitous
//! non-quantity morphemes. That conflation is the silent-death mode of §0.
//!
//! # Audited exception principle (F-2b)
//!
//! **ある語が数量読みを持ち得るなら、表に載せない。**
//! 偽陽性は安価である —— 固定テンプレートへ落ちるだけだ。偽陰性は破滅的である
//! —— 捏造された数が計器の読みとしてユーザーに届く。
//! **迷ったら載せない。通過率のために境界を緩めない。**
//! `AUDITED_V1` は F-3 出荷前につき版のまま拡張してよい。**F-3 以降の変更は
//! 版番号の bump を要する。**

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
/// F-2b closes SPEC §4.3 gaps that the F-1 corpus alone did not force.
pub(crate) const LEXICAL_QUANTITY: &[&str] = &[
    "トリプル", // 4
    "過半数",   // 3
    "ひとつ",
    "ふたり",
    "ひとり",
    "みっつ",
    "よっつ",
    "いつつ",
    "ダース",
    "ハーフ",
    "ダブル",
    "わずか",
    "大幅",
    "倍増",
    "半減",
    "数倍",
    "半分",
    "無料",
    "無人",
    "両名",
    "大量",
    "大半",
    "多数",
    "少数",
    "ゼロ",
    "ペア",
    "ワン",
];

/// Plan B (F-2b): `数` immediately followed by a counter. Open set closed by
/// a small counter alphabet — not by putting bare `数` in L2.
///
/// Multi-char counters are tried first (`週間`). Single-char set must NOT
/// fire on `手数` (`数が`) or `数える` (`数え`).
pub(crate) const SUU_COUNTER_MULTI: &[&str] = &["週間"];

pub(crate) const SUU_COUNTER_CHARS: &[char] = &[
    '人', '日', '回', '件', '個', '台', '枚', '年', '月', '週', '間', '度',
];

/// Audited exception terminals for policy edition 1 (grammar terminals).
///
/// Consumed left-to-right longest-match **before** numeral scan (shield stage).
///
/// # Deliberately NOT listed (F-2b §3.2 — do not "fix" these in)
///
/// - **一層**: `層` is a counter. Gate G correctly rejected `三層`; exempting
///   only `一層` would be ad hoc, not a rule. Readings `いっそう` / `ひとそう`
///   are the same ambiguity class as `十分`.
/// - **一体**: `体` is a counter for statues / corpses (`一体の像`).
/// - **一部**: SPEC §4.4 explicit ban — partitivity + counter reading.
/// - **一定**: `一定の水準` / `一定量` are quantitative claims.
/// - **一律**: often a quantitative operation (`一律 5% 減`).
///
/// Ambiguous forms also absent: `十分` / bare `一方` / `十八番` / `二枚目`.
pub(crate) const AUDITED_V1: &[&str] = &[
    // Longest first among overlapping candidates.
    "一環として", // 5
    "ひとりでに", // 4 — pairs with L3 `ひとり` (quantity) vs adverb
    "一貫して",   // 4
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
    "一瞬",
    "一斉",
    "一切",
    "一見",
    "一応",
    "一連",
    "一致",
    "一気",
];
