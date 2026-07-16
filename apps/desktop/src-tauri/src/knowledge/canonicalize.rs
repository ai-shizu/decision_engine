//! Mirror of Python `core.e0b_attestation.canonicalize_for_match` (byte-exact).
#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::string_slice
)]

use unicode_normalization::UnicodeNormalization;

fn is_remove(cp: u32) -> bool {
    matches!(
        cp,
        0x00..=0x08
            | 0x0E..=0x1F
            | 0x7F..=0x9F
            | 0x00AD
            | 0x061C
            | 0x180E
            | 0x200B
            | 0x200C
            | 0x200D
            | 0x200E
            | 0x200F
            | 0x202A
            | 0x202B
            | 0x202C
            | 0x202D
            | 0x202E
            | 0x2060
            | 0x2061
            | 0x2062
            | 0x2063
            | 0x2064
            | 0x2066
            | 0x2067
            | 0x2068
            | 0x2069
            | 0xFEFF
    )
}

fn is_whitespace_map(cp: u32) -> bool {
    matches!(cp, 0x09 | 0x0A | 0x0B | 0x0C | 0x0D | 0x20 | 0x2028 | 0x2029)
}

/// Deterministic match normalization — order fixed to match Python STEP 1.
pub fn canonicalize_for_match(s: &str) -> String {
    let stripped: String = s.chars().filter(|c| !is_remove(*c as u32)).collect();
    let nfkc1: String = stripped.nfkc().collect();
    let folded = caseless::default_case_fold_str(&nfkc1);
    let nfkc2: String = folded.nfkc().collect();

    let mut mapped = String::with_capacity(nfkc2.len());
    for c in nfkc2.chars() {
        if is_whitespace_map(c as u32) {
            mapped.push(' ');
        } else {
            mapped.push(c);
        }
    }

    let mut collapsed = String::with_capacity(mapped.len());
    let mut prev_space = false;
    for c in mapped.chars() {
        if c == ' ' {
            if prev_space {
                continue;
            }
            prev_space = true;
            collapsed.push(c);
        } else {
            prev_space = false;
            collapsed.push(c);
        }
    }
    collapsed.trim_matches(' ').to_string()
}
