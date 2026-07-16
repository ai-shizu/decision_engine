//! E0b Dual-Run verification core (network-adjacent gate; no egress client).
//! Parent: docs/SPEC_E0B_STEP3_RUST_VERIFIER.md
#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::string_slice
)]

pub mod canonicalize;
pub mod pii_snapshot;

pub use canonicalize::canonicalize_for_match;
pub use pii_snapshot::{snapshot_hash_hex, snapshot_preimage};
