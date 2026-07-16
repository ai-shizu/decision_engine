//! Mirror of Python STEP 2 attestation framing + constant-time tag verify.
#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::string_slice
)]

use hmac::{Hmac, KeyInit, Mac};
use sha2::Sha256;
use subtle::ConstantTimeEq;

use crate::knowledge::VerifyError;

type HmacSha256 = Hmac<Sha256>;

fn require_lowercase_hex(value: &str, expected_len: usize) -> Result<(), VerifyError> {
    if value.len() != expected_len {
        return Err(VerifyError::MalformedField);
    }
    if !value.chars().all(|c| matches!(c, '0'..='9' | 'a'..='f')) {
        return Err(VerifyError::MalformedField);
    }
    Ok(())
}

/// Length-prefixed injective framing (STEP 2 §1.4 mirror).
pub fn attestation_framing(
    session_id: &str,
    txn_nonce: &str,
    sidecar_generation: u64,
    policy_epoch: u64,
    dict_hash: &str,
    queries: &[String],
) -> Result<Vec<u8>, VerifyError> {
    require_lowercase_hex(session_id, 32)?;
    require_lowercase_hex(txn_nonce, 64)?;
    require_lowercase_hex(dict_hash, 64)?;

    let mut out = Vec::new();
    out.extend_from_slice(session_id.as_bytes());
    out.extend_from_slice(txn_nonce.as_bytes());
    out.extend_from_slice(&sidecar_generation.to_be_bytes());
    out.extend_from_slice(&policy_epoch.to_be_bytes());
    out.extend_from_slice(dict_hash.as_bytes());
    for q in queries {
        let raw = q.as_bytes();
        out.extend_from_slice(&(raw.len() as u64).to_be_bytes());
        out.extend_from_slice(raw);
    }
    Ok(out)
}

/// Constant-time HMAC-SHA256 tag verification (subtle::ct_eq only).
pub fn verify_tag(
    k_spawn_hex: &str,
    framing: &[u8],
    received_tag_hex: &str,
) -> Result<(), VerifyError> {
    require_lowercase_hex(k_spawn_hex, 64)?;
    require_lowercase_hex(received_tag_hex, 64)?;

    let key = hex::decode(k_spawn_hex).map_err(|_| VerifyError::MalformedField)?;
    if key.len() != 32 {
        return Err(VerifyError::MalformedField);
    }
    let received = hex::decode(received_tag_hex).map_err(|_| VerifyError::MalformedField)?;
    if received.len() != 32 {
        return Err(VerifyError::MalformedField);
    }

    let mut mac =
        HmacSha256::new_from_slice(&key).map_err(|_| VerifyError::MalformedField)?;
    mac.update(framing);
    let computed = mac.finalize().into_bytes();

    // ★ Never use == for tag comparison — timing side-channel.
    let equal = computed.ct_eq(received.as_slice());
    if bool::from(equal) {
        Ok(())
    } else {
        Err(VerifyError::AttestationMismatch)
    }
}
