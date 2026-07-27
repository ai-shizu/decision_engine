//! Bias axes, N/A semantics, and the calibration type-gate (SPEC §10–§12).
//!
//! Every estimate is micro-quantized (×1e6, floor-half-up — the §4.15
//! quantization). `value_micro = None` is the ONLY representation of "not
//! measured": fabricating 0 for missing data is lying (LAW-20 / BXS-I-09).
//!
//! The 6D projection surface is type-gated: `authoritative_projection`
//! requires a `CalibrationCertificate`, which has NO production constructor
//! until Phase 6 lands the verified known-answer path AND the Commander
//! adjudicates (SPEC §11 double gate). Until then the only reachable output
//! is `uncalibrated_projection()` — all N/A, mirroring
//! `analytics::tensor::authoritative_profile`'s FSA-05 posture (that module
//! is intentionally NOT imported — wall W-b keeps this crate-corner free of
//! profile dependencies).

use serde::{Deserialize, Serialize};

use super::money::{floor_half_up, MoneyError, MICRO};
use super::stimulus::{StimulusLedger, StimulusParams, SunkArm};
use super::telemetry::{ActionIntent, DecisionEvent, StimulusRef};

/// Lane numbers frozen forever (BXS-I-13, PKBTEN01 discipline):
/// never renumber, never reuse a retired lane. Additions append only.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[repr(u8)]
pub enum BiasAxis {
    LossAversion = 0,
    DispositionEffect = 1,
    Anchoring = 2,
    Overconfidence = 3,
    EscalationCommitment = 4,
    PressureDegradation = 5,
}

pub const N_AXES: usize = 6;

impl BiasAxis {
    pub const ALL: [BiasAxis; N_AXES] = [
        BiasAxis::LossAversion,
        BiasAxis::DispositionEffect,
        BiasAxis::Anchoring,
        BiasAxis::Overconfidence,
        BiasAxis::EscalationCommitment,
        BiasAxis::PressureDegradation,
    ];

    #[must_use]
    pub const fn lane(self) -> u8 {
        self as u8
    }

    #[inline]
    const fn index(self) -> usize {
        self as usize
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BiasError {
    SufficiencyOutOfRange { got_micro: i64 },
    MeasuredWithoutObservations,
    Quantization(MoneyError),
}

/// One micro-quantized estimate with its own evidence accounting.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BiasEstimate {
    /// None = not measured (the honest N/A). Never coerce to 0.
    pub value_micro: Option<i64>,
    pub n_obs: u32,
    /// Data sufficiency in [0, 1e6] — the §6.2-4 data_sufficiency discipline
    /// carried into this instrument.
    pub sufficiency_micro: i64,
}

impl BiasEstimate {
    pub const NOT_MEASURED: BiasEstimate = BiasEstimate {
        value_micro: None,
        n_obs: 0,
        sufficiency_micro: 0,
    };

    pub fn measured(
        value_micro: i64,
        n_obs: u32,
        sufficiency_micro: i64,
    ) -> Result<Self, BiasError> {
        if n_obs == 0 {
            return Err(BiasError::MeasuredWithoutObservations);
        }
        if !(0..=MICRO).contains(&sufficiency_micro) {
            return Err(BiasError::SufficiencyOutOfRange {
                got_micro: sufficiency_micro,
            });
        }
        Ok(Self {
            value_micro: Some(value_micro),
            n_obs,
            sufficiency_micro,
        })
    }
}

/// The shared confidence gate every estimator must pass through (SPEC §10):
/// below `min_n` observations the answer is NOT_MEASURED — never a fabricated
/// zero. At or above the gate, sufficiency = min(1, n / (2·min_n)) in micro
/// units (floor-half-up).
pub fn gate_estimate(value_micro: i64, n_obs: u32, min_n: u32) -> Result<BiasEstimate, BiasError> {
    if min_n == 0 {
        // A zero gate would let a single anecdote brand a human (LAW-21).
        return Err(BiasError::MeasuredWithoutObservations);
    }
    if n_obs < min_n {
        return Ok(BiasEstimate::NOT_MEASURED);
    }
    let num = i128::from(n_obs)
        .checked_mul(i128::from(MICRO))
        .ok_or(BiasError::Quantization(MoneyError::Overflow))?;
    let den = i128::from(min_n)
        .checked_mul(2)
        .ok_or(BiasError::Quantization(MoneyError::Overflow))?;
    let raw = floor_half_up(num, den).map_err(BiasError::Quantization)?;
    let sufficiency = raw.min(i128::from(MICRO));
    let sufficiency_micro =
        i64::try_from(sufficiency).map_err(|_| BiasError::Quantization(MoneyError::Overflow))?;
    BiasEstimate::measured(value_micro, n_obs, sufficiency_micro)
}

// ---------------------------------------------------------------------------
// Phase 4: trial extraction — pure log-mining, zero economics.
// ---------------------------------------------------------------------------
//
// Every `*Trial` below is the minimal, self-contained input one lane's
// estimator needs, extracted once from a played campaign's decision log and
// its answer key. Splitting extraction from estimation keeps the lane math
// testable against hand-built trials without a live `Session`, and it keeps
// this module fixture-blind by construction: a trial records only shapes a
// player already saw (or, for the sealed lottery/anchor truth, shapes that
// arrive from `StimulusLedger` — the same side channel `stimulus.rs` already
// grants sim → analysis, wall W-a's one permitted direction). Nothing here
// reads `phantom_bot`, and `phantom_bot` never reads this section (SPEC §12
// fixture-blindness) — the two sides are checked against each other only by
// the calibration suite, a third file that imports both.

fn planted(ledger: &StimulusLedger, reference: StimulusRef) -> Option<StimulusParams> {
    ledger.get(reference.stimulus_seq).map(|row| row.params)
}

/// Lane 0 input: one accept/decline answer to a published (gain, loss, p)
/// lottery. `good_state` is deliberately absent — the accept/decline choice
/// is made before it resolves, so it can play no part in *this* lane (it is
/// lane 0's own upstream RNG, not a signal about the chooser).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GambleTrial {
    pub win_probability_micro: i64,
    pub gain_minor: i64,
    pub loss_minor: i64,
    pub accepted: bool,
}

