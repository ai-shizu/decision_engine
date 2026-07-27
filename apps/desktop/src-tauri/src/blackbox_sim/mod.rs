//! BLACKBOX SIMULATOR — deterministic offline finance-sim core.
//! Parent: docs/SPEC_BLACKBOX_SIMULATOR.md (invariants BXS-I-nn / traps BXS-W-nn).
//!
//! Phase 0 = data structures + guards. Phase 1 adds the deterministic math
//! layer, the frozen world artifact and the market kernel. Still no IPC
//! commands and no vault I/O — those land in later, individually adjudicated
//! phases (same "module tree only" precedent as the llm module's Phase 0).
//!
//! Isolation walls (SPEC §2) enforced here by construction:
//! - no imports from `crate::analytics` / `crate::db` / `crate::llm` /
//!   `crate::coliseum` (wall W-b; guarded by
//!   tests/test_blackbox_sim_contract.py::test_no_profile_dependency),
//! - authoritative state is integer-only; no libm transcendentals
//!   (SPEC §3.3, trap BXS-W-02).
#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::string_slice
)]

pub mod action;
pub mod bias;
pub mod det_math;
pub mod director;
pub mod firm;
pub mod fsm;
pub mod genesis;
pub mod ledger;
pub mod market;
pub mod money;
pub mod normal;
pub mod oracle;
pub mod persist;
// PHANTOM-BOT and the calibration suite are test-only (SPEC §12): neither the
// bot nor its known-answer harness has any production runtime role, and
// `CalibrationCertificate::test_only()` (bias.rs) is itself `#[cfg(test)]`
// `pub(crate)` — the calibration module needs that crate-internal reach,
// which an external `tests/*.rs` integration crate could never see.
#[cfg(test)]
pub mod phantom_bot;
#[cfg(test)]
pub mod calibration;
pub mod ring;
pub mod rng;
pub mod settle;
pub mod snapshot;
pub mod stimulus;
pub mod telemetry;
