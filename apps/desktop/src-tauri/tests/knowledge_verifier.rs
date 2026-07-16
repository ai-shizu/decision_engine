//! Cross-language golden anchors for E0b Dual-Run (STEP 3).
//! Values are copied from tests/golden/* — never regenerated from Rust.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use pkb_desktop_lib::knowledge::{
    attestation_framing, canonicalize_for_match, snapshot_hash_hex, snapshot_preimage,
    verify_tag, VerifyError,
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

// ---------------------------------------------------------------------------
// STEP 3.B — HMAC framing + constant-time verify (STEP 2 KAT)
// ---------------------------------------------------------------------------
struct KatVec {
    name: &'static str,
    k_spawn: &'static str,
    session_id: &'static str,
    txn_nonce: &'static str,
    gen: u64,
    epoch: u64,
    dict_hash: &'static str,
    queries: &'static [&'static str],
    framing_hex: &'static str,
    tag_hex: &'static str,
}

// Copied from tests/golden/e0b_attestation_kat.json (STEP 2).
const KAT: &[KatVec] = &[
    KatVec {
        name: "KAT-1",
        k_spawn: "000102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f",
        session_id: "0123456789abcdeffedcba9876543210",
        txn_nonce: "00112233445566778899aabbccddeeffffeeddccbbaa99887766554433221100",
        gen: 7,
        epoch: 3,
        dict_hash: "f3b8fd0c8070d2127fd0b3daaacd12c25ab5e819cd3c7d17d11cd9fa632d34e0",
        queries: &["ai career", "日本の就職"],
        framing_hex: "30313233343536373839616263646566666564636261393837363534333231303030313132323333343435353636373738383939616162626363646465656666666665656464636362626161393938383737363635353434333332323131303000000000000000070000000000000003663362386664306338303730643231323766643062336461616163643132633235616235653831396364336337643137643131636439666136333264333465300000000000000009616920636172656572000000000000000fe697a5e69cace381aee5b0b1e881b7",
        tag_hex: "6c4390f16549039ca60fca51104a4a6c204252d6ce0583d4237eae27c328d9fa",
    },
    KatVec {
        name: "KAT-2",
        k_spawn: "000102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f",
        session_id: "0123456789abcdeffedcba9876543210",
        txn_nonce: "00112233445566778899aabbccddeeffffeeddccbbaa99887766554433221100",
        gen: 1,
        epoch: 1,
        dict_hash: "f3b8fd0c8070d2127fd0b3daaacd12c25ab5e819cd3c7d17d11cd9fa632d34e0",
        queries: &["python async"],
        framing_hex: "3031323334353637383961626364656666656463626139383736353433323130303031313232333334343535363637373838393961616262636364646565666666666565646463636262616139393838373736363535343433333232313130300000000000000001000000000000000166336238666430633830373064323132376664306233646161616364313263323561623565383139636433633764313764313163643966613633326433346530000000000000000c707974686f6e206173796e63",
        tag_hex: "c36b1fe4c46564af2dfd5310c771b5bf95eb81daff071a2e8b9bb87aac284861",
    },
    KatVec {
        name: "KAT-3",
        k_spawn: "000102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f",
        session_id: "0123456789abcdeffedcba9876543210",
        txn_nonce: "00112233445566778899aabbccddeeffffeeddccbbaa99887766554433221100",
        gen: 7,
        epoch: 3,
        dict_hash: "f3b8fd0c8070d2127fd0b3daaacd12c25ab5e819cd3c7d17d11cd9fa632d34e0",
        queries: &["😀 test"],
        framing_hex: "30313233343536373839616263646566666564636261393837363534333231303030313132323333343435353636373738383939616162626363646465656666666665656464636362626161393938383737363635353434333332323131303000000000000000070000000000000003663362386664306338303730643231323766643062336461616163643132633235616235653831396364336337643137643131636439666136333264333465300000000000000009f09f98802074657374",
        tag_hex: "dec75bd5da278801c3d75074e329a8349c483263f799519164f5dc17dd9ccee8",
    },
];

fn kat_queries(row: &KatVec) -> Vec<String> {
    row.queries.iter().map(|s| (*s).to_string()).collect()
}

#[test]
fn kat_framing_and_tag_verify() {
    for row in KAT {
        let queries = kat_queries(row);
        let framing = attestation_framing(
            row.session_id,
            row.txn_nonce,
            row.gen,
            row.epoch,
            row.dict_hash,
            &queries,
        )
        .expect("framing");
        assert_eq!(
            hex::encode(&framing),
            row.framing_hex,
            "{}: framing mismatch",
            row.name
        );
        assert!(
            verify_tag(row.k_spawn, &framing, row.tag_hex).is_ok(),
            "{}: good tag must verify",
            row.name
        );
    }
}

#[test]
fn kat_tamper_rejected() {
    let row = &KAT[0];
    let queries = kat_queries(row);
    let framing = attestation_framing(
        row.session_id,
        row.txn_nonce,
        row.gen,
        row.epoch,
        row.dict_hash,
        &queries,
    )
    .expect("framing");

    let mut bad_tag = row.tag_hex.to_string();
    let last = bad_tag.pop().unwrap();
    bad_tag.push(if last == 'a' { 'b' } else { 'a' });
    assert_eq!(
        verify_tag(row.k_spawn, &framing, &bad_tag),
        Err(VerifyError::AttestationMismatch)
    );

    let framing_gen8 = attestation_framing(
        row.session_id,
        row.txn_nonce,
        8,
        row.epoch,
        row.dict_hash,
        &queries,
    )
    .expect("framing gen8");
    assert_eq!(
        verify_tag(row.k_spawn, &framing_gen8, row.tag_hex),
        Err(VerifyError::AttestationMismatch)
    );

    let mut tweaked = queries.clone();
    tweaked[0].push('x');
    let framing_q = attestation_framing(
        row.session_id,
        row.txn_nonce,
        row.gen,
        row.epoch,
        row.dict_hash,
        &tweaked,
    )
    .expect("framing q");
    assert_eq!(
        verify_tag(row.k_spawn, &framing_q, row.tag_hex),
        Err(VerifyError::AttestationMismatch)
    );
}

#[test]
fn kat_failclosed_bad_key() {
    let framing = hex::decode(KAT[0].framing_hex).unwrap();
    assert!(verify_tag(&KAT[0].k_spawn[..63], &framing, KAT[0].tag_hex).is_err());
    assert!(verify_tag(&(KAT[0].k_spawn.to_string() + "00"), &framing, KAT[0].tag_hex).is_err());
    assert!(verify_tag(&"z".repeat(64), &framing, KAT[0].tag_hex).is_err());
}

#[test]
fn kat_attestation_source_uses_ct_eq_not_eq() {
    let src = include_str!("../src/knowledge/attestation.rs");
    assert!(
        src.contains("ct_eq"),
        "attestation.rs must use subtle::ct_eq"
    );
    // Forbid naive tag string equality on the verification path.
    assert!(
        !src.contains("computed_hex ==") && !src.contains("received_hex =="),
        "must not compare tags with =="
    );
}
