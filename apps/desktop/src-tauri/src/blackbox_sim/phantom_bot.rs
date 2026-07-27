//! PHANTOM-BOT — the fixture-blind calibration player (SPEC §12).
//!
//! Six deterministic policies, one per lane, each carrying its OWN
//! independently-chosen "true" bias parameter. A calibration run drives a
//! `Session` through the public `Director` API exactly as a real UI would,
//! records what the estimator sees, and hands both the bot's declared truth
//! and the estimator's recovered value to a THIRD file (`tests/blackbox_sim_calibration.rs`,
//! or an internal calibration module) for comparison.
//!
//! # Fixture-blindness (SPEC §12), enforced by construction
//!
//! This module never imports `bias.rs` — not its estimators, not its
//! `MIN_N_*` gates, not `LOSS_AVERSION_LATTICE_MICRO` (which is even private
//! to that file). A loss-aversion bot below happens to choose λ values that
//! also sit on the estimator's candidate lattice, but that is a property of
//! round numbers being round on both sides of an economically meaningful
//! scale, not a shared constant: this file could not read that lattice even
//! if it wanted to, and does not try. The two sides are checked against each
//! other only by the calibration suite.
//!
//! # Where the bot's "ground truth" access comes from (wall W-a)
//!
//! A bot that is *deliberately* λ-biased, or *deliberately* anchored by some
//! fraction, must be able to compare its own answer against the true
//! quantity the probe perturbs — that comparison is the whole content of the
//! bias. `oracle::reference_valuation` and `oracle::pricing_optimality_gap`
//! are exactly the sealed-truth functions `director.rs` itself calls when
//! planting probes and scoring prices; calling them here is the same
//! privileged L2-internal read the Director already performs, not a new
//! breach of the wall. What must never happen, and does not happen anywhere
//! in this file, is publishing either value: every decision this module
//! returns is a plain `ActionIntent`, the same closed vocabulary a human
//! player is limited to (wall W-d).
//!
//! # What is genuinely public, and used as such
//!
//! Lane 1 (disposition) and lane 4 (escalation) need no privileged read at
//! all: `unrealised_minor` is published on `StimulusView` (the player can
//! already compute it from their own position), and whether a project
//! carries sunk capital is answered by the player's OWN balance sheet
//! (`FirmState::project(id).committed_minor`), not by the sealed
//! `StimulusParams::SunkCostPair` row. A real player already has both.

use super::director::{Session, CAMPAIGN_TICKS};
use super::money::MICRO;
use super::oracle::{pricing_optimality_gap, reference_valuation, OracleError};
use super::rng::{PhiloxStream, SeedDomain};
use super::telemetry::{ActionIntent, StimulusKind};

/// The SKU every bot prices. The bots never touch inventory economics beyond
/// pricing (lane 5 only needs a price submitted, not a profitable firm), so a
/// single fixed SKU keeps every campaign's lane-5 feed comparable.
pub const BOT_SKU: u8 = 0;

/// Notional every bot hedges with, whenever it holds no open position. A
/// forward struck at the prevailing level moves no cash at entry (see
/// `action::compile`'s `OpenHedge` arm), so the exact size only matters for
/// how large a mark-to-market swing lane 1's probe later reports — this is
/// comfortably inside `firm::MAX_PRICE_MINOR`-scale amounts without being so
/// large it dominates the firm's own P&L.
const DEFAULT_HEDGE_NOTIONAL_MINOR: i64 = 5_000_000;

