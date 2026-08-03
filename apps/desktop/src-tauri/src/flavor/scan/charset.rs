//! Stage 1: invisible / bidi / default-ignorable / variation selectors /
//! control characters / markup. Classification only — no normalization.

use std::ops::Range;

use super::Finding;

/// Reject invisible, bidi, DI, VS, controls, and markup markers.
pub(crate) fn scan(raw: &str, out: &mut Vec<Finding>) {
    let bytes = raw.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        // Markup multi-byte / multi-char probes first (at byte index).
        if let Some(span) = markup_at(raw, i) {
            out.push(Finding::Markup { span: span.clone() });
            i = span.end;
            continue;
        }

        let ch = raw[i..].chars().next().expect("char boundary");
        let len = ch.len_utf8();
        if is_invisible_or_control(ch) {
            out.push(Finding::Invisible {
                at: i,
                cp: ch as u32,
            });
        }
        i += len;
    }
}

fn markup_at(raw: &str, i: usize) -> Option<Range<usize>> {
    let rest = &raw[i..];
    let b = rest.as_bytes();
    if b.is_empty() {
        return None;
    }
    match b[0] {
        b'<' | b'>' => Some(i..i + 1),
        b'`' if b.len() >= 3 && b[1] == b'`' && b[2] == b'`' => Some(i..i + 3),
        b'&' if b.len() >= 2 && b[1] == b'#' => Some(i..i + 2),
        b'%' if b.len() >= 3 && is_hex(b[1]) && is_hex(b[2]) => Some(i..i + 3),
        _ => None,
    }
}

fn is_hex(b: u8) -> bool {
    b.is_ascii_hexdigit()
}

/// Invisible / bidi / default-ignorable / variation selector / Cc.
fn is_invisible_or_control(c: char) -> bool {
    if c.is_control() {
        return true;
    }
    let cp = c as u32;
    // Explicit invisibles named in the directive.
    if matches!(cp, 0x200B | 0x200C | 0x200D | 0xFEFF | 0x2060) {
        return true;
    }
    // Bidi controls.
    if (0x202A..=0x202E).contains(&cp) || (0x2066..=0x2069).contains(&cp) {
        return true;
    }
    // Variation selectors.
    if (0xFE00..=0xFE0F).contains(&cp) || (0xE0100..=0xE01EF).contains(&cp) {
        return true;
    }
    // Broader Default_Ignorable_Code_Point ranges used by flavor threat model.
    if matches!(cp, 0x00AD | 0x034F | 0x061C | 0x180E | 0xE0001)
        || (0x200E..=0x200F).contains(&cp)
        || (0x2061..=0x2064).contains(&cp)
        || (0x206A..=0x206F).contains(&cp)
        || (0xFFF0..=0xFFF8).contains(&cp)
        || (0x1D173..=0x1D17A).contains(&cp)
        || (0xE0020..=0xE007F).contains(&cp)
    {
        return true;
    }
    false
}