#[must_use]
pub fn extract_gamble_trials(events: &[DecisionEvent], stimuli: &StimulusLedger) -> Vec<GambleTrial> {
    let mut out = Vec::new();
    for event in events {
        let accepted = match event.action {
            ActionIntent::AcceptOffer { .. } => true,
            ActionIntent::DeclineOffer { .. } => false,
            _ => continue,
        };
        let reference = match event.stimulus {
            Some(r) => r,
            None => continue,
        };
        if let Some(StimulusParams::GamblePair {
            win_probability_micro,
            gain_minor,
            loss_minor,
            ..
        }) = planted(stimuli, reference)
        {
            out.push(GambleTrial {
                win_probability_micro,
                gain_minor,
                loss_minor,
                accepted,
            });
        }
    }
    out
}

/// Lane 1 input: one disposition-window opportunity and whether the player
/// realised it. A row with no matching event is a real, common observation
/// — the position was held — not a missing one; see `extract_..` below for
/// why matching is by `stimulus_seq` alone.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DispositionTrial {
    pub unrealised_minor: i64,
    pub realized: bool,
}

/// A ledger `seq` is assigned once, globally, at plant time
/// (`StimulusLedger::plant`), so matching a decision's `stimulus_seq` against
/// a row's `seq` alone — without also checking `kind` — cannot cross-match a
/// different probe. `attribute()` only ever binds a `ClosePosition` intent to
/// a `DispositionWindow` row in the first place (SPEC §9.2 shape rule), so a
/// match found here is guaranteed to BE the realisation this row asks about.
#[must_use]
pub fn extract_disposition_trials(
    events: &[DecisionEvent],
    stimuli: &StimulusLedger,
) -> Vec<DispositionTrial> {
    stimuli
        .rows()
        .iter()
        .filter_map(|row| match row.params {
            StimulusParams::DispositionWindow {
                unrealised_minor, ..
            } => {
                let realized = events
                    .iter()
                    .any(|e| e.stimulus.map(|r| r.stimulus_seq) == Some(row.seq));
                Some(DispositionTrial {
                    unrealised_minor,
                    realized,
                })
            }
            _ => None,
        })
        .collect()
}

/// Lane 2 input: one forecast against the (published) anchor it followed and
/// the (sealed) reference it should have tracked instead.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AnchorTrial {
    pub reference_minor: i64,
    pub anchor_minor: i64,
    pub midpoint_minor: i64,
}

#[must_use]
pub fn extract_anchor_trials(events: &[DecisionEvent], stimuli: &StimulusLedger) -> Vec<AnchorTrial> {
    let mut out = Vec::new();
    for event in events {
        let (lo_minor, hi_minor) = match event.action {
            ActionIntent::ForecastInterval { lo_minor, hi_minor } => (lo_minor, hi_minor),
            _ => continue,
        };
        let reference = match event.stimulus {
            Some(r) => r,
            None => continue,
        };
        if let Some(StimulusParams::AnchorProbe {
            anchor_minor,
            reference_minor,
            ..
        }) = planted(stimuli, reference)
        {
            // Both legs are non-negative and bounded by
            // `action::MAX_ACTION_AMOUNT_MINOR` (1e12), so the sum cannot
            // approach i64::MAX; plain integer division is exact enough for
            // a midpoint (unlike ledger money, this is not a settled amount).
            out.push(AnchorTrial {
                reference_minor,
                anchor_minor,
                midpoint_minor: lo_minor.saturating_add(hi_minor) / 2,
            });
        }
    }
    out
}

/// Lane 3 input: every forecast against the sealed reference it is scored
/// against, anchored or not — overconfidence is about calibration, not about
/// the anchoring manipulation, so both `AnchorProbe` and
/// `ForecastElicitation` rows contribute here.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ForecastTrial {
    pub reference_minor: i64,
    pub lo_minor: i64,
    pub hi_minor: i64,
}

#[must_use]
pub fn extract_forecast_trials(
    events: &[DecisionEvent],
    stimuli: &StimulusLedger,
) -> Vec<ForecastTrial> {
    let mut out = Vec::new();
    for event in events {
        let (lo_minor, hi_minor) = match event.action {
            ActionIntent::ForecastInterval { lo_minor, hi_minor } => (lo_minor, hi_minor),
            _ => continue,
        };
        let reference = match event.stimulus {
            Some(r) => r,
            None => continue,
        };
        let reference_minor = match planted(stimuli, reference) {
            Some(StimulusParams::AnchorProbe { reference_minor, .. })
            | Some(StimulusParams::ForecastElicitation { reference_minor }) => reference_minor,
            _ => continue,
        };
        out.push(ForecastTrial {
            reference_minor,
            lo_minor,
            hi_minor,
        });
    }
    out
}

/// Lane 4 input: one sunk-cost probe response, tagged with which arm it was.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SunkTrial {
    pub with_sunk: bool,
    pub continued: bool,
}

#[must_use]
pub fn extract_sunk_trials(events: &[DecisionEvent], stimuli: &StimulusLedger) -> Vec<SunkTrial> {
    let mut out = Vec::new();
    for event in events {
        let continued = match event.action {
            ActionIntent::ContinueProject { .. } | ActionIntent::Invest { .. } => true,
            ActionIntent::AbandonProject { .. } => false,
            _ => continue,
        };
        let reference = match event.stimulus {
            Some(r) => r,
            None => continue,
        };
        if let Some(StimulusParams::SunkCostPair { arm, .. }) = planted(stimuli, reference) {
            out.push(SunkTrial {
                with_sunk: matches!(arm, SunkArm::WithSunk),
                continued,
            });
        }
    }
    out
}

