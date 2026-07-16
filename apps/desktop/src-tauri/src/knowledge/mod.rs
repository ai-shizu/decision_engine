//! E0b Dual-Run verification core (network-adjacent gate; no egress client).
//! Parent: docs/SPEC_E0B_STEP3_RUST_VERIFIER.md
#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::string_slice
)]

pub mod attestation;
pub mod canonicalize;
pub mod dns_guard;
pub mod dual_run;
pub mod fsm;
pub mod net_gateway;
pub mod orchestrator;
pub mod pii_snapshot;
pub mod policy_store;
pub mod render_guard;

pub use attestation::{attestation_framing, verify_tag};
pub use canonicalize::canonicalize_for_match;
pub use dual_run::{verify_and_gate, AttestedIntentPayload};
pub use fsm::{
    AbortReason, Completed, Fetching, FsmError, Pending, ReadyToIntegrate, ResearchSlot, Txn,
};
pub use orchestrator::{
    refuse_if_egress_unavailable, refuse_if_policy_off, InjectedFetch, NetworkPolicy,
    OrchestratorError,
};
pub use policy_store::NetworkPolicyStore;
pub use pii_snapshot::{snapshot_hash_hex, snapshot_preimage};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VerifyError {
    MalformedField,
    AttestationMismatch,
    PiiRejected,
}

impl std::fmt::Display for VerifyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MalformedField => write!(f, "malformed attestation field"),
            Self::AttestationMismatch => write!(f, "attestation tag mismatch"),
            Self::PiiRejected => write!(f, "PII rejected by dual-run gate"),
        }
    }
}

impl std::error::Error for VerifyError {}
