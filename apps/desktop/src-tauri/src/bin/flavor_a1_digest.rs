//! Dump Decide-time `state_digest` series for A-1 deletability (F-3 T-6).
//!
//! Driven by `scripts/flavor_a1_deletability.sh` — never nested inside
//! `cargo test` (SPEC §9.2).

fn main() {
    let code = pkb_desktop_lib::flavor_a1_harness::run_cli(&std::env::args().collect::<Vec<_>>());
    std::process::exit(code);
}