/// Lane 5 input. Built directly by `director.rs` at the moment a `SetPrice`
/// intent executes (`oracle::pricing_optimality_gap` + whether a
/// `CrisisCountdown` was active that turn) rather than mined from the
/// decision log after the fact: unlike the other five lanes, a pricing
/// decision is not tied to a fixed-cadence planted probe, so there is no
/// `StimulusRef` to recover it from later. See `Session::pricing_trials`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PricingTrial {
    pub under_pressure: bool,
    /// `optimal_profit_minor - actual_profit_minor`, always >= 0.
    pub gap_minor: i64,
    pub optimal_profit_minor: i64,
}

// ---------------------------------------------------------------------------
// Phase 4: lane estimators (SPEC §10). Every function is a pure, closed-form,
// integer-arithmetic reduction of trials to one `BiasEstimate` — no floats,
// no iterative solvers, no `unwrap`/`expect`, and never a fabricated 0 in
// place of an honest N/A (LAW-20). `min_n` is threaded in by the caller
// rather than hardcoded so the calibration suite can drive it independently
// of production defaults without touching this file.
// ---------------------------------------------------------------------------

pub const MIN_N_LOSS_AVERSION: u32 = 12;
pub const MIN_N_DISPOSITION_EFFECT: u32 = 10;
pub const MIN_N_ANCHORING: u32 = 12;
pub const MIN_N_OVERCONFIDENCE: u32 = 8;
pub const MIN_N_ESCALATION_COMMITMENT: u32 = 8;
pub const MIN_N_PRESSURE_DEGRADATION: u32 = 6;

/// Candidate loss-aversion coefficients (×1e6) the grid search scores against
/// the observed accept/decline pattern. Frozen like `stimulus.rs`'s draw
/// lattices: widening it changes every historical λ.
const LOSS_AVERSION_LATTICE_MICRO: [i64; 9] = [
    500_000, 750_000, 1_000_000, 1_250_000, 1_500_000, 1_750_000, 2_000_000, 2_500_000, 3_000_000,
];

/// Lane 0. For a candidate λ, a rational loss-averse agent accepts trial `i`
/// iff `p·gain >= (1-p)·loss·λ` (the premium is an asset-for-cash swap under
/// `action::compile`'s `Expansion` accounting, so it never enters the
/// player's expected-value comparison — only the lottery leg does). The
/// estimate is the λ on the fixed lattice that best reproduces the observed
/// accept/decline pattern (ties broken toward the smaller λ by scanning the
/// lattice in ascending order and requiring a STRICT improvement) — a grid
/// search rather than an iterative MLE, so the result is bit-identical across
/// platforms with no solver convergence to reason about.
pub fn loss_aversion(trials: &[GambleTrial], min_n: u32) -> Result<BiasEstimate, BiasError> {
    let n_obs = u32::try_from(trials.len()).unwrap_or(u32::MAX);
    let mut best_lambda = LOSS_AVERSION_LATTICE_MICRO[0];
    let mut best_score: i64 = -1;
    for &lambda in &LOSS_AVERSION_LATTICE_MICRO {
        let mut score: i64 = 0;
        for t in trials {
            // `p·gain >= (1-p)·loss·λ`, both sides scaled by 1e6 so `λ`'s own
            // micro scale clears without any division (no precision to lose,
            // and no possibility of a platform-dependent rounding mode).
            let lhs = i128::from(t.win_probability_micro)
                * i128::from(t.gain_minor)
                * i128::from(MICRO);
            let rhs = i128::from(MICRO - t.win_probability_micro)
                * i128::from(t.loss_minor)
                * i128::from(lambda);
            let predicted_accept = lhs >= rhs;
            if predicted_accept == t.accepted {
                score += 1;
            }
        }
        if score > best_score {
            best_score = score;
            best_lambda = lambda;
        }
    }
    gate_estimate(best_lambda, n_obs, min_n)
}

/// Lane 1. `realized_rate(gain) - realized_rate(loss)`, in micro units.
/// Positive = classic disposition effect (winners sold more readily than
/// losers). Requires at least one observation of EACH sign — a rate with an
/// empty denominator is not a smaller sample, it is not a rate — so this
/// guard is enforced here rather than deferred to the shared `min_n` gate,
/// which only knows how to count, not how to shape the count.
pub fn disposition_effect(
    trials: &[DispositionTrial],
    min_n: u32,
) -> Result<BiasEstimate, BiasError> {
    let (mut gain_total, mut gain_realized) = (0_u32, 0_u32);
    let (mut loss_total, mut loss_realized) = (0_u32, 0_u32);
    for t in trials {
        match t.unrealised_minor.cmp(&0) {
            std::cmp::Ordering::Greater => {
                gain_total = gain_total.saturating_add(1);
                if t.realized {
                    gain_realized = gain_realized.saturating_add(1);
                }
            }
            std::cmp::Ordering::Less => {
                loss_total = loss_total.saturating_add(1);
                if t.realized {
                    loss_realized = loss_realized.saturating_add(1);
                }
            }
            std::cmp::Ordering::Equal => {} // exactly flat: no directional signal
        }
    }
    if gain_total == 0 || loss_total == 0 {
        return Ok(BiasEstimate::NOT_MEASURED);
    }
    let gain_rate = floor_half_up(i128::from(gain_realized) * i128::from(MICRO), i128::from(gain_total))
        .map_err(BiasError::Quantization)?;
    let loss_rate = floor_half_up(i128::from(loss_realized) * i128::from(MICRO), i128::from(loss_total))
        .map_err(BiasError::Quantization)?;
    let value = i64::try_from(gain_rate - loss_rate)
        .map_err(|_| BiasError::Quantization(MoneyError::Overflow))?;
    gate_estimate(value, gain_total.saturating_add(loss_total), min_n)
}

