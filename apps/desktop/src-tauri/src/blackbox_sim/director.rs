//! Director front stage — the turn driver (SPEC §7, §3.4; BXS-I-01).
//!
//! This is the part of the Director that owns the clock: it advances the FSM
//! through Observe → Decide → Execute → Settle → Report, steps the market,
//! routes the turn's decision through the ActionCompiler, runs operations,
//! closes periods on the quarter, and checkpoints. Stimulus planting and vault
//! persistence are Phase 3 and are deliberately absent — nothing here reads or
//! writes a profile, a vault, or an LLM (wall W-b).
//!
//! # One decision per turn
//!
//! The FSM allows exactly one `DecisionSubmitted` between Decide and Execute,
//! and that is not a simplification to be relaxed later: the turn IS the
//! measurement unit. Every lane in §10 compares one decision against a
//! counterfactual of the same decision, which requires a one-to-one mapping
//! between turns and choices. A batch-submit convenience would quietly destroy
//! the estimator's unit of observation.
//!
//! A refused intent is NOT a decision. Refusals return a typed error and leave
//! the session in Decide, so the player chooses again; nothing enters the
//! replay log, because nothing happened.
//!
//! # Replay (BXS-I-01)
//!
//! `state(t) = fold(Genesis, decisions[0..t])`. The log carries only the tick
//! and the intent. It carries no latency, no timestamps and no market data,
//! because none of those are inputs — the market is regenerated from the
//! genesis key, and latency is an observation about the player, not about the
//! world (trap BXS-W-01). [`replay_digest`] reconstructs from that alone, and
//! the digest it produces must equal the live session's.

use super::action::{
    apply_plans, checked_amount, execute_intent, transfer, ActionContext, ActionPlan, CompileError,
    Execution, SimBooks,
};
use super::bias::PricingTrial;
use super::firm::{FirmEffect, FirmError, OfferStatus};
use super::fsm::{advance, FailureReason, FsmError, SessionEvent, SessionState, TurnPhase};
use super::genesis::{build_campaign_genesis, CampaignGenesis, GenesisError, GenesisRequest};
use super::market::{MarketError, MarketKernel, MarketTickView};
use super::oracle::{pricing_optimality_gap, reference_valuation};
use super::persist::{
    DecisionBatch, DecisionSink, FlushReceipt, SinkError, BLACKBOX_LOG_SCHEMA_V1,
};
use super::ring::{FixedRing, RingConfigError};
use super::settle::{close_period, operate_tick, PeriodClose, SettleError, TickResult, TICKS_PER_QUARTER};
use super::snapshot::{digest_state, GenerationStore, SnapshotError, StateDigest};
use super::stimulus::{
    anchor_arm, attribute, bind_offer_id, bind_project_id, crisis_ticks_remaining, disposition_target,
    draw_anchor, draw_crisis, draw_gamble, draw_sunk, gamble_offer, is_anchored_forecast_turn,
    is_control_forecast_turn, is_disposition_turn, is_gamble_turn, is_sunk_cost_turn,
    PlantedStimulus, StimulusError, StimulusLedger, StimulusParams, StimulusView,
};
use super::telemetry::{
    ActionIntent, DecisionEvent, DecisionRecord, EventLog, RefusalLog, RefusalLogError,
    RefusalReason, TelemetryError,
};
use super::ledger::{AccountCode, Balances, TxKind};

/// Turns in a full campaign: four quarters.
pub const CAMPAIGN_TICKS: u32 = TICKS_PER_QUARTER * 4;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DirectorError {
    Fsm(FsmError),
    Genesis(GenesisError),
    Market(MarketError),
    Compile(CompileError),
    Settle(SettleError),
    Snapshot(SnapshotError),
    Telemetry(TelemetryError),
    Refusal(RefusalLogError),
    Stimulus(StimulusError),
    Sink(SinkError),
    Ring(RingConfigError),
    /// The caller asked for a phase action the session is not in. Phases are
    /// not advisory.
    WrongPhase {
        state: SessionState,
        wanted: TurnPhase,
    },
    CampaignExhausted,
    /// Something asked for the market before the world had been observed.
    /// Returning a zeroed view instead would be a fabricated observation.
    NoObservationYet,
    /// The replay log does not describe a well-formed sequence of turns.
    MalformedLog {
        at_index: usize,
    },
}

macro_rules! from_error {
    ($src:ty, $variant:ident) => {
        impl From<$src> for DirectorError {
            fn from(e: $src) -> Self {
                DirectorError::$variant(e)
            }
        }
    };
}
from_error!(FsmError, Fsm);
from_error!(GenesisError, Genesis);
from_error!(MarketError, Market);
from_error!(CompileError, Compile);
from_error!(SettleError, Settle);
from_error!(SnapshotError, Snapshot);
from_error!(TelemetryError, Telemetry);
from_error!(RefusalLogError, Refusal);
from_error!(StimulusError, Stimulus);
from_error!(SinkError, Sink);
from_error!(RingConfigError, Ring);

/// Narrow the compiler's full refusal vocabulary down to the ruling's scope:
/// a well-formed intent that a business rule turned away. Deliberately NOT a
/// catch-all — `AmountOutOfRange`, `InvalidForecastInterval`,
/// `UnknownFacility`, `PriceOutOfRange`, `OrderOutOfRange`, and every
/// `Unknown*`/`RegistryFull`/`InventoryShort`/`OrderTooLarge` firm error stay
/// unclassified (`None`) on purpose: those are shape or existence problems,
/// not "the world said no", and logging them would let a fat-fingered UI or
/// an adversarial fuzz script pollute the pressure/escalation signal this log
/// exists to protect.
fn classify_refusal(err: &CompileError) -> Option<RefusalReason> {
    match err {
        CompileError::InsufficientCash { .. } => Some(RefusalReason::InsufficientCash),
        CompileError::RepayExceedsOutstanding { .. } => {
            Some(RefusalReason::RepayExceedsOutstanding)
        }
        CompileError::WriteOffExceedsCarryingValue { .. } => {
            Some(RefusalReason::WriteOffExceedsCarryingValue)
        }
        CompileError::Firm(FirmError::ProjectNotActive { .. }) => {
            Some(RefusalReason::ProjectNotActive)
        }
        CompileError::Firm(FirmError::OfferNotOpen { .. }) => Some(RefusalReason::OfferNotOpen),
        CompileError::Firm(FirmError::PositionNotOpen { .. }) => {
            Some(RefusalReason::PositionNotOpen)
        }
        _ => None,
    }
}

/// One entry of the replay log. Everything needed to reproduce the world, and
/// nothing that would make reproduction depend on the player's hardware.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LoggedDecision {
    pub tick: u32,
    pub intent: ActionIntent,
}

/// What the player is shown at the end of a turn.
///
/// Wall W-a: every field here is either the published market view or a
/// consequence of the player's own books. `CampaignGenesis` is not reachable
/// from this type, so true drift and true value cannot leak into the UI even
/// by a careless `..Default::default()`.
#[derive(Debug, Clone, PartialEq)]
pub struct TurnReport {
    pub tick: u32,
    pub market: MarketTickView,
    pub operations: TickResult,
    pub period_close: Option<PeriodClose>,
    pub state_digest: StateDigest,
}

