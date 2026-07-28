//! Stage 3: numeral / lexical-quantity detection outside shielded spans.
//! L1 = `char::is_numeric()`, L2 = `HAN_NUMERALS`, L3 = tokens + `数`+counter.

use std::ops::Range;

use super::shield::is_shielded;
use super::tables::{HAN_NUMERALS, LEXICAL_QUANTITY, SUU_COUNTER_CHARS, SUU_COUNTER_MULTI};
use super::Finding;

/// Scan non-shielded spans for L1 / L2 / L3 quantity expressions.
pub(crate) fn scan(raw: &str, shields: &[Range<usize>], out: &mut Vec<Finding>) {
    let mut i = 0;
    while i < raw.len() {
        if is_shielded(i, shields) {
            let ch = raw[i..].chars().next().expect("char boundary");
            i += ch.len_utf8();
            continue;
        }

        // L3 first (multi-char longest match + 数+助数詞).
        if let Some(span) = longest_lexical_span(raw, i) {
            if !(span.start..span.end).any(|b| is_shielded(b, shields)) {
                out.push(Finding::LexicalQuantity { span: span.clone() });
                i = span.end;
                continue;
            }
        }

        let ch = raw[i..].chars().next().expect("char boundary");
        let len = ch.len_utf8();
        if is_numeral_char(ch) {
            if ch.is_numeric() {
                out.push(Finding::UnicodeNumeral { at: i, ch });
            } else {
                out.push(Finding::HanNumeral { at: i, ch });
            }
        }
        i += len;
    }
}

/// L1 ∨ L2. The forced pair in tests proves L2 carries the Han load.
pub(crate) fn is_numeral_char(c: char) -> bool {
    c.is_numeric() || HAN_NUMERALS.contains(&c)
}

fn longest_lexical_span(haystack: &str, byte_pos: usize) -> Option<Range<usize>> {
    let rest = &haystack[byte_pos..];
    let mut best_len: Option<usize> = None;

    for &n in LEXICAL_QUANTITY {
        if rest.starts_with(n) && best_len.is_none_or(|b| n.len() > b) {
            best_len = Some(n.len());
        }
    }

    if let Some(n) = suu_counter_len(rest) {
        if best_len.is_none_or(|b| n > b) {
            best_len = Some(n);
        }
    }

    best_len.map(|n| byte_pos..byte_pos + n)
}

/// Plan B: `数` + counter (multi then single). Returns matched byte length.
fn suu_counter_len(rest: &str) -> Option<usize> {
    if !rest.starts_with('数') {
        return None;
    }
    let after = &rest['数'.len_utf8()..];
    for &m in SUU_COUNTER_MULTI {
        if after.starts_with(m) {
            return Some('数'.len_utf8() + m.len());
        }
    }
    let next = after.chars().next()?;
    if SUU_COUNTER_CHARS.contains(&next) {
        return Some('数'.len_utf8() + next.len_utf8());
    }
    None
}

#[cfg(test)]
mod trap1_force {
    use super::is_numeral_char;

    /// Trap 1 forced: implementation **depends** on L2 for `一`.
    /// Removing `HAN_NUMERALS` entry for `一` drops both asserts together.
    #[test]
    fn is_numeral_char_loads_l2_for_han_one() {
        assert!(is_numeral_char('一')); // L2 carries the load
        assert!(!'一'.is_numeric()); // L1 alone misses it
    }
}

#[cfg(test)]
mod suu_boundary {
    use super::suu_counter_len;

    #[test]
    fn suu_counter_matches_数人_not_手数_or_数える() {
        assert_eq!(suu_counter_len("数人が離脱した。"), Some("数人".len()));
        assert_eq!(suu_counter_len("数が増える"), None); // 手数…
        assert_eq!(suu_counter_len("数える間もなく"), None);
        assert_eq!(suu_counter_len("数週間の余波"), Some("数週間".len()));
    }
}