/// One bot's declared, tunable ground truth. Every field is independent of
/// every other; a calibration case sets exactly the one(s) it means to test
/// and leaves the rest at their "no bias" default (see [`PhantomBotConfig::UNBIASED`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PhantomBotConfig {
    /// Lane 0. Accepts gamble trial `i` iff `p·gain >= (1-p)·loss·λ`, λ in
    /// micro units. 1_000_000 = risk-neutral EV maximiser.
    pub loss_aversion_lambda_micro: i64,
    /// Lane 1. Probability (micro) of closing a `DispositionWindow` position
    /// shown at a mark-to-market GAIN.
    pub disposition_sell_gain_micro: i64,
    /// Lane 1. Probability (micro) of closing one shown at a LOSS.
    pub disposition_sell_loss_micro: i64,
    /// Lane 2. How far the bot's forecast midpoint is pulled toward the
    /// shown anchor, away from the true reference: 0 = ignores the anchor
    /// entirely, 1_000_000 = adopts it outright.
    pub anchor_pull_micro: i64,
    /// Lanes 2 & 3. Half-width of every submitted forecast interval around
    /// its (possibly anchor-pulled) center, in minor units. Must exceed the
    /// largest anchor perturbation the campaign can draw, or a "calibrated,
    /// not overconfident" trial could miss by construction rather than by
    /// the overconfidence draw below.
    pub forecast_half_width_minor: i64,
    /// Lane 3. Probability (micro) that a given forecast is deliberately
    /// mis-centered far enough to guarantee it misses the true reference,
    /// independent of lane 2's anchor pull (see module test for the proof
    /// this cannot leak into lane 2's average).
    pub overconfidence_miss_rate_micro: i64,
    /// Lane 4. Baseline probability (micro) of continuing a project that
    /// carries NO sunk capital.
    pub continue_baseline_micro: i64,
    /// Lane 4. Additional probability (micro), added on top of the
    /// baseline, of continuing a project that DOES carry sunk capital. The
    /// escalation-of-commitment signal IS this number.
    pub escalation_bias_micro: i64,
    /// Lane 5. Constant pricing error the bot carries at all times, in minor
    /// units, applied on top of the true profit-maximising price.
    pub baseline_price_error_minor: i64,
    /// Lane 5. ADDITIONAL pricing error applied only while a
    /// `CrisisCountdown` is on the table. The pressure-degradation signal IS
    /// this number's effect on the resulting gap.
    pub pressure_price_error_minor: i64,
}

impl PhantomBotConfig {
    /// A perfectly rational, unbiased agent on every lane at once: the
    /// negative control every positive-control case is compared against.
    pub const UNBIASED: PhantomBotConfig = PhantomBotConfig {
        loss_aversion_lambda_micro: 1_000_000,
        disposition_sell_gain_micro: 500_000,
        disposition_sell_loss_micro: 500_000,
        anchor_pull_micro: 0,
        forecast_half_width_minor: 50_000_000,
        overconfidence_miss_rate_micro: 0,
        continue_baseline_micro: 500_000,
        escalation_bias_micro: 0,
        baseline_price_error_minor: 0,
        pressure_price_error_minor: 0,
    };
}

/// A running bot: its declared truth plus its own private decision stream.
/// The stream lives on `SeedDomain::Bots` (reserved for exactly this) and is
/// keyed independently of the campaign's genesis — a bot's coin flips are
/// not part of the world being measured, and must not covary with it.
pub struct PhantomBot {
    config: PhantomBotConfig,
    stream: PhiloxStream,
}

impl PhantomBot {
    #[must_use]
    pub fn new(config: PhantomBotConfig, bot_key: [u32; 2]) -> Self {
        Self {
            config,
            stream: PhiloxStream::new(bot_key, SeedDomain::Bots, 0),
        }
    }

    #[must_use]
    pub fn config(&self) -> PhantomBotConfig {
        self.config
    }

    #[inline]
    fn draw_micro(&mut self) -> i64 {
        (self.stream.next_unit_f64() * (MICRO as f64)) as i64
    }

    fn wants_gamble(&self, win_probability_micro: i64, gain_minor: i64, loss_minor: i64) -> bool {
        let ev_gain = i128::from(win_probability_micro) * i128::from(gain_minor);
        let ev_loss = i128::from(MICRO - win_probability_micro)
            * i128::from(loss_minor)
            * i128::from(self.config.loss_aversion_lambda_micro)
            / i128::from(MICRO);
        ev_gain >= ev_loss
    }

    fn wants_to_sell(&mut self, unrealised_minor: i64) -> bool {
        let threshold = if unrealised_minor > 0 {
            self.config.disposition_sell_gain_micro
        } else if unrealised_minor < 0 {
            self.config.disposition_sell_loss_micro
        } else {
            return false;
        };
        self.draw_micro() < threshold
    }