pub struct Session {
    genesis: CampaignGenesis,
    kernel: MarketKernel,
    books: SimBooks,
    state: SessionState,
    /// `None` until the first Observe. Not a zeroed placeholder: a session
    /// that has not looked at the world must be unable to claim it has.
    market: Option<MarketTickView>,
    /// Balances as of the start of the open settlement period.
    period_begin: Balances,
    generations: GenerationStore,
    events: EventLog,
    decisions: Vec<LoggedDecision>,
    turns_completed: u32,
    pending_operations: Option<TickResult>,
    pending_close: Option<PeriodClose>,
    /// The answer key: every probe ever planted, with its sealed parameters.
    stimuli: StimulusLedger,
    /// Probes planted this turn and still awaiting a response. Rebuilt each
    /// Observe, so a probe is answerable only on the turn it appears.
    active: Vec<PlantedStimulus>,
    /// The sunk-cost probe project from the previous plant. Retired when the
    /// next one is planted so the fixed-capacity registry cannot fill with
    /// probes and start refusing (第三律 — bounded before allocated).
    planted_project: Option<u32>,
    /// How many answer-key rows a sink has already acknowledged.
    stimuli_flushed: usize,
    /// Valid-but-business-rule-refused attempts made while at least one
    /// stimulus was active this turn (Commander's ruling, 2026-07-27). A
    /// malformed or type-level refusal never reaches this log — see
    /// `classify_refusal`.
    refusals: RefusalLog,
    /// Lane 5 feed (BXS-I-20 companion for pricing): one `PricingTrial` per
    /// successful `SetPrice`, built at decision time because — unlike the
    /// other five lanes — there is no planted `StimulusRef` to mine it back
    /// out of the decision log afterward. Derived data, not a record, so it
    /// uses the evicting policy (BXS-W-03): losing the oldest sample under
    /// pathological replay is acceptable, silently faking one is not.
    pricing_trials: FixedRing<PricingTrial>,
}

impl Session {
    /// Genesis → Active{Observe}. The opening capital is posted here, not
    /// through the compiler: an endowment is not a decision (wall W-d).
    pub fn start(request: GenesisRequest) -> Result<Self, DirectorError> {
        let genesis = build_campaign_genesis(request)?;
        genesis.verify_fingerprint()?;
        let kernel = MarketKernel::new(&genesis)?;
        let mut books = SimBooks::new(&genesis.firm)?;
        books.seed_capital(genesis.firm.opening_capital_minor)?;
        let period_begin = books.balances.clone();
        let state = advance(SessionState::Genesis, SessionEvent::StartConfirmed)?;
        Ok(Self {
            genesis,
            kernel,
            books,
            state,
            market: None,
            period_begin,
            generations: GenerationStore::new(),
            events: EventLog::new()?,
            decisions: Vec::new(),
            turns_completed: 0,
            pending_operations: None,
            pending_close: None,
            stimuli: StimulusLedger::new(),
            active: Vec::with_capacity(4),
            planted_project: None,
            stimuli_flushed: 0,
            refusals: RefusalLog::new()?,
            pricing_trials: FixedRing::new(CAMPAIGN_TICKS as usize)?,
        })
    }

    #[must_use]
    pub fn state(&self) -> SessionState {
        self.state
    }

    #[must_use]
    pub fn turns_completed(&self) -> u32 {
        self.turns_completed
    }

    #[must_use]
    pub fn decisions(&self) -> &[LoggedDecision] {
        &self.decisions
    }

    #[must_use]
    pub fn generations(&self) -> &GenerationStore {
        &self.generations
    }

    #[must_use]
    pub fn books(&self) -> &SimBooks {
        &self.books
    }

    /// The published market view — the ONLY market information that leaves
    /// this module (wall W-a). `None` before the first observation.
    #[must_use]
    pub fn market(&self) -> Option<MarketTickView> {
        self.market
    }

    pub fn state_digest(&self) -> Result<StateDigest, DirectorError> {
        Ok(digest_state(&self.kernel, &self.books.balances, &self.books.firm)?)
    }

    /// Crate-internal only. `MarketKernel` carries the true Genesis parameters,
    /// so handing it past the module boundary would put wall W-a one `pub`
    /// away from failing. L2 may read it; nothing else may see it.
    pub(super) fn kernel(&self) -> &MarketKernel {
        &self.kernel
    }

    pub(super) fn context(&self) -> Result<ActionContext, DirectorError> {
        Ok(ActionContext {
            market: self.market.ok_or(DirectorError::NoObservationYet)?,
            firm_params: self.genesis.firm,
        })
    }

    fn observed(&self) -> Result<MarketTickView, DirectorError> {
        self.market.ok_or(DirectorError::NoObservationYet)
    }

    fn require(&self, wanted: TurnPhase) -> Result<(), DirectorError> {
        match self.state {
            SessionState::Active { phase } if phase == wanted => Ok(()),
            state => Err(DirectorError::WrongPhase { state, wanted }),
        }
    }

    /// Kill the session on a detected invariant breach. Returns the original
    /// error so the caller reports the cause, not the consequence.
    fn die(&mut self, reason: FailureReason, cause: DirectorError) -> DirectorError {
        match advance(self.state, SessionEvent::CorruptionDetected { reason }) {
            Ok(next) => self.state = next,
            Err(_) => {
                // Already absorbing (Sealed/Dead); nothing left to kill.
            }
        }
        cause
    }

    /// Observe: advance the world one tick, plant this turn's probes, and
    /// publish the view.
    pub fn observe(&mut self) -> Result<MarketTickView, DirectorError> {
        self.require(TurnPhase::Observe)?;
        if self.turns_completed >= CAMPAIGN_TICKS {
            return Err(DirectorError::CampaignExhausted);
        }
        let view = self.kernel.step()?;
        self.market = Some(view);
        self.plant(view)?;
        self.state = advance(self.state, SessionEvent::ObservationClosed)?;
        Ok(view)
    }

    /// The probes visible this turn, stripped of their answer key.
    #[must_use]
    pub fn stimulus_views(&self) -> Vec<StimulusView> {
        self.active.iter().map(StimulusView::publish).collect()
    }

    /// The answer key. Crate-internal: this is the sim → analysis direction,
    /// and it must reach the vault without passing through a view model.
    pub(super) fn stimuli(&self) -> &StimulusLedger {
        &self.stimuli
    }

    /// Valid-but-refused attempts recorded under an active stimulus.
    /// Crate-internal for the same reason as `stimuli()`: this feeds the
    /// estimator (Phase 4), not the UI. Exercised today only by the
    /// PHANTOM-BOT calibration suite (`calibration.rs`); Phase 5 wires a
    /// live end-of-campaign call into `bias::estimate_profile`.
    #[allow(dead_code)]
    pub(super) fn refusals(&self) -> &RefusalLog {
        &self.refusals
    }

    /// The decision log, read-only. Crate-internal for the same reason as
    /// `stimuli()`: this feeds the estimator (Phase 4), not the UI. See
    /// `refusals()` for why this is presently `#[allow(dead_code)]`.
    #[allow(dead_code)]
    pub(super) fn events(&self) -> impl Iterator<Item = &DecisionEvent> {
        self.events.records()
    }

    /// Lane 5 feed: one sample per successful `SetPrice`. Crate-internal for
    /// the same reason as `stimuli()`. See `refusals()` for why this is
    /// presently `#[allow(dead_code)]`.
    #[allow(dead_code)]
    pub(super) fn pricing_trials(&self) -> impl Iterator<Item = &PricingTrial> {
        self.pricing_trials.iter()
    }

