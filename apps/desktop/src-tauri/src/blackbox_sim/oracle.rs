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
//! # Phase 4: the pricing optimality-gap oracle (lane 5)
//!
//! `pressure_degradation` needs a genuinely different object from the
//! reference valuation above — not the value of the price the player already
//! chose, but the profit-MAXIMISING price they could have chosen instead.
//! [`pricing_optimality_gap`] solves that, using the same demand curve and
//! the same unit-cost formula the compiler charges (`action::unit_cost_minor`)
//! so the "optimal" price is optimal on the identical curve the player faced,
//! never a friendlier one.
//!
//! The demand curve `units(price) = base·index·max(0, 2·ref − price) / (1e6·ref)`
//! (`settle::demand_units`) makes `profit(price) = (price − cost)·units(price)`
//! a downward parabola in `price`, so it has a single unconstrained maximiser
//! at `price* = ref + cost/2`. Two things keep the search closed-form and
//! integer-only rather than an iterative solve:
//!
//! 1. The maximiser of a concave function over an integer domain is always
//!    the floor or the ceiling of its real-valued maximiser (never further
//!    away), so only two candidates need pricing once the continuous vertex
//!    is known.
//! 2. Clamping the continuous vertex into the feasible price box BEFORE
//!    flooring/ceiling turns "the optimum might be outside where the player
//!    is even allowed to price" into "evaluate the box's own edges too" — so
//!    the candidate set `{floor(v), ceil(v), box_min, box_max}` is provably
//!    sufficient for every configuration of cost and reference price,
//!    including the degenerate one where cost exceeds the demand curve's
//!    choke price and the true optimum is to not sell at all.

use crate::blackbox_sim::action::{unit_cost_minor, ActionContext};
use crate::blackbox_sim::firm::{FirmState, MAX_PRICE_MINOR, MIN_PRICE_MINOR};
use crate::blackbox_sim::market::MarketKernel;
use crate::blackbox_sim::settle::demand_units;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OracleError {
    Overflow,
    Kernel,
    UnknownSku { sku: u8 },
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

/// One SKU's profit-maximising price and the cost of the price the player
/// actually chose, both evaluated on the identical unconstrained-demand
/// curve `reference_valuation` uses. Deliberately not `Serialize`: the same
/// wall W-a containment as [`ReferenceValuation`] applies (see module
/// header) — this exposes the true cost curve as clearly as the reference
/// itself does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PricingOptimum {
    pub optimal_price_minor: i64,
    /// Expected profit at `optimal_price_minor`, next tick, unconstrained by
    /// inventory (the same convention as `ReferenceValuation`).
    pub optimal_profit_minor: i64,
    pub actual_profit_minor: i64,
    /// `optimal_profit_minor - actual_profit_minor`. Never negative: the
    /// candidate search always includes the box edges, so nothing the
    /// compiler would have accepted as a legal price can out-earn it.
    pub gap_minor: i64,
}

fn profit_at_price(
    base_units_per_tick: i64,
    reference_price_minor: i64,
    price_minor: i64,
    demand_index_micro: i64,
    cost_minor: i64,
) -> Result<i64, OracleError> {
    let units = demand_units(
        base_units_per_tick,
        reference_price_minor,
        price_minor,
        demand_index_micro,
    )
    .map_err(|_| OracleError::Overflow)?;
    let margin = i128::from(price_minor) - i128::from(cost_minor);
    let profit = margin
        .checked_mul(i128::from(units))
        .ok_or(OracleError::Overflow)?;
    i64::try_from(profit).map_err(|_| OracleError::Overflow)
}