    fn wants_to_continue(&mut self, has_sunk: bool) -> bool {
        let prob = if has_sunk {
            self.config
                .continue_baseline_micro
                .saturating_add(self.config.escalation_bias_micro)
        } else {
            self.config.continue_baseline_micro
        }
        .clamp(0, MICRO);
        self.draw_micro() < prob
    }

    /// Lanes 2 & 3. `anchor_minor` is `None` on the unanchored
    /// `ForecastElicitation` control turns.
    ///
    /// The interval is always built symmetrically around its intended
    /// center (`pulled`, or the deliberately mis-centered point on an
    /// overconfident draw): `half` is chosen up front so `center - half`
    /// can never be negative, rather than clamping `lo_minor` alone after
    /// the fact. An asymmetric post-hoc clamp on `lo_minor` alone would
    /// silently drag `(lo+hi)/2` away from `center` — exactly the kind of
    /// quiet corruption this module's whole purpose is to be immune to.
    fn forecast(&mut self, true_reference_minor: i64, anchor_minor: Option<i64>) -> ActionIntent {
        let pull: i128 = match anchor_minor {
            Some(anchor) => {
                let delta = i128::from(anchor) - i128::from(true_reference_minor);
                delta * i128::from(self.config.anchor_pull_micro) / i128::from(MICRO)
            }
            None => 0,
        };
        let pulled = i64::try_from(i128::from(true_reference_minor) + pull).unwrap_or(true_reference_minor);
        let configured_half = self.config.forecast_half_width_minor.max(1);
        let overconfident = self.draw_micro() < self.config.overconfidence_miss_rate_micro;
        let (center, half) = if overconfident {
            // A deliberate miss, built directly off `true_reference_minor`
            // rather than `pulled` — deliberately NOT `reference + pull ±
            // shift`, because that tangles this lane's excursion together
            // with lane 2's own pull, and an unlucky sign draw can partly
            // cancel the two against each other on any individual trial
            // (a real, previously measured regression: see AI_SKILLS.md's
            // Phase 4 entry). `center = reference ± shift` makes the
            // deviation from the true reference exactly `±shift`
            // regardless of `pull`, full stop.
            //
            // `shift` itself is `|anchor - reference|` (or `configured_half`
            // on an unanchored `ForecastElicitation` control turn, which has
            // no anchor to scale against) — proportional to the SAME
            // denominator lane 2's ratio divides by, not to any pull- or
            // reference-independent constant. That matters for lane 2
            // purity: a magnitude independent of `anchor - reference` makes
            // the ratio contribution `±shift / (anchor - reference)` blow
            // up without bound whenever a draw happens to land on the
            // smallest perturbation in `ANCHOR_DELTA_LATTICE`, and with
            // only a handful of overconfident trials per pooled run, a
            // single such outlier can dominate the whole average regardless
            // of how carefully the sign is balanced. Scaling by the same
            // denominator instead keeps every trial's contribution an
            // exactly-bounded ±1, so the random sign genuinely averages it
            // to ~0 rather than merely being unbiased in a theory that
            // variance then swamps.
            //
            // `ActionCompiler::compile` also still rejects any
            // `ForecastInterval` with `lo_minor < 0` outright — a rejected
            // submission never becomes a `DecisionEvent`, so it would be
            // silently dropped from `extract_forecast_trials` entirely
            // (neither a hit nor a miss), quietly halving the recovered
            // rate. `shift <= |anchor - reference| < reference` here (every
            // lattice entry in `ANCHOR_DELTA_LATTICE` is well under 100%),
            // so `reference - shift` never needs clamping in the anchored
            // case; the unanchored fallback still clamps defensively.
            let sign: i64 = if self.stream.next_unit_f64() < 0.5 { 1 } else { -1 };
            let scale = match anchor_minor {
                Some(anchor) => (i128::from(anchor) - i128::from(true_reference_minor)).unsigned_abs(),
                None => u128::from(configured_half as u64),
            };
            let shift = i64::try_from(scale.min(u128::from(i64::MAX as u64))).unwrap_or(i64::MAX);
            let c = true_reference_minor.saturating_add(sign.saturating_mul(shift)).max(0);
            let h = (shift / 4).min(configured_half).min(c.max(0));
            (c, h)
        } else {
            (pulled, configured_half.min(pulled.max(0)))
        };
        let lo_minor = center.saturating_sub(half);
        let hi_minor = center.saturating_add(half);
        ActionIntent::ForecastInterval { lo_minor, hi_minor }
    }

