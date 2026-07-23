//! LINE トーク履歴 (.txt) の堅牢な読み込み (M20 データ連携 Part 1).
//!
//! LINE のエクスポート形式は端末・アプリバージョンによって文字コードが揺れる
//! (UTF-8 / UTF-8+BOM / UTF-16 (BOM 付き) / 稀に Shift-JIS)。ここは常に
//! **panic せず**、常に `String` を返す (デコード自体が失敗する入力は存在しない
//! —最悪でも置換文字混じりの文字列になる)。iOS 実機の File Picker /
//! App Sandbox から渡される生バイト列 (`Vec<u8>`) を直接受け取り、フロント側の
//! `TextDecoder` に依存しない — iOS では Python サイドカーが起動できない
//! (`engine.rs` の `#[cfg(not(mobile))]` 参照) ため、この経路が唯一の取り込み先。

use encoding_rs::{SHIFT_JIS, UTF_16BE, UTF_16LE};

const UTF8_BOM: [u8; 3] = [0xEF, 0xBB, 0xBF];
const UTF16LE_BOM: [u8; 2] = [0xFF, 0xFE];
const UTF16BE_BOM: [u8; 2] = [0xFE, 0xFF];

/// Decode raw file bytes into text, never panicking and never returning `Err`.
///
/// Priority:
/// 1. Explicit BOM (UTF-8 / UTF-16 LE / UTF-16 BE) — unambiguous, always wins.
/// 2. No BOM: strict UTF-8 (LINE's default export encoding).
/// 3. Strict UTF-8 fails: Shift-JIS (older / some Android exports), matching
///    the same fallback policy as `core/es_manager.py::_read_text_lenient`.
///
/// A stray leading `\u{FEFF}` that survives decoding (e.g. a UTF-8 BOM prefix
/// on an otherwise non-UTF-8 file, decoded via the Shift-JIS fallback) is
/// stripped as a final defensive pass so downstream chunking never sees it.
pub fn decode_line_export_bytes(bytes: &[u8]) -> String {
    let decoded = if let Some(rest) = bytes.strip_prefix(&UTF8_BOM) {
        String::from_utf8_lossy(rest).into_owned()
    } else if let Some(rest) = bytes.strip_prefix(&UTF16LE_BOM) {
        UTF_16LE.decode(rest).0.into_owned()
    } else if let Some(rest) = bytes.strip_prefix(&UTF16BE_BOM) {
        UTF_16BE.decode(rest).0.into_owned()
    } else {
        match std::str::from_utf8(bytes) {
            Ok(s) => s.to_string(),
            Err(_) => SHIFT_JIS.decode(bytes).0.into_owned(),
        }
    };
    decoded.trim_start_matches('\u{FEFF}').to_string()
}

/// T-23 (IMP-2) parity with `core/facade.py::format_line_import`: any append
/// without a `[LINE]` header collapses block boundaries downstream (dedup /
/// chunking degrades to treating the whole file as one section). Prepend a
/// header from the filename stem so every ingested LINE file keeps a
/// recognizable boundary, matching the desktop Python behavior byte-for-byte
/// in spirit (not file-identical — this feeds on-device RAG chunking, not the
/// Python `LINE_HISTORY` flat file).
pub fn format_line_import(text: &str, filename: &str) -> String {
    let body = text.trim();
    if body.contains("[LINE]") {
        return body.to_string();
    }
    let label = std::path::Path::new(filename)
        .file_stem()
        .and_then(|s| s.to_str())
        .filter(|s| !s.trim().is_empty())
        .unwrap_or("不明");
    format!("[LINE] {label}とのトーク履歴\n{body}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_plain_utf8_without_bom() {
        let bytes = "こんにちは\n13:45\tAlice\t元気?".as_bytes();
        assert_eq!(decode_line_export_bytes(bytes), "こんにちは\n13:45\tAlice\t元気?");
    }

    #[test]
    fn strips_utf8_bom() {
        let mut bytes = UTF8_BOM.to_vec();
        bytes.extend_from_slice("2024/01/15(月)\n13:45\tAlice\tこんにちは".as_bytes());
        let decoded = decode_line_export_bytes(&bytes);
        assert!(!decoded.starts_with('\u{FEFF}'));
        assert!(decoded.starts_with("2024/01/15"));
    }

    #[test]
    fn decodes_utf16le_with_bom() {
        let text = "2024/01/15(月)\n13:45\tAlice\tこんにちは";
        let mut bytes = UTF16LE_BOM.to_vec();
        for u in text.encode_utf16() {
            bytes.extend_from_slice(&u.to_le_bytes());
        }
        let decoded = decode_line_export_bytes(&bytes);
        assert_eq!(decoded, text);
    }

    #[test]
    fn decodes_utf16be_with_bom() {
        let text = "2024/01/15(月)\n13:45\tAlice\tこんにちは";
        let mut bytes = UTF16BE_BOM.to_vec();
        for u in text.encode_utf16() {
            bytes.extend_from_slice(&u.to_be_bytes());
        }
        let decoded = decode_line_export_bytes(&bytes);
        assert_eq!(decoded, text);
    }

    #[test]
    fn falls_back_to_shift_jis_when_not_valid_utf8() {
        // Encode a Japanese string as Shift-JIS bytes (invalid as UTF-8).
        let (sjis_bytes, _, had_errors) = SHIFT_JIS.encode("こんにちは");
        assert!(!had_errors);
        assert!(std::str::from_utf8(&sjis_bytes).is_err(), "fixture must not be valid utf-8");
        assert_eq!(decode_line_export_bytes(&sjis_bytes), "こんにちは");
    }

    #[test]
    fn never_panics_on_arbitrary_garbage_bytes() {
        // Not valid UTF-8, not clean Shift-JIS either — must still return some
        // String rather than panicking (encoding_rs replaces undecodable bytes).
        let garbage: Vec<u8> = vec![0xFF, 0xFE, 0xFE, 0xFF, 0x80, 0x81, 0x00, 0x01, 0xC0];
        let _ = decode_line_export_bytes(&garbage); // must not panic
    }

    #[test]
    fn empty_bytes_decode_to_empty_string() {
        assert_eq!(decode_line_export_bytes(&[]), "");
    }

    #[test]
    fn format_adds_header_from_filename_stem() {
        let out = format_line_import("13:45\tAlice\tこんにちは", "line_2024.txt");
        assert!(out.starts_with("[LINE] line_2024とのトーク履歴\n"));
        assert!(out.ends_with("こんにちは"));
    }

    #[test]
    fn format_uses_placeholder_label_when_filename_empty() {
        let out = format_line_import("body", "");
        assert!(out.starts_with("[LINE] 不明とのトーク履歴\n"));
    }

    #[test]
    fn format_does_not_double_header_when_already_present() {
        let already = "[LINE] 既存とのトーク履歴\nbody";
        assert_eq!(format_line_import(already, "ignored.txt"), already);
    }

    #[test]
    fn format_trims_surrounding_whitespace() {
        let out = format_line_import("  \n body \n  ", "x.txt");
        assert!(out.ends_with("body"));
        assert!(!out.ends_with(" \n  "));
    }
}
