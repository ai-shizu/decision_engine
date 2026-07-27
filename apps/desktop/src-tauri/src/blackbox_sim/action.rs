//! ActionCompiler — the implementation of wall W-d (SPEC §2, §6.1).
//!
//! This module is the ONLY place where an outside-originated `ActionIntent`
//! becomes a ledger `Transaction` or a change to the operating state. UI and
//! LLM submit intents from the closed catalog and nothing else; they can
//! neither name accounts nor choose sides nor invent amounts. Every intent is
//! classified exhaustively — the `match` in `compile` has no wildcard arm, so
//! adding a variant to `ActionIntent` breaks the build rather than silently
//! falling through to a no-op.
//!
//! Compilation is a PURE FUNCTION into an `ActionPlan`. Nothing mutates while
//! the plan is being derived, which is what makes the whole action stageable:
//! `execute_intent` builds the plan, applies it to private copies of the
//! balances and the firm state, and only commits after the journal has
//! accepted the entry (BXS-I-16). A refusal at any point costs nothing and
//! leaves all three stores exactly as they were.
//!
//! Phase 2 lifted the Phase 1 deferral: with `FirmState` in place, every
//! intent in the catalog now compiles. `UnsupportedInPhase1` is gone — a
//! refusal today is a real business rule (no stock, no cash, dead project),
//! never "not built yet".

use super::firm::{
    position_pnl_minor, FirmEffect, FirmError, FirmState, OfferKind, OfferStatus, ProjectStatus,
    MAX_ORDER_UNITS, MAX_PRICE_MINOR, MIN_PRICE_MINOR,
};
use super::genesis::FirmParams;
use super::ledger::{
    AccountCode, Balances, Journal, LedgerError, Posting, Side, Transaction, TxKind,
};
use super::market::MarketTickView;
use super::money::{floor_half_up, Money, MoneyError};
use super::telemetry::ActionIntent;

/// Ceiling on any single action amount, enforced before a `Money` is built
/// (第六律 4: limits bite before construction, not after). 10 billion CRD is
/// far above any legitimate campaign move and far below i64 overflow, so the
/// downstream i128 accumulations cannot be driven anywhere near their bounds.
pub const MAX_ACTION_AMOUNT_MINOR: i64 = 1_000_000_000_000;

/// Debt facilities addressable by `Borrow` / `Repay`. Wire ids are frozen
/// forever (BXS-I-13): never renumber, never reuse a retired number.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum Facility {
    Senior = 0,
    Mezzanine = 1,
}

impl Facility {
    fn from_wire(v: u8) -> Result<Self, CompileError> {
        match v {
            0 => Ok(Facility::Senior),
            1 => Ok(Facility::Mezzanine),
            other => Err(CompileError::UnknownFacility { facility: other }),
        }
    }

    #[must_use]
    pub const fn account(self) -> AccountCode {
        match self {
            Facility::Senior => AccountCode::SeniorDebt,
            Facility::Mezzanine => AccountCode::MezzanineDebt,
        }
    }
}

/// Everything compilation needs from outside the books: the published market
/// tick and the frozen firm constants.
///
/// Note what is absent — this carries `MarketTickView`, the wall W-a boundary
/// object, NOT `CampaignGenesis`. The compiler cannot see true drift, true
/// value, or the optimal play even by accident, because those types never
/// enter its signature (§4.57-style type-level isolation).
#[derive(Debug, Clone, Copy)]
pub struct ActionContext {
    pub market: MarketTickView,
    pub firm_params: FirmParams,
}

/// The compiled result: at most one balanced transaction plus one operating
/// mutation. Both are data, so a plan can be inspected and asserted on without
/// executing it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActionPlan {
    pub transaction: Option<Transaction>,
    pub firm_effect: FirmEffect,
}

impl ActionPlan {
    fn inert() -> Self {
        Self {
            transaction: None,
            firm_effect: FirmEffect::None,
        }
    }

    fn firm_only(firm_effect: FirmEffect) -> Self {
        Self {
            transaction: None,
            firm_effect,
        }
    }

