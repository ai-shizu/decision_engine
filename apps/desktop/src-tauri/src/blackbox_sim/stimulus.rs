//! Stimulus planting — the measurement instrument (SPEC §9.1; walls W-a, W-b).
//!
//! The Director plants measurement probes disguised as ordinary game events.
//! This module decides *what* is planted and *when*, seals the truth side, and
//! attributes each decision to the probe it answered.
//!
//! # The cadence is fixed, and that is the point
//!
//! Which turn carries which probe is a compile-time constant, identical for
//! every player and every campaign. Only the probe's *parameters* come from
//! `DOM_STIMULI`. This is wall W-b in its constructive form: the moment
//! exposure adapts to how a player is doing, two players' λ estimates stop
//! being comparable and the instrument measures the scheduler instead of the
//! subject. Do not add difficulty-adaptive scheduling here. Same-session game
//! performance may drive flavour and narrative; it may never drive *which*
//! probe fires.
//!
//! # The three-way split of every probe
//!
//! Each planted probe has parameters that fall into exactly one of three
//! classes, and confusing them is how this instrument dies:
//!
//! - **Published** — the player sees it ([`StimulusView`]). An anchor value, an
//!   offer id, a countdown.
//! - **Sealed** — the truth the probe perturbs ([`StimulusParams`]). The
//!   reference valuation, the predetermined outcome. It travels sim → analysis
//!   → vault, which is the one direction the walls allow, and it must never
//!   turn around and reach a view model or an LLM prompt (wall W-a).
//! - **Derived** — the digest that binds a log entry to its parameter row.
//!
//! [`StimulusView`] and [`StimulusParams`] are separate types for exactly this
//! reason. A single type with a "don't show these fields" comment would be one
//! careless spread operator away from publishing the answer key.
//!
//! # Why parameters live in a side table
//!
//! `StimulusRef` carries a *digest*, not the parameters, and a digest cannot be
//! inverted. The estimators need the real (G, L, p) and the real anchor, so the
//! planted rows are kept in [`StimulusLedger`] and the digest binds the two
//! (SPEC §16.3 pointer–payload binding, §10 "オラクル別表"). A mismatch is
//! fatal rather than a hint: a decision log pointing at parameters that are not
//! the ones the player faced is worse than no measurement at all.

use serde::Serialize;
use sha2::{Digest, Sha256};

use super::firm::{position_pnl_minor, FirmState, Offer, OfferKind, OfferStatus};
use super::rng::{PhiloxStream, SeedDomain};
use super::settle::TICKS_PER_QUARTER;
use super::telemetry::{ActionIntent, StimulusKind, StimulusRef};

/// Capacity of the planted-parameter table. A campaign plants ~46 probes; the
/// headroom absorbs schedule changes without silently dropping rows.
///
/// Reject-full, never evicting: these rows are the answer key for the decision
/// log, and a log entry whose parameters were evicted is unanalysable
/// (第八律 — records are sacred; only derived series may evict).
pub const MAX_PLANTED_STIMULI: usize = 128;

// ---------------------------------------------------------------------------
// Cadence. Frozen constants — see the module header before touching these.
// ---------------------------------------------------------------------------

const GAMBLE_PERIOD: u32 = 4;
const GAMBLE_PHASE: u32 = 1;
const FORECAST_PERIOD: u32 = 4;
const FORECAST_PHASE: u32 = 2;
const SUNK_PERIOD: u32 = 6;
const SUNK_PHASE: u32 = 3;
const DISPOSITION_PERIOD: u32 = 5;
const DISPOSITION_PHASE: u32 = 0;
/// Turns on which a margin-call window opens.
const CRISIS_TURNS: [u32; 2] = [20, 44];
/// How many turns a crisis window stays open once it opens.
const CRISIS_WINDOW: u32 = 3;

#[inline]
fn hits(turn: u32, period: u32, phase: u32) -> bool {
    turn != 0 && turn % period == phase
}

/// Quarter ends carry the SPEC §9.1 Report forecast, which is deliberately
/// *unanchored* and therefore acts as the control arm.
#[must_use]
pub fn is_control_forecast_turn(turn: u32) -> bool {
    turn != 0 && turn.is_multiple_of(TICKS_PER_QUARTER)
}

/// Anchored forecast turns yield to the quarter-end control when they collide.
///
/// Over 52 turns the four quarter ends land on all four residues mod 4, so
/// exactly one anchored turn is displaced no matter which phase is chosen.
/// Losing that one is what leaves 12 anchored forecasts — precisely the 6
/// High/Low pairs lane 2 needs.
#[must_use]
pub fn is_anchored_forecast_turn(turn: u32) -> bool {
    hits(turn, FORECAST_PERIOD, FORECAST_PHASE) && !is_control_forecast_turn(turn)
}

#[must_use]
pub fn is_gamble_turn(turn: u32) -> bool {
    hits(turn, GAMBLE_PERIOD, GAMBLE_PHASE)
}

