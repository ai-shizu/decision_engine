//! Dual-Run AND gate: HMAC verify ∧ independent aho-corasick PII re-check.
#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::string_slice
)]

use aho_corasick::AhoCorasick;
use serde::Deserialize;

use crate::knowledge::attestation::{attestation_framing, verify_tag};
use crate::knowledge::canonicalize::canonicalize_for_match;
use crate::knowledge::VerifyError;

/// Wire payload for attested intent (no unknown fields).
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct AttestedIntentPayload {
    pub session_id: String,
    pub txn_nonce: String,
    pub sidecar_generation: u64,
    pub policy_epoch: u64,
    pub dict_hash: String,
    pub queries: Vec<String>,
    pub attestation: String,
}

/// AND gate: constant-time HMAC check then independent PII dual-run.
pub fn verify_and_gate(
    payload: &AttestedIntentPayload,
    dict_terms: &[String],
    k_spawn: &str,
) -> Result<(), VerifyError> {
    // (A) Reconstruct framing and verify HMAC (constant-time).
    let framing = attestation_framing(
        &payload.session_id,
        &payload.txn_nonce,
        payload.sidecar_generation,
        payload.policy_epoch,
        &payload.dict_hash,
        &payload.queries,
    )?;
    verify_tag(k_spawn, &framing, &payload.attestation)?;

    // (B) Independent PII re-check on match-canonicalized queries.
    let automaton =
        AhoCorasick::new(dict_terms).map_err(|_| VerifyError::MalformedField)?;
    for q in &payload.queries {
        let canonical = canonicalize_for_match(q);
        if automaton.is_match(&canonical) {
            return Err(VerifyError::PiiRejected);
        }
    }
    Ok(())
}
