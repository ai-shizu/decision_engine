//! Runtime tests for E0b research typestate FSM (STEP 4).
#![allow(clippy::unwrap_used, clippy::expect_used)]

use pkb_desktop_lib::knowledge::fsm::ResearchSlot;
use pkb_desktop_lib::knowledge::AttestedIntentPayload;

const DICT_A: &str = "f3b8fd0c8070d2127fd0b3daaacd12c25ab5e819cd3c7d17d11cd9fa632d34e0";
const SID: &str = "0123456789abcdeffedcba9876543210";
const NONCE1: &str = "00112233445566778899aabbccddeeffffeeddccbbaa99887766554433221100";
const NONCE2: &str = "ffeeddccbbaa9988776655443322110000112233445566778899aabbccddeeff";

fn payload(nonce: &str, dict_hash: &str) -> AttestedIntentPayload {
    AttestedIntentPayload {
        session_id: SID.to_string(),
        txn_nonce: nonce.to_string(),
        sidecar_generation: 1,
        policy_epoch: 1,
        dict_hash: dict_hash.to_string(),
        queries: vec!["safe query".to_string()],
        attestation: "00".repeat(32),
    }
}

// ---------------------------------------------------------------------------
// STEP 4.A — happy-path transitions + slot release
// ---------------------------------------------------------------------------
#[test]
fn transition_pending_to_completed_releases_slot() {
    let slot = ResearchSlot::new(DICT_A);
    let txn = slot.begin(payload(NONCE1, DICT_A)).expect("begin");
    let txn = txn.transition_to_fetching();
    let txn = txn.transition_to_ready();
    let completed = txn.transition_to_completed();
    drop(completed);

    // Slot released; fresh nonce can begin.
    let again = slot.begin(payload(NONCE2, DICT_A));
    assert!(again.is_ok());
}

#[test]
fn transition_mid_drop_releases_slot() {
    let slot = ResearchSlot::new(DICT_A);
    {
        let txn = slot.begin(payload(NONCE1, DICT_A)).expect("begin");
        let _fetching = txn.transition_to_fetching();
        // drop mid-flight
    }
    assert!(slot.begin(payload(NONCE2, DICT_A)).is_ok());
}