    #[must_use]
    pub fn moves_money(&self) -> bool {
        self.transaction.is_some()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompileError {
    AmountOutOfRange {
        got_minor: i64,
    },
    InvalidForecastInterval {
        lo_minor: i64,
        hi_minor: i64,
    },
    UnknownFacility {
        facility: u8,
    },
    InsufficientCash {
        needed_minor: i64,
        available_minor: i64,
    },
    RepayExceedsOutstanding {
        requested_minor: i64,
        outstanding_minor: i64,
    },
    PriceOutOfRange {
        price_minor: i64,
    },
    OrderOutOfRange {
        units: u32,
    },
    /// Abandoning a project must release exactly what was capitalised. If the
    /// carrying value is short, the books and the project register disagree —
    /// that is corruption, and clamping the write-off would hide it (§16.2).
    WriteOffExceedsCarryingValue {
        committed_minor: i64,
        carrying_minor: i64,
    },
    Firm(FirmError),
    Ledger(LedgerError),
    Money(MoneyError),
}

impl From<LedgerError> for CompileError {
    fn from(e: LedgerError) -> Self {
        CompileError::Ledger(e)
    }
}

impl From<FirmError> for CompileError {
    fn from(e: FirmError) -> Self {
        CompileError::Firm(e)
    }
}

pub(super) fn checked_amount(v: i64) -> Result<Money, CompileError> {
    if v <= 0 || v > MAX_ACTION_AMOUNT_MINOR {
        return Err(CompileError::AmountOutOfRange { got_minor: v });
    }
    Money::positive_minor(v).map_err(CompileError::Money)
}

fn posting(account: AccountCode, side: Side, amount: Money) -> Posting {
    Posting {
        account,
        side,
        amount,
    }
}

/// A two-legged transfer, the shape almost every business event takes here.
pub(super) fn transfer(
    kind: TxKind,
    debit: AccountCode,
    credit: AccountCode,
    amount: Money,
) -> Result<Transaction, CompileError> {
    Transaction::new(
        kind,
        vec![
            posting(debit, Side::Debit, amount),
            posting(credit, Side::Credit, amount),
        ],
    )
    .map_err(CompileError::Ledger)
}

/// Cash on hand. Assets are debit-normal, so the signed balance is already the
/// natural-sign figure.
fn cash_available(balances: &Balances) -> i64 {
    balances.balance_minor(AccountCode::Cash)
}

/// Outstanding principal on a facility. Liabilities are credit-normal, so the
/// natural-sign figure is the negation of the signed balance.
fn outstanding(balances: &Balances, facility: Facility) -> i64 {
    balances
        .balance_minor(facility.account())
        .checked_neg()
        .unwrap_or(i64::MAX)
}

fn require_cash(balances: &Balances, needed_minor: i64) -> Result<(), CompileError> {
    let available_minor = cash_available(balances);
    if available_minor < needed_minor {
        return Err(CompileError::InsufficientCash {
            needed_minor,
            available_minor,
        });
    }
    Ok(())
}

/// Procurement cost of one unit: the market commodity price scaled by the
/// product line's frozen multiplier. This is the single channel through which
/// the SDE reaches the income statement.
fn unit_cost_minor(commodity_price_minor: i64, multiplier_micro: i64) -> Result<i64, CompileError> {
    let num = i128::from(commodity_price_minor)
        .checked_mul(i128::from(multiplier_micro))
        .ok_or(CompileError::Money(MoneyError::Overflow))?;
    let q = floor_half_up(num, 1_000_000).map_err(CompileError::Money)?;
    i64::try_from(q).map_err(|_| CompileError::Money(MoneyError::Overflow))
}

/// Intent → plan. Read-only in every store: compilation must be free of side
/// effects so a refusal costs nothing (`execute_intent` owns mutation).
pub fn compile(
    intent: ActionIntent,
    ctx: &ActionContext,
    balances: &Balances,
    firm: &FirmState,
) -> Result<ActionPlan, CompileError> {
    match intent {
        // ---- Financing ---------------------------------------------------
        ActionIntent::Borrow {
            facility,
            amount_minor,
        } => {
            let f = Facility::from_wire(facility)?;
            let amount = checked_amount(amount_minor)?;
            Ok(ActionPlan {
                transaction: Some(transfer(
                    TxKind::DrawDebt,
                    AccountCode::Cash,
                    f.account(),
                    amount,
                )?),
                firm_effect: FirmEffect::None,
            })
        }
        ActionIntent::Repay {
            facility,
            amount_minor,
        } => {
            let f = Facility::from_wire(facility)?;
            let amount = checked_amount(amount_minor)?;
            let outstanding_minor = outstanding(balances, f);
            if amount.minor() > outstanding_minor {
                return Err(CompileError::RepayExceedsOutstanding {
                    requested_minor: amount.minor(),
                    outstanding_minor,
                });
            }
            require_cash(balances, amount.minor())?;
            Ok(ActionPlan {
                transaction: Some(transfer(
                    TxKind::ServiceDebt,
                    f.account(),
                    AccountCode::Cash,
                    amount,
                )?),
                firm_effect: FirmEffect::None,
            })
        }

        // ---- Investment and projects -------------------------------------
        ActionIntent::Invest {
            project_id,
            amount_minor,
        } => {
            let amount = checked_amount(amount_minor)?;
            // A dead project cannot absorb new capital; refuse before any
            // cash check so the diagnostic names the real reason.
            if let Ok(p) = firm.project(project_id) {
                if p.status != ProjectStatus::Active {
                    return Err(CompileError::Firm(FirmError::ProjectNotActive {
                        project_id,
                    }));
                }
            }
            require_cash(balances, amount.minor())?;
            Ok(ActionPlan {
                transaction: Some(transfer(
                    TxKind::CapexPurchase,
                    AccountCode::PropertyPlantEquipment,
                    AccountCode::Cash,
                    amount,
                )?),
                firm_effect: FirmEffect::CommitToProject {
                    project_id,
                    amount_minor: amount.minor(),
                },
            })
        }
        ActionIntent::ContinueProject { project_id } => {
            let p = firm.project(project_id)?;
            if p.status != ProjectStatus::Active {
                return Err(CompileError::Firm(FirmError::ProjectNotActive { project_id }));
            }
            // Continuing costs nothing by itself — that is precisely why it is
            // a clean escalation probe (SPEC §10 lane 4).
            Ok(ActionPlan::firm_only(FirmEffect::ContinueProject {
                project_id,
            }))
        }
        ActionIntent::AbandonProject { project_id } => {
            let p = firm.project(project_id)?;
            if p.status != ProjectStatus::Active {
                return Err(CompileError::Firm(FirmError::ProjectNotActive { project_id }));
            }
            let effect = FirmEffect::AbandonProject { project_id };
            if p.committed_minor == 0 {
                return Ok(ActionPlan::firm_only(effect));
            }
            let carrying_minor = balances.balance_minor(AccountCode::PropertyPlantEquipment);
            if p.committed_minor > carrying_minor {
                return Err(CompileError::WriteOffExceedsCarryingValue {
                    committed_minor: p.committed_minor,
                    carrying_minor,
                });
            }
            let amount = checked_amount(p.committed_minor)?;
            Ok(ActionPlan {
                transaction: Some(transfer(
                    TxKind::PayOpex,
                    AccountCode::OperatingExpense,
                    AccountCode::PropertyPlantEquipment,
                    amount,
                )?),
                firm_effect: effect,
            })
        }

        // ---- Operations --------------------------------------------------
        ActionIntent::SetPrice { sku, tick_price } => {
            let _ = firm.sku(sku)?;
            if !(MIN_PRICE_MINOR..=MAX_PRICE_MINOR).contains(&tick_price) {
                return Err(CompileError::PriceOutOfRange {
                    price_minor: tick_price,
                });
            }
            Ok(ActionPlan::firm_only(FirmEffect::SetPrice {
                sku,
                price_minor: tick_price,
            }))
        }
        ActionIntent::OrderInventory { sku, units } => {
            let _ = firm.sku(sku)?;
            if units == 0 || units > MAX_ORDER_UNITS {
                return Err(CompileError::OrderOutOfRange { units });
            }
            let cfg = ctx
                .firm_params
                .skus
                .get(usize::from(sku))
                .copied()
                .ok_or(CompileError::Firm(FirmError::UnknownSku { sku }))?;
            let per_unit =
                unit_cost_minor(ctx.market.commodity_price_minor, cfg.cost_multiplier_micro)?;
            let total = i128::from(per_unit)
                .checked_mul(i128::from(units))
                .ok_or(CompileError::Money(MoneyError::Overflow))?;
            let total_minor =
                i64::try_from(total).map_err(|_| CompileError::Money(MoneyError::Overflow))?;
            let amount = checked_amount(total_minor)?;
            require_cash(balances, amount.minor())?;
            Ok(ActionPlan {
                transaction: Some(transfer(
                    TxKind::PurchaseInventory,
                    AccountCode::Inventory,
                    AccountCode::Cash,
                    amount,
                )?),
                firm_effect: FirmEffect::AddInventory {
                    sku,
                    units,
                    cost_minor: amount.minor(),
                },
            })
        }

        // ---- Offers ------------------------------------------------------
        ActionIntent::AcceptOffer { offer_id } => {
            let offer = firm.offer(offer_id)?;
            if offer.status != OfferStatus::Open {
                return Err(CompileError::Firm(FirmError::OfferNotOpen { offer_id }));
            }
            let amount = checked_amount(offer.cost_minor)?;
            require_cash(balances, amount.minor())?;
            let (kind, debit) = match offer.kind {
                // A premium buys future cover, so it lands in prepaid expense
                // and is consumed by the settle loop, not expensed on day one.
                OfferKind::Insurance => (TxKind::PayOpex, AccountCode::PrepaidExpenses),
                OfferKind::Expansion => {
                    (TxKind::CapexPurchase, AccountCode::PropertyPlantEquipment)
                }
            };
            Ok(ActionPlan {
                transaction: Some(transfer(kind, debit, AccountCode::Cash, amount)?),
                firm_effect: FirmEffect::ResolveOffer {
                    offer_id,
                    accepted: true,
                },
            })
        }
        ActionIntent::DeclineOffer { offer_id } => {
            let offer = firm.offer(offer_id)?;
            if offer.status != OfferStatus::Open {
                return Err(CompileError::Firm(FirmError::OfferNotOpen { offer_id }));
            }
            Ok(ActionPlan::firm_only(FirmEffect::ResolveOffer {
                offer_id,
                accepted: false,
            }))
        }

        // ---- Positions ---------------------------------------------------
        ActionIntent::OpenHedge {
            instrument,
            notional_minor,
        } => {
            let amount = checked_amount(notional_minor)?;
            // A forward struck at the prevailing level has zero initial value,
            // so opening one moves no cash. Posting a notional here would
            // inflate the balance sheet with a position that was never bought.
            Ok(ActionPlan::firm_only(FirmEffect::OpenPosition {
                instrument,
                notional_minor: amount.minor(),
                entry_index_centi: ctx.market.equity_index_centi,
            }))
        }
        ActionIntent::ClosePosition { position_id } => {
            let position = firm.position(position_id)?;
            let pnl = position_pnl_minor(&position, ctx.market.equity_index_centi)?;
            let effect = FirmEffect::ClosePosition { position_id };
            // Realisation is the ONLY moment a position touches the ledger
            // (BXS-W-04). A flat close is genuinely a non-event.
            let transaction = match pnl {
                0 => None,
                gain if gain > 0 => Some(transfer(
                    TxKind::HedgeSettlement,
                    AccountCode::Cash,
                    AccountCode::OtherIncome,
                    checked_amount(gain)?,
                )?),
                loss => {
                    let magnitude = loss
                        .checked_neg()
                        .ok_or(CompileError::Money(MoneyError::Overflow))?;
                    let amount = checked_amount(magnitude)?;
                    require_cash(balances, amount.minor())?;
                    Some(transfer(
                        TxKind::HedgeSettlement,
                        AccountCode::OperatingExpense,
                        AccountCode::Cash,
                        amount,
                    )?)
                }
            };
            Ok(ActionPlan {
                transaction,
                firm_effect: effect,
            })
        }

        // ---- Pure measurement acts ---------------------------------------
        ActionIntent::ForecastInterval { lo_minor, hi_minor } => {
            if lo_minor < 0 || hi_minor > MAX_ACTION_AMOUNT_MINOR || lo_minor > hi_minor {
                return Err(CompileError::InvalidForecastInterval { lo_minor, hi_minor });
            }
            Ok(ActionPlan::inert())
        }
        ActionIntent::Abstain | ActionIntent::ForcedDefault => Ok(ActionPlan::inert()),
    }
}

/// The mutable half of the simulation core: what a decision can change.
/// Bundled so the staging discipline in `execute_intent` covers every store,
/// and so a future caller cannot accidentally advance one and not the others.
#[derive(Debug, Clone)]
pub struct SimBooks {
    pub balances: Balances,
    pub firm: FirmState,
    pub journal: Journal,
}

impl SimBooks {
    pub fn new(params: &FirmParams) -> Result<Self, CompileError> {
        Ok(Self {
            balances: Balances::new(),
            firm: FirmState::new(params),
            journal: Journal::new().map_err(CompileError::Ledger)?,
        })
    }

    /// Post the opening capital structure. Not an intent: the player does not
    /// decide their own endowment, so it must not be reachable through the
    /// compiler (wall W-d cuts both ways).
    pub fn seed_capital(&mut self, opening_minor: i64) -> Result<(), CompileError> {
        let amount = checked_amount(opening_minor)?;
        let tx = transfer(
            TxKind::OpeningEquity,
            AccountCode::Cash,
            AccountCode::ShareCapital,
            amount,
        )?;
        self.balances.apply(&tx)?;
        self.journal.append(tx)?;
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Execution {
    pub ledger_effect: bool,
    pub journal_seq: Option<u64>,
}

/// Compile and apply, all-or-nothing across all three stores.
///
/// The accounting invariants are re-verified on every single execution rather
/// than only at settle: the identity holds by induction over balanced applies,
/// but the receiver still checks (第六律), and a per-tick check is what turns a
/// silent corruption into a loud one at the tick that caused it.
pub fn execute_intent(
    intent: ActionIntent,
    ctx: &ActionContext,
    books: &mut SimBooks,
) -> Result<Execution, CompileError> {
    let plan = compile(intent, ctx, &books.balances, &books.firm)?;
    apply_plan(plan, books)
}

/// Apply an already-compiled plan. Split out so the settle loop can post
/// engine-authored transactions through the identical staging path.
pub fn apply_plan(plan: ActionPlan, books: &mut SimBooks) -> Result<Execution, CompileError> {
    let ActionPlan {
        transaction,
        firm_effect,
    } = plan;
    let mut staged_balances = books.balances.clone();
    let mut staged_firm = books.firm.clone();
    if let Some(tx) = transaction.as_ref() {
        staged_balances.apply(tx)?;
        staged_balances.verify_zero_sum()?;
        staged_balances.verify_accounting_identity()?;
    }
    staged_firm.apply_effect(firm_effect)?;
    // Journal acceptance is the last fallible step; committing the staged
    // stores after it is infallible, so the whole action is atomic.
    let journal_seq = match transaction {
        Some(tx) => Some(books.journal.append(tx)?),
        None => None,
    };
    books.balances = staged_balances;
    books.firm = staged_firm;
    Ok(Execution {
        ledger_effect: journal_seq.is_some(),
        journal_seq,
    })
}

/// Apply several plans as ONE indivisible act.
///
/// A tick's operations span every product line, and a half-applied tick would
/// leave the firm holding stock it had already been paid for. Staging alone is
/// not enough, because the journal could fill up between the first append and
/// the last; so the room is reserved before anything is written, after which
/// the appends cannot fail and the commit is unconditional.
pub fn apply_plans(plans: &[ActionPlan], books: &mut SimBooks) -> Result<usize, CompileError> {
    let needed = plans.iter().filter(|p| p.moves_money()).count();
    if books.journal.remaining_capacity() < needed {
        return Err(CompileError::Ledger(LedgerError::JournalTailFull));
    }
    let mut staged_balances = books.balances.clone();
    let mut staged_firm = books.firm.clone();
    for plan in plans {
        if let Some(tx) = plan.transaction.as_ref() {
            staged_balances.apply(tx)?;
        }
        staged_firm.apply_effect(plan.firm_effect)?;
    }
    staged_balances.verify_zero_sum()?;
    staged_balances.verify_accounting_identity()?;
    for plan in plans {
        if let Some(tx) = plan.transaction.as_ref() {
            books.journal.append(tx.clone())?;
        }
    }
    books.balances = staged_balances;
    books.firm = staged_firm;
    Ok(needed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::blackbox_sim::firm::{Offer, OfferKind, OfferStatus};
    use crate::blackbox_sim::genesis::{build_campaign_genesis, Difficulty, GenesisRequest};
    use crate::blackbox_sim::ledger::JOURNAL_TAIL_CAPACITY;
    use crate::blackbox_sim::market::{MarketKernel, Regime};

    fn ok<T, E: std::fmt::Debug>(r: Result<T, E>) -> T {
        match r {
            Ok(v) => v,
            Err(e) => unreachable!("action test setup failed: {e:?}"),
        }
    }

    fn fixture(cash: i64) -> (ActionContext, SimBooks) {
        let g = ok(build_campaign_genesis(GenesisRequest {
            scenario_id: 7,
            difficulty: Difficulty::Standard,
            campaign_index: 0,
            created_date: "2026-07-27".to_string(),
        }));
        let mut kernel = ok(MarketKernel::new(&g));
        let market = ok(kernel.step());
        let mut books = ok(SimBooks::new(&g.firm));
        ok(books.seed_capital(cash));
        (
            ActionContext {
                market,
                firm_params: g.firm,
            },
            books,
        )
    }

    fn flat_market(ctx: &ActionContext, index_centi: i64) -> ActionContext {
        ActionContext {
            market: MarketTickView {
                equity_index_centi: index_centi,
                ..ctx.market
            },
            firm_params: ctx.firm_params,
        }
    }

    #[test]
    fn borrow_credits_the_named_facility_only() {
        let (ctx, mut books) = fixture(1_000_000);
        let out = ok(execute_intent(
            ActionIntent::Borrow {
                facility: 1,
                amount_minor: 250_000,
            },
            &ctx,
            &mut books,
        ));
        assert!(out.ledger_effect);
        assert_eq!(books.balances.balance_minor(AccountCode::Cash), 1_250_000);
        assert_eq!(
            books.balances.balance_minor(AccountCode::MezzanineDebt),
            -250_000
        );
        assert_eq!(books.balances.balance_minor(AccountCode::SeniorDebt), 0);
        ok(books.balances.verify_zero_sum());
    }

    #[test]
    fn repay_beyond_outstanding_is_refused() {
        let (ctx, mut books) = fixture(1_000_000);
        let _ = ok(execute_intent(
            ActionIntent::Borrow {
                facility: 0,
                amount_minor: 100_000,
            },
            &ctx,
            &mut books,
        ));
        let before = books.balances.clone();
        assert!(matches!(
            execute_intent(
                ActionIntent::Repay {
                    facility: 0,
                    amount_minor: 100_001
                },
                &ctx,
                &mut books
            ),
            Err(CompileError::RepayExceedsOutstanding {
                requested_minor: 100_001,
                outstanding_minor: 100_000
            })
        ));
        assert_eq!(books.balances, before);
    }

    #[test]
    fn spending_beyond_cash_is_refused() {
        let (ctx, mut books) = fixture(500);
        let before = books.balances.clone();
        assert!(matches!(
            execute_intent(
                ActionIntent::Invest {
                    project_id: 3,
                    amount_minor: 501
                },
                &ctx,
                &mut books
            ),
            Err(CompileError::InsufficientCash {
                needed_minor: 501,
                available_minor: 500
            })
        ));
        assert_eq!(books.balances, before);
    }

    #[test]
    fn amounts_are_bounded_before_money_is_built() {
        let (ctx, books) = fixture(1_000_000);
        for bad in [0_i64, -1, MAX_ACTION_AMOUNT_MINOR + 1, i64::MAX] {
            assert!(matches!(
                compile(
                    ActionIntent::Borrow {
                        facility: 0,
                        amount_minor: bad
                    },
                    &ctx,
                    &books.balances,
                    &books.firm
                ),
                Err(CompileError::AmountOutOfRange { got_minor }) if got_minor == bad
            ));
        }
    }

    #[test]
    fn unknown_facility_fails_closed() {
        let (ctx, books) = fixture(1_000);
        assert!(matches!(
            compile(
                ActionIntent::Borrow {
                    facility: 7,
                    amount_minor: 100
                },
                &ctx,
                &books.balances,
                &books.firm
            ),
            Err(CompileError::UnknownFacility { facility: 7 })
        ));
    }

    #[test]
    fn forecast_is_validated_yet_moves_no_money() {
        let (ctx, books) = fixture(1_000);
        assert_eq!(
            ok(compile(
                ActionIntent::ForecastInterval {
                    lo_minor: 100,
                    hi_minor: 900
                },
                &ctx,
                &books.balances,
                &books.firm
            )),
            ActionPlan::inert()
        );
        for (lo, hi) in [(900_i64, 100_i64), (-1, 100)] {
            assert!(matches!(
                compile(
                    ActionIntent::ForecastInterval {
                        lo_minor: lo,
                        hi_minor: hi
                    },
                    &ctx,
                    &books.balances,
                    &books.firm
                ),
                Err(CompileError::InvalidForecastInterval { .. })
            ));
        }
    }

    #[test]
    fn abstain_and_forced_default_are_inert() {
        let (ctx, books) = fixture(1_000);
        for intent in [ActionIntent::Abstain, ActionIntent::ForcedDefault] {
            assert_eq!(
                ok(compile(intent, &ctx, &books.balances, &books.firm)),
                ActionPlan::inert()
            );
        }
    }

    // ---- Phase 2: the formerly deferred intents ---------------------------

    #[test]
    fn set_price_changes_the_line_and_nothing_else() {
        let (ctx, mut books) = fixture(1_000_000);
        let before_cash = books.balances.balance_minor(AccountCode::Cash);
        let out = ok(execute_intent(
            ActionIntent::SetPrice {
                sku: 0,
                tick_price: 9_900,
            },
            &ctx,
            &mut books,
        ));
        assert!(!out.ledger_effect);
        assert_eq!(ok(books.firm.sku(0)).unit_price_minor, 9_900);
        assert_eq!(books.balances.balance_minor(AccountCode::Cash), before_cash);
    }

    #[test]
    fn ordering_inventory_prices_off_the_market_tick() {
        let (ctx, mut books) = fixture(100_000_000);
        let cfg = ok(ctx
            .firm_params
            .skus
            .first()
            .copied()
            .ok_or("no sku configured"));
        let expected_unit = ok(unit_cost_minor(
            ctx.market.commodity_price_minor,
            cfg.cost_multiplier_micro,
        ));
        let cash_before = books.balances.balance_minor(AccountCode::Cash);
        let _ = ok(execute_intent(
            ActionIntent::OrderInventory { sku: 0, units: 250 },
            &ctx,
            &mut books,
        ));
        let spend = expected_unit * 250;
        assert_eq!(
            books.balances.balance_minor(AccountCode::Cash),
            cash_before - spend
        );
        assert_eq!(books.balances.balance_minor(AccountCode::Inventory), spend);
        let sku = ok(books.firm.sku(0));
        assert_eq!(sku.inventory_units, 250);
        // BXS-I-17: the firm's carrying value mirrors the ledger exactly.
        assert_eq!(sku.inventory_value_minor, spend);
        assert_eq!(books.firm.inventory_value_minor(), spend);
    }

    #[test]
    fn degenerate_orders_are_refused() {
        let (ctx, mut books) = fixture(100_000_000);
        for bad in [0_u32, MAX_ORDER_UNITS + 1] {
            assert!(matches!(
                execute_intent(
                    ActionIntent::OrderInventory { sku: 0, units: bad },
                    &ctx,
                    &mut books
                ),
                Err(CompileError::OrderOutOfRange { units }) if units == bad
            ));
        }
        assert!(matches!(
            execute_intent(
                ActionIntent::OrderInventory { sku: 77, units: 1 },
                &ctx,
                &mut books
            ),
            Err(CompileError::Firm(FirmError::UnknownSku { sku: 77 }))
        ));
    }

    #[test]
    fn accepting_an_insurance_offer_prepays_it() {
        let (ctx, mut books) = fixture(10_000_000);
        let id = ok(books.firm.place_offer(Offer {
            kind: OfferKind::Insurance,
            cost_minor: 120_000,
            status: OfferStatus::Open,
        }));
        let _ = ok(execute_intent(
            ActionIntent::AcceptOffer { offer_id: id },
            &ctx,
            &mut books,
        ));
        assert_eq!(
            books.balances.balance_minor(AccountCode::PrepaidExpenses),
            120_000
        );
        assert_eq!(ok(books.firm.offer(id)).status, OfferStatus::Accepted);
        // Resolved offers are terminal — no double-dipping.
        assert!(matches!(
            execute_intent(ActionIntent::DeclineOffer { offer_id: id }, &ctx, &mut books),
            Err(CompileError::Firm(FirmError::OfferNotOpen { .. }))
        ));
    }

    #[test]
    fn declining_an_offer_costs_nothing_but_closes_it() {
        let (ctx, mut books) = fixture(10_000_000);
        let id = ok(books.firm.place_offer(Offer {
            kind: OfferKind::Expansion,
            cost_minor: 500_000,
            status: OfferStatus::Open,
        }));
        let cash_before = books.balances.balance_minor(AccountCode::Cash);
        let out = ok(execute_intent(
            ActionIntent::DeclineOffer { offer_id: id },
            &ctx,
            &mut books,
        ));
        assert!(!out.ledger_effect);
        assert_eq!(books.balances.balance_minor(AccountCode::Cash), cash_before);
        assert_eq!(ok(books.firm.offer(id)).status, OfferStatus::Declined);
    }

    #[test]
    fn opening_a_hedge_posts_nothing_and_closing_realises_it() {
        let (ctx, mut books) = fixture(10_000_000);
        let entry = ctx.market.equity_index_centi;
        let cash_before = books.balances.balance_minor(AccountCode::Cash);
        let out = ok(execute_intent(
            ActionIntent::OpenHedge {
                instrument: 0,
                notional_minor: 1_000_000,
            },
            &ctx,
            &mut books,
        ));
        assert!(
            !out.ledger_effect,
            "a forward at the prevailing level has zero initial value \
             (BXS-W-04: no mark-to-market posting)"
        );
        assert_eq!(books.balances.balance_minor(AccountCode::Cash), cash_before);
        // Index up 10% => a gain of 10% of notional lands as cash income.
        let up = flat_market(&ctx, entry + entry / 10);
        let _ = ok(execute_intent(
            ActionIntent::ClosePosition { position_id: 0 },
            &up,
            &mut books,
        ));
        let gain = books.balances.balance_minor(AccountCode::OtherIncome);
        assert!(gain < 0, "revenue is credit-normal, so signed balance is negative");
        assert_eq!(
            books.balances.balance_minor(AccountCode::Cash),
            cash_before + gain.abs()
        );
        ok(books.balances.verify_accounting_identity());
    }

    #[test]
    fn a_flat_close_is_a_non_event() {
        let (ctx, mut books) = fixture(10_000_000);
        let _ = ok(execute_intent(
            ActionIntent::OpenHedge {
                instrument: 0,
                notional_minor: 750_000,
            },
            &ctx,
            &mut books,
        ));
        let before = books.balances.clone();
        let out = ok(execute_intent(
            ActionIntent::ClosePosition { position_id: 0 },
            &ctx,
            &mut books,
        ));
        assert!(!out.ledger_effect);
        assert_eq!(books.balances, before);
    }

    #[test]
    fn abandoning_a_project_writes_off_exactly_what_was_capitalised() {
        let (ctx, mut books) = fixture(10_000_000);
        let _ = ok(execute_intent(
            ActionIntent::Invest {
                project_id: 0,
                amount_minor: 400_000,
            },
            &ctx,
            &mut books,
        ));
        let _ = ok(execute_intent(
            ActionIntent::Invest {
                project_id: 0,
                amount_minor: 150_000,
            },
            &ctx,
            &mut books,
        ));
        assert_eq!(
            books.balances.balance_minor(AccountCode::PropertyPlantEquipment),
            550_000
        );
        let _ = ok(execute_intent(
            ActionIntent::AbandonProject { project_id: 0 },
            &ctx,
            &mut books,
        ));
        assert_eq!(
            books.balances.balance_minor(AccountCode::PropertyPlantEquipment),
            0
        );
        assert_eq!(
            books.balances.balance_minor(AccountCode::OperatingExpense),
            550_000
        );
        // The sunk figure survives so lane 4 can still see it.
        assert_eq!(ok(books.firm.project(0)).committed_minor, 550_000);
        ok(books.balances.verify_zero_sum());
    }

    #[test]
    fn dead_projects_absorb_nothing_further() {
        let (ctx, mut books) = fixture(10_000_000);
        let _ = ok(execute_intent(
            ActionIntent::Invest {
                project_id: 1,
                amount_minor: 10_000,
            },
            &ctx,
            &mut books,
        ));
        let _ = ok(execute_intent(
            ActionIntent::AbandonProject { project_id: 1 },
            &ctx,
            &mut books,
        ));
        for intent in [
            ActionIntent::Invest {
                project_id: 1,
                amount_minor: 10_000,
            },
            ActionIntent::ContinueProject { project_id: 1 },
            ActionIntent::AbandonProject { project_id: 1 },
        ] {
            assert!(matches!(
                execute_intent(intent, &ctx, &mut books),
                Err(CompileError::Firm(FirmError::ProjectNotActive { project_id: 1 }))
            ));
        }
    }

    #[test]
    fn acting_on_absent_entities_fails_closed() {
        let (ctx, mut books) = fixture(10_000_000);
        assert!(matches!(
            execute_intent(ActionIntent::ContinueProject { project_id: 5 }, &ctx, &mut books),
            Err(CompileError::Firm(FirmError::UnknownProject { project_id: 5 }))
        ));
        assert!(matches!(
            execute_intent(ActionIntent::AcceptOffer { offer_id: 5 }, &ctx, &mut books),
            Err(CompileError::Firm(FirmError::UnknownOffer { offer_id: 5 }))
        ));
        assert!(matches!(
            execute_intent(ActionIntent::ClosePosition { position_id: 5 }, &ctx, &mut books),
            Err(CompileError::Firm(FirmError::UnknownPosition { position_id: 5 }))
        ));
    }

    /// The composite atomicity contract across all three stores: when the
    /// journal refuses, neither the books nor the firm may have advanced.
    #[test]
    fn journal_rejection_rolls_back_every_store() {
        let (ctx, mut books) = fixture(i64::from(u32::MAX));
        let borrow = ActionIntent::Borrow {
            facility: 0,
            amount_minor: 1,
        };
        // seed_capital already consumed one journal slot.
        while books.journal.len() < JOURNAL_TAIL_CAPACITY {
            let _ = ok(execute_intent(borrow, &ctx, &mut books));
        }
        let balances_before = books.balances.clone();
        let firm_before = books.firm.clone();
        let seq_before = books.journal.next_seq();
        assert!(matches!(
            execute_intent(borrow, &ctx, &mut books),
            Err(CompileError::Ledger(LedgerError::JournalTailFull))
        ));
        assert_eq!(books.balances, balances_before);
        assert_eq!(books.firm, firm_before);
        assert_eq!(books.journal.next_seq(), seq_before);
        // An intent with only a firm effect is unaffected by a full journal —
        // it never reaches the journal at all.
        ok(execute_intent(
            ActionIntent::SetPrice {
                sku: 0,
                tick_price: 4_242,
            },
            &ctx,
            &mut books,
        ));
        assert_eq!(ok(books.firm.sku(0)).unit_price_minor, 4_242);
        let drained = books.journal.drain_settled();
        assert_eq!(drained.len(), JOURNAL_TAIL_CAPACITY);
        let out = ok(execute_intent(borrow, &ctx, &mut books));
        assert!(out.ledger_effect);
    }

    #[test]
    fn facility_wire_ids_frozen() {
        assert_eq!(Facility::Senior as u8, 0);
        assert_eq!(Facility::Mezzanine as u8, 1);
        assert_eq!(Facility::Senior.account(), AccountCode::SeniorDebt);
        assert_eq!(Facility::Mezzanine.account(), AccountCode::MezzanineDebt);
    }

    #[test]
    fn context_carries_the_view_not_the_oracle() {
        // Wall W-a as a type-level fact: the compiler's only market input is
        // the published view, whose regime tag is the most it ever learns.
        let (ctx, _books) = fixture(1_000);
        assert!(matches!(ctx.market.regime, Regime::Calm | Regime::Stress));
    }
}