    /// Plant the fixed cadence's probes for this turn.
    ///
    /// Planting happens after the market step and before the player sees
    /// anything, so a probe never depends on the decision it is measuring.
    /// Probes that cannot be materialised (no open position to watch, no free
    /// project slot, not enough cash to have sunk) are *skipped*, not faked:
    /// a smaller sample is honest, an invented one is not (trap BXS-W-13).
    fn plant(&mut self, view: MarketTickView) -> Result<(), DirectorError> {
        self.active.clear();
        let turn = self.turns_completed.saturating_add(1);
        let key = self.genesis.campaign_key;
        let tick = view.tick;

        if is_gamble_turn(turn) {
            let drawn = draw_gamble(key, turn);
            if let Some(offer) = gamble_offer(drawn) {
                if let Ok(offer_id) = self.books.firm.place_offer(offer) {
                    let planted = self.stimuli.plant(tick, bind_offer_id(drawn, offer_id))?;
                    self.active.push(planted);
                }
            }
        }

        // The forecast probe: anchored on most turns, unanchored at quarter end.
        // Both need the sealed reference, and neither publishes it.
        if is_anchored_forecast_turn(turn) || is_control_forecast_turn(turn) {
            let ctx = self.context()?;
            if let Ok(reference) = reference_valuation(self.kernel(), &ctx, &self.books.firm) {
                let params = match anchor_arm(turn) {
                    Some(arm) => draw_anchor(key, turn, arm, reference.expected_revenue_minor),
                    None => StimulusParams::ForecastElicitation {
                        reference_minor: reference.expected_revenue_minor,
                    },
                };
                let planted = self.stimuli.plant(tick, params)?;
                self.active.push(planted);
            }
        }

        if let Some(arm) = super::stimulus::sunk_arm(turn) {
            if is_sunk_cost_turn(turn) {
                let (magnitude, drawn) = draw_sunk(key, turn, arm);
                if let Some(project_id) = self.seed_project(drawn, magnitude)? {
                    let planted = self.stimuli.plant(tick, bind_project_id(drawn, project_id))?;
                    self.active.push(planted);
                }
            }
        }

        if is_disposition_turn(turn) {
            if let Some(params) = disposition_target(&self.books.firm, view.equity_index_centi) {
                let planted = self.stimuli.plant(tick, params)?;
                self.active.push(planted);
            }
        }

        if let Some(ticks_remaining) = crisis_ticks_remaining(turn) {
            let planted = self.stimuli.plant(tick, draw_crisis(key, turn, ticks_remaining))?;
            self.active.push(planted);
        }
        Ok(())
    }

    /// Bring a sunk-cost project into being, returning its id.
    ///
    /// `None` when the firm cannot afford to have sunk the capital. Skipping is
    /// the right failure: a project whose sunk cost the books do not show is
    /// not a sunk cost, and lane 4 compares continue-rates against a
    /// counterfactual that must differ in exactly one respect.
    fn seed_project(
        &mut self,
        params: StimulusParams,
        magnitude: i64,
    ) -> Result<Option<u32>, DirectorError> {
        let sunk = match params {
            StimulusParams::SunkCostPair { sunk_minor, .. } => sunk_minor,
            _ => return Ok(None),
        };
        // Retire the previous probe first. A probe is answerable only on the
        // turn it appears, so leaving it Active would do nothing but consume a
        // slot until the registry refused and the lane quietly starved.
        if let Some(previous) = self.planted_project.take() {
            // Already abandoned by the player, or already gone: nothing to do.
            let _ = self
                .books
                .firm
                .apply_effect(FirmEffect::CompleteProject {
                    project_id: previous,
                });
        }
        // The counterfactual arm is a live project with no history: it costs
        // nothing now, so it is pure firm state. `magnitude` is what the arm
        // *would* have sunk, and is deliberately not spent.
        if sunk == 0 {
            let _ = magnitude;
            let id = self
                .books
                .firm
                .open_project(0)
                .map_err(|e| DirectorError::Compile(CompileError::Firm(e)))?;
            self.planted_project = Some(id);
            return Ok(Some(id));
        }
        if self.books.balances.balance_minor(AccountCode::Cash) < sunk {
            return Ok(None);
        }
        let amount = checked_amount(sunk)?;
        let project_id = self
            .books
            .firm
            .open_project(0)
            .map_err(|e| DirectorError::Compile(CompileError::Firm(e)))?;
        let plan = ActionPlan {
            transaction: Some(transfer(
                TxKind::CapexPurchase,
                AccountCode::PropertyPlantEquipment,
                AccountCode::Cash,
                amount,
            )?),
            firm_effect: FirmEffect::CommitToProject {
                project_id,
                amount_minor: sunk,
            },
        };
        apply_plans(&[plan], &mut self.books)?;
        self.planted_project = Some(project_id);
        Ok(Some(project_id))
    }

    /// Decide: submit the turn's one decision.
    ///
    /// `latency_ms` is recorded for the estimators and then has no further
    /// effect on anything. It is not passed to the compiler, not hashed into
    /// the state digest, and not written to the replay log.
    pub fn submit(
        &mut self,
        intent: ActionIntent,
        latency_ms: Option<u32>,
    ) -> Result<Execution, DirectorError> {
        self.submit_inner(intent, latency_ms, false)
    }

    /// The Director's timeout default. Recorded as a first-class decision with
    /// its own flag rather than as a missing turn (BXS-W-07): a silently
    /// skipped turn would bias the pressure and escalation lanes toward
    /// whoever happened to be slow.
    pub fn submit_timeout_default(&mut self) -> Result<Execution, DirectorError> {
        self.submit_inner(ActionIntent::ForcedDefault, None, true)
    }

    fn submit_inner(
        &mut self,
        intent: ActionIntent,
        latency_ms: Option<u32>,
        forced_default: bool,
    ) -> Result<Execution, DirectorError> {
        self.require(TurnPhase::Decide)?;
        let ctx = self.context()?;
        // The digest is taken BEFORE the decision: it anchors the state the
        // player was actually looking at when they chose.
        let state_digest = self.state_digest()?;
        // Execute first — a refusal must not consume the turn or reach the
        // decision log. It MAY reach the refusal log, but only narrowly: see
        // `classify_refusal` for the line the ruling draws.
        let execution = match execute_intent(intent, &ctx, &mut self.books) {
            Ok(exec) => exec,
            Err(err) => {
                if !self.active.is_empty() {
                    if let Some(reason) = classify_refusal(&err) {
                        let stimulus = attribute(intent, &self.active);
                        // Best-effort: a full refusal log must not mask the
                        // original compile error the caller is expecting.
                        let _ = self.refusals.record(ctx.market.tick, intent, reason, stimulus);
                    }
                }
                return Err(err.into());
            }
        };
        // Lane 5: score the price the player just chose against the
        // profit-maximising one on the identical curve, before the decision
        // record is written. Best-effort — an oracle failure (e.g. an
        // unrecognised SKU) must not unwind an otherwise-successful
        // `SetPrice`; it only costs that one turn's sample.
        if let ActionIntent::SetPrice { sku, tick_price } = intent {
            if let Ok(gap) = pricing_optimality_gap(self.kernel(), &ctx, sku, tick_price) {
                let under_pressure = self
                    .active
                    .iter()
                    .any(|p| matches!(p.params, StimulusParams::CrisisCountdown { .. }));
                self.pricing_trials.evicting_push(PricingTrial {
                    under_pressure,
                    gap_minor: gap.gap_minor,
                    optimal_profit_minor: gap.optimal_profit_minor,
                });
            }
        }
        // Attribution happens only after the act succeeded: a refused intent
        // answered no probe, and crediting one would put a response in the
        // record that the world never saw.
        self.events.record(DecisionRecord {
            tick: ctx.market.tick,
            phase: TurnPhase::Decide,
            action: intent,
            stimulus: attribute(intent, &self.active),
            latency_ms,
            forced_default,
            state_digest: state_digest.short(),
        })?;
        self.decisions.push(LoggedDecision {
            tick: ctx.market.tick,
            intent,
        });
        self.state = advance(self.state, SessionEvent::DecisionSubmitted)?;
        Ok(execution)
    }