/// Lane 2. Per trial, `(midpoint - reference) / (anchor - reference)` in
/// micro units: 0 = the forecast tracked the true reference regardless of the
/// anchor shown, 1e6 = the forecast landed exactly on the anchor. The lane
/// estimate is the mean of that ratio across every trial from either arm —
/// the High/Low sign difference is already absorbed by the ratio's own
/// denominator, so High and Low trials pool directly without a separate
/// arm-difference step. `anchor == reference` never occurs for a real
/// `AnchorProbe` row (`draw_anchor` guarantees a nonzero perturbation), but a
/// hand-built trial could set it, and dividing by zero fails closed rather
/// than panicking (BXS-I: Zero Panic extends past the ledger).
pub fn anchoring(trials: &[AnchorTrial], min_n: u32) -> Result<BiasEstimate, BiasError> {
    let mut sum_ratio_micro: i128 = 0;
    let mut counted: u32 = 0;
    for t in trials {
        let denom = i128::from(t.anchor_minor) - i128::from(t.reference_minor);
        if denom == 0 {
            continue;
        }
        let numer = (i128::from(t.midpoint_minor) - i128::from(t.reference_minor))
            .checked_mul(i128::from(MICRO))
            .ok_or(BiasError::Quantization(MoneyError::Overflow))?;
        // `floor_half_up` requires a positive denominator; flipping the sign
        // of both operands leaves the quotient unchanged (SPEC §4.15).
        let (numer, denom) = if denom < 0 { (-numer, -denom) } else { (numer, denom) };
        let ratio = floor_half_up(numer, denom).map_err(BiasError::Quantization)?;
        sum_ratio_micro = sum_ratio_micro
            .checked_add(ratio)
            .ok_or(BiasError::Quantization(MoneyError::Overflow))?;
        counted = counted.saturating_add(1);
    }
    if counted == 0 {
        return Ok(BiasEstimate::NOT_MEASURED);
    }
    let mean_ratio =
        floor_half_up(sum_ratio_micro, i128::from(counted)).map_err(BiasError::Quantization)?;
    let value = i64::try_from(mean_ratio).map_err(|_| BiasError::Quantization(MoneyError::Overflow))?;
    gate_estimate(value, counted, min_n)
}

/// Lane 3. The fraction of forecasts whose stated `[lo, hi]` interval MISSED
/// the true reference, in micro units. A player's own chosen width is their
/// only stated confidence signal here (there is no separate "confidence
/// level" field to compare against — SPEC §9.1's forecast intents carry only
/// the interval); a high miss rate against a self-chosen interval is
/// overconfidence by the textbook definition regardless of what width the
/// player thought was safe.
pub fn overconfidence(trials: &[ForecastTrial], min_n: u32) -> Result<BiasEstimate, BiasError> {
    let n_obs = u32::try_from(trials.len()).unwrap_or(u32::MAX);
    if n_obs == 0 {
        return Ok(BiasEstimate::NOT_MEASURED);
    }
    let miss = trials
        .iter()
        .filter(|t| t.reference_minor < t.lo_minor || t.reference_minor > t.hi_minor)
        .count();
    let miss = u32::try_from(miss).unwrap_or(u32::MAX);
    let value_i128 = floor_half_up(i128::from(miss) * i128::from(MICRO), i128::from(n_obs))
        .map_err(BiasError::Quantization)?;
    let value = i64::try_from(value_i128).map_err(|_| BiasError::Quantization(MoneyError::Overflow))?;
    gate_estimate(value, n_obs, min_n)
}

/// Lane 4. `continuation_rate(WithSunk) - continuation_rate(WithoutSunk)`, in
/// micro units. Positive = escalation of commitment / the sunk-cost fallacy:
/// continuing more often specifically BECAUSE capital is already committed,
/// holding the forward economics identical across arms. Same two-cell guard
/// as lane 1, for the same reason.
pub fn escalation_commitment(trials: &[SunkTrial], min_n: u32) -> Result<BiasEstimate, BiasError> {
    let (mut sunk_total, mut sunk_continued) = (0_u32, 0_u32);
    let (mut control_total, mut control_continued) = (0_u32, 0_u32);
    for t in trials {
        if t.with_sunk {
            sunk_total = sunk_total.saturating_add(1);
            if t.continued {
                sunk_continued = sunk_continued.saturating_add(1);
            }
        } else {
            control_total = control_total.saturating_add(1);
            if t.continued {
                control_continued = control_continued.saturating_add(1);
            }
        }
    }
    if sunk_total == 0 || control_total == 0 {
        return Ok(BiasEstimate::NOT_MEASURED);
    }
    let sunk_rate = floor_half_up(i128::from(sunk_continued) * i128::from(MICRO), i128::from(sunk_total))
        .map_err(BiasError::Quantization)?;
    let control_rate = floor_half_up(
        i128::from(control_continued) * i128::from(MICRO),
        i128::from(control_total),
    )
    .map_err(BiasError::Quantization)?;
    let value = i64::try_from(sunk_rate - control_rate)
        .map_err(|_| BiasError::Quantization(MoneyError::Overflow))?;
    gate_estimate(value, sunk_total.saturating_add(control_total), min_n)
}

