//! STEP 6.E — cross-language sanitize golden (byte parity with Python).
#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::string_slice
)]

use pkb_desktop_lib::knowledge::render_guard::sanitize_external_text;
use serde::Deserialize;
use std::path::PathBuf;

#[derive(Debug, Deserialize)]
struct Row {
    name: String,
    input_hex: String,
    max_bytes: usize,
    expected_hex: Option<String>,
    ok: bool,
}

fn golden_path() -> PathBuf {
    // tests/render_golden.rs -> src-tauri -> desktop -> apps -> repo
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../..")
        .join("tests/golden/e0b_render_golden.json")
}

#[test]
fn rust_sanitize_matches_golden_byte_exact() {
    let raw = std::fs::read_to_string(golden_path()).expect("read golden");
    let rows: Vec<Row> = serde_json::from_str(&raw).expect("parse golden");
    assert!(!rows.is_empty());
    for row in rows {
        let bytes = hex::decode(&row.input_hex).expect("input hex");
        let input = String::from_utf8(bytes).expect("utf8");
        match sanitize_external_text(&input, row.max_bytes) {
            Ok(got) if row.ok => {
                let expected = row.expected_hex.expect("expected_hex");
                assert_eq!(
                    hex::encode(got.as_bytes()),
                    expected,
                    "mismatch on {}",
                    row.name
                );
            }
            Err(()) if !row.ok => {}
            other => panic!("unexpected outcome for {}: {other:?}", row.name),
        }
    }
}
