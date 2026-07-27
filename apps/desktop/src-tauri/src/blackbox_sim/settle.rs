//! Operating engine, period close, and the cash-flow statement (SPEC §6.2,
//! BXS-I-03).
//!
//! # Why the cash-flow identity here is a theorem, not a reconciliation
//!
//! Every account except `Cash` is assigned to exactly one cash-flow section by
//! [`section_of`], an exhaustive match with no wildcard arm. Because the
//! ledger is zero-sum by construction (BXS-I-02), the signed balances satisfy
//!
//! ```text
//!     Δ(Cash) + Σ_{a ≠ Cash} Δ(a) = 0
//! ```
//!
//! and each section total is defined as `−Σ Δ(a)` over its own accounts. The
//! three sections therefore sum to `−Σ_{a ≠ Cash} Δ(a)`, which is `Δ(Cash)`.
//! The identity cannot drift by a rounding unit because nothing is rounded:
//! it is a partition of integers that already balance.
//!
//! Grouping the operating accounts recovers the ordinary indirect-method
//! presentation — net income, plus depreciation added back, less the change in
//! working capital — so this is a real statement, not a plug. What it buys
//! over the hand-assembled version is that a NEW ACCOUNT CANNOT BE FORGOTTEN:
//! adding a variant to `AccountCode` fails to compile until it is classified.
//!
//! The check is still performed at every close (第六律: the receiver verifies
//! even what the sender proved). A failure is not a rounding complaint — it
//! means a balance moved outside the double-entry path, so it raises
//! [`SettleError::AccountingBreach`], which the driver turns into a dead
//! session rather than a repair (SPEC §6.2: no self-healing books).
//!
//! # On engine-authored postings and wall W-d
//!
//! Depreciation, interest, amortisation, tax and revenue recognition are
//! posted by this module without passing through `compile`. That is not a hole
//! in wall W-d: the wall governs OUTSIDE-ORIGINATED intents, and these are
//! consequences of the calendar and of prior decisions, not decisions. They
//! are still applied through `apply_plans`, so they face the same staging,
//! zero-sum and identity checks as anything a player submits.

use super::action::{apply_plans, checked_amount, transfer, ActionContext, CompileError, SimBooks};
use super::firm::{FirmEffect, FirmError};
use super::ledger::{AccountCode, Balances, TxKind};
use super::money::{floor_half_up, MoneyError};
use super::{action::ActionPlan, genesis::MAX_SKUS};

/// Ticks in a settlement period. A quarter of weeks — chosen so a campaign of
/// a few dozen ticks spans several closes and the estimator sees repeated
/// decisions under changed conditions.
pub const TICKS_PER_QUARTER: u32 = 13;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CfSection {
    Operating,
    Investing,
    Financing,
}