/// Lane 5. `mean(relative_gap | under pressure) - mean(relative_gap | calm)`,
/// in micro units, where `relative_gap = gap_minor / optimal_profit_minor`.
/// Positive = decisions get worse specifically under the margin-call
/// countdown, holding the pricing problem's own difficulty (via the
/// normalisation) roughly comparable across ticks. Trials whose optimum earns
/// nothing (a SKU whose cost exceeds the demand curve's choke price) cannot
/// be normalised into a ratio and are excluded rather than div-by-zero'd or
/// coerced to an arbitrary sentinel.
pub fn pressure_degradation(trials: &[PricingTrial], min_n: u32) -> Result<BiasEstimate, BiasError> {
    let mut pressure_sum: i128 = 0;
    let mut pressure_n: u32 = 0;
    let mut calm_sum: i128 = 0;
    let mut calm_n: u32 = 0;
    for t in trials {
        if t.optimal_profit_minor <= 0 {
            continue;
        }
        let relative = floor_half_up(
            i128::from(t.gap_minor) * i128::from(MICRO),
            i128::from(t.optimal_profit_minor),
        )
        .map_err(BiasError::Quantization)?;
        if t.under_pressure {
            pressure_sum = pressure_sum
                .checked_add(relative)
                .ok_or(BiasError::Quantization(MoneyError::Overflow))?;
            pressure_n = pressure_n.saturating_add(1);
        } else {
            calm_sum = calm_sum
                .checked_add(relative)
                .ok_or(BiasError::Quantization(MoneyError::Overflow))?;
            calm_n = calm_n.saturating_add(1);
        }
    }
    if pressure_n == 0 || calm_n == 0 {
        return Ok(BiasEstimate::NOT_MEASURED);
    }
    let pressure_mean =
        floor_half_up(pressure_sum, i128::from(pressure_n)).map_err(BiasError::Quantization)?;
    let calm_mean = floor_half_up(calm_sum, i128::from(calm_n)).map_err(BiasError::Quantization)?;
    let value = i64::try_from(pressure_mean - calm_mean)
        .map_err(|_| BiasError::Quantization(MoneyError::Overflow))?;
    gate_estimate(value, pressure_n.saturating_add(calm_n), min_n)
}

/// One campaign's worth of estimator input. A borrowed view rather than an
/// owned copy: `estimate_profile` pools across campaigns purely by
/// concatenating trials, and campaigns can be large enough (a full
/// `CAMPAIGN_TICKS` run) that copying every event per call would be wasteful
/// for a suite that calls it once per calibration bot per lane.
#[derive(Debug, Clone, Copy)]
pub struct CampaignLog<'a> {
    pub events: &'a [DecisionEvent],
    pub stimuli: &'a StimulusLedger,
    pub pricing: &'a [PricingTrial],
}

/// Run all six lanes over one or more campaigns, pooling trials across every
/// campaign given (SPEC §12: some lanes, especially 1 and 5, do not clear a
/// single campaign's `min_n` on their own — see `docs/SPEC_BLACKBOX_SIMULATOR.md`
/// Phase 4 as-built). Each lane's own two-cell guards still apply after
/// pooling: pooling more campaigns that all skew the same direction does not
/// manufacture the missing arm.
pub fn estimate_profile(
    campaigns: &[CampaignLog<'_>],
    campaign_digest: [u8; 8],
) -> Result<BlackboxProfile, BiasError> {
    let mut gamble = Vec::new();
    let mut disposition = Vec::new();
    let mut anchor = Vec::new();
    let mut forecast = Vec::new();
    let mut sunk = Vec::new();
    let mut pricing = Vec::new();
    for c in campaigns {
        gamble.extend(extract_gamble_trials(c.events, c.stimuli));
        disposition.extend(extract_disposition_trials(c.events, c.stimuli));
        anchor.extend(extract_anchor_trials(c.events, c.stimuli));
        forecast.extend(extract_forecast_trials(c.events, c.stimuli));
        sunk.extend(extract_sunk_trials(c.events, c.stimuli));
        pricing.extend_from_slice(c.pricing);
    }
    let mut profile = BlackboxProfile::empty(campaign_digest);
    profile.set_axis(
        BiasAxis::LossAversion,
        loss_aversion(&gamble, MIN_N_LOSS_AVERSION)?,
    );
    profile.set_axis(
        BiasAxis::DispositionEffect,
        disposition_effect(&disposition, MIN_N_DISPOSITION_EFFECT)?,
    );
    profile.set_axis(BiasAxis::Anchoring, anchoring(&anchor, MIN_N_ANCHORING)?);
    profile.set_axis(
        BiasAxis::Overconfidence,
        overconfidence(&forecast, MIN_N_OVERCONFIDENCE)?,
    );
    profile.set_axis(
        BiasAxis::EscalationCommitment,
        escalation_commitment(&sunk, MIN_N_ESCALATION_COMMITMENT)?,
    );
    profile.set_axis(
        BiasAxis::PressureDegradation,
        pressure_degradation(&pricing, MIN_N_PRESSURE_DEGRADATION)?,
    );
    Ok(profile)
}

pub const SCHEMA_BLACKBOX_PROFILE_V1: &str = "blackbox_profile.v1";
pub const SCHEMA_BLACKBOX_PROJECTION_6D_V1: &str = "blackbox_projection.6d.v1";
pub const INSTRUMENT_ID: &str = "blackbox_sim";
pub const CALIBRATION_UNCALIBRATED: &str = "uncalibrated-instrument";

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BlackboxProfile {
    pub schema: &'static str,
    /// Truncated fingerprint of the CampaignGenesis this profile derives from.
    pub campaign_digest: [u8; 8],
    pub axes: [BiasEstimate; N_AXES],
}

impl BlackboxProfile {
    #[must_use]
    pub fn empty(campaign_digest: [u8; 8]) -> Self {
        Self {
            schema: SCHEMA_BLACKBOX_PROFILE_V1,
            campaign_digest,
            axes: [BiasEstimate::NOT_MEASURED; N_AXES],
        }
    }

    #[must_use]
    pub fn axis(&self, axis: BiasAxis) -> BiasEstimate {
        self.axes
            .get(axis.index())
            .copied()
            .unwrap_or(BiasEstimate::NOT_MEASURED)
    }

    pub fn set_axis(&mut self, axis: BiasAxis, estimate: BiasEstimate) {
        if let Some(slot) = self.axes.get_mut(axis.index()) {
            *slot = estimate;
        }
    }
}

/// Proof token that the known-answer calibration suite passed (SPEC §12).
/// Deliberately impossible to construct in production code in Phase 0–5:
/// the only constructor is `#[cfg(test)]`. Phase 6 introduces the verified
/// production path AFTER calibration is GREEN and the Commander adjudicates.
#[derive(Debug)]
pub struct CalibrationCertificate {
    _sealed: (),
}

impl CalibrationCertificate {
    #[cfg(test)]
    pub(crate) fn test_only() -> Self {
        Self { _sealed: () }
    }
}

/// Projection onto the interview 6D dimension order (SPEC §11). This is a
/// SEPARATE instrument surface — provenance-tagged, never merged silently
/// into `tensor_profile.6d.v1` (construct purity, §4.59).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Projection6D {
    pub schema: &'static str,
    pub instrument: &'static str,
    /// Canonical interview-dimension order (problem_structuring,
    /// quantitative_rigor, hypothesis_evidence, synthesis_judgment,
    /// communication, collaboration_adaptability). Entries with no
    /// deterministic observer stay None forever from this instrument.
    pub scores_micro: [Option<i64>; 6],
    pub calibration: &'static str,
}

