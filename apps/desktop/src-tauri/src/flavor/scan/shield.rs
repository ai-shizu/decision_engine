//! Stage 2: consume audited exception terminals left-to-right longest-match
//! and register shielded byte spans. Must run **before** numeral scan so
//! `一気に` passes and `万が一、一万円` only shields the former.

use std::ops::Range;

use super::tables::AUDITED_V1;

/// Collect shielded spans for edition-1 audited terminals.
pub(crate) fn collect_shields(raw: &str) -> Vec<Range<usize>> {
    let mut shields = Vec::new();
    let mut i = 0;
    while i < raw.len() {
        if let Some(tok) = longest_at(raw, i, AUDITED_V1) {
            let end = i + tok.len();
            shields.push(i..end);
            i = end;
        } else {
            let ch = raw[i..].chars().next().expect("char boundary");
            i += ch.len_utf8();
        }
    }
    shields
}

pub(crate) fn is_shielded(byte_idx: usize, shields: &[Range<usize>]) -> bool {
    shields.iter().any(|r| r.contains(&byte_idx))
}

fn longest_at<'a>(haystack: &str, byte_pos: usize, needles: &[&'a str]) -> Option<&'a str> {
    let rest = &haystack[byte_pos..];
    let mut best: Option<&'a str> = None;
    for &n in needles {
        if rest.starts_with(n) && best.is_none_or(|b| n.len() > b.len()) {
            best = Some(n);
        }
    }
    best
}
