//! Adversarial corpus + `is_numeric` tripwire (F-1).
//!
//! MUST_REJECT vectors are committed **before** the F-2 detector so the
//! detector cannot be fitted to a post-hoc corpus (SPEC §4.5 / FLV-W-05).
//! Invisible code points MUST be written with `\u{...}` escapes — never
//! as literal glyphs in source.

/// Specimens that MUST be rejected once the F-2 guard lands.
///
/// Covering SPEC §4.3 families: half/full-width Arabic, scientific, kanji
/// digits, daiji, positional, transcription, mixed, kana numerals, loan
/// numerals, counters, ordinals, ratios, proportions, fractions, currency,
/// ranges, bounds, approx, vague quantity, relative change, and
/// number-free quantity claims (`無料` / `両名` / …).
pub const MUST_REJECT: &[&str] = &[
    // half-width Arabic
    "損失は2500円相当だった。",
    // full-width Arabic
    "損失は２５００円相当だった。",
    // scientific
    "係数は1.5e3まで膨らんだ。",
    // kanji digits (positional)
    "損失は二千五百に達した。",
    // daiji
    "計上は壱万円を超えた。",
    // transcription (kanji zero forms)
    "年号は二〇二六と記された。",
    // mixed arabic+kanji
    "損失は2千を超えた。",
    "残高は1万5千まで減った。",
    // kana numerals
    "残響はゼロになった。",
    "賛同者はふたりだけだった。",
    // loan numerals
    "まとめてダース単位で落ちた。",
    "利益はハーフに削られた。",
    // counters / ordinals
    "三人が同時に手を挙げた。",
    "失敗は三つ重なった。",
    "第3の波が来た。",
    "五番目の合図だった。",
    // ratios / proportions / fractions
    "五割が消えた。",
    "三割五分まで戻した。",
    "半分が蒸発した。",
    "比は3対2に開いた。",
    "圧力は二倍になった。",
    "確率は二分の一を切った。",
    // currency / range / bound
    "費用は¥3,000を超えた。",
    "期間は3〜5日に及んだ。",
    "水準は3以上を要求した。",
    // approx / vague / relative
    "約3の余波が残った。",
    "十数の兆候が並んだ。",
    "数倍に膨らんだ緊張。",
    "大幅な揺らぎが走った。",
    "損失は倍増した。",
    "利益は半減した。",
    // number-free quantity claims
    "参加費は無料だった。",
    "現場は無人だった。",
    "両名が沈黙した。",
    "ペアで撤退した。",
    // invisible-char smuggling (MUST use escapes — FLV-W-05)
    "料金は2\u{200B}500円です。",
    "残高は1\u{FEFF}000を切った。",
    "比は3\u{200C}対2に開いた。",
];

/// Specimens that MUST be accepted once the F-2 guard lands.
///
/// F-1 skeleton rejects everything (`verify` → `None`), so these stay
/// `#[ignore]` until F-2.
pub const MUST_ACCEPT: &[&str] = &[
    "市場の空気が張り詰めた。",
    "決断の余韻がアリーナに残った。",
    "緊張が波紋のように広がった。",
    "静かな決着の気配が漂う。",
    "視線が交差し、間が生まれた。",
    "勢いがふっと削がれた瞬間。",
    "場の温度が一気に変わった。",
    "迷いが輪郭を帯び始めた。",
    "勝ち筋が霞の向こうに揺れる。",
    "呼吸を整え、次の手を待つ。",
];

#[cfg(test)]
mod corpus_tests {
    use super::*;
    use crate::flavor::policy::FlavorPolicy;
    use crate::flavor::verified::VerifiedFlavor;

    #[test]
    fn f1_skeleton_rejects_entire_must_reject_corpus() {
        let policy = FlavorPolicy::v1_empty();
        for (i, sample) in MUST_REJECT.iter().enumerate() {
            assert!(
                VerifiedFlavor::verify(sample, &policy).is_none(),
                "MUST_REJECT[{i}] unexpectedly accepted: {sample:?}"
            );
        }
        assert!(
            MUST_REJECT.len() >= 20,
            "MUST_REJECT must have >= 20 vectors, got {}",
            MUST_REJECT.len()
        );
    }

    #[test]
    #[ignore = "F-2 でガード実装時に解除"]
    fn f2_guard_must_accept_clean_flavor() {
        let policy = FlavorPolicy::v1_empty();
        for (i, sample) in MUST_ACCEPT.iter().enumerate() {
            assert!(
                VerifiedFlavor::verify(sample, &policy).is_some(),
                "MUST_ACCEPT[{i}] rejected: {sample:?}"
            );
        }
        assert_eq!(MUST_ACCEPT.len(), 10);
    }
}

/// Trap FLV-W-04 / 罠1: `char::is_numeric()` is necessary but **not
/// sufficient**. Han/kana numeral tables must be combined with it.
///
/// `is_numeric()` returns true only for General_Category **N*** (Nd/Nl/No).
/// Common Japanese kanji numerals are `Lo` and silently pass a naive filter
/// while rare forms (`②` `Ⅳ` `½` `٢`) are caught — the worst possible shape
/// for a detector.
///
/// `is_numeric()` は必要だが不十分な網であり、明示的な漢字・仮名テーブルとの併用が必須。
#[cfg(test)]
mod is_numeric_tripwire {
    /// Independently re-measured characters (Gate D / FLV-W-04).
    const CASES: &[(char, bool)] = &[
        ('一', false),
        ('二', false),
        ('十', false),
        ('壱', false),
        ('零', false),
        ('〇', true),
        ('Ⅳ', true),
        ('②', true),
        ('½', true),
        ('٢', true),
        ('२', true),
        ('5', true),
        ('５', true),
    ];

    #[test]
    fn char_is_numeric_misses_kanji_numerals_but_catches_nl_no_nd() {
        for &(ch, expected) in CASES {
            assert_eq!(
                ch.is_numeric(),
                expected,
                "char::is_numeric() for {ch:?} (U+{:04X}) expected {expected}, got {}",
                ch as u32,
                ch.is_numeric()
            );
        }
        // Explicit anchors required by Gate D acceptance criteria.
        assert!(!'一'.is_numeric());
        assert!('〇'.is_numeric());
        assert!('②'.is_numeric());
    }
}