    /// Execute: run the tick's operations.
    pub fn execute(&mut self) -> Result<TickResult, DirectorError> {
        self.require(TurnPhase::Execute)?;
        let ctx = self.context()?;
        let result = match operate_tick(&ctx, &mut self.books) {
            Ok(r) => r,
            Err(e) => return Err(self.die(FailureReason::InternalInvariantBroken, e.into())),
        };
        self.pending_operations = Some(result.clone());
        self.state = advance(self.state, SessionEvent::ExecutionApplied)?;
        Ok(result)
    }

    /// Settle: close the period on a quarter boundary and checkpoint.
    ///
    /// A cash-flow breach here is terminal. There is no repair branch, and
    /// adding one would be the single most damaging change possible to this
    /// module — books that heal themselves cannot be trusted to report a loss.
    /// Pay out the lotteries the player bought into this turn.
    ///
    /// The outcome was drawn at plant time, so it is a fixed property of the
    /// campaign rather than of when the player answered — accepting on a slow
    /// turn cannot buy a better draw.
    ///
    /// Settlement runs through receivables and payables rather than cash so a
    /// bad outcome can always be posted. A loss that silently shrank to fit the
    /// available cash would change the gamble's terms after the choice, which
    /// is precisely the thing lane 0 is trying to measure.
    fn resolve_gambles(&mut self) -> Result<(), DirectorError> {
        let mut plans: Vec<ActionPlan> = Vec::with_capacity(self.active.len());
        for planted in &self.active {
            let (offer_id, gain_minor, loss_minor, good_state) = match planted.params {
                StimulusParams::GamblePair {
                    offer_id,
                    gain_minor,
                    loss_minor,
                    good_state,
                    ..
                } => (offer_id, gain_minor, loss_minor, good_state),
                _ => continue,
            };
            if self.books.firm.offer(offer_id).map(|o| o.status) != Ok(OfferStatus::Accepted) {
                continue;
            }
            let transaction = if good_state {
                transfer(
                    TxKind::HedgeSettlement,
                    AccountCode::AccountsReceivable,
                    AccountCode::OtherIncome,
                    checked_amount(gain_minor)?,
                )?
            } else {
                transfer(
                    TxKind::PayOpex,
                    AccountCode::OperatingExpense,
                    AccountCode::AccountsPayable,
                    checked_amount(loss_minor)?,
                )?
            };
            plans.push(ActionPlan {
                transaction: Some(transaction),
                firm_effect: FirmEffect::None,
            });
        }
        if !plans.is_empty() {
            apply_plans(&plans, &mut self.books)?;
        }
        Ok(())
    }

    pub fn settle(&mut self) -> Result<Option<PeriodClose>, DirectorError> {
        self.require(TurnPhase::Settle)?;
        self.resolve_gambles()?;
        let ctx = self.context()?;
        let turn = self.turns_completed.saturating_add(1);
        let mut closed = None;
        if turn.is_multiple_of(TICKS_PER_QUARTER) {
            let period_begin = self.period_begin.clone();
            let close = match close_period(&ctx, &mut self.books, &period_begin) {
                Ok(c) => c,
                Err(e @ SettleError::AccountingBreach { .. }) => {
                    return Err(self.die(FailureReason::AccountingBreach, e.into()));
                }
                Err(e) => {
                    return Err(self.die(FailureReason::InternalInvariantBroken, e.into()));
                }
            };
            self.period_begin = self.books.balances.clone();
            let generation = self
                .generations
                .capture(&self.kernel, &self.books.balances, &self.books.firm)?;
            if let Err(e) = self.generations.verify(generation) {
                return Err(self.die(FailureReason::SnapshotDigestMismatch, e.into()));
            }
            closed = Some(close);
        }
        self.pending_close = closed;
        self.state = advance(self.state, SessionEvent::SettlementVerified)?;
        Ok(closed)
    }

    /// Report: hand back the turn summary and roll into the next Observe, or
    /// seal the campaign if this was the last turn.
    pub fn report(&mut self) -> Result<TurnReport, DirectorError> {
        self.require(TurnPhase::Report)?;
        let operations = match self.pending_operations.take() {
            Some(r) => r,
            None => {
                return Err(self.die(
                    FailureReason::InternalInvariantBroken,
                    DirectorError::WrongPhase {
                        state: self.state,
                        wanted: TurnPhase::Execute,
                    },
                ))
            }
        };
        let market = self.observed()?;
        let report = TurnReport {
            tick: market.tick,
            market,
            operations,
            period_close: self.pending_close.take(),
            state_digest: self.state_digest()?,
        };
        self.turns_completed = self.turns_completed.saturating_add(1);
        let event = if self.turns_completed >= CAMPAIGN_TICKS {
            SessionEvent::CampaignCompleted
        } else {
            SessionEvent::ReportAcknowledged
        };
        self.state = advance(self.state, event)?;
        Ok(report)
    }

    /// Run one whole turn. Convenience for drivers and replay; the phase
    /// methods remain the real interface so a UI can stop between them.
    pub fn run_turn(
        &mut self,
        intent: ActionIntent,
        latency_ms: Option<u32>,
    ) -> Result<TurnReport, DirectorError> {
        self.observe()?;
        self.submit(intent, latency_ms)?;
        self.execute()?;
        self.settle()?;
        self.report()
    }

    /// Hand every record produced since the last successful flush to `sink`.
    ///
    /// The ring is cleared only after the sink acknowledges. A sink that fails
    /// therefore costs a retry and nothing else — see `persist.rs` for why that
    /// ordering is the whole point rather than an implementation detail.
    ///
    /// The stimulus ledger is append-only and is never cleared, so a watermark
    /// tracks how much of it has already been sent. Re-sending the whole ledger
    /// each quarter would grow the write quadratically over a campaign.
    pub fn flush_to(&mut self, sink: &mut dyn DecisionSink) -> Result<FlushReceipt, DirectorError> {
        let events: Vec<DecisionEvent> = self.events.records().copied().collect();
        let pending_stimuli: Vec<PlantedStimulus> = self
            .stimuli()
            .rows()
            .iter()
            .skip(self.stimuli_flushed)
            .copied()
            .collect();
        let batch = DecisionBatch {
            schema: BLACKBOX_LOG_SCHEMA_V1,
            campaign_fingerprint: self.genesis.fingerprint,
            events: &events,
            stimuli: &pending_stimuli,
        };
        if batch.is_empty() {
            return Ok(FlushReceipt::default());
        }
        let receipt = sink.persist(&batch)?;
        // A sink that acknowledges less than it was given has not stored the
        // batch, whatever it returned. Keep everything and report the fault.
        if usize::try_from(receipt.events_persisted).unwrap_or(0) != events.len()
            || usize::try_from(receipt.stimuli_persisted).unwrap_or(0) != pending_stimuli.len()
        {
            return Err(DirectorError::Sink(SinkError::Partial));
        }
        let _ = self.events.drain_for_flush();
        self.stimuli_flushed = self.stimuli.len();
        Ok(receipt)
    }

