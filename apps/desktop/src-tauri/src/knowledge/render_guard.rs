//! External-text render safety — Markdown / HTML / special-token neutralization.
//! Byte-parity with Python `core.external_evidence.sanitize_external_text`.
#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::string_slice
)]

use unicode_normalization::UnicodeNormalization;

const NAMED: &[(&str, &str)] = &[
    ("amp", "&"),
    ("lt", "<"),
    ("gt", ">"),
    ("quot", "\""),
    ("apos", "'"),
    ("nbsp", " "),
];

fn map_char(ch: char) -> char {
    match ch {
        '<' => '＜',
        '>' => '＞',
        '|' => '｜',
        '`' => '｀',
        '~' => '～',
        '[' => '［',
        ']' => '］',
        '(' => '（',
        ')' => '）',
        '!' => '！',
        '\\' => '＼',
        '/' => '／',
        '&' => '＆',
        '*' => '＊',
        '_' => '＿',
        '#' => '＃',
        _ => ch,
    }
}

fn should_remove(cp: u32) -> bool {
    matches!(
        cp,
        0x00..=0x08
            | 0x0E..=0x1F
            | 0x7F..=0x9F
            | 0x00AD
            | 0x061C
            | 0x180E
            | 0x200B..=0x200F
            | 0x2028..=0x202E
            | 0x2060..=0x2064
            | 0x2066..=0x2069
            | 0xFEFF
    )
}

fn utf8_truncate(s: &str, max_bytes: usize) -> String {
    let raw = s.as_bytes();
    if raw.len() <= max_bytes {
        return s.to_string();
    }
    let mut end = max_bytes;
    while end > 0 && (raw.get(end).copied().unwrap_or(0) & 0xC0) == 0x80 {
        end -= 1;
    }
    match std::str::from_utf8(raw.get(..end).unwrap_or(&[])) {
        Ok(t) => t.to_string(),
        Err(_) => String::new(),
    }
}

fn decode_entities(s: &str) -> String {
    let chars: Vec<char> = s.chars().collect();
    let mut out = String::new();
    let mut i = 0usize;
    while i < chars.len() {
        if chars.get(i).copied() != Some('&') {
            if let Some(ch) = chars.get(i) {
                out.push(*ch);
            }
            i += 1;
            continue;
        }
        let mut semi = None;
        let mut j = i + 1;
        while j < chars.len() && j - i <= 16 {
            if chars.get(j).copied() == Some(';') {
                semi = Some(j);
                break;
            }
            j += 1;
        }
        let Some(semi_idx) = semi else {
            out.push('&');
            i += 1;
            continue;
        };
        let body: String = chars
            .get((i + 1)..semi_idx)
            .unwrap_or(&[])
            .iter()
            .collect();
        let repl = if let Some(hex) = body.strip_prefix("#x").or_else(|| body.strip_prefix("#X")) {
            u32::from_str_radix(hex, 16).ok().and_then(char::from_u32)
        } else if let Some(dec) = body.strip_prefix('#') {
            dec.parse::<u32>().ok().and_then(char::from_u32)
        } else {
            NAMED
                .iter()
                .find(|(k, _)| *k == body)
                .and_then(|(_, v)| v.chars().next())
        };
        // Filter surrogates
        let repl = repl.filter(|c| {
            let cp = *c as u32;
            !(0xD800..=0xDFFF).contains(&cp)
        });
        if let Some(ch) = repl {
            out.push(ch);
            i = semi_idx + 1;
        } else {
            out.push('&');
            i += 1;
        }
    }
    out
}

fn strip_tags(s: &str) -> String {
    let chars: Vec<char> = s.chars().collect();
    let mut out = String::new();
    let mut i = 0usize;
    while i < chars.len() {
        if chars.get(i).copied() != Some('<') {
            if let Some(ch) = chars.get(i) {
                out.push(*ch);
            }
            i += 1;
            continue;
        }
        let mut j = i + 1;
        while j < chars.len() && j - i < 64 && chars.get(j).copied() != Some('>') {
            j += 1;
        }
        if j < chars.len() && chars.get(j).copied() == Some('>') {
            i = j + 1;
            continue;
        }
        out.push('<');
        i += 1;
    }
    out
}

/// Sanitize external title/snippet for prompt + persistence.
pub fn sanitize_external_text(text: &str, max_bytes: usize) -> Result<String, ()> {
    let mut s = text.to_string();
    for _ in 0..4 {
        let nxt = decode_entities(&s);
        if nxt == s {
            break;
        }
        s = nxt;
    }
    s = strip_tags(&s);
    // NFKC before replacement so compatibility forms collapse into triggers we map.
    s = s.nfkc().collect::<String>();

    let mut cleaned = String::new();
    for ch in s.chars() {
        let cp = ch as u32;
        if should_remove(cp) {
            continue;
        }
        cleaned.push(map_char(ch));
    }
    let truncated = utf8_truncate(&cleaned, max_bytes);
    if truncated.trim().is_empty() {
        return Err(());
    }
    Ok(truncated)
}
