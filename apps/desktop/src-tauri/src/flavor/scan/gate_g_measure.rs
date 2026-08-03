//! Gate G (F-2b) — pass-rate measurement against the architect-authored
//! flavor-domain population (`MUST_ACCEPT_IDIOM_FAMILY` ∪
//! `MUST_REJECT_IDIOM_BOUNDARY` = 24 sentences).
//!
//! Not a pass/fail gate: report totals + Finding kind breakdown.
//! Prior F-2 Gate G used coliseum analytical prose (RATE=60.9% on a
//! **different** population) — do not compare axes as if identical.
//! Do not quietly loosen tables when the rate is low — report the number.

use crate::flavor::corpus::{MUST_ACCEPT_IDIOM_FAMILY, MUST_REJECT_IDIOM_BOUNDARY};
use crate::flavor::policy::FlavorPolicy;
use crate::flavor::scan::{self, Finding};
use crate::flavor::verified::VerifiedFlavor;

#[test]
fn gate_g_flavor_domain_pass_rate_measurement() {
    let policy = FlavorPolicy::v1_empty();
    let population: Vec<&str> = MUST_ACCEPT_IDIOM_FAMILY
        .iter()
        .chain(MUST_REJECT_IDIOM_BOUNDARY.iter())
        .copied()
        .collect();
    let total = population.len();
    let mut accepted = 0usize;
    let mut kind_counts: Vec<(&str, usize)> = Vec::new();

    eprintln!(
        "GATE_G NOTE: population=flavor-domain 24 (idiom family+boundary); \
         prior F-2 coliseum analytical prose RATE=60.9% is NOT comparable \
         (different population)."
    );

    for sample in &population {
        let ok = VerifiedFlavor::verify(sample, &policy).is_some();
        if ok {
            accepted += 1;
            eprintln!("GATE_G ACCEPT: {sample:?}");
        } else {
            let findings = scan::scan(sample, &policy).findings;
            eprintln!("GATE_G REJECT: {sample:?} findings={findings:?}");
            for f in findings {
                bump_kind(&mut kind_counts, f.kind_name());
            }
        }
    }

    let rate = if total == 0 {
        0.0
    } else {
        (accepted as f64) * 100.0 / (total as f64)
    };
    eprintln!("GATE_G TOTAL={total} ACCEPTED={accepted} RATE={rate:.1}%");
    eprintln!("GATE_G PRIOR_F2_COLISEUM_RATE=60.9% (incomparable axis)");
    eprintln!("GATE_G FINDING_BREAKDOWN:");
    for (k, n) in &kind_counts {
        eprintln!("GATE_G   {k}={n}");
    }

    assert_eq!(total, 24, "flavor-domain Gate G population must be 16+8");
}

fn bump_kind(counts: &mut Vec<(&str, usize)>, kind: &'static str) {
    if let Some((_, n)) = counts.iter_mut().find(|(k, _)| *k == kind) {
        *n += 1;
    } else {
        counts.push((kind, 1));
    }
}

#[test]
fn gate_g_rejects_are_reasoned_not_bool() {
    let policy = FlavorPolicy::v1_empty();
    for sample in MUST_ACCEPT_IDIOM_FAMILY
        .iter()
        .chain(MUST_REJECT_IDIOM_BOUNDARY.iter())
    {
        if VerifiedFlavor::verify(sample, &policy).is_none() {
            let s = scan::scan(sample, &policy);
            assert!(
                !s.findings.is_empty(),
                "reject without findings: {sample:?}"
            );
            let _ = s.findings.iter().map(Finding::kind_name).collect::<Vec<_>>();
        }
    }
}
