//! Mirror of Python `PIISnapshot` preimage / SHA-256 (byte-exact).
#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::string_slice
)]

use sha2::{Digest, Sha256};

const DOMAIN_SEP: &[u8] = b"PKB-PII-DICT-V1";

/// Length-prefixed injective encoding (UTF-8 byte lengths, big-endian u64).
pub fn snapshot_preimage(terms: &[String], revision: u64) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(DOMAIN_SEP);
    out.extend_from_slice(&revision.to_be_bytes());
    for t in terms {
        let raw = t.as_bytes();
        out.extend_from_slice(&(raw.len() as u64).to_be_bytes());
        out.extend_from_slice(raw);
    }
    out
}

/// SHA-256(preimage) as 64 lowercase hex.
pub fn snapshot_hash_hex(terms: &[String], revision: u64) -> String {
    let pre = snapshot_preimage(terms, revision);
    let digest = Sha256::digest(&pre);
    hex::encode(digest)
}