    /// Identity of this campaign for storage and joins.
    ///
    /// The fingerprint, never the seed: the seed regenerates every true
    /// parameter in the world, so it must not travel with the analysis
    /// (wall W-a).
    #[must_use]
    pub fn campaign_fingerprint(&self) -> [u8; 32] {
        self.genesis.fingerprint
    }

    /// Records written to the log but not yet acknowledged by a sink.
    #[must_use]
    pub fn unflushed_records(&self) -> usize {
        self.events.len()
    }

    pub fn abort(&mut self) -> Result<(), DirectorError> {
        self.state = advance(self.state, SessionEvent::AbortRequested)?;
        Ok(())
    }
}

/// Reconstruct a session from its genesis request and decision log, and return
/// the resulting state digest. This is the executable form of BXS-I-01.
///
/// Note what the signature does NOT accept: no market data, no timings, no
/// snapshots. If reproduction needed any of those, the claim "the world is a
/// pure function of the seed and the decisions" would be false.
pub fn replay_digest(
    request: GenesisRequest,
    log: &[LoggedDecision],
) -> Result<StateDigest, DirectorError> {
    replay_session(request, log)?.state_digest()
}

/// The same reconstruction, handing back the whole rebuilt session.
///
/// Callers that need more than the digest use this — in particular, comparing
/// the replayed answer key against the live one proves that the *stimuli* were
/// reproduced too, not merely the balances. The two can diverge: a scheduler
/// that consulted session history would still land on identical books while
/// planting different probes, and the digest alone would not notice.
pub fn replay_session(
    request: GenesisRequest,
    log: &[LoggedDecision],
) -> Result<Session, DirectorError> {
    let mut session = Session::start(request)?;
    for (index, entry) in log.iter().enumerate() {
        let view = session.observe()?;
        if view.tick != entry.tick {
            // The log claims a tick the reconstructed world does not reach —
            // the log is not a log of this campaign.
            return Err(DirectorError::MalformedLog { at_index: index });
        }
        session.submit(entry.intent, None)?;
        session.execute()?;
        session.settle()?;
        session.report()?;
    }
    session.stimuli.verify()?;
    Ok(session)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::blackbox_sim::genesis::Difficulty;
    use crate::blackbox_sim::ledger::AccountCode;

    fn ok<T, E: std::fmt::Debug>(r: Result<T, E>) -> T {
        match r {
            Ok(v) => v,
            Err(e) => unreachable!("director test setup failed: {e:?}"),
        }
    }

    fn request() -> GenesisRequest {
        GenesisRequest {
            scenario_id: 3,
            difficulty: Difficulty::Standard,
            campaign_index: 1,
            created_date: "2026-07-27".to_string(),
        }
    }

    /// A deterministic but varied script: enough different intents that a
    /// replay bug in any one of them would show up.
    fn scripted_intent(turn: u32) -> ActionIntent {
        match turn % 8 {
            0 => ActionIntent::OrderInventory {
                sku: 0,
                units: 400,
            },
            1 => ActionIntent::SetPrice {
                sku: 0,
                tick_price: 7_000 + i64::from(turn) * 11,
            },
            2 => ActionIntent::Borrow {
                facility: 0,
                amount_minor: 120_000,
            },
            3 => ActionIntent::OrderInventory {
                sku: 1,
                units: 250,
            },
            4 => ActionIntent::Invest {
                project_id: 0,
                amount_minor: 60_000,
            },
            5 => ActionIntent::ForecastInterval {
                lo_minor: 1_000,
                hi_minor: 5_000,
            },
            6 => ActionIntent::OrderInventory {
                sku: 2,
                units: 120,
            },
            _ => ActionIntent::Abstain,
        }
    }

    /// One scripted turn.
    ///
    /// The script names entities blindly, so some turns are legitimately
    /// refused — the Director now plants probe projects into the same id space.
    /// A refusal is deterministic, so the fallback keeps the run reproducible;
    /// what it must not do is abort the campaign.
    fn scripted_turn(session: &mut Session, turn: u32, latency: Option<u32>) -> TurnReport {
        ok(session.observe());
        if session.submit(scripted_intent(turn), latency).is_err() {
            ok(session.submit(ActionIntent::Abstain, latency));
        }
        ok(session.execute());
        ok(session.settle());
        ok(session.report())
    }

    fn run_scripted(turns: u32, latency: impl Fn(u32) -> Option<u32>) -> Session {
        let mut session = ok(Session::start(request()));
        for turn in 0..turns {
            let _ = scripted_turn(&mut session, turn, latency(turn));
        }
        session
    }

    /// Answer whatever probe is on the table, so attribution has something to
    /// bite on. Falls back to the ordinary script on unprobed turns.
    fn responsive_intent(session: &Session, turn: u32) -> ActionIntent {
        for view in session.stimulus_views() {
            match view.kind {
                crate::blackbox_sim::telemetry::StimulusKind::GamblePair => {
                    if let Some(offer_id) = view.offer_id {
                        return if turn.is_multiple_of(3) {
                            ActionIntent::DeclineOffer { offer_id }
                        } else {
                            ActionIntent::AcceptOffer { offer_id }
                        };
                    }
                }
                crate::blackbox_sim::telemetry::StimulusKind::AnchorProbe
                | crate::blackbox_sim::telemetry::StimulusKind::ForecastElicitation => {
                    return ActionIntent::ForecastInterval {
                        lo_minor: 100_000,
                        hi_minor: 900_000,
                    };
                }
                crate::blackbox_sim::telemetry::StimulusKind::SunkCostPair => {
                    if let Some(project_id) = view.project_id {
                        return ActionIntent::ContinueProject { project_id };
                    }
                }
                _ => {}
            }
        }
        scripted_intent(turn)
    }

    fn run_responsive(turns: u32) -> Session {
        let mut session = ok(Session::start(request()));
        for turn in 0..turns {
            ok(session.observe());
            let intent = responsive_intent(&session, turn);
            // A refused intent is a legal outcome of a script; fall back to the
            // default so the campaign keeps moving.
            if session.submit(intent, None).is_err() {
                ok(session.submit_timeout_default());
            }
            ok(session.execute());
            ok(session.settle());
            ok(session.report());
        }
        session
    }

    #[test]
    fn a_successful_flush_hands_over_records_and_clears_the_ring() {
        use crate::blackbox_sim::persist::testing::RecordingSink;
        let mut session = run_scripted(14, |_| None);
        let produced = session.unflushed_records();
        assert!(produced > 0, "a campaign produces records");
        let mut sink = RecordingSink::default();
        let receipt = ok(session.flush_to(&mut sink));
        assert_eq!(receipt.events_persisted as usize, produced);
        assert_eq!(sink.events.len(), produced);
        assert_eq!(session.unflushed_records(), 0, "the ring was cleared");
        // Every attribution must still resolve against the flushed answer key.
        for event in &sink.events {
            if let Some(reference) = event.stimulus {
                assert!(
                    sink.stimuli
                        .iter()
                        .any(|row| row.seq == reference.stimulus_seq
                            && row.params_digest == reference.params_digest),
                    "flushed log references a row that was not flushed"
                );
            }
        }
    }

    #[test]
    fn a_failed_flush_loses_nothing() {
        use crate::blackbox_sim::persist::testing::RecordingSink;
        use crate::blackbox_sim::persist::SinkError;
        let mut session = run_scripted(10, |_| None);
        let produced = session.unflushed_records();
        let mut sink = RecordingSink {
            fail_with: Some(SinkError::Unavailable),
            ..RecordingSink::default()
        };
        assert!(matches!(
            session.flush_to(&mut sink),
            Err(DirectorError::Sink(SinkError::Unavailable))
        ));
        assert_eq!(
            session.unflushed_records(),
            produced,
            "a locked vault must not cost a single record (第八律)"
        );
        // And the retry succeeds with the full set.
        sink.fail_with = None;
        let receipt = ok(session.flush_to(&mut sink));
        assert_eq!(receipt.events_persisted as usize, produced);
        assert_eq!(sink.events.len(), produced);
    }

    #[test]
    fn a_partial_acknowledgement_is_treated_as_failure() {
        use crate::blackbox_sim::persist::{
            DecisionBatch, DecisionSink, FlushReceipt, SinkError,
        };
        /// Claims to have stored one fewer record than it was given.
        struct Undercounting;
        impl DecisionSink for Undercounting {
            fn persist(
                &mut self,
                batch: &DecisionBatch<'_>,
            ) -> Result<FlushReceipt, SinkError> {
                Ok(FlushReceipt {
                    events_persisted: u32::try_from(batch.events.len().saturating_sub(1))
                        .unwrap_or(0),
                    stimuli_persisted: u32::try_from(batch.stimuli.len()).unwrap_or(0),
                })
            }
        }
        let mut session = run_scripted(6, |_| None);
        let produced = session.unflushed_records();
        assert!(matches!(
            session.flush_to(&mut Undercounting),
            Err(DirectorError::Sink(SinkError::Partial))
        ));
        assert_eq!(session.unflushed_records(), produced);
    }

    #[test]
    fn flushing_never_resends_an_answer_key_row() {
        use crate::blackbox_sim::persist::testing::RecordingSink;
        let mut session = ok(Session::start(request()));
        let mut sink = RecordingSink::default();
        for turn in 0..CAMPAIGN_TICKS {
            let _ = scripted_turn(&mut session, turn, None);
            let _ = ok(session.flush_to(&mut sink));
        }
        let mut seen: Vec<u32> = sink.stimuli.iter().map(|r| r.seq).collect();
        let total = seen.len();
        seen.sort_unstable();
        seen.dedup();
        assert_eq!(seen.len(), total, "a row was persisted twice");
        assert_eq!(total, session.stimuli().len(), "every row reached the sink");
    }

    #[test]
    fn a_sink_can_only_be_written_to() {
        // Wall W-b as a compile-time fact: `DecisionSink` exposes exactly one
        // method and it takes a batch and returns counts. If this assertion
        // ever needs updating because a read method appeared, stop and read
        // the persist.rs header — the fix is a separate loader, not a wider
        // trait.
        use crate::blackbox_sim::persist::testing::RecordingSink;
        use crate::blackbox_sim::persist::{DecisionSink, NullSink};
        fn accepts_any_sink(_: &mut dyn DecisionSink) {}
        accepts_any_sink(&mut NullSink);
        accepts_any_sink(&mut RecordingSink::default());
    }

    #[test]
    fn a_full_campaign_plants_the_probes_the_lanes_need() {
        use crate::blackbox_sim::telemetry::StimulusKind;
        let s = run_responsive(CAMPAIGN_TICKS);
        let count = |kind: StimulusKind| {
            s.stimuli()
                .rows()
                .iter()
                .filter(|r| r.params.kind() == kind)
                .count()
        };
        assert!(count(StimulusKind::GamblePair) >= 12, "lane 0 starved");
        assert!(
            count(StimulusKind::AnchorProbe) >= 12,
            "lane 2 needs 6 High/Low pairs"
        );
        assert_eq!(count(StimulusKind::ForecastElicitation), 4, "one per quarter");
        assert!(count(StimulusKind::SunkCostPair) >= 8, "lane 4 needs 4 pairs");
        assert!(count(StimulusKind::CrisisCountdown) >= 4, "two windows");
        assert!(s.stimuli().verify().is_ok(), "every binding holds");
    }

    #[test]
    fn responses_are_attributed_and_idle_turns_are_not() {
        let s = run_responsive(CAMPAIGN_TICKS);
        let attributed = s.events.records().filter(|r| r.stimulus.is_some()).count();
        assert!(attributed >= 24, "probes went unanswered: {attributed}");
        // Every attribution must point at a row that actually exists, with a
        // digest that matches it. A dangling reference is worse than none.
        for record in s.events.records() {
            if let Some(reference) = record.stimulus {
                let row = s
                    .stimuli()
                    .get(reference.stimulus_seq)
                    .unwrap_or_else(|| unreachable!("dangling stimulus {reference:?}"));
                assert_eq!(row.params_digest, reference.params_digest);
                assert_eq!(row.params.kind(), reference.kind);
            }
        }
    }

    #[test]
    fn a_refused_intent_is_never_attributed() {
        let mut s = ok(Session::start(request()));
        ok(s.observe());
        // Answer an offer that does not exist: the compiler refuses, so no
        // record and therefore no attribution may appear.
        let before = s.events.len();
        assert!(s.submit(ActionIntent::AcceptOffer { offer_id: 900 }, None).is_err());
        assert_eq!(s.events.len(), before);
    }

    #[test]
    fn replay_reproduces_the_answer_key_not_just_the_balances() {
        let live = run_responsive(20);
        let replayed = ok(replay_session(request(), live.decisions()));
        assert_eq!(
            ok(replayed.state_digest()),
            ok(live.state_digest()),
            "BXS-I-01"
        );
        assert_eq!(
            replayed.stimuli(),
            live.stimuli(),
            "the schedule must be reproducible from the seed alone"
        );
    }

    #[test]
    fn published_views_carry_no_answer_key() {
        let mut s = ok(Session::start(request()));
        for turn in 0..CAMPAIGN_TICKS {
            ok(s.observe());
            for view in s.stimulus_views() {
                let json = serde_json::to_string(&view).unwrap_or_default();
                // Field names, not fragments: `sunk_cost_pair` is a public kind
                // label and must not be mistaken for a leaked `sunkMinor`.
                for sealed in [
                    "referenceMinor",
                    "deltaMicro",
                    "goodState",
                    "arm",
                    "sunkMinor",
                ] {
                    assert!(!json.contains(sealed), "turn {turn} leaked {sealed}: {json}");
                }
            }
            let intent = responsive_intent(&s, turn);
            if s.submit(intent, None).is_err() {
                ok(s.submit_timeout_default());
            }
            ok(s.execute());
            ok(s.settle());
            ok(s.report());
        }
    }

    #[test]
    fn every_set_price_leaves_a_pricing_trial_and_the_gap_is_never_negative() {
        let s = run_scripted(CAMPAIGN_TICKS, |_| None);
        let set_price_attempts = s
            .events()
            .filter(|e| matches!(e.action, ActionIntent::SetPrice { .. }))
            .count();
        let trials: Vec<&PricingTrial> = s.pricing_trials().collect();
        assert_eq!(
            trials.len(),
            set_price_attempts,
            "one lane-5 sample per successful SetPrice, no more, no fewer"
        );
        assert!(!trials.is_empty(), "the script never priced anything");
        for t in &trials {
            assert!(t.gap_minor >= 0, "the optimum must never lose to the chosen price");
        }
    }

    #[test]
    fn a_pricing_trial_under_an_active_crisis_is_flagged_under_pressure() {
        // Drive turns until a CrisisCountdown is planted, then price into it.
        use crate::blackbox_sim::telemetry::StimulusKind;
        let mut s = ok(Session::start(request()));
        let mut saw_pressured_trial = false;
        for turn in 0..CAMPAIGN_TICKS {
            ok(s.observe());
            let under_crisis = s
                .stimulus_views()
                .iter()
                .any(|v| v.kind == StimulusKind::CrisisCountdown);
            let intent = ActionIntent::SetPrice {
                sku: 0,
                tick_price: 7_500,
            };
            if s.submit(intent, None).is_err() {
                ok(s.submit_timeout_default());
            } else if under_crisis {
                saw_pressured_trial = true;
            }
            ok(s.execute());
            ok(s.settle());
            ok(s.report());
            let _ = turn;
        }
        assert!(saw_pressured_trial, "the script never priced during a crisis window");
        assert!(
            s.pricing_trials().any(|t| t.under_pressure),
            "at least one lane-5 sample must carry the pressure flag"
        );
        assert!(
            s.pricing_trials().any(|t| !t.under_pressure),
            "at least one lane-5 sample must be unflagged, for contrast"
        );
    }

    #[test]
    fn planting_preserves_every_accounting_invariant() {
        let s = run_responsive(CAMPAIGN_TICKS);
        assert!(s.books().balances.verify_zero_sum().is_ok(), "BXS-I-02");
        assert!(
            crate::blackbox_sim::settle::verify_inventory_reconciled(s.books()).is_ok(),
            "BXS-I-17"
        );
    }

    #[test]
    fn the_sunk_arm_differs_from_its_counterfactual_in_exactly_one_respect() {
        let s = run_responsive(CAMPAIGN_TICKS);
        let mut with_sunk = 0_u32;
        let mut without = 0_u32;
        for row in s.stimuli().rows() {
            if let StimulusParams::SunkCostPair {
                project_id,
                sunk_minor,
                arm,
            } = row.params
            {
                let project = ok(s.books().firm.project(project_id));
                match arm {
                    crate::blackbox_sim::stimulus::SunkArm::WithSunk => {
                        with_sunk += 1;
                        assert!(sunk_minor > 0, "the sunk arm must have sunk something");
                        assert!(project.committed_minor >= sunk_minor);
                    }
                    crate::blackbox_sim::stimulus::SunkArm::WithoutSunk => {
                        without += 1;
                        assert_eq!(sunk_minor, 0, "the control arm has no history");
                    }
                }
            }
        }
        assert!(with_sunk >= 4 && without >= 4, "{with_sunk}/{without}");
    }

    #[test]
    fn an_accepted_gamble_pays_out_and_a_declined_one_does_not() {
        // Drive the first gamble turn twice from identical starts, accepting in
        // one run and declining in the other. The outcome is predetermined, so
        // the only difference is whether the player took the ticket.
        let mut accept = ok(Session::start(request()));
        let mut decline = ok(Session::start(request()));
        let mut answered = false;
        for _ in 0..4 {
            ok(accept.observe());
            ok(decline.observe());
            let offer = accept.stimulus_views().into_iter().find_map(|v| v.offer_id);
            let (a, d) = match offer {
                Some(offer_id) if !answered => {
                    answered = true;
                    (
                        ActionIntent::AcceptOffer { offer_id },
                        ActionIntent::DeclineOffer { offer_id },
                    )
                }
                _ => (ActionIntent::Abstain, ActionIntent::Abstain),
            };
            ok(accept.submit(a, None));
            ok(decline.submit(d, None));
            for s in [&mut accept, &mut decline] {
                ok(s.execute());
                ok(s.settle());
                ok(s.report());
            }
        }
        assert!(answered, "no gamble was planted in the first four turns");
        let outcome_accounts = [
            AccountCode::OtherIncome,
            AccountCode::AccountsReceivable,
            AccountCode::AccountsPayable,
        ];
        let moved = outcome_accounts.iter().any(|a| {
            accept.books().balances.balance_minor(*a) != decline.books().balances.balance_minor(*a)
        });
        assert!(moved, "accepting a lottery must change the books");
        assert!(decline.books().balances.verify_zero_sum().is_ok());
        assert!(accept.books().balances.verify_zero_sum().is_ok());
    }

    #[test]
    fn a_session_walks_the_spec_phase_order() {
        let mut s = ok(Session::start(request()));
        assert_eq!(
            s.state(),
            SessionState::Active {
                phase: TurnPhase::Observe
            }
        );
        let _ = ok(s.observe());
        assert_eq!(
            s.state(),
            SessionState::Active {
                phase: TurnPhase::Decide
            }
        );
        let _ = ok(s.submit(ActionIntent::Abstain, Some(1_200)));
        assert_eq!(
            s.state(),
            SessionState::Active {
                phase: TurnPhase::Execute
            }
        );
        let _ = ok(s.execute());
        assert_eq!(
            s.state(),
            SessionState::Active {
                phase: TurnPhase::Settle
            }
        );
        let _ = ok(s.settle());
        assert_eq!(
            s.state(),
            SessionState::Active {
                phase: TurnPhase::Report
            }
        );
        let _ = ok(s.report());
        assert_eq!(
            s.state(),
            SessionState::Active {
                phase: TurnPhase::Observe
            }
        );
        assert_eq!(s.turns_completed(), 1);
    }

    #[test]
    fn phases_cannot_be_skipped() {
        let mut s = ok(Session::start(request()));
        assert!(matches!(
            s.submit(ActionIntent::Abstain, None),
            Err(DirectorError::WrongPhase {
                wanted: TurnPhase::Decide,
                ..
            })
        ));
        assert!(matches!(
            s.execute(),
            Err(DirectorError::WrongPhase {
                wanted: TurnPhase::Execute,
                ..
            })
        ));
        assert!(matches!(
            s.report(),
            Err(DirectorError::WrongPhase {
                wanted: TurnPhase::Report,
                ..
            })
        ));
        let _ = ok(s.observe());
        assert!(matches!(
            s.observe(),
            Err(DirectorError::WrongPhase {
                wanted: TurnPhase::Observe,
                ..
            })
        ));
    }

    /// Repaying a facility with nothing outstanding is always
    /// `RepayExceedsOutstanding`, independent of which probe is on the table,
    /// which makes it a clean vehicle for testing the refusal-log gate itself.
    fn repay_nonexistent_debt(s: &mut Session) -> Result<Execution, DirectorError> {
        s.submit(
            ActionIntent::Repay {
                facility: 0,
                amount_minor: 1,
            },
            None,
        )
    }

    /// Plant a deterministic, minimal probe directly into `active` — the
    /// director's own planting cadence is RNG-gated (an offer may or may not
    /// materialise on any given gamble turn), so exercising the refusal-log
    /// GATE itself must not depend on that luck.
    fn force_active_stimulus(s: &mut Session) {
        let planted = ok(s
            .stimuli
            .plant(0, StimulusParams::ForecastElicitation { reference_minor: 0 }));
        s.active.push(planted);
    }

    #[test]
    fn a_business_rule_refusal_under_an_active_stimulus_is_logged() {
        let mut s = ok(Session::start(request()));
        ok(s.observe());
        force_active_stimulus(&mut s);
        let before = s.refusals().len();
        assert!(matches!(
            repay_nonexistent_debt(&mut s),
            Err(DirectorError::Compile(CompileError::RepayExceedsOutstanding { .. }))
        ));
        assert_eq!(s.refusals().len(), before + 1, "refusal must be logged");
        let logged = s
            .refusals()
            .records()
            .last()
            .unwrap_or_else(|| unreachable!("just recorded one"));
        assert_eq!(logged.reason, RefusalReason::RepayExceedsOutstanding);
    }

    #[test]
    fn a_business_rule_refusal_with_no_active_stimulus_is_not_logged() {
        let mut s = ok(Session::start(request()));
        ok(s.observe());
        // Whatever the turn's own RNG-gated cadence happened to plant, force
        // the no-stimulus condition explicitly so this test does not depend
        // on the genesis seed's luck.
        s.active.clear();
        let before = s.refusals().len();
        assert!(matches!(
            repay_nonexistent_debt(&mut s),
            Err(DirectorError::Compile(CompileError::RepayExceedsOutstanding { .. }))
        ));
        assert_eq!(s.refusals().len(), before, "no active stimulus, no log entry");
    }

    #[test]
    fn a_malformed_or_unknown_entity_refusal_is_never_logged() {
        let mut s = ok(Session::start(request()));
        ok(s.observe());
        force_active_stimulus(&mut s);
        let before = s.refusals().len();
        assert!(s
            .submit(
                ActionIntent::Borrow {
                    facility: 0,
                    amount_minor: -5,
                },
                None
            )
            .is_err());
        assert!(s
            .submit(ActionIntent::AcceptOffer { offer_id: 9_999 }, None)
            .is_err());
        assert_eq!(
            s.refusals().len(),
            before,
            "AmountOutOfRange and UnknownOffer are shape/existence errors, not business rules"
        );
    }

    #[test]
    fn a_refused_intent_does_not_consume_the_turn() {
        let mut s = ok(Session::start(request()));
        let _ = ok(s.observe());
        assert!(matches!(
            s.submit(
                ActionIntent::Borrow {
                    facility: 0,
                    amount_minor: -5
                },
                None
            ),
            Err(DirectorError::Compile(CompileError::AmountOutOfRange {
                got_minor: -5
            }))
        ));
        assert_eq!(
            s.state(),
            SessionState::Active {
                phase: TurnPhase::Decide
            },
            "a refusal is not a decision"
        );
        assert!(s.decisions().is_empty(), "refusals must not enter the log");
        let _ = ok(s.submit(ActionIntent::Abstain, None));
        assert_eq!(s.decisions().len(), 1);
    }

    /// BXS-I-01, stated directly.
    #[test]
    fn replay_reproduces_the_live_state_exactly() {
        let live = run_scripted(20, |t| Some(t * 37 + 100));
        let live_digest = ok(live.state_digest());
        let replayed = ok(replay_digest(request(), live.decisions()));
        assert_eq!(
            live_digest, replayed,
            "state(t) must equal fold(Genesis, decisions[0..t])"
        );
    }

    /// BXS-W-01: latency is observed, never causal. Two runs that differ only
    /// in how long the player took must be the same world.
    #[test]
    fn latency_cannot_reach_the_state() {
        let fast = run_scripted(15, |_| Some(5));
        let slow = run_scripted(15, |t| Some(90_000 + t));
        let none = run_scripted(15, |_| None);
        let a = ok(fast.state_digest());
        assert_eq!(a, ok(slow.state_digest()));
        assert_eq!(a, ok(none.state_digest()));
        assert_eq!(fast.decisions(), slow.decisions());
    }

    #[test]
    fn a_timeout_default_is_a_logged_decision_not_a_gap() {
        let mut s = ok(Session::start(request()));
        let _ = ok(s.observe());
        let _ = ok(s.submit_timeout_default());
        let _ = ok(s.execute());
        let _ = ok(s.settle());
        let _ = ok(s.report());
        assert_eq!(s.decisions().len(), 1);
        match s.decisions().first() {
            Some(d) => assert_eq!(d.intent, ActionIntent::ForcedDefault),
            None => unreachable!("a decision was just submitted"),
        }
        // And it replays like any other decision.
        assert_eq!(ok(s.state_digest()), ok(replay_digest(request(), s.decisions())));
    }

    #[test]
    fn quarters_close_and_checkpoint_on_schedule() {
        let s = run_scripted(TICKS_PER_QUARTER * 2, |_| None);
        assert_eq!(s.generations().len(), 2, "one checkpoint per closed quarter");
        let latest = ok(s.generations().latest());
        assert_eq!(latest.tick, TICKS_PER_QUARTER * 2);
        ok(s.generations().verify(latest.generation));
    }

    #[test]
    fn every_close_satisfies_the_cash_flow_identity() {
        let mut session = ok(Session::start(request()));
        let mut closes = 0;
        for turn in 0..CAMPAIGN_TICKS {
            let report = scripted_turn(&mut session, turn, None);
            if let Some(close) = report.period_close {
                ok(close.cash_flow.verify());
                closes += 1;
            }
        }
        assert_eq!(closes, 4, "a campaign is four quarters");
        assert_eq!(session.state(), SessionState::Sealed);
        ok(session.books().balances.verify_zero_sum());
        ok(session.books().balances.verify_accounting_identity());
    }

    #[test]
    fn a_sealed_campaign_accepts_nothing_further() {
        let mut session = run_scripted(CAMPAIGN_TICKS, |_| None);
        assert_eq!(session.state(), SessionState::Sealed);
        assert!(matches!(
            session.observe(),
            Err(DirectorError::WrongPhase { .. })
        ));
        assert!(matches!(
            session.run_turn(ActionIntent::Abstain, None),
            Err(DirectorError::WrongPhase { .. })
        ));
    }

    #[test]
    fn a_log_from_a_different_campaign_is_rejected() {
        let live = run_scripted(6, |_| None);
        let mut forged: Vec<LoggedDecision> = live.decisions().to_vec();
        match forged.get_mut(3) {
            Some(d) => d.tick += 99,
            None => unreachable!("six decisions were logged"),
        }
        assert!(matches!(
            replay_digest(request(), &forged),
            Err(DirectorError::MalformedLog { at_index: 3 })
        ));
    }

    #[test]
    fn different_campaigns_replay_to_different_states() {
        let live = run_scripted(10, |_| None);
        let other = GenesisRequest {
            campaign_index: 2,
            ..request()
        };
        assert_ne!(
            ok(live.state_digest()),
            ok(replay_digest(other, live.decisions())),
            "the same decisions in a different world must not land in the same place"
        );
    }

    #[test]
    fn the_report_carries_the_view_and_not_the_world() {
        let mut s = ok(Session::start(request()));
        let report = ok(s.run_turn(ActionIntent::Abstain, None));
        assert_eq!(report.tick, report.market.tick);
        // Wall W-a is enforced structurally: TurnReport has no genesis field,
        // so the strongest runtime statement available is that what it does
        // carry is the published view.
        assert_eq!(Some(report.market), s.market());
        assert!(report.market.commodity_price_minor > 0);
    }

    #[test]
    fn an_unobserved_session_does_not_invent_a_market() {
        let s = ok(Session::start(request()));
        assert_eq!(
            s.market(),
            None,
            "a zeroed placeholder view would be a fabricated observation"
        );
    }

    #[test]
    fn opening_capital_is_posted_once_and_only_by_the_engine() {
        let s = ok(Session::start(request()));
        let cash = s.books().balances.balance_minor(AccountCode::Cash);
        assert_eq!(cash, s.genesis.firm.opening_capital_minor);
        assert_eq!(
            s.books().balances.balance_minor(AccountCode::ShareCapital),
            -cash
        );
        ok(s.books().balances.verify_zero_sum());
    }
}
