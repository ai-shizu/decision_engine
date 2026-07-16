//! Cross-language golden anchors for E0b Dual-Run (STEP 3).
//! Values are copied from tests/golden/* — never regenerated from Rust.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use pkb_desktop_lib::knowledge::{
    canonicalize_for_match, snapshot_hash_hex, snapshot_preimage,
};

fn decode_utf8_hex(hex_str: &str) -> String {
    let bytes = hex::decode(hex_str).expect("golden input_hex");
    String::from_utf8(bytes).expect("golden utf-8")
}

// Copied from tests/golden/e0b_canonicalize_golden.jsonl (STEP 1).
const CANON_GOLDEN: &[(&str, &str, &str)] = &[
    (
        "zwsp_evasion",
        "417473756b69e2808b20497368697a75",
        "617473756b6920697368697a75",
    ),
    (
        "plain_ref",
        "417473756b6920497368697a75",
        "617473756b6920697368697a75",
    ),
    ("rlo_prefix", "e280ae417473756b69", "617473756b69"),
    ("soft_hyphen", "41747375c2ad6b69", "617473756b69"),
    (
        "fullwidth",
        "efbca1efbcb4efbcb3efbcb5efbcabefbca9",
        "617473756b69",
    ),
    ("ligature_fi", "efac816c65", "66696c65"),
    ("combining_nfc", "65cc81", "c3a9"),
    ("precomposed", "c3a9", "c3a9"),
    ("dotted_I", "c4b0", "69cc87"),
    ("superscript2", "c2b2", "32"),
    (
        "halfwidth_kana",
        "efbdb1efbdb2efbdb3",
        "e382a2e382a4e382a6",
    ),
    ("tab_evasion", "4a6f686e09536d697468", "6a6f686e20736d697468"),
    ("bidi_isolate", "e281a6736563726574e281a9", "736563726574"),
    (
        "double_space",
        "417473756b692020497368697a75",
        "617473756b6920697368697a75",
    ),
    ("empty_after", "e2808befbbbf", ""),
];

#[test]
fn canonical_golden_byte_exact() {
    for (name, input_hex, expected_hex) in CANON_GOLDEN {
        let input = decode_utf8_hex(input_hex);
        let got = hex::encode(canonicalize_for_match(&input).as_bytes());
        assert_eq!(
            got.as_str(),
            *expected_hex,
            "{name}: canonicalize mismatch"
        );
    }
}

#[test]
fn canonical_empty_and_multibyte_no_panic() {
    assert_eq!(canonicalize_for_match(""), "");
    let mixed = "日😀é";
    let _ = canonicalize_for_match(mixed);
}

// Copied from tests/golden/e0b_pii_snapshot_golden.json (STEP 1).
struct SnapshotVec {
    name: &'static str,
    terms: &'static [&'static str],
    revision: u64,
    preimage_hex: &'static str,
    hash_hex: &'static str,
}

const SNAPSHOT_GOLDEN: &[SnapshotVec] = &[
    SnapshotVec {
        name: "ab_c",
        terms: &["ab", "c"],
        revision: 1,
        preimage_hex: "504b422d5049492d444943542d5631000000000000000100000000000000026162000000000000000163",
        hash_hex: "1fe182bd334ab3fe0d98ce260b378ec3ca4f3e18b6c959ceef7a35ff4a8d94c2",
    },
    SnapshotVec {
        name: "a_bc",
        terms: &["a", "bc"],
        revision: 1,
        preimage_hex: "504b422d5049492d444943542d5631000000000000000100000000000000016100000000000000026263",
        hash_hex: "95c0696115df12c4e92dead04ac9d01815d457069513df4d4da805a4745eb630",
    },
    SnapshotVec {
        name: "cjk",
        terms: &["日"],
        revision: 1,
        preimage_hex: "504b422d5049492d444943542d563100000000000000010000000000000003e697a5",
        hash_hex: "1fdaaaeb4d3ce1d5c052f7146fda829e86a419e1f18cafff3b590da6d766c2eb",
    },
    SnapshotVec {
        name: "emoji",
        terms: &["😀"],
        revision: 1,
        preimage_hex: "504b422d5049492d444943542d563100000000000000010000000000000004f09f9880",
        hash_hex: "6988184a1fecad5281f314f091a83af43ae0bff8eb62c69aeaa4406c71a59d44",
    },
    SnapshotVec {
        name: "rev2",
        terms: &["a"],
        revision: 2,
        preimage_hex: "504b422d5049492d444943542d56310000000000000002000000000000000161",
        hash_hex: "08c61fc9cd5d91c8d00f76cd4dde0900d07fafb355fa59e634be3cfdfb2060f1",
    },
    SnapshotVec {
        name: "empty",
        terms: &[],
        revision: 1,
        preimage_hex: "504b422d5049492d444943542d56310000000000000001",
        hash_hex: "f3b8fd0c8070d2127fd0b3daaacd12c25ab5e819cd3c7d17d11cd9fa632d34e0",
    },
];

#[test]
fn snapshot_golden_byte_exact() {
    for row in SNAPSHOT_GOLDEN {
        let terms: Vec<String> = row.terms.iter().map(|s| (*s).to_string()).collect();
        let pre = snapshot_preimage(&terms, row.revision);
        assert_eq!(
            hex::encode(&pre),
            row.preimage_hex,
            "{}: preimage mismatch",
            row.name
        );
        assert_eq!(
            snapshot_hash_hex(&terms, row.revision),
            row.hash_hex,
            "{}: hash mismatch",
            row.name
        );
    }
}

#[test]
fn snapshot_injective_and_byte_length() {
    let ab_c = SNAPSHOT_GOLDEN
        .iter()
        .find(|r| r.name == "ab_c")
        .expect("ab_c");
    let a_bc = SNAPSHOT_GOLDEN
        .iter()
        .find(|r| r.name == "a_bc")
        .expect("a_bc");
    assert_ne!(ab_c.hash_hex, a_bc.hash_hex);
    assert!(SNAPSHOT_GOLDEN
        .iter()
        .find(|r| r.name == "cjk")
        .unwrap()
        .preimage_hex
        .contains("0000000000000003e697a5"));
    assert!(SNAPSHOT_GOLDEN
        .iter()
        .find(|r| r.name == "emoji")
        .unwrap()
        .preimage_hex
        .contains("0000000000000004f09f9880"));
}