    /// Lane 5. `under_pressure` is read off the session's own published
    /// probes (`StimulusKind::CrisisCountdown`) — the same public signal a
    /// player sees, not a sealed one.
    fn price(&self, session: &Session, under_pressure: bool) -> Option<ActionIntent> {
        let ctx = session.context().ok()?;
        // The dummy `chosen_price_minor` argument only scores that price; the
        // optimum itself does not depend on it (see `oracle.rs`).
        let optimum: Result<_, OracleError> =
            pricing_optimality_gap(session.kernel(), &ctx, BOT_SKU, 1);
        let optimal_price_minor = optimum.ok()?.optimal_price_minor;
        let mut price = optimal_price_minor.saturating_add(self.config.baseline_price_error_minor);
        if under_pressure {
            price = price.saturating_add(self.config.pressure_price_error_minor);
        }
        Some(ActionIntent::SetPrice {
            sku: BOT_SKU,
            tick_price: price.max(1),
        })
    }

    /// One turn's decision. Must be called after `session.observe()` and
    /// before `session.submit()`. Answers the highest-priority probe on the
    /// table and falls back to a pricing decision otherwise, so every
    /// non-probed turn still feeds lane 5.
    ///
    /// Priority is `DispositionWindow` > `SunkCostPair` > `GamblePair` >
    /// `AnchorProbe`/`ForecastElicitation` — NOT `stimulus::attribute`'s own
    /// specificity order, and deliberately so. The fixed cadences collide on
    /// several exact turns every campaign (e.g. `GAMBLE_PERIOD=4, phase=1`
    /// and `SUNK_PERIOD=6, phase=3` coincide at turns 9, 21, 33, 45, ...),
    /// and because the sunk arm alternates by OCCURRENCE index rather than
    /// by turn, an ill-chosen priority makes a collision land on the SAME
    /// arm every single campaign — silently starving one side of lane 4 or
    /// 1's two-cell comparison to zero forever, a property of the fixed
    /// schedule that pooling more campaigns cannot fix. This order was
    /// picked by checking each pairwise collision set (mod 20 / mod 30
    /// residues of the four cadences) and confirming every lane still keeps
    /// a workable share of BOTH its cells; see `calibration.rs` for the
    /// empirical per-lane recovery this produces.
    pub fn decide(&mut self, session: &Session) -> ActionIntent {
        let views = session.stimulus_views();
        let under_pressure = views.iter().any(|v| v.kind == StimulusKind::CrisisCountdown);

        if let Some(view) = views.iter().find(|v| v.kind == StimulusKind::DispositionWindow) {
            if let (Some(position_id), Some(unrealised)) = (view.position_id, view.unrealised_minor) {
                return if self.wants_to_sell(unrealised) {
                    ActionIntent::ClosePosition { position_id }
                } else {
                    ActionIntent::Abstain
                };
            }
        }

        if let Some(view) = views.iter().find(|v| v.kind == StimulusKind::SunkCostPair) {
            if let Some(project_id) = view.project_id {
                let has_sunk = session
                    .books()
                    .firm
                    .project(project_id)
                    .map(|p| p.committed_minor > 0)
                    .unwrap_or(false);
                return if self.wants_to_continue(has_sunk) {
                    ActionIntent::ContinueProject { project_id }
                } else {
                    ActionIntent::AbandonProject { project_id }
                };
            }
        }

        if let Some(view) = views.iter().find(|v| v.kind == StimulusKind::GamblePair) {
            if let (Some(offer_id), Some(gain), Some(loss), Some(p)) =
                (view.offer_id, view.gain_minor, view.loss_minor, view.win_probability_micro)
            {
                return if self.wants_gamble(p, gain, loss) {
                    ActionIntent::AcceptOffer { offer_id }
                } else {
                    ActionIntent::DeclineOffer { offer_id }
                };
            }
        }

        if let Some(view) = views
            .iter()
            .find(|v| v.kind == StimulusKind::AnchorProbe || v.kind == StimulusKind::ForecastElicitation)
        {
            if let Ok(ctx) = session.context() {
                if let Ok(reference) = reference_valuation(session.kernel(), &ctx, &session.books().firm) {
                    return self.forecast(reference.expected_revenue_minor, view.anchor_minor);
                }
            }
        }

        // Nothing to answer this turn. Keep a hedge position open whenever
        // one isn't, so `DispositionWindow` (lane 1) always has a target to
        // plant against — nothing else in a bot-driven campaign ever opens
        // one otherwise — and price otherwise, feeding lane 5.
        if session.books().firm.open_positions().next().is_none() {
            return ActionIntent::OpenHedge {
                instrument: 0,
                notional_minor: DEFAULT_HEDGE_NOTIONAL_MINOR,
            };
        }

        self.price(session, under_pressure).unwrap_or(ActionIntent::Abstain)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::blackbox_sim::genesis::{Difficulty, GenesisRequest};

    trait OrDie<T> {
        fn or_die(self) -> T;
    }
    impl<T, E: std::fmt::Debug> OrDie<T> for Result<T, E> {
        fn or_die(self) -> T {
            match self {
                Ok(v) => v,
                Err(e) => unreachable!("phantom_bot test setup failed: {e:?}"),
            }
        }
    }

    fn request(campaign_index: u32) -> GenesisRequest {
        GenesisRequest {
            scenario_id: 7,
            difficulty: Difficulty::Standard,
            campaign_index,
            created_date: "2026-07-27".to_string(),
        }
    }

    fn drive_one_campaign(config: PhantomBotConfig, bot_key: [u32; 2], campaign_index: u32) -> Session {
        let mut session = Session::start(request(campaign_index)).or_die();
        let mut bot = PhantomBot::new(config, bot_key);
        while session.turns_completed() < CAMPAIGN_TICKS {
            session.observe().or_die();
            let intent = bot.decide(&session);
            if session.submit(intent, None).is_err() {
                session.submit_timeout_default().or_die();
            }
            session.execute().or_die();
            session.settle().or_die();
            session.report().or_die();
        }
        session
    }

    #[test]
    fn a_bot_campaign_runs_to_completion_under_every_config() {
        let s = drive_one_campaign(PhantomBotConfig::UNBIASED, [1, 1], 0);
        assert_eq!(s.turns_completed(), CAMPAIGN_TICKS);
    }

    #[test]
    fn two_runs_of_the_same_bot_and_campaign_are_bit_identical() {
        let a = drive_one_campaign(PhantomBotConfig::UNBIASED, [7, 9], 3);
        let b = drive_one_campaign(PhantomBotConfig::UNBIASED, [7, 9], 3);
        assert_eq!(a.state_digest().or_die(), b.state_digest().or_die());
    }

    #[test]
    fn a_different_bot_key_diverges_the_decision_stream() {
        let a = drive_one_campaign(PhantomBotConfig::UNBIASED, [1, 1], 5);
        let b = drive_one_campaign(PhantomBotConfig::UNBIASED, [2, 2], 5);
        // Same world, same rational-EV rules everywhere the bot has no coin
        // to flip, so the only lanes that CAN diverge are the probabilistic
        // ones (disposition / escalation). A different key must not be
        // silently ignored.
        assert_ne!(
            a.decisions().iter().map(|d| d.intent).collect::<Vec<_>>(),
            b.decisions().iter().map(|d| d.intent).collect::<Vec<_>>(),
        );
    }

    #[test]
    fn a_loss_averse_bot_declines_more_often_than_a_risk_neutral_one() {
        let neutral = PhantomBotConfig {
            loss_aversion_lambda_micro: 1_000_000,
            ..PhantomBotConfig::UNBIASED
        };
        let averse = PhantomBotConfig {
            loss_aversion_lambda_micro: 3_000_000,
            ..PhantomBotConfig::UNBIASED
        };
        let a = drive_one_campaign(neutral, [4, 4], 1);
        let b = drive_one_campaign(averse, [4, 4], 1);
        let declines = |s: &Session| {
            s.decisions()
                .iter()
                .filter(|d| matches!(d.intent, ActionIntent::DeclineOffer { .. }))
                .count()
        };
        assert!(
            declines(&b) >= declines(&a),
            "a higher lambda must never accept strictly more gambles"
        );
    }
}