/// The only projection reachable without a certificate: all N/A.
#[must_use]
pub fn uncalibrated_projection() -> Projection6D {
    Projection6D {
        schema: SCHEMA_BLACKBOX_PROJECTION_6D_V1,
        instrument: INSTRUMENT_ID,
        scores_micro: [None; 6],
        calibration: CALIBRATION_UNCALIBRATED,
    }
}

/// Certificate-gated projection. Phase 6 defines the projection weights AFTER
/// calibration data exists (LAW-19: freeze structure, not speculative
/// constants) — until then even the gated path yields the all-N/A surface.
#[must_use]
pub fn authoritative_projection(
    _profile: &BlackboxProfile,
    _certificate: &CalibrationCertificate,
) -> Projection6D {
    uncalibrated_projection()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lane_numbers_frozen() {
        assert_eq!(BiasAxis::LossAversion.lane(), 0);
        assert_eq!(BiasAxis::DispositionEffect.lane(), 1);
        assert_eq!(BiasAxis::Anchoring.lane(), 2);
        assert_eq!(BiasAxis::Overconfidence.lane(), 3);
        assert_eq!(BiasAxis::EscalationCommitment.lane(), 4);
        assert_eq!(BiasAxis::PressureDegradation.lane(), 5);
        assert_eq!(BiasAxis::ALL.len(), N_AXES);
    }

    #[test]
    fn insufficient_is_none_not_zero() {
        let e = match gate_estimate(123_456, 5, 12) {
            Ok(v) => v,
            Err(err) => unreachable!("gate failed: {err:?}"),
        };
        assert_eq!(e, BiasEstimate::NOT_MEASURED);
        assert_eq!(e.value_micro, None, "missing data must stay N/A (LAW-20)");
    }

    #[test]
    fn gate_at_min_measures_with_half_sufficiency() {
        let e = match gate_estimate(-250_000, 12, 12) {
            Ok(v) => v,
            Err(err) => unreachable!("gate failed: {err:?}"),
        };
        assert_eq!(e.value_micro, Some(-250_000));
        assert_eq!(e.n_obs, 12);
        assert_eq!(e.sufficiency_micro, 500_000); // 12/(2·12) = 0.5
        let full = match gate_estimate(1, 24, 12) {
            Ok(v) => v,
            Err(err) => unreachable!("gate failed: {err:?}"),
        };
        assert_eq!(full.sufficiency_micro, MICRO); // capped at 1.0
    }

    #[test]
    fn zero_min_gate_is_rejected() {
        assert!(matches!(
            gate_estimate(0, 100, 0),
            Err(BiasError::MeasuredWithoutObservations)
        ));
    }

    #[test]
    fn measured_validates_ranges() {
        assert!(matches!(
            BiasEstimate::measured(0, 0, 0),
            Err(BiasError::MeasuredWithoutObservations)
        ));
        assert!(matches!(
            BiasEstimate::measured(0, 1, MICRO + 1),
            Err(BiasError::SufficiencyOutOfRange { .. })
        ));
        assert!(matches!(
            BiasEstimate::measured(0, 1, -1),
            Err(BiasError::SufficiencyOutOfRange { .. })
        ));
    }

    #[test]
    fn empty_profile_is_all_not_measured() {
        let p = BlackboxProfile::empty([9; 8]);
        assert_eq!(p.schema, SCHEMA_BLACKBOX_PROFILE_V1);
        for axis in BiasAxis::ALL {
            assert_eq!(p.axis(axis), BiasEstimate::NOT_MEASURED);
        }
    }

    #[test]
    fn uncalibrated_projection_is_all_na() {
        let proj = uncalibrated_projection();
        assert_eq!(proj.schema, SCHEMA_BLACKBOX_PROJECTION_6D_V1);
        assert_eq!(proj.instrument, INSTRUMENT_ID);
        assert_eq!(proj.calibration, CALIBRATION_UNCALIBRATED);
        assert!(proj.scores_micro.iter().all(Option::is_none));
    }

    // -----------------------------------------------------------------
    // Phase 4 lane estimators.
    // -----------------------------------------------------------------

    /// A rational, loss-averse EV-maximiser with a fixed λ: accepts trial
    /// `i` iff `p·gain >= (1-p)·loss·λ`. Used only to build known-answer
    /// trials for THIS module's own tests — the real calibration BOT lives
    /// in `phantom_bot.rs` and must never call this (fixture-blindness).
    fn rational_accept(win_probability_micro: i64, gain_minor: i64, loss_minor: i64, lambda_micro: i64) -> bool {
        let lhs = i128::from(win_probability_micro) * i128::from(gain_minor) * i128::from(MICRO);
        let rhs =
            i128::from(MICRO - win_probability_micro) * i128::from(loss_minor) * i128::from(lambda_micro);
        lhs >= rhs
    }

    #[test]
    fn loss_aversion_recovers_a_known_lambda_from_a_clean_split() {
        let true_lambda = 1_500_000_i64;
        let draws: [(i64, i64, i64); 14] = [
            (250_000, 160_000, 40_000),
            (250_000, 40_000, 140_000),
            (400_000, 120_000, 60_000),
            (400_000, 50_000, 130_000),
            (500_000, 100_000, 100_000),
            (500_000, 60_000, 120_000),
            (600_000, 90_000, 80_000),
            (600_000, 45_000, 135_000),
            (750_000, 80_000, 90_000),
            (750_000, 55_000, 110_000),
            (250_000, 150_000, 45_000),
            (400_000, 130_000, 55_000),
            (600_000, 100_000, 70_000),
            (750_000, 70_000, 100_000),
        ];
        let trials: Vec<GambleTrial> = draws
            .iter()
            .map(|&(p, gain, loss)| GambleTrial {
                win_probability_micro: p,
                gain_minor: gain,
                loss_minor: loss,
                accepted: rational_accept(p, gain, loss, true_lambda),
            })
            .collect();
        let e = match loss_aversion(&trials, MIN_N_LOSS_AVERSION) {
            Ok(v) => v,
            Err(err) => unreachable!("loss_aversion failed: {err:?}"),
        };
        assert_eq!(e.n_obs, 14);
        assert_eq!(
            e.value_micro,
            Some(true_lambda),
            "grid search must recover the exact lattice point that generated the data"
        );
    }

    #[test]
    fn loss_aversion_below_min_n_is_not_measured() {
        let trials = vec![
            GambleTrial {
                win_probability_micro: 500_000,
                gain_minor: 100_000,
                loss_minor: 100_000,
                accepted: true,
            };
            3
        ];
        let e = match loss_aversion(&trials, MIN_N_LOSS_AVERSION) {
            Ok(v) => v,
            Err(err) => unreachable!("loss_aversion failed: {err:?}"),
        };
        assert_eq!(e, BiasEstimate::NOT_MEASURED);
    }

    #[test]
    fn disposition_effect_is_positive_when_winners_are_sold_more_often() {
        let mut trials = Vec::new();
        for _ in 0..8 {
            trials.push(DispositionTrial {
                unrealised_minor: 50_000,
                realized: true,
            });
        }
        for _ in 0..2 {
            trials.push(DispositionTrial {
                unrealised_minor: 50_000,
                realized: false,
            });
        }
        for _ in 0..2 {
            trials.push(DispositionTrial {
                unrealised_minor: -50_000,
                realized: true,
            });
        }
        for _ in 0..8 {
            trials.push(DispositionTrial {
                unrealised_minor: -50_000,
                realized: false,
            });
        }
        let e = match disposition_effect(&trials, MIN_N_DISPOSITION_EFFECT) {
            Ok(v) => v,
            Err(err) => unreachable!("disposition_effect failed: {err:?}"),
        };
        assert_eq!(e.value_micro, Some(600_000), "80% realized gains - 20% realized losses");
        assert_eq!(e.n_obs, 20);
    }

    #[test]
    fn disposition_effect_needs_both_arms_represented() {
        let trials = vec![
            DispositionTrial {
                unrealised_minor: 50_000,
                realized: true,
            };
            20
        ];
        let e = match disposition_effect(&trials, MIN_N_DISPOSITION_EFFECT) {
            Ok(v) => v,
            Err(err) => unreachable!("disposition_effect failed: {err:?}"),
        };
        assert_eq!(
            e,
            BiasEstimate::NOT_MEASURED,
            "20 observations is plenty, but they are all gains — no loss arm to compare"
        );
    }

    #[test]
    fn anchoring_index_is_one_when_forecasts_land_on_the_anchor() {
        let trials = vec![
            AnchorTrial {
                reference_minor: 1_000_000,
                anchor_minor: 1_300_000,
                midpoint_minor: 1_300_000,
            },
            AnchorTrial {
                reference_minor: 1_000_000,
                anchor_minor: 700_000,
                midpoint_minor: 700_000,
            },
        ];
        let trials: Vec<AnchorTrial> = trials.into_iter().cycle().take(12).collect();
        let e = match anchoring(&trials, MIN_N_ANCHORING) {
            Ok(v) => v,
            Err(err) => unreachable!("anchoring failed: {err:?}"),
        };
        assert_eq!(e.value_micro, Some(MICRO), "fully anchored: midpoint == anchor");
    }

    #[test]
    fn anchoring_index_is_zero_when_forecasts_ignore_the_anchor() {
        let trials: Vec<AnchorTrial> = [
            (1_000_000, 1_300_000),
            (1_000_000, 700_000),
        ]
        .into_iter()
        .cycle()
        .take(12)
        .map(|(reference_minor, anchor_minor)| AnchorTrial {
            reference_minor,
            anchor_minor,
            midpoint_minor: reference_minor,
        })
        .collect();
        let e = match anchoring(&trials, MIN_N_ANCHORING) {
            Ok(v) => v,
            Err(err) => unreachable!("anchoring failed: {err:?}"),
        };
        assert_eq!(e.value_micro, Some(0), "unanchored: midpoint == the true reference");
    }

    #[test]
    fn overconfidence_is_the_true_miss_rate() {
        let mut trials = Vec::new();
        for _ in 0..4 {
            trials.push(ForecastTrial {
                reference_minor: 1_000_000,
                lo_minor: 900_000,
                hi_minor: 1_100_000,
            });
        }
        for _ in 0..4 {
            trials.push(ForecastTrial {
                reference_minor: 2_000_000,
                lo_minor: 900_000,
                hi_minor: 1_100_000,
            });
        }
        let e = match overconfidence(&trials, MIN_N_OVERCONFIDENCE) {
            Ok(v) => v,
            Err(err) => unreachable!("overconfidence failed: {err:?}"),
        };
        assert_eq!(e.value_micro, Some(500_000), "half of the intervals missed");
        assert_eq!(e.n_obs, 8);
    }

    #[test]
    fn escalation_commitment_is_positive_when_sunk_capital_buys_continuation() {
        let mut trials = Vec::new();
        for _ in 0..6 {
            trials.push(SunkTrial {
                with_sunk: true,
                continued: true,
            });
        }
        for _ in 0..2 {
            trials.push(SunkTrial {
                with_sunk: true,
                continued: false,
            });
        }
        for _ in 0..2 {
            trials.push(SunkTrial {
                with_sunk: false,
                continued: true,
            });
        }
        for _ in 0..6 {
            trials.push(SunkTrial {
                with_sunk: false,
                continued: false,
            });
        }
        let e = match escalation_commitment(&trials, MIN_N_ESCALATION_COMMITMENT) {
            Ok(v) => v,
            Err(err) => unreachable!("escalation_commitment failed: {err:?}"),
        };
        assert_eq!(e.value_micro, Some(500_000), "75% - 25% continuation rate");
        assert_eq!(e.n_obs, 16);
    }

    #[test]
    fn pressure_degradation_is_positive_when_the_crisis_window_makes_pricing_worse() {
        let mut trials = Vec::new();
        for _ in 0..4 {
            trials.push(PricingTrial {
                under_pressure: true,
                gap_minor: 40_000,
                optimal_profit_minor: 100_000,
            });
        }
        for _ in 0..4 {
            trials.push(PricingTrial {
                under_pressure: false,
                gap_minor: 5_000,
                optimal_profit_minor: 100_000,
            });
        }
        let e = match pressure_degradation(&trials, MIN_N_PRESSURE_DEGRADATION) {
            Ok(v) => v,
            Err(err) => unreachable!("pressure_degradation failed: {err:?}"),
        };
        assert_eq!(e.value_micro, Some(350_000), "40% relative gap under pressure vs 5% calm");
        assert_eq!(e.n_obs, 8);
    }

    #[test]
    fn pressure_degradation_excludes_unnormalisable_trials() {
        let trials = vec![
            PricingTrial {
                under_pressure: true,
                gap_minor: 0,
                optimal_profit_minor: 0,
            };
            20
        ];
        let e = match pressure_degradation(&trials, MIN_N_PRESSURE_DEGRADATION) {
            Ok(v) => v,
            Err(err) => unreachable!("pressure_degradation failed: {err:?}"),
        };
        assert_eq!(
            e,
            BiasEstimate::NOT_MEASURED,
            "every trial is unnormalisable, so nothing survives to gate"
        );
    }

    #[test]
    fn estimate_profile_pools_trials_across_campaigns() {
        use crate::blackbox_sim::stimulus::AnchorArm;

        fn campaign_with_forecasts(n: usize) -> (Vec<DecisionEvent>, StimulusLedger) {
            let mut stimuli = StimulusLedger::new();
            let mut events = Vec::new();
            for i in 0..n {
                let tick = u32::try_from(i).unwrap_or(0);
                let row = match stimuli.plant(
                    tick,
                    StimulusParams::AnchorProbe {
                        anchor_minor: 1_300_000,
                        arm: AnchorArm::High,
                        reference_minor: 1_000_000,
                        delta_micro: 300_000,
                    },
                ) {
                    Ok(r) => r,
                    Err(err) => unreachable!("plant failed: {err:?}"),
                };
                events.push(DecisionEvent {
                    seq: tick.into(),
                    tick,
                    phase: crate::blackbox_sim::fsm::TurnPhase::Decide,
                    action: ActionIntent::ForecastInterval {
                        lo_minor: 1_200_000,
                        hi_minor: 1_400_000,
                    },
                    stimulus: Some(row.reference()),
                    latency_ms: None,
                    forced_default: false,
                    state_digest: [0; 8],
                });
            }
            (events, stimuli)
        }

        let (events_a, stimuli_a) = campaign_with_forecasts(6);
        let (events_b, stimuli_b) = campaign_with_forecasts(6);
        let single = CampaignLog {
            events: &events_a,
            stimuli: &stimuli_a,
            pricing: &[],
        };
        let pooled = [
            single,
            CampaignLog {
                events: &events_b,
                stimuli: &stimuli_b,
                pricing: &[],
            },
        ];
        let solo_profile = match estimate_profile(&[single], [0; 8]) {
            Ok(p) => p,
            Err(err) => unreachable!("estimate_profile failed: {err:?}"),
        };
        assert_eq!(
            solo_profile.axis(BiasAxis::Anchoring),
            BiasEstimate::NOT_MEASURED,
            "6 trials alone must not clear MIN_N_ANCHORING"
        );
        let pooled_profile = match estimate_profile(&pooled, [0; 8]) {
            Ok(p) => p,
            Err(err) => unreachable!("estimate_profile failed: {err:?}"),
        };
        let axis = pooled_profile.axis(BiasAxis::Anchoring);
        assert_eq!(axis.n_obs, 12, "pooling two campaigns must sum their trials");
        assert_eq!(axis.value_micro, Some(MICRO));
    }

    #[test]
    fn certificate_gated_projection_stays_na_in_phase0() {
        // Even holding a (test-only) certificate, Phase 0 has no projection
        // weights: the surface must remain all-N/A until Phase 6 (SPEC §11).
        let cert = CalibrationCertificate::test_only();
        let mut profile = BlackboxProfile::empty([1; 8]);
        let measured = match gate_estimate(300_000, 20, 10) {
            Ok(v) => v,
            Err(err) => unreachable!("gate failed: {err:?}"),
        };
        profile.set_axis(BiasAxis::LossAversion, measured);
        let proj = authoritative_projection(&profile, &cert);
        assert!(proj.scores_micro.iter().all(Option::is_none));
    }
}
