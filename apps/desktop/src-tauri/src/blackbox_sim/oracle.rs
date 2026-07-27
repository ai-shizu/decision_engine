//! Hidden oracle — the sealed truth side of the instrument (SPEC §8 seed, 壁 W-a).
//!
//! Two stimuli need a quantity the player cannot compute: the anchoring probe
//! needs a *true value* to perturb, and the overconfidence probe needs a
//! reference against which an interval can later be scored. Both are served
//! here.
//!
//! # Why this module is dangerous, and how it is contained
//!
//! Everything in here is derived from the TRUE Genesis parameters and the
//! private kernel state. Publishing any of it verbatim would collapse the
//! measurement into self-report (壁 W-a). The containment rules are:
//!
//! 1. Nothing in this module derives `Serialize`. A value that cannot be
//!    serialised cannot reach a view model or an LLM prompt by accident.
//! 2. The only value that reaches the player is the *perturbed* anchor built in
//!    `stimulus.rs`, and the perturbation is the measurement itself.
//! 3. The reference is recomputed from state on demand and never stored in the
//!    decision log. What the log stores is the anchor the player saw plus the
//!    digest of the parameters that produced it, so an estimator with the
//!    Genesis can reconstruct the truth while a player with the log cannot.
//!
//! The optimality-gap oracle that lane 5 (`pressure_degradation`) needs is a
//! genuinely different object — it requires solving for the optimal action, not
//! just valuing the current one — and is deferred to Phase 4 rather than
//! faked here (LAW-20: an unmeasured axis reports N/A, it does not guess).

use crate::blackbox_sim::action::ActionContext;
use crate::blackbox_sim::firm::FirmState;
use crate::blackbox_sim::market::MarketKernel;
use crate::blackbox_sim::settle::demand_units;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OracleError {
    Overflow,
    Kernel,
}

/// Deliberately not `Serialize`, not `Display`, and not reachable from any view
/// model. See the module header.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReferenceValuation {
    /// Expected demand index one tick ahead, in micro units.
    pub expected_demand_micro: i64,
    /// Expected revenue one tick ahead at today's prices, in minor units.
    pub expected_revenue_minor: i64,
}

/// Expected revenue for the next tick at today's posted prices.
///
/// This is *unconstrained* demand revenue: it does not cap sales at current
/// inventory, because next tick's inventory depends on orders the player has
/// not placed yet. It is therefore an analyst-style forecast of the demand
/// side, which is exactly the quantity the anchor claims to be a consensus on.
/// Capping it here would make the anchor a function of the player's own
/// stocking decisions and destroy comparability across players.
pub fn reference_valuation(
    kernel: &MarketKernel,
    ctx: &ActionContext,
    firm: &FirmState,
) -> Result<ReferenceValuation, OracleError> {
    let expected_demand_micro = kernel
        .oracle_expected_next_demand_micro()
        .map_err(|_| OracleError::Kernel)?;
    let mut expected_revenue_minor: i64 = 0;
    for (id, sku) in firm.skus() {
        let params = match ctx.firm_params.skus.get(usize::from(id)) {
            Some(params) => params,
            None => continue,
        };
        let units = demand_units(
            params.base_units_per_tick,
            params.reference_price_minor,
            sku.unit_price_minor,
            expected_demand_micro,
        )
        .map_err(|_| OracleError::Overflow)?;
        let revenue = i64::from(units)
            .checked_mul(sku.unit_price_minor)
            .ok_or(OracleError::Overflow)?;
        expected_revenue_minor = expected_revenue_minor
            .checked_add(revenue)
            .ok_or(OracleError::Overflow)?;
    }
    Ok(ReferenceValuation {
        expected_demand_micro,
        expected_revenue_minor,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::blackbox_sim::director::Session;
    use crate::blackbox_sim::genesis::{Difficulty, GenesisRequest};

    /// The crate denies `unwrap`/`expect` even in tests (see `mod.rs`), so
    /// failures go through this instead: same brevity, no linted panic path.
    trait OrDie<T> {
        fn or_die(self) -> T;
    }

    impl<T, E: std::fmt::Debug> OrDie<T> for Result<T, E> {
        fn or_die(self) -> T {
            match self {
                Ok(value) => value,
                Err(error) => unreachable!("test setup failed: {error:?}"),
            }
        }
    }

    impl<T> OrDie<T> for Option<T> {
        fn or_die(self) -> T {
            match self {
                Some(value) => value,
                None => unreachable!("test expected a value and found none"),
            }
        }
    }

    fn session() -> Session {
        Session::start(GenesisRequest {
            scenario_id: 3,
            difficulty: Difficulty::Standard,
            campaign_index: 0,
            created_date: "2026-07-27".to_string(),
        })
        .or_die()
    }

    fn finish_turn(s: &mut Session) {
        s.submit_timeout_default().or_die();
        s.execute().or_die();
        s.settle().or_die();
        s.report().or_die();
    }

    #[test]
    fn a_reference_exists_from_the_first_observation() {
        let mut s = session();
        s.observe().or_die();
        let ctx = s.context().or_die();
        let v = reference_valuation(s.kernel(), &ctx, &s.books().firm).or_die();
        assert!(v.expected_demand_micro > 0, "demand index is positive");
        assert!(
            v.expected_revenue_minor > 0,
            "an opening SKU set sells something in expectation"
        );
    }

    #[test]
    fn the_reference_is_a_pure_function_of_state() {
        let mut a = session();
        let mut b = session();
        for _ in 0..5 {
            a.observe().or_die();
            b.observe().or_die();
            let va = reference_valuation(a.kernel(), &a.context().or_die(), &a.books().firm)
                .or_die();
            let vb = reference_valuation(b.kernel(), &b.context().or_die(), &b.books().firm)
                .or_die();
            assert_eq!(va, vb, "same campaign, same tick, same reference");
            finish_turn(&mut a);
            finish_turn(&mut b);
        }
    }

    #[test]
    fn raising_price_cannot_raise_expected_units() {
        // The demand curve is downward sloping; this pins the sign so a future
        // refactor of `demand_units` cannot silently invert it.
        let mut s = session();
        s.observe().or_die();
        let ctx = s.context().or_die();
        let base = reference_valuation(s.kernel(), &ctx, &s.books().firm).or_die();
        let sku = ctx.firm_params.skus.first().copied().or_die();
        let units_at = |price: i64| {
            demand_units(
                sku.base_units_per_tick,
                sku.reference_price_minor,
                price,
                base.expected_demand_micro,
            )
            .or_die()
        };
        let cheap = units_at(sku.reference_price_minor / 2);
        let dear = units_at(sku.reference_price_minor * 2);
        assert!(cheap > dear, "cheaper must not sell fewer units");
    }
}