/// Statement classification of every account. `Cash` maps to `None` because it
/// is the subject of the statement, not a line in it.
///
/// Exhaustive on purpose — do NOT add a wildcard arm. The compile error you
/// get when introducing an account is the only thing standing between a new
/// balance and a silently unexplained change in cash.
#[must_use]
pub const fn section_of(account: AccountCode) -> Option<CfSection> {
    match account {
        AccountCode::Cash => None,
        AccountCode::AccountsReceivable
        | AccountCode::Inventory
        | AccountCode::PrepaidExpenses
        | AccountCode::AccumulatedDepreciation
        | AccountCode::AccountsPayable
        | AccountCode::AccruedLiabilities
        | AccountCode::TaxPayable
        | AccountCode::SalesRevenue
        | AccountCode::OtherIncome
        | AccountCode::CostOfGoodsSold
        | AccountCode::OperatingExpense
        | AccountCode::DepreciationExpense
        | AccountCode::InterestExpense
        | AccountCode::TaxExpense => Some(CfSection::Operating),
        AccountCode::PropertyPlantEquipment => Some(CfSection::Investing),
        AccountCode::SeniorDebt
        | AccountCode::MezzanineDebt
        | AccountCode::ShareCapital
        | AccountCode::RetainedEarnings => Some(CfSection::Financing),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettleError {
    /// BXS-I-03 violated: the statement and the cash account disagree. The
    /// session must die; the books must not be "corrected".
    AccountingBreach {
        statement_minor: i128,
        cash_minor: i128,
    },
    /// BXS-I-17 violated: the firm's carrying value and the `Inventory`
    /// account have diverged.
    InventoryDesync {
        ledger_minor: i64,
        firm_minor: i64,
    },
    Compile(CompileError),
    Firm(FirmError),
    Money(MoneyError),
    Overflow,
}

impl From<CompileError> for SettleError {
    fn from(e: CompileError) -> Self {
        SettleError::Compile(e)
    }
}

impl From<FirmError> for SettleError {
    fn from(e: FirmError) -> Self {
        SettleError::Firm(e)
    }
}

impl From<MoneyError> for SettleError {
    fn from(e: MoneyError) -> Self {
        SettleError::Money(e)
    }
}

/// Indirect-method statement for one period, in minor units.
///
/// `net_change_minor` is derived from the section partition; `cash_delta_minor`
/// is read straight off the `Cash` account. They are computed by different
/// routes precisely so that comparing them means something (§16.6 — two
/// wrappers around one computation agreeing proves nothing).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CashFlowStatement {
    pub operating_minor: i64,
    pub investing_minor: i64,
    pub financing_minor: i64,
    pub net_change_minor: i64,
    pub cash_delta_minor: i64,
    pub net_income_minor: i64,
    pub depreciation_minor: i64,
}

fn narrow(v: i128) -> Result<i64, SettleError> {
    i64::try_from(v).map_err(|_| SettleError::Overflow)
}

impl CashFlowStatement {
    pub fn between(begin: &Balances, end: &Balances) -> Result<Self, SettleError> {
        let mut operating: i128 = 0;
        let mut investing: i128 = 0;
        let mut financing: i128 = 0;
        let mut cash_delta: i128 = 0;
        for account in AccountCode::ALL {
            let delta =
                i128::from(end.balance_minor(account)) - i128::from(begin.balance_minor(account));
            match section_of(account) {
                None => cash_delta += delta,
                Some(CfSection::Operating) => operating -= delta,
                Some(CfSection::Investing) => investing -= delta,
                Some(CfSection::Financing) => financing -= delta,
            }
        }
        let b = begin.class_totals();
        let e = end.class_totals();
        let net_income = (e.revenue - e.expense) - (b.revenue - b.expense);
        // Depreciation charged in the period is the growth of the contra-asset,
        // whose signed balance is negative — hence the negation.
        let depreciation = -(i128::from(end.balance_minor(AccountCode::AccumulatedDepreciation))
            - i128::from(begin.balance_minor(AccountCode::AccumulatedDepreciation)));
        Ok(Self {
            operating_minor: narrow(operating)?,
            investing_minor: narrow(investing)?,
            financing_minor: narrow(financing)?,
            net_change_minor: narrow(operating + investing + financing)?,
            cash_delta_minor: narrow(cash_delta)?,
            net_income_minor: narrow(net_income)?,
            depreciation_minor: narrow(depreciation)?,
        })
    }

    /// BXS-I-03. Fails closed and loudly; there is no repair path.
    pub fn verify(&self) -> Result<(), SettleError> {
        if self.net_change_minor == self.cash_delta_minor {
            Ok(())
        } else {
            Err(SettleError::AccountingBreach {
                statement_minor: i128::from(self.net_change_minor),
                cash_minor: i128::from(self.cash_delta_minor),
            })
        }
    }
}

/// What one product line did this tick.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LineResult {
    pub sku: u8,
    pub units_sold: u32,
    pub units_demanded: u32,
    pub revenue_minor: i64,
    pub cogs_minor: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TickResult {
    pub lines: Vec<LineResult>,
    pub revenue_minor: i64,
    pub cogs_minor: i64,
    /// Demand the firm could not serve. This is the stockout signal the
    /// player is meant to feel without being told the parameters behind it.
    pub unmet_units: u32,
}

/// Linear demand around the reference price, scaled by the market's demand
/// index:
///
/// ```text
///     units = base × index × max(0, 2·ref − price) / (1e6 × ref)
/// ```
///
/// At the reference price the line sells `base × index`; at twice the
/// reference it sells nothing. Deliberately a closed integer form rather than
/// an elasticity exponent — a `powf` here would put a libm transcendental on
/// the authoritative path and forfeit cross-platform bit identity (BXS-W-02).
/// Shared with `oracle.rs` on purpose: if the reference valuation used a
/// different demand curve than realised sales, the anchoring measurement would
/// be contaminated by a modelling mismatch rather than by the player's bias.
pub(super) fn demand_units(
    base_units: i64,
    reference_price_minor: i64,
    price_minor: i64,
    demand_index_micro: i64,
) -> Result<u32, SettleError> {
    if reference_price_minor <= 0 || base_units <= 0 {
        return Ok(0);
    }
    let headroom = reference_price_minor
        .saturating_mul(2)
        .saturating_sub(price_minor)
        .max(0);
    let num = i128::from(base_units)
        .checked_mul(i128::from(demand_index_micro.max(0)))
        .and_then(|v| v.checked_mul(i128::from(headroom)))
        .ok_or(SettleError::Overflow)?;
    let den = 1_000_000_i128
        .checked_mul(i128::from(reference_price_minor))
        .ok_or(SettleError::Overflow)?;
    let units = floor_half_up(num, den)?;
    u32::try_from(units.max(0)).map_err(|_| SettleError::Overflow)
}

/// Derive this tick's sales without touching anything. Pure, so the caller can
/// stage the whole tick as one act.
fn plan_operations(
    ctx: &ActionContext,
    books: &SimBooks,
) -> Result<(Vec<ActionPlan>, TickResult), SettleError> {
    let mut plans = Vec::with_capacity(MAX_SKUS);
    let mut lines = Vec::with_capacity(MAX_SKUS);
    let mut revenue_total: i64 = 0;
    let mut cogs_total: i64 = 0;
    let mut unmet_units: u32 = 0;

    for (sku_id, state) in books.firm.skus() {
        let cfg = match ctx.firm_params.skus.get(usize::from(sku_id)) {
            Some(c) => *c,
            None => continue,
        };
        let demanded = demand_units(
            cfg.base_units_per_tick,
            cfg.reference_price_minor,
            state.unit_price_minor,
            ctx.market.demand_index_micro,
        )?;
        let sold = demanded.min(state.inventory_units);
        unmet_units = unmet_units.saturating_add(demanded.saturating_sub(sold));
        if sold == 0 {
            lines.push(LineResult {
                sku: sku_id,
                units_sold: 0,
                units_demanded: demanded,
                revenue_minor: 0,
                cogs_minor: 0,
            });
            continue;
        }
        let revenue = i128::from(state.unit_price_minor)
            .checked_mul(i128::from(sold))
            .ok_or(SettleError::Overflow)?;
        let revenue_minor = narrow(revenue)?;
        let cogs_minor = books.firm.cogs_for(sku_id, sold)?;

        // Recognition and cost relief are ONE transaction: a crash between
        // them would leave revenue without its matching cost.
        let mut postings = transfer(
            TxKind::RecognizeSale,
            AccountCode::Cash,
            AccountCode::SalesRevenue,
            checked_amount(revenue_minor)?,
        )?;
        if cogs_minor > 0 {
            postings = super::ledger::Transaction::new(
                TxKind::RecognizeSale,
                postings
                    .postings()
                    .iter()
                    .copied()
                    .chain(
                        transfer(
                            TxKind::RecognizeSale,
                            AccountCode::CostOfGoodsSold,
                            AccountCode::Inventory,
                            checked_amount(cogs_minor)?,
                        )?
                        .postings()
                        .iter()
                        .copied(),
                    )
                    .collect(),
            )
            .map_err(|e| SettleError::Compile(CompileError::Ledger(e)))?;
        }
        plans.push(ActionPlan {
            transaction: Some(postings),
            firm_effect: FirmEffect::ConsumeInventory {
                sku: sku_id,
                units: sold,
                cogs_minor,
            },
        });
        revenue_total = revenue_total
            .checked_add(revenue_minor)
            .ok_or(SettleError::Overflow)?;
        cogs_total = cogs_total
            .checked_add(cogs_minor)
            .ok_or(SettleError::Overflow)?;
        lines.push(LineResult {
            sku: sku_id,
            units_sold: sold,
            units_demanded: demanded,
            revenue_minor,
            cogs_minor,
        });
    }

    Ok((
        plans,
        TickResult {
            lines,
            revenue_minor: revenue_total,
            cogs_minor: cogs_total,
            unmet_units,
        },
    ))
}

/// Run one tick of operations. Atomic across all product lines.
pub fn operate_tick(ctx: &ActionContext, books: &mut SimBooks) -> Result<TickResult, SettleError> {
    let (plans, result) = plan_operations(ctx, books)?;
    apply_plans(&plans, books)?;
    verify_inventory_reconciled(books)?;
    Ok(result)
}

/// BXS-I-17 at the boundary: the two representations of inventory value must
/// agree to the minor unit.
pub fn verify_inventory_reconciled(books: &SimBooks) -> Result<(), SettleError> {
    let ledger_minor = books.balances.balance_minor(AccountCode::Inventory);
    let firm_minor = books.firm.inventory_value_minor();
    if ledger_minor == firm_minor {
        Ok(())
    } else {
        Err(SettleError::InventoryDesync {
            ledger_minor,
            firm_minor,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PeriodClose {
    pub depreciation_minor: i64,
    pub interest_minor: i64,
    pub amortised_prepaid_minor: i64,
    pub tax_minor: i64,
    pub cash_flow: CashFlowStatement,
}

fn apply_bp(base_minor: i64, rate_bp: u32) -> Result<i64, SettleError> {
    if base_minor <= 0 || rate_bp == 0 {
        return Ok(0);
    }
    let num = i128::from(base_minor)
        .checked_mul(i128::from(rate_bp))
        .ok_or(SettleError::Overflow)?;
    narrow(floor_half_up(num, 10_000)?)
}

/// Close the period: depreciate, accrue interest, consume prepaid cover, then
/// tax whatever profit survives.
///
/// All four postings are staged together. Tax is derived analytically from the
/// other three rather than being computed after posting them, because a close
/// that half-applied would produce a period whose tax does not correspond to
/// its own income — an error that reads as plausible and would never be
/// noticed.
pub fn close_period(
    ctx: &ActionContext,
    books: &mut SimBooks,
    period_begin: &Balances,
) -> Result<PeriodClose, SettleError> {
    let params = ctx.firm_params;
    let mut plans: Vec<ActionPlan> = Vec::with_capacity(4);

    // 1. Depreciation, capped at remaining net book value: an asset cannot be
    //    written down past zero, and this cap is the accounting rule itself,
    //    not a convenience clamp over a bad input.
    let gross_ppe = books
        .balances
        .balance_minor(AccountCode::PropertyPlantEquipment);
    let accumulated = -books
        .balances
        .balance_minor(AccountCode::AccumulatedDepreciation);
    let net_book = gross_ppe.saturating_sub(accumulated).max(0);
    let depreciation_minor = apply_bp(gross_ppe, params.depreciation_rate_bp)?.min(net_book);
    if depreciation_minor > 0 {
        plans.push(ActionPlan {
            transaction: Some(transfer(
                TxKind::RecordDepreciation,
                AccountCode::DepreciationExpense,
                AccountCode::AccumulatedDepreciation,
                checked_amount(depreciation_minor)?,
            )?),
            firm_effect: FirmEffect::None,
        });
    }

    // 2. Interest on both tranches, accrued rather than paid — the cash leaves
    //    only when the player services the debt.
    let senior = -books.balances.balance_minor(AccountCode::SeniorDebt);
    let mezz = -books.balances.balance_minor(AccountCode::MezzanineDebt);
    let interest_minor = apply_bp(senior, params.senior_rate_bp)?
        .checked_add(apply_bp(mezz, params.mezz_rate_bp)?)
        .ok_or(SettleError::Overflow)?;
    if interest_minor > 0 {
        plans.push(ActionPlan {
            transaction: Some(transfer(
                TxKind::AccrueInterest,
                AccountCode::InterestExpense,
                AccountCode::AccruedLiabilities,
                checked_amount(interest_minor)?,
            )?),
            firm_effect: FirmEffect::None,
        });
    }

    // 3. Prepaid cover buys exactly one period, so the whole balance is
    //    consumed at the close that follows its purchase.
    let amortised_prepaid_minor = books.balances.balance_minor(AccountCode::PrepaidExpenses).max(0);
    if amortised_prepaid_minor > 0 {
        plans.push(ActionPlan {
            transaction: Some(transfer(
                TxKind::PayOpex,
                AccountCode::OperatingExpense,
                AccountCode::PrepaidExpenses,
                checked_amount(amortised_prepaid_minor)?,
            )?),
            firm_effect: FirmEffect::None,
        });
    }

    // 4. Tax on income after the three charges above.
    let begin = period_begin.class_totals();
    let now = books.balances.class_totals();
    let pretax = (now.revenue - now.expense)
        - (begin.revenue - begin.expense)
        - i128::from(depreciation_minor)
        - i128::from(interest_minor)
        - i128::from(amortised_prepaid_minor);
    let tax_minor = if pretax > 0 {
        apply_bp(narrow(pretax)?, params.tax_rate_bp)?
    } else {
        // Losses are not refunded and no carry-forward asset is recognised:
        // recognising one would require judging future profitability, which
        // this engine has no business asserting.
        0
    };
    if tax_minor > 0 {
        plans.push(ActionPlan {
            transaction: Some(transfer(
                TxKind::PayTax,
                AccountCode::TaxExpense,
                AccountCode::TaxPayable,
                checked_amount(tax_minor)?,
            )?),
            firm_effect: FirmEffect::None,
        });
    }

    apply_plans(&plans, books)?;
    verify_inventory_reconciled(books)?;

    let cash_flow = CashFlowStatement::between(period_begin, &books.balances)?;
    cash_flow.verify()?;

    Ok(PeriodClose {
        depreciation_minor,
        interest_minor,
        amortised_prepaid_minor,
        tax_minor,
        cash_flow,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::blackbox_sim::action::execute_intent;
    use crate::blackbox_sim::firm::{Offer, OfferKind, OfferStatus};
    use crate::blackbox_sim::genesis::{build_campaign_genesis, Difficulty, GenesisRequest};
    use crate::blackbox_sim::market::MarketKernel;
    use crate::blackbox_sim::telemetry::ActionIntent;

    fn ok<T, E: std::fmt::Debug>(r: Result<T, E>) -> T {
        match r {
            Ok(v) => v,
            Err(e) => unreachable!("settle test setup failed: {e:?}"),
        }
    }

    fn fixture() -> (ActionContext, SimBooks, MarketKernel) {
        let g = ok(build_campaign_genesis(GenesisRequest {
            scenario_id: 7,
            difficulty: Difficulty::Standard,
            campaign_index: 0,
            created_date: "2026-07-27".to_string(),
        }));
        let mut kernel = ok(MarketKernel::new(&g));
        let market = ok(kernel.step());
        let mut books = ok(SimBooks::new(&g.firm));
        ok(books.seed_capital(g.firm.opening_capital_minor));
        (
            ActionContext {
                market,
                firm_params: g.firm,
            },
            books,
            kernel,
        )
    }

    #[test]
    fn every_account_has_exactly_one_home() {
        let mut classified = 0;
        for account in AccountCode::ALL {
            match section_of(account) {
                None => assert_eq!(account, AccountCode::Cash),
                Some(_) => classified += 1,
            }
        }
        assert_eq!(
            classified,
            AccountCode::ALL.len() - 1,
            "every account but Cash must sit in exactly one section"
        );
    }

    #[test]
    fn an_empty_period_produces_a_zero_statement() {
        let (_ctx, books, _k) = fixture();
        let cf = ok(CashFlowStatement::between(&books.balances, &books.balances));
        ok(cf.verify());
        assert_eq!(cf.net_change_minor, 0);
        assert_eq!(cf.cash_delta_minor, 0);
        assert_eq!(cf.net_income_minor, 0);
    }

    #[test]
    fn financing_and_investing_land_in_their_own_sections() {
        let (ctx, mut books, _k) = fixture();
        let begin = books.balances.clone();
        let _ = ok(execute_intent(
            ActionIntent::Borrow {
                facility: 0,
                amount_minor: 400_000,
            },
            &ctx,
            &mut books,
        ));
        let _ = ok(execute_intent(
            ActionIntent::Invest {
                project_id: 0,
                amount_minor: 250_000,
            },
            &ctx,
            &mut books,
        ));
        let cf = ok(CashFlowStatement::between(&begin, &books.balances));
        ok(cf.verify());
        assert_eq!(cf.financing_minor, 400_000, "a debt draw is financing in");
        assert_eq!(cf.investing_minor, -250_000, "capex is investing out");
        assert_eq!(cf.operating_minor, 0);
        assert_eq!(cf.net_change_minor, 150_000);
        assert_eq!(cf.cash_delta_minor, 150_000);
    }

    #[test]
    fn selling_shows_up_as_operating_cash() {
        let (ctx, mut books, _k) = fixture();
        let _ = ok(execute_intent(
            ActionIntent::OrderInventory { sku: 0, units: 500 },
            &ctx,
            &mut books,
        ));
        let begin = books.balances.clone();
        let tick = ok(operate_tick(&ctx, &mut books));
        assert!(tick.revenue_minor > 0, "the firm must actually sell something");
        let cf = ok(CashFlowStatement::between(&begin, &books.balances));
        ok(cf.verify());
        assert_eq!(cf.investing_minor, 0);
        assert_eq!(cf.financing_minor, 0);
        assert_eq!(cf.operating_minor, cf.cash_delta_minor);
        assert_eq!(
            cf.net_income_minor,
            tick.revenue_minor - tick.cogs_minor,
            "net income for a pure trading tick is gross margin"
        );
    }

    #[test]
    fn demand_falls_as_price_rises_and_dies_at_twice_reference() {
        let base = 1_000;
        let reference = 10_000;
        let at_ref = ok(demand_units(base, reference, reference, 1_000_000));
        let cheap = ok(demand_units(base, reference, reference / 2, 1_000_000));
        let dear = ok(demand_units(base, reference, reference * 3 / 2, 1_000_000));
        let choked = ok(demand_units(base, reference, reference * 2, 1_000_000));
        assert_eq!(at_ref, 1_000);
        assert!(cheap > at_ref && at_ref > dear && dear > choked);
        assert_eq!(choked, 0);
        assert_eq!(
            ok(demand_units(base, reference, reference * 5, 1_000_000)),
            0,
            "an absurd price must not produce negative demand"
        );
    }

    #[test]
    fn stockouts_are_reported_not_conjured() {
        let (ctx, mut books, _k) = fixture();
        let _ = ok(execute_intent(
            ActionIntent::OrderInventory { sku: 0, units: 5 },
            &ctx,
            &mut books,
        ));
        let tick = ok(operate_tick(&ctx, &mut books));
        assert!(tick.unmet_units > 0, "demand should exceed five units");
        assert_eq!(ok(books.firm.sku(0)).inventory_units, 0);
        // BXS-I-17: an emptied line leaves no residue behind.
        assert_eq!(ok(books.firm.sku(0)).inventory_value_minor, 0);
        assert_eq!(books.balances.balance_minor(AccountCode::Inventory), 0);
        ok(verify_inventory_reconciled(&books));
    }

    #[test]
    fn a_close_charges_depreciation_interest_and_tax() {
        let (ctx, mut books, _k) = fixture();
        let _ = ok(execute_intent(
            ActionIntent::Borrow {
                facility: 0,
                amount_minor: 2_000_000,
            },
            &ctx,
            &mut books,
        ));
        let _ = ok(execute_intent(
            ActionIntent::Invest {
                project_id: 0,
                amount_minor: 1_000_000,
            },
            &ctx,
            &mut books,
        ));
        let _ = ok(execute_intent(
            ActionIntent::OrderInventory {
                sku: 0,
                units: 2_000,
            },
            &ctx,
            &mut books,
        ));
        let begin = books.balances.clone();
        let _ = ok(operate_tick(&ctx, &mut books));
        let close = ok(close_period(&ctx, &mut books, &begin));
        assert!(close.depreciation_minor > 0, "capitalised PPE must depreciate");
        assert!(close.interest_minor > 0, "drawn debt must accrue interest");
        ok(close.cash_flow.verify());
        assert_eq!(
            close.cash_flow.depreciation_minor, close.depreciation_minor,
            "the addback must equal the charge"
        );
        // Accrued interest and tax are non-cash this period, so operating cash
        // must exceed net income by the accruals plus the depreciation addback.
        assert!(close.cash_flow.operating_minor > close.cash_flow.net_income_minor);
    }

    #[test]
    fn depreciation_never_drives_book_value_below_zero() {
        let (ctx, mut books, _k) = fixture();
        let _ = ok(execute_intent(
            ActionIntent::Invest {
                project_id: 0,
                amount_minor: 100_000,
            },
            &ctx,
            &mut books,
        ));
        for _ in 0..200 {
            let begin = books.balances.clone();
            let _ = ok(close_period(&ctx, &mut books, &begin));
        }
        let gross = books
            .balances
            .balance_minor(AccountCode::PropertyPlantEquipment);
        let accumulated = -books
            .balances
            .balance_minor(AccountCode::AccumulatedDepreciation);
        assert_eq!(
            accumulated, gross,
            "the asset should be exactly fully depreciated, never over-depreciated"
        );
    }

    #[test]
    fn a_loss_making_period_is_not_taxed() {
        let (ctx, mut books, _k) = fixture();
        let _ = ok(execute_intent(
            ActionIntent::Borrow {
                facility: 1,
                amount_minor: 5_000_000,
            },
            &ctx,
            &mut books,
        ));
        let begin = books.balances.clone();
        // No sales, but interest accrues — the period is squarely loss-making.
        let close = ok(close_period(&ctx, &mut books, &begin));
        assert!(close.interest_minor > 0);
        assert_eq!(close.tax_minor, 0);
        assert!(close.cash_flow.net_income_minor < 0);
        ok(close.cash_flow.verify());
    }

    #[test]
    fn prepaid_cover_is_consumed_at_the_next_close() {
        let (ctx, mut books, _k) = fixture();
        let id = ok(books.firm.place_offer(Offer {
            kind: OfferKind::Insurance,
            cost_minor: 300_000,
            status: OfferStatus::Open,
        }));
        let _ = ok(execute_intent(
            ActionIntent::AcceptOffer { offer_id: id },
            &ctx,
            &mut books,
        ));
        assert_eq!(
            books.balances.balance_minor(AccountCode::PrepaidExpenses),
            300_000
        );
        let begin = books.balances.clone();
        let close = ok(close_period(&ctx, &mut books, &begin));
        assert_eq!(close.amortised_prepaid_minor, 300_000);
        assert_eq!(books.balances.balance_minor(AccountCode::PrepaidExpenses), 0);
        ok(close.cash_flow.verify());
    }

    /// The identity has to survive a period in which everything happens at
    /// once — that is the case where a hand-assembled statement drifts.
    #[test]
    fn the_identity_survives_a_period_with_every_kind_of_event() {
        let (mut ctx, mut books, mut kernel) = fixture();
        let begin = books.balances.clone();
        let offer = ok(books.firm.place_offer(Offer {
            kind: OfferKind::Insurance,
            cost_minor: 90_000,
            status: OfferStatus::Open,
        }));
        let intents = [
            ActionIntent::Borrow {
                facility: 0,
                amount_minor: 3_000_000,
            },
            ActionIntent::Borrow {
                facility: 1,
                amount_minor: 1_500_000,
            },
            ActionIntent::Invest {
                project_id: 0,
                amount_minor: 800_000,
            },
            ActionIntent::Invest {
                project_id: 1,
                amount_minor: 200_000,
            },
            ActionIntent::AcceptOffer { offer_id: offer },
            ActionIntent::SetPrice {
                sku: 1,
                tick_price: 7_777,
            },
            ActionIntent::OrderInventory {
                sku: 0,
                units: 1_200,
            },
            ActionIntent::OrderInventory { sku: 1, units: 900 },
            ActionIntent::OrderInventory { sku: 2, units: 300 },
            ActionIntent::OpenHedge {
                instrument: 0,
                notional_minor: 500_000,
            },
            ActionIntent::Repay {
                facility: 0,
                amount_minor: 400_000,
            },
            ActionIntent::AbandonProject { project_id: 1 },
            ActionIntent::ForecastInterval {
                lo_minor: 10,
                hi_minor: 20,
            },
            ActionIntent::Abstain,
        ];
        for intent in intents {
            let _ = ok(execute_intent(intent, &ctx, &mut books));
        }
        for _ in 0..TICKS_PER_QUARTER {
            ctx = ActionContext {
                market: ok(kernel.step()),
                firm_params: ctx.firm_params,
            };
            let _ = ok(operate_tick(&ctx, &mut books));
        }
        let _ = ok(execute_intent(
            ActionIntent::ClosePosition { position_id: 0 },
            &ctx,
            &mut books,
        ));
        let close = ok(close_period(&ctx, &mut books, &begin));
        ok(close.cash_flow.verify());
        ok(books.balances.verify_zero_sum());
        ok(books.balances.verify_accounting_identity());
        ok(verify_inventory_reconciled(&books));
        let cf = close.cash_flow;
        assert_eq!(
            cf.operating_minor + cf.investing_minor + cf.financing_minor,
            cf.cash_delta_minor
        );
        assert!(cf.operating_minor != 0 && cf.investing_minor != 0 && cf.financing_minor != 0);
    }

    /// A breach must be detectable, so prove the check can actually fail:
    /// a statement whose two independently-derived sides disagree is refused.
    #[test]
    fn a_disagreeing_statement_is_refused() {
        let (_ctx, books, _k) = fixture();
        let mut cf = ok(CashFlowStatement::between(&books.balances, &books.balances));
        cf.net_change_minor += 1;
        assert!(matches!(
            cf.verify(),
            Err(SettleError::AccountingBreach {
                statement_minor: 1,
                cash_minor: 0
            })
        ));
    }

    #[test]
    fn inventory_desync_is_detected() {
        let (ctx, mut books, _k) = fixture();
        let _ = ok(execute_intent(
            ActionIntent::OrderInventory { sku: 0, units: 10 },
            &ctx,
            &mut books,
        ));
        ok(verify_inventory_reconciled(&books));
        // Corrupt only the ledger side and confirm the guard notices.
        let tamper = ok(transfer(
            TxKind::PurchaseInventory,
            AccountCode::Inventory,
            AccountCode::Cash,
            ok(checked_amount(1)),
        ));
        ok(books.balances.apply(&tamper));
        assert!(matches!(
            verify_inventory_reconciled(&books),
            Err(SettleError::InventoryDesync { .. })
        ));
    }
}