/// Solve for the profit-maximising integer price of `sku` and score the
/// price the player chose against it.
///
/// `chosen_price_minor` need not be `sku`'s currently posted price — callers
/// score the price a `SetPrice` intent just asked for, before or after it is
/// applied, since the oracle only needs the market tick and the firm's cost
/// structure, neither of which a pricing decision changes.
pub fn pricing_optimality_gap(
    kernel: &MarketKernel,
    ctx: &ActionContext,
    sku: u8,
    chosen_price_minor: i64,
) -> Result<PricingOptimum, OracleError> {
    let cfg = ctx
        .firm_params
        .skus
        .get(usize::from(sku))
        .copied()
        .ok_or(OracleError::UnknownSku { sku })?;
    let cost_minor = unit_cost_minor(ctx.market.commodity_price_minor, cfg.cost_multiplier_micro)
        .map_err(|_| OracleError::Overflow)?;
    let demand_index_micro = kernel
        .oracle_expected_next_demand_micro()
        .map_err(|_| OracleError::Kernel)?;
    let box_min = MIN_PRICE_MINOR;
    let box_max = MAX_PRICE_MINOR.min(cfg.reference_price_minor.saturating_mul(2));
    let vertex_floor = cfg.reference_price_minor.saturating_add(cost_minor / 2);
    let vertex_ceil = vertex_floor.saturating_add(i64::from(cost_minor % 2 != 0));
    let mut candidates = [vertex_floor, vertex_ceil, box_min, box_max];
    for c in &mut candidates {
        *c = (*c).clamp(box_min, box_max);
    }
    let mut optimal_price_minor = box_min;
    let mut optimal_profit_minor = i64::MIN;
    for &price in &candidates {
        let profit = profit_at_price(
            cfg.base_units_per_tick,
            cfg.reference_price_minor,
            price,
            demand_index_micro,
            cost_minor,
        )?;
        if profit > optimal_profit_minor {
            optimal_profit_minor = profit;
            optimal_price_minor = price;
        }
    }
    let actual_profit_minor = profit_at_price(
        cfg.base_units_per_tick,
        cfg.reference_price_minor,
        chosen_price_minor,
        demand_index_micro,
        cost_minor,
    )?;
    Ok(PricingOptimum {
        optimal_price_minor,
        optimal_profit_minor,
        actual_profit_minor,
        gap_minor: optimal_profit_minor.saturating_sub(actual_profit_minor),
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

    #[test]
    fn the_optimum_never_loses_to_a_legal_price() {
        let mut s = session();
        s.observe().or_die();
        let ctx = s.context().or_die();
        for price in [1_i64, 500, 5_000, 50_000, 9_999_999, MAX_PRICE_MINOR] {
            let g = pricing_optimality_gap(s.kernel(), &ctx, 0, price).or_die();
            assert!(
                g.gap_minor >= 0,
                "price {price} beat the claimed optimum by {}",
                -g.gap_minor
            );
            assert_eq!(g.optimal_profit_minor - g.actual_profit_minor, g.gap_minor);
            assert!(
                (MIN_PRICE_MINOR..=MAX_PRICE_MINOR).contains(&g.optimal_price_minor),
                "the oracle must not recommend a price the compiler would refuse"
            );
        }
    }

    #[test]
    fn a_price_at_the_optimum_has_zero_gap() {
        let mut s = session();
        s.observe().or_die();
        let ctx = s.context().or_die();
        let g = pricing_optimality_gap(s.kernel(), &ctx, 0, 1).or_die();
        let at_optimum =
            pricing_optimality_gap(s.kernel(), &ctx, 0, g.optimal_price_minor).or_die();
        assert_eq!(at_optimum.gap_minor, 0);
        assert_eq!(at_optimum.optimal_price_minor, g.optimal_price_minor);
    }

    #[test]
    fn the_optimum_is_a_pure_function_of_state_not_of_the_price_asked_about() {
        let mut s = session();
        s.observe().or_die();
        let ctx = s.context().or_die();
        let a = pricing_optimality_gap(s.kernel(), &ctx, 0, 1).or_die();
        let b = pricing_optimality_gap(s.kernel(), &ctx, 0, MAX_PRICE_MINOR).or_die();
        assert_eq!(a.optimal_price_minor, b.optimal_price_minor);
        assert_eq!(a.optimal_profit_minor, b.optimal_profit_minor);
    }

    #[test]
    fn an_unpriceable_sku_fails_closed() {
        let mut s = session();
        s.observe().or_die();
        let ctx = s.context().or_die();
        assert!(matches!(
            pricing_optimality_gap(s.kernel(), &ctx, 250, 1_000),
            Err(OracleError::UnknownSku { sku: 250 })
        ));
    }
}
