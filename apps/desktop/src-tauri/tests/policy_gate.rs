//! STEP 8: two-factor egress gate (consent ∧ egress-live).
#![allow(clippy::unwrap_used, clippy::expect_used)]

use pkb_desktop_lib::knowledge::{
    refuse_if_egress_unavailable, refuse_if_policy_off, NetworkPolicy,
};

#[test]
fn off_policy_refuses_before_egress_check() {
    assert!(refuse_if_policy_off(NetworkPolicy::Off).is_err());
}

#[test]
fn live_consent_passes_policy_gate() {
    assert!(refuse_if_policy_off(NetworkPolicy::Live).is_ok());
}

#[test]
fn live_without_egress_live_build_fails_second_factor() {
    assert!(refuse_if_policy_off(NetworkPolicy::Live).is_ok());
    assert!(
        refuse_if_egress_unavailable().is_err(),
        "default build must refuse after consent without egress-live"
    );
}
