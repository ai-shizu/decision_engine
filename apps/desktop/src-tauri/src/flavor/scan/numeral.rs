//! Stage 3: numeral / lexical-quantity detection outside shielded spans.
//! L1 = `char::is_numeric()`, L2 = `HAN_NUMERALS`, L3 = `LEXICAL_QUANTITY`.

use std::ops::Range;

use super::shield::is_shielded;
use super::tables::{HAN_NUMERALS, LEXICAL_QUANTITY};
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

        // L3 first (multi-char longest match).
        if let Some(tok) = longest_lexical_at(raw, i) {
            // Token must not start inside a shield (already ensured) and must
            // not extend into a shielded region.
            let end = i + tok.len();
            if !(i..end).any(|b| is_shielded(b, shields)) {
                out.push(Finding::LexicalQuantity { span: i..end });
                i = end;
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

fn longest_lexical_at<'a>(haystack: &str, byte_pos: usize) -> Option<&'a str> {
    let rest = &haystack[byte_pos..];
    let mut best: Option<&str> = None;
    for &n in LEXICAL_QUANTITY {
        if rest.starts_with(n) && best.is_none_or(|b| n.len() > b.len()) {
            best = Some(n);
        }
    }
    best
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