#[must_use]
pub fn is_sunk_cost_turn(turn: u32) -> bool {
    hits(turn, SUNK_PERIOD, SUNK_PHASE)
}

#[must_use]
pub fn is_disposition_turn(turn: u32) -> bool {
    hits(turn, DISPOSITION_PERIOD, DISPOSITION_PHASE)
}

/// Turns left in the open crisis window, or `None` outside one.
#[must_use]
pub fn crisis_ticks_remaining(turn: u32) -> Option<u32> {
    CRISIS_TURNS
        .iter()
        .find(|start| turn >= **start && turn < start.saturating_add(CRISIS_WINDOW))
        .map(|start| start.saturating_add(CRISIS_WINDOW).saturating_sub(turn))
}

/// How many times this predicate has already fired before `turn`.
///
/// O(turn) by design. Deriving the arm from a running counter would make it a
/// function of session history rather than of the turn number, and the whole
/// schedule would stop being randomly addressable under replay.
fn occurrence_index(turn: u32, fires: fn(u32) -> bool) -> Option<u32> {
    if !fires(turn) {
        return None;
    }
    u32::try_from((1..turn).filter(|t| fires(*t)).count()).ok()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
#[repr(u8)]
pub enum AnchorArm {
    High = 0,
    Low = 1,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
#[repr(u8)]
pub enum SunkArm {
    /// The project already carries committed capital.
    WithSunk = 0,
    /// Same forward economics, no history. The counterfactual.
    WithoutSunk = 1,
}

/// Strict alternation, not a coin flip: within-subject balance is a design
/// requirement (SPEC §9.1「被験者内交互割付」), and a random arm would leave the
/// High/Low counts unbalanced in a short campaign.
#[must_use]
pub fn anchor_arm(turn: u32) -> Option<AnchorArm> {
    occurrence_index(turn, is_anchored_forecast_turn).map(|i| {
        if i.is_multiple_of(2) {
            AnchorArm::High
        } else {
            AnchorArm::Low
        }
    })
}

#[must_use]
pub fn sunk_arm(turn: u32) -> Option<SunkArm> {
    occurrence_index(turn, is_sunk_cost_turn).map(|i| {
        if i.is_multiple_of(2) {
            SunkArm::WithSunk
        } else {
            SunkArm::WithoutSunk
        }
    })
}

// ---------------------------------------------------------------------------
// Planted parameters — the SEALED side.
// ---------------------------------------------------------------------------

/// The full description of a planted probe, truth included.
///
/// `Serialize` is present because these rows are persisted for the estimators.
/// That is the sim → analysis → vault direction the walls permit. What must
/// never happen is a view model or an LLM prompt reaching this type; the
/// contract test `test_stimulus_truth_never_reaches_a_view` guards it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase", tag = "kind", content = "params")]
pub enum StimulusParams {
    /// A mixed gamble dressed as an offer. Accepting buys the certain side.
    GamblePair {
        offer_id: u32,
        /// Payoff if the good state occurs, in minor units.
        gain_minor: i64,
        /// Payoff if the bad state occurs, in minor units (positive magnitude).
        loss_minor: i64,
        /// Probability of the good state, in micro units.
        win_probability_micro: i64,
        /// Cost of accepting, in minor units.
        premium_minor: i64,
        /// SEALED. Drawn at plant time so the outcome is a fixed lottery under
        /// replay rather than a function of when the player answered.
        good_state: bool,
    },
    AnchorProbe {
        /// Published. The "analyst consensus" the player is shown.
        anchor_minor: i64,
        arm: AnchorArm,
        /// SEALED. The reference the anchor was built from (`oracle.rs`).
        reference_minor: i64,
        /// SEALED. Perturbation applied to the reference, in micro units.
        delta_micro: i64,
    },
    /// The unanchored control forecast at quarter end.
    ForecastElicitation {
        /// SEALED.
        reference_minor: i64,
    },
    SunkCostPair {
        project_id: u32,
        /// Capital already committed. Zero on the `WithoutSunk` arm — that is
        /// the whole comparison.
        sunk_minor: i64,
        arm: SunkArm,
    },
    CrisisCountdown {
        ticks_remaining: u32,
        /// Cash the margin call demands, in minor units.
        required_cash_minor: i64,
    },
    DispositionWindow {
        position_id: u32,
        /// Mark-to-market gain (positive) or loss (negative). Reported for the
        /// measurement only; it is never posted (trap BXS-W-04).
        unrealised_minor: i64,
    },
}

impl StimulusParams {
    #[must_use]
    pub fn kind(self) -> StimulusKind {
        match self {
            StimulusParams::GamblePair { .. } => StimulusKind::GamblePair,
            StimulusParams::AnchorProbe { .. } => StimulusKind::AnchorProbe,
            StimulusParams::ForecastElicitation { .. } => StimulusKind::ForecastElicitation,
            StimulusParams::SunkCostPair { .. } => StimulusKind::SunkCostPair,
            StimulusParams::CrisisCountdown { .. } => StimulusKind::CrisisCountdown,
            StimulusParams::DispositionWindow { .. } => StimulusKind::DispositionWindow,
        }
    }

    /// Fixed-order byte encoding. Hand-rolled rather than serde-derived so the
    /// digest cannot shift under a rename or a serde attribute change.
    fn canonical(self, out: &mut Vec<u8>) {
        out.push(self.kind() as u8);
        let push_i64 = |v: i64, out: &mut Vec<u8>| out.extend_from_slice(&v.to_be_bytes());
        match self {
            StimulusParams::GamblePair {
                offer_id,
                gain_minor,
                loss_minor,
                win_probability_micro,
                premium_minor,
                good_state,
            } => {
                out.extend_from_slice(&offer_id.to_be_bytes());
                push_i64(gain_minor, out);
                push_i64(loss_minor, out);
                push_i64(win_probability_micro, out);
                push_i64(premium_minor, out);
                out.push(u8::from(good_state));
            }
            StimulusParams::AnchorProbe {
                anchor_minor,
                arm,
                reference_minor,
                delta_micro,
            } => {
                push_i64(anchor_minor, out);
                out.push(arm as u8);
                push_i64(reference_minor, out);
                push_i64(delta_micro, out);
            }
            StimulusParams::ForecastElicitation { reference_minor } => {
                push_i64(reference_minor, out);
            }
            StimulusParams::SunkCostPair {
                project_id,
                sunk_minor,
                arm,
            } => {
                out.extend_from_slice(&project_id.to_be_bytes());
                push_i64(sunk_minor, out);
                out.push(arm as u8);
            }
            StimulusParams::CrisisCountdown {
                ticks_remaining,
                required_cash_minor,
            } => {
                out.extend_from_slice(&ticks_remaining.to_be_bytes());
                push_i64(required_cash_minor, out);
            }
            StimulusParams::DispositionWindow {
                position_id,
                unrealised_minor,
            } => {
                out.extend_from_slice(&position_id.to_be_bytes());
                push_i64(unrealised_minor, out);
            }
        }
    }

    /// Truncated to 8 bytes: this is a cross-reference between a log entry and
    /// a parameter row, not a security boundary (SPEC §9.2).
    #[must_use]
    pub fn digest(self, tick: u32, seq: u32) -> [u8; 8] {
        let mut buf = Vec::with_capacity(64);
        buf.extend_from_slice(b"BXSSTIM1");
        buf.extend_from_slice(&tick.to_be_bytes());
        buf.extend_from_slice(&seq.to_be_bytes());
        self.canonical(&mut buf);
        let full: [u8; 32] = Sha256::digest(&buf).into();
        let mut short = [0_u8; 8];
        for (dst, src) in short.iter_mut().zip(full.iter()) {
            *dst = *src;
        }
        short
    }
}

/// One row of the answer key.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlantedStimulus {
    pub seq: u32,
    pub tick: u32,
    pub params: StimulusParams,
    pub params_digest: [u8; 8],
}

impl PlantedStimulus {
    #[must_use]
    pub fn reference(&self) -> StimulusRef {
        StimulusRef {
            kind: self.params.kind(),
            stimulus_seq: self.seq,
            params_digest: self.params_digest,
        }
    }

    /// Recompute the binding. Used on load: parameters that do not hash to the
    /// digest the log points at are not this campaign's parameters.
    #[must_use]
    pub fn binding_holds(&self) -> bool {
        self.params.digest(self.tick, self.seq) == self.params_digest
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StimulusError {
    /// The answer-key table is full. Reject, never evict (第八律).
    LedgerFull,
    Overflow,
    /// A stored row does not hash to its own digest (SPEC §16.3).
    BindingBroken { seq: u32 },
}

/// Append-only table of planted parameters.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StimulusLedger {
    rows: Vec<PlantedStimulus>,
}

impl Default for StimulusLedger {
    fn default() -> Self {
        Self::new()
    }
}

impl StimulusLedger {
    #[must_use]
    pub fn new() -> Self {
        Self {
            rows: Vec::with_capacity(MAX_PLANTED_STIMULI),
        }
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.rows.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }

    #[must_use]
    pub fn rows(&self) -> &[PlantedStimulus] {
        &self.rows
    }

    #[must_use]
    pub fn get(&self, seq: u32) -> Option<&PlantedStimulus> {
        self.rows.iter().find(|r| r.seq == seq)
    }

    /// Append a row and hand back its reference. There is no `remove` and no
    /// `set`; adding either would break 第八律 for the answer key just as it
    /// would for the decision log.
    pub fn plant(
        &mut self,
        tick: u32,
        params: StimulusParams,
    ) -> Result<PlantedStimulus, StimulusError> {
        if self.rows.len() >= MAX_PLANTED_STIMULI {
            return Err(StimulusError::LedgerFull);
        }
        let seq = u32::try_from(self.rows.len()).map_err(|_| StimulusError::Overflow)?;
        let row = PlantedStimulus {
            seq,
            tick,
            params,
            params_digest: params.digest(tick, seq),
        };
        self.rows.push(row);
        Ok(row)
    }

    /// Verify every binding. Cheap enough to run on load and after replay.
    pub fn verify(&self) -> Result<(), StimulusError> {
        match self.rows.iter().find(|r| !r.binding_holds()) {
            Some(bad) => Err(StimulusError::BindingBroken { seq: bad.seq }),
            None => Ok(()),
        }
    }
}

// ---------------------------------------------------------------------------
// Publication — the PLAYER-VISIBLE side.
// ---------------------------------------------------------------------------

/// What the UI is allowed to know about an active probe.
///
/// Every field here is either already visible in the game (an offer id, a
/// countdown) or is the deliberately perturbed anchor. Note what is absent:
/// the reference the anchor was built from, the perturbation, and the
/// predetermined outcome. Adding any of those turns the anchoring measurement
/// into a question with the answer printed underneath it (wall W-a).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StimulusView {
    pub kind: StimulusKind,
    pub stimulus_seq: u32,
    pub offer_id: Option<u32>,
    pub project_id: Option<u32>,
    pub position_id: Option<u32>,
    /// The analyst consensus, when one is being shown.
    pub anchor_minor: Option<i64>,
    pub gain_minor: Option<i64>,
    pub loss_minor: Option<i64>,
    pub win_probability_micro: Option<i64>,
    pub premium_minor: Option<i64>,
    pub ticks_remaining: Option<u32>,
    pub unrealised_minor: Option<i64>,
}

impl StimulusView {
    const BLANK: StimulusView = StimulusView {
        kind: StimulusKind::GamblePair,
        stimulus_seq: 0,
        offer_id: None,
        project_id: None,
        position_id: None,
        anchor_minor: None,
        gain_minor: None,
        loss_minor: None,
        win_probability_micro: None,
        premium_minor: None,
        ticks_remaining: None,
        unrealised_minor: None,
    };

    /// The single narrow gate from sealed parameters to published ones. Every
    /// arm names the fields it publishes explicitly; there is no struct spread
    /// and no `..params`, so a new sealed field cannot leak by default.
    #[must_use]
    pub fn publish(planted: &PlantedStimulus) -> StimulusView {
        let base = StimulusView {
            kind: planted.params.kind(),
            stimulus_seq: planted.seq,
            ..StimulusView::BLANK
        };
        match planted.params {
            StimulusParams::GamblePair {
                offer_id,
                gain_minor,
                loss_minor,
                win_probability_micro,
                premium_minor,
                good_state: _,
            } => StimulusView {
                offer_id: Some(offer_id),
                gain_minor: Some(gain_minor),
                loss_minor: Some(loss_minor),
                win_probability_micro: Some(win_probability_micro),
                premium_minor: Some(premium_minor),
                ..base
            },
            StimulusParams::AnchorProbe { anchor_minor, .. } => StimulusView {
                anchor_minor: Some(anchor_minor),
                ..base
            },
            StimulusParams::ForecastElicitation { .. } => base,
            StimulusParams::SunkCostPair { project_id, .. } => StimulusView {
                project_id: Some(project_id),
                ..base
            },
            StimulusParams::CrisisCountdown {
                ticks_remaining, ..
            } => StimulusView {
                ticks_remaining: Some(ticks_remaining),
                ..base
            },
            StimulusParams::DispositionWindow {
                position_id,
                unrealised_minor,
            } => StimulusView {
                position_id: Some(position_id),
                // The player can already compute this from the entry price and
                // the published index, so withholding it would be theatre.
                unrealised_minor: Some(unrealised_minor),
                ..base
            },
        }
    }
}

// ---------------------------------------------------------------------------
// Attribution.
// ---------------------------------------------------------------------------

/// Which planted probe a decision answered.
///
/// Attribution is by intent *shape*, not by recency: a probe and its response
/// are paired by the kind of act the response is. Priority matters only for the
/// crisis window, which is a moderator rather than a prompt and therefore only
/// claims decisions no specific probe wanted.
///
/// A decision that answers nothing is recorded with `stimulus: None`. That is a
/// real and common case — most turns are ordinary business — and inventing an
/// attribution for it would pollute every lane at once.
#[must_use]
pub fn attribute(intent: ActionIntent, active: &[PlantedStimulus]) -> Option<StimulusRef> {
    let specific = active.iter().find(|p| match (intent, p.params) {
        (
            ActionIntent::AcceptOffer { offer_id } | ActionIntent::DeclineOffer { offer_id },
            StimulusParams::GamblePair {
                offer_id: planted, ..
            },
        ) => offer_id == planted,
        (
            ActionIntent::ForecastInterval { .. },
            StimulusParams::AnchorProbe { .. } | StimulusParams::ForecastElicitation { .. },
        ) => true,
        (
            ActionIntent::ContinueProject { project_id }
            | ActionIntent::AbandonProject { project_id }
            | ActionIntent::Invest { project_id, .. },
            StimulusParams::SunkCostPair {
                project_id: planted,
                ..
            },
        ) => project_id == planted,
        (
            ActionIntent::ClosePosition { position_id },
            StimulusParams::DispositionWindow {
                position_id: planted,
                ..
            },
        ) => position_id == planted,
        _ => false,
    });
    if let Some(found) = specific {
        return Some(found.reference());
    }
    active
        .iter()
        .find(|p| matches!(p.params, StimulusParams::CrisisCountdown { .. }))
        .map(PlantedStimulus::reference)
}

// ---------------------------------------------------------------------------
// Parameter drawing.
// ---------------------------------------------------------------------------

/// One draw stream per turn. Counter-based addressing means the parameters for
/// turn N do not depend on how many draws turns 0..N happened to make, so a
/// change to one probe's arithmetic cannot shift every later probe (a variant
/// of trap BXS-W-05).
fn stream(campaign_key: [u32; 2], turn: u32) -> PhiloxStream {
    PhiloxStream::new(campaign_key, SeedDomain::Stimuli, u64::from(turn))
}

/// Scale a magnitude into `[lo, hi]` from one uniform draw.
fn span(u: f64, lo: i64, hi: i64) -> i64 {
    let width = hi.saturating_sub(lo).max(0);
    let scaled = (u * width as f64) as i64;
    lo.saturating_add(scaled.clamp(0, width))
}

/// The known (G, L, p) lattice. Frozen: the estimator's grid search assumes
/// these magnitudes, and shifting them without re-calibrating silently rescales
/// every historical λ.
const GAMBLE_GAIN_RANGE: (i64, i64) = (40_000, 160_000);
const GAMBLE_LOSS_RANGE: (i64, i64) = (30_000, 140_000);
const GAMBLE_PROBABILITY_LATTICE: [i64; 5] = [250_000, 400_000, 500_000, 600_000, 750_000];
const ANCHOR_DELTA_LATTICE: [i64; 3] = [150_000, 300_000, 450_000];
const SUNK_RANGE: (i64, i64) = (80_000, 400_000);
const CRISIS_CASH_RANGE: (i64, i64) = (100_000, 600_000);

fn pick<const N: usize>(u: f64, lattice: [i64; N]) -> i64 {
    let index = ((u * N as f64) as usize).min(N.saturating_sub(1));
    lattice.into_iter().nth(index).unwrap_or(0)
}

/// Draw the gamble for `turn`. The offer id is filled in by the caller once the
/// offer has a slot.
pub(super) fn draw_gamble(campaign_key: [u32; 2], turn: u32) -> StimulusParams {
    let mut s = stream(campaign_key, turn);
    let gain_minor = span(s.next_unit_f64(), GAMBLE_GAIN_RANGE.0, GAMBLE_GAIN_RANGE.1);
    let loss_minor = span(s.next_unit_f64(), GAMBLE_LOSS_RANGE.0, GAMBLE_LOSS_RANGE.1);
    let win_probability_micro = pick(s.next_unit_f64(), GAMBLE_PROBABILITY_LATTICE);
    // The premium is the risk-neutral value of the loss leg, so a λ of exactly
    // 1 is indifferent and the accept/decline split identifies λ around 1
    // rather than around an arbitrary offset.
    let premium_minor = i64::try_from(
        i128::from(loss_minor)
            .saturating_mul(i128::from(1_000_000 - win_probability_micro))
            / 1_000_000,
    )
    .unwrap_or(loss_minor);
    let good_state = s.next_unit_f64() * 1_000_000.0 < win_probability_micro as f64;
    StimulusParams::GamblePair {
        offer_id: u32::MAX,
        gain_minor,
        loss_minor,
        win_probability_micro,
        premium_minor,
        good_state,
    }
}

/// Build the anchor by perturbing the sealed reference.
///
/// `delta` is never zero: a zero perturbation would publish the oracle's own
/// number verbatim, which is the exact shape of a wall W-a breach.
pub(super) fn draw_anchor(
    campaign_key: [u32; 2],
    turn: u32,
    arm: AnchorArm,
    reference_minor: i64,
) -> StimulusParams {
    let mut s = stream(campaign_key, turn);
    let delta_micro = pick(s.next_unit_f64(), ANCHOR_DELTA_LATTICE);
    let signed = match arm {
        AnchorArm::High => 1_000_000_i128.saturating_add(i128::from(delta_micro)),
        AnchorArm::Low => 1_000_000_i128.saturating_sub(i128::from(delta_micro)),
    };
    let anchor_minor =
        i64::try_from(i128::from(reference_minor).saturating_mul(signed) / 1_000_000)
            .unwrap_or(reference_minor);
    StimulusParams::AnchorProbe {
        anchor_minor,
        arm,
        reference_minor,
        delta_micro,
    }
}

pub(super) fn draw_sunk(campaign_key: [u32; 2], turn: u32, arm: SunkArm) -> (i64, StimulusParams) {
    let mut s = stream(campaign_key, turn);
    let magnitude = span(s.next_unit_f64(), SUNK_RANGE.0, SUNK_RANGE.1);
    let sunk_minor = match arm {
        SunkArm::WithSunk => magnitude,
        SunkArm::WithoutSunk => 0,
    };
    (
        magnitude,
        StimulusParams::SunkCostPair {
            project_id: u32::MAX,
            sunk_minor,
            arm,
        },
    )
}

pub(super) fn draw_crisis(
    campaign_key: [u32; 2],
    turn: u32,
    ticks_remaining: u32,
) -> StimulusParams {
    let mut s = stream(campaign_key, turn);
    StimulusParams::CrisisCountdown {
        ticks_remaining,
        required_cash_minor: span(s.next_unit_f64(), CRISIS_CASH_RANGE.0, CRISIS_CASH_RANGE.1),
    }
}

/// The offer that carries a gamble into the game: a store opening that pays off
/// in the good state and costs in the bad one (SPEC §9.1「新規出店オファー」).
///
/// Always `Expansion`. An `Insurance` vehicle would invert the sign convention
/// — accepting would be the *safe* branch rather than the risky one — and the λ
/// grid in lane 0 reads the accept/decline split with a fixed sign.
pub(super) fn gamble_offer(params: StimulusParams) -> Option<Offer> {
    match params {
        StimulusParams::GamblePair { premium_minor, .. } => Some(Offer {
            kind: OfferKind::Expansion,
            // The compiler refuses non-positive amounts, and a free lottery is
            // not a choice worth measuring anyway.
            cost_minor: premium_minor.max(1),
            status: OfferStatus::Open,
        }),
        _ => None,
    }
}

/// Fill in the id the game assigned once the entity has a slot. Planting is a
/// two-step act — draw the terms, then bind them to a slot — and the digest is
/// only taken after the binding, so a reference always points at parameters
/// that name the real entity.
pub(super) fn bind_offer_id(params: StimulusParams, id: u32) -> StimulusParams {
    match params {
        StimulusParams::GamblePair {
            gain_minor,
            loss_minor,
            win_probability_micro,
            premium_minor,
            good_state,
            ..
        } => StimulusParams::GamblePair {
            offer_id: id,
            gain_minor,
            loss_minor,
            win_probability_micro,
            premium_minor,
            good_state,
        },
        other => other,
    }
}

pub(super) fn bind_project_id(params: StimulusParams, id: u32) -> StimulusParams {
    match params {
        StimulusParams::SunkCostPair {
            sunk_minor, arm, ..
        } => StimulusParams::SunkCostPair {
            project_id: id,
            sunk_minor,
            arm,
        },
        other => other,
    }
}

/// Pick the position the disposition window watches: the lowest-id open one.
pub(super) fn disposition_target(
    firm: &FirmState,
    equity_index_centi: i64,
) -> Option<StimulusParams> {
    let (position_id, position) = firm.open_positions().next()?;
    let unrealised_minor = position_pnl_minor(&position, equity_index_centi).ok()?;
    Some(StimulusParams::DispositionWindow {
        position_id,
        unrealised_minor,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::blackbox_sim::director::CAMPAIGN_TICKS;

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

    fn key() -> [u32; 2] {
        crate::blackbox_sim::rng::campaign_key_from_seed([3, 1, 4, 1, 5, 9, 2, 6])
    }

    #[test]
    fn the_cadence_is_independent_of_everything_but_the_turn() {
        // Wall W-b in test form: the schedule takes no state, so it cannot
        // adapt. If this ever needs a second argument, stop and re-read §2.
        for turn in 1..=CAMPAIGN_TICKS {
            assert_eq!(is_gamble_turn(turn), turn % 4 == 1);
            assert_eq!(is_sunk_cost_turn(turn), turn % 6 == 3);
        }
    }

    #[test]
    fn a_campaign_yields_the_sample_sizes_the_lanes_need() {
        let turns = || 1..=CAMPAIGN_TICKS;
        let gambles = turns().filter(|t| is_gamble_turn(*t)).count();
        let anchored = turns().filter(|t| is_anchored_forecast_turn(*t)).count();
        let controls = turns().filter(|t| is_control_forecast_turn(*t)).count();
        let sunk = turns().filter(|t| is_sunk_cost_turn(*t)).count();
        // Lane 0 needs >= 12 choices, lane 3 >= 8 forecasts, lane 2 >= 6 pairs,
        // lane 4 >= 4 pairs. These are the per-campaign yields; the confidence
        // gates are evaluated per profile, which pools campaigns.
        assert!(gambles >= 12, "lane 0 yield: {gambles}");
        assert!(anchored + controls >= 8, "lane 3 yield: {anchored}+{controls}");
        assert!(anchored / 2 >= 6, "lane 2 pairs: {}", anchored / 2);
        assert!(sunk / 2 >= 4, "lane 4 pairs: {}", sunk / 2);
    }

    #[test]
    fn the_quarter_control_displaces_exactly_one_anchored_turn() {
        let collisions = (1..=CAMPAIGN_TICKS)
            .filter(|t| t % FORECAST_PERIOD == FORECAST_PHASE && is_control_forecast_turn(*t))
            .count();
        assert_eq!(collisions, 1, "the mod-4/mod-13 overlap is structural");
        assert!(
            !is_anchored_forecast_turn(26),
            "the control must win the collision"
        );
    }

    #[test]
    fn anchor_arms_alternate_and_balance() {
        let arms: Vec<AnchorArm> = (1..=CAMPAIGN_TICKS).filter_map(anchor_arm).collect();
        let high = arms.iter().filter(|a| **a == AnchorArm::High).count();
        let low = arms.len() - high;
        assert_eq!(high, low, "unbalanced arms bias the anchoring estimate");
        for pair in arms.windows(2) {
            assert_ne!(pair.first(), pair.last(), "arms must strictly alternate");
        }
    }

    #[test]
    fn a_displaced_turn_does_not_desync_the_alternation() {
        // Turn 26 is skipped; the arm at 30 must continue the surviving
        // sequence, not the naive mod-4 one.
        let before = anchor_arm(22).or_die();
        let after = anchor_arm(30).or_die();
        assert_ne!(before, after, "alternation continues across the gap");
    }

    #[test]
    fn crisis_windows_open_and_close() {
        assert_eq!(crisis_ticks_remaining(19), None);
        assert_eq!(crisis_ticks_remaining(20), Some(3));
        assert_eq!(crisis_ticks_remaining(22), Some(1));
        assert_eq!(crisis_ticks_remaining(23), None);
        assert_eq!(crisis_ticks_remaining(44), Some(3));
    }

    #[test]
    fn parameters_are_a_pure_function_of_key_and_turn() {
        for turn in (1..=CAMPAIGN_TICKS).filter(|t| is_gamble_turn(*t)) {
            assert_eq!(draw_gamble(key(), turn), draw_gamble(key(), turn));
        }
    }

    #[test]
    fn different_campaigns_draw_different_gambles() {
        let other = crate::blackbox_sim::rng::campaign_key_from_seed([9, 9, 9, 9, 9, 9, 9, 9]);
        let differ = (1..=CAMPAIGN_TICKS)
            .filter(|t| is_gamble_turn(*t))
            .filter(|t| draw_gamble(key(), *t) != draw_gamble(other, *t))
            .count();
        assert!(differ > 8, "campaigns must not share an answer key");
    }

    #[test]
    fn gambles_land_inside_the_declared_lattice() {
        for turn in (1..=CAMPAIGN_TICKS).filter(|t| is_gamble_turn(*t)) {
            match draw_gamble(key(), turn) {
                StimulusParams::GamblePair {
                    gain_minor,
                    loss_minor,
                    win_probability_micro,
                    premium_minor,
                    ..
                } => {
                    assert!((GAMBLE_GAIN_RANGE.0..=GAMBLE_GAIN_RANGE.1).contains(&gain_minor));
                    assert!((GAMBLE_LOSS_RANGE.0..=GAMBLE_LOSS_RANGE.1).contains(&loss_minor));
                    assert!(GAMBLE_PROBABILITY_LATTICE.contains(&win_probability_micro));
                    assert!(premium_minor >= 0 && premium_minor <= loss_minor);
                }
                other => unreachable!("expected a gamble, got {other:?}"),
            }
        }
    }

    #[test]
    fn an_anchor_is_never_the_reference_itself() {
        for turn in (1..=CAMPAIGN_TICKS).filter(|t| is_anchored_forecast_turn(*t)) {
            let arm = anchor_arm(turn).or_die();
            match draw_anchor(key(), turn, arm, 1_000_000) {
                StimulusParams::AnchorProbe {
                    anchor_minor,
                    reference_minor,
                    delta_micro,
                    arm: got,
                } => {
                    assert_eq!(got, arm);
                    assert!(delta_micro > 0, "a zero delta publishes the oracle");
                    assert_ne!(anchor_minor, reference_minor);
                    match arm {
                        AnchorArm::High => assert!(anchor_minor > reference_minor),
                        AnchorArm::Low => assert!(anchor_minor < reference_minor),
                    }
                }
                other => unreachable!("expected an anchor, got {other:?}"),
            }
        }
    }

    #[test]
    fn publication_drops_every_sealed_field() {
        let mut ledger = StimulusLedger::new();
        let planted = ledger
            .plant(
                7,
                StimulusParams::AnchorProbe {
                    anchor_minor: 1_300_000,
                    arm: AnchorArm::High,
                    reference_minor: 1_000_000,
                    delta_micro: 300_000,
                },
            )
            .or_die();
        let view = StimulusView::publish(&planted);
        assert_eq!(view.anchor_minor, Some(1_300_000));
        // The reference and the perturbation have nowhere to live in the view
        // type at all, which is the guarantee. Serialising proves no serde
        // attribute smuggles them through. Assert on key names rather than on
        // rendered numbers: `1300000` legitimately contains `300000`, so a
        // substring search would fail on a correct view.
        let json = serde_json::to_string(&view).or_die();
        for sealed in ["reference", "delta", "arm"] {
            assert!(!json.contains(sealed), "{sealed} leaked into {json}");
        }
    }

    #[test]
    fn a_gamble_view_hides_the_predetermined_outcome() {
        let mut ledger = StimulusLedger::new();
        let planted = ledger
            .plant(
                1,
                StimulusParams::GamblePair {
                    offer_id: 2,
                    gain_minor: 50_000,
                    loss_minor: 40_000,
                    win_probability_micro: 500_000,
                    premium_minor: 20_000,
                    good_state: true,
                },
            )
            .or_die();
        let view = StimulusView::publish(&planted);
        let json = serde_json::to_string(&view).or_die();
        assert!(!json.contains("good"), "outcome leaked: {json}");
        // The lottery's terms are public; only its resolution is not.
        assert_eq!(view.win_probability_micro, Some(500_000));
        assert_eq!(view.premium_minor, Some(20_000));
    }

    #[test]
    fn the_ledger_binds_pointer_to_payload() {
        let mut ledger = StimulusLedger::new();
        let a = ledger
            .plant(1, StimulusParams::ForecastElicitation { reference_minor: 5 })
            .or_die();
        let b = ledger
            .plant(2, StimulusParams::ForecastElicitation { reference_minor: 5 })
            .or_die();
        // Same parameters, different rows: the digest carries tick and seq, so
        // two identical probes are still distinguishable references.
        assert_ne!(a.params_digest, b.params_digest);
        assert!(ledger.verify().is_ok());
        assert_eq!(ledger.get(1).map(|r| r.tick), Some(2));
    }

    #[test]
    fn a_tampered_row_fails_its_binding() {
        let mut ledger = StimulusLedger::new();
        let _ = ledger
            .plant(
                1,
                StimulusParams::ForecastElicitation {
                    reference_minor: 1_000,
                },
            )
            .or_die();
        let mut tampered = ledger.clone();
        if let Some(row) = tampered.rows.first_mut() {
            row.params = StimulusParams::ForecastElicitation {
                reference_minor: 9_999,
            };
        }
        assert_eq!(
            tampered.verify(),
            Err(StimulusError::BindingBroken { seq: 0 }),
            "a swapped answer key must be detected, not tolerated"
        );
    }

    #[test]
    fn the_ledger_rejects_when_full_and_never_evicts() {
        let mut ledger = StimulusLedger::new();
        for i in 0..MAX_PLANTED_STIMULI {
            let tick = u32::try_from(i).or_die();
            ledger
                .plant(tick, StimulusParams::ForecastElicitation { reference_minor: 1 })
                .or_die();
        }
        assert_eq!(
            ledger.plant(999, StimulusParams::ForecastElicitation { reference_minor: 1 }),
            Err(StimulusError::LedgerFull)
        );
        assert_eq!(ledger.len(), MAX_PLANTED_STIMULI);
        assert_eq!(ledger.rows().first().map(|r| r.tick), Some(0), "row 0 survived");
    }

    #[test]
    fn attribution_follows_the_shape_of_the_act() {
        let mut ledger = StimulusLedger::new();
        let gamble = ledger
            .plant(
                1,
                StimulusParams::GamblePair {
                    offer_id: 3,
                    gain_minor: 1,
                    loss_minor: 1,
                    win_probability_micro: 500_000,
                    premium_minor: 1,
                    good_state: false,
                },
            )
            .or_die();
        let active = [gamble];
        assert_eq!(
            attribute(ActionIntent::AcceptOffer { offer_id: 3 }, &active),
            Some(gamble.reference())
        );
        assert_eq!(
            attribute(ActionIntent::DeclineOffer { offer_id: 3 }, &active),
            Some(gamble.reference())
        );
        // A different offer is a different question.
        assert_eq!(
            attribute(ActionIntent::AcceptOffer { offer_id: 4 }, &active),
            None
        );
        // Ordinary business answers nothing.
        assert_eq!(
            attribute(
                ActionIntent::SetPrice {
                    sku: 0,
                    tick_price: 10
                },
                &active
            ),
            None
        );
    }

    #[test]
    fn a_crisis_claims_only_what_no_probe_wanted() {
        let mut ledger = StimulusLedger::new();
        let gamble = ledger
            .plant(
                1,
                StimulusParams::GamblePair {
                    offer_id: 3,
                    gain_minor: 1,
                    loss_minor: 1,
                    win_probability_micro: 500_000,
                    premium_minor: 1,
                    good_state: false,
                },
            )
            .or_die();
        let crisis = ledger
            .plant(
                1,
                StimulusParams::CrisisCountdown {
                    ticks_remaining: 2,
                    required_cash_minor: 100,
                },
            )
            .or_die();
        let active = [gamble, crisis];
        assert_eq!(
            attribute(ActionIntent::AcceptOffer { offer_id: 3 }, &active),
            Some(gamble.reference()),
            "the specific probe wins"
        );
        assert_eq!(
            attribute(
                ActionIntent::SetPrice {
                    sku: 0,
                    tick_price: 10
                },
                &active
            ),
            Some(crisis.reference()),
            "the moderator takes the remainder"
        );
    }
}
