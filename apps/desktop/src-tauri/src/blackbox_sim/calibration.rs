//! Phase 4 calibration suite — the known-answer verification device (SPEC §12).
//!
//! This module NEVER mints a [`super::bias::CalibrationCertificate`] into
//! anything reachable from production code; the certificate's only
//! constructor is `#[cfg(test)] pub(crate) fn test_only()`, and this module
//! is itself `#[cfg(test)]`-only, so a certificate minted here cannot escape
//! into `authoritative_projection`'s call sites (there are none outside this
//! file and `bias.rs`'s own tests). What this file verifies is recovery: for
//! each lane, a [`PhantomBot`](super::phantom_bot::PhantomBot) declares a
//! known bias parameter, plays one or more full campaigns, and
//! `bias::estimate_profile` must recover a value close to (lane 0: exactly)
//! what the bot declared. GREEN/RED is a plain test assertion — there is no
//! other output.
//!
//! Fixture-blindness (SPEC §12): this is the ONE file allowed to import both
//! `bias` and `phantom_bot`. Neither of those two files imports the other.

#[cfg(test)]
mod tests {
    use crate::blackbox_sim::bias::{
        self, BiasAxis, CampaignLog, MIN_N_ANCHORING, MIN_N_DISPOSITION_EFFECT,
        MIN_N_ESCALATION_COMMITMENT, MIN_N_LOSS_AVERSION, MIN_N_OVERCONFIDENCE,
        MIN_N_PRESSURE_DEGRADATION,
    };
    use crate::blackbox_sim::director::{Session, CAMPAIGN_TICKS};
    use crate::blackbox_sim::genesis::{Difficulty, GenesisRequest};
    use crate::blackbox_sim::money::MICRO;
    use crate::blackbox_sim::phantom_bot::{PhantomBot, PhantomBotConfig};

    trait OrDie<T> {
        fn or_die(self) -> T;
    }
    impl<T, E: std::fmt::Debug> OrDie<T> for Result<T, E> {
        fn or_die(self) -> T {
            match self {
                Ok(v) => v,
                Err(e) => unreachable!("calibration setup failed: {e:?}"),
            }
        }
    }
    impl<T> OrDie<T> for Option<T> {
        fn or_die(self) -> T {
            match self {
                Some(v) => v,
                None => unreachable!("calibration expected a measured value and found N/A"),
            }
        }
    }

    /// Pooled across several campaigns per SPEC §12: lanes 1 and 5
    /// particularly do not clear their `min_n` from a single campaign once a
    /// bot only ever answers a fraction of the probes it sees.
    const CAMPAIGNS_PER_BOT: u32 = 10;

    fn request(scenario_id: u32, campaign_index: u32) -> GenesisRequest {
        GenesisRequest {
            scenario_id,
            difficulty: Difficulty::Standard,
            campaign_index,
            created_date: "2026-07-27".to_string(),
        }
    }

    /// Play `CAMPAIGNS_PER_BOT` full campaigns under one bot config and hand
    /// back the sessions, kept alive for the caller to borrow event/stimulus
    /// slices from (mirrors `CampaignLog`'s own borrowed-view shape).
    fn play_campaigns(scenario_id: u32, config: PhantomBotConfig) -> Vec<Session> {
        (0..CAMPAIGNS_PER_BOT)
            .map(|campaign_index| {
                let mut session = Session::start(request(scenario_id, campaign_index)).or_die();
                let bot_key = [scenario_id, campaign_index];
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
            })
            .collect()
    }

    fn profile_of(sessions: &[Session]) -> bias::BlackboxProfile {
        let events: Vec<Vec<_>> = sessions.iter().map(|s| s.events().copied().collect()).collect();
        let pricing: Vec<Vec<_>> = sessions
            .iter()
            .map(|s| s.pricing_trials().copied().collect())
            .collect();
        let logs: Vec<CampaignLog<'_>> = sessions
            .iter()
            .zip(events.iter())
            .zip(pricing.iter())
            .map(|((s, ev), pr)| CampaignLog {
                events: ev,
                stimuli: s.stimuli(),
                pricing: pr,
            })
            .collect();
        bias::estimate_profile(&logs, [0; 8]).or_die()
    }

    fn value_micro(profile: &bias::BlackboxProfile, axis: BiasAxis) -> Option<i64> {
        profile.axis(axis).value_micro
    }

    fn n_obs(profile: &bias::BlackboxProfile, axis: BiasAxis) -> u32 {
        profile.axis(axis).n_obs
    }

    // -----------------------------------------------------------------
    // Lane 0 — Loss aversion. Exact recovery: the bot's decision rule and
    // the estimator's grid search are both the closed-form rational-EV
    // comparison, so a bot whose true λ sits on the lattice must win it
    // outright once enough trials are pooled.
    // -----------------------------------------------------------------

    fn loss_aversion_bot(lambda_micro: i64) -> PhantomBotConfig {
        PhantomBotConfig {
            loss_aversion_lambda_micro: lambda_micro,
            ..PhantomBotConfig::UNBIASED
        }
    }

    #[test]
    fn lane0_recovers_a_risk_neutral_bot_exactly() {
        let sessions = play_campaigns(101, loss_aversion_bot(1_000_000));
        let profile = profile_of(&sessions);
        assert!(n_obs(&profile, BiasAxis::LossAversion) >= MIN_N_LOSS_AVERSION);
        assert_eq!(value_micro(&profile, BiasAxis::LossAversion), Some(1_000_000));
    }

    #[test]
    fn lane0_recovers_a_loss_averse_bot_exactly() {
        let sessions = play_campaigns(102, loss_aversion_bot(2_000_000));
        let profile = profile_of(&sessions);
        assert!(n_obs(&profile, BiasAxis::LossAversion) >= MIN_N_LOSS_AVERSION);
        assert_eq!(value_micro(&profile, BiasAxis::LossAversion), Some(2_000_000));
    }

    // -----------------------------------------------------------------
    // Lane 1 — Disposition effect. Rate-based; recovered within a tolerance
    // band around the bot's declared sell-probability gap.
    // -----------------------------------------------------------------

    #[test]
    fn lane1_recovers_a_declared_disposition_gap() {
        let config = PhantomBotConfig {
            disposition_sell_gain_micro: 900_000,
            disposition_sell_loss_micro: 200_000,
            ..PhantomBotConfig::UNBIASED
        };
        let sessions = play_campaigns(111, config);
        let profile = profile_of(&sessions);
        assert!(n_obs(&profile, BiasAxis::DispositionEffect) >= MIN_N_DISPOSITION_EFFECT);
        let recovered = value_micro(&profile, BiasAxis::DispositionEffect).or_die();
        let expected = 700_000_i64; // 900_000 - 200_000
        assert!(
            (recovered - expected).abs() <= 150_000,
            "expected disposition gap near {expected}, got {recovered}"
        );
        assert!(recovered > 0, "sign must show winners sold more than losers");
    }

    #[test]
    fn lane1_shows_no_disposition_effect_for_the_unbiased_bot() {
        let sessions = play_campaigns(112, PhantomBotConfig::UNBIASED);
        let profile = profile_of(&sessions);
        assert!(n_obs(&profile, BiasAxis::DispositionEffect) >= MIN_N_DISPOSITION_EFFECT);
        let recovered = value_micro(&profile, BiasAxis::DispositionEffect).or_die();
        assert!(
            recovered.abs() <= 150_000,
            "an equal-probability seller must not read as strongly disposed either way, got {recovered}"
        );
    }

    // -----------------------------------------------------------------
    // Lane 2 — Anchoring. The bot's pull is applied against the SAME sealed
    // reference the estimator later pulls from `StimulusLedger`, so recovery
    // should be tight even with sampling noise from the deliberate-miss
    // draws lane 3 introduces on the same submissions.
    // -----------------------------------------------------------------

    fn anchor_bot(pull_micro: i64) -> PhantomBotConfig {
        PhantomBotConfig {
            anchor_pull_micro: pull_micro,
            forecast_half_width_minor: 50_000_000,
            overconfidence_miss_rate_micro: 0,
            ..PhantomBotConfig::UNBIASED
        }
    }

    #[test]
    fn lane2_recovers_zero_anchoring_for_an_unpulled_bot() {
        let sessions = play_campaigns(121, anchor_bot(0));
        let profile = profile_of(&sessions);
        assert!(n_obs(&profile, BiasAxis::Anchoring) >= MIN_N_ANCHORING);
        let recovered = value_micro(&profile, BiasAxis::Anchoring).or_die();
        assert!(recovered.abs() <= 20_000, "expected ~0 anchoring, got {recovered}");
    }

    #[test]
    fn lane2_recovers_full_anchoring_for_a_fully_pulled_bot() {
        let sessions = play_campaigns(122, anchor_bot(MICRO));
        let profile = profile_of(&sessions);
        assert!(n_obs(&profile, BiasAxis::Anchoring) >= MIN_N_ANCHORING);
        let recovered = value_micro(&profile, BiasAxis::Anchoring).or_die();
        assert!((recovered - MICRO).abs() <= 20_000, "expected ~1e6 anchoring, got {recovered}");
    }

    #[test]
    fn lane2_recovers_partial_anchoring_for_a_half_pulled_bot() {
        let sessions = play_campaigns(123, anchor_bot(500_000));
        let profile = profile_of(&sessions);
        assert!(n_obs(&profile, BiasAxis::Anchoring) >= MIN_N_ANCHORING);
        let recovered = value_micro(&profile, BiasAxis::Anchoring).or_die();
        assert!(
            (recovered - 500_000).abs() <= 40_000,
            "expected ~0.5e6 anchoring, got {recovered}"
        );
    }

    // -----------------------------------------------------------------
    // Lane 3 — Overconfidence. Independent of lane 2's pull by construction
    // (see `phantom_bot::forecast`'s sign-independent miss offset).
    // -----------------------------------------------------------------

    #[test]
    fn lane3_recovers_a_declared_miss_rate() {
        let config = PhantomBotConfig {
            anchor_pull_micro: 500_000,
            forecast_half_width_minor: 50_000_000,
            overconfidence_miss_rate_micro: 400_000,
            ..PhantomBotConfig::UNBIASED
        };
        let sessions = play_campaigns(131, config);
        let profile = profile_of(&sessions);
        assert!(n_obs(&profile, BiasAxis::Overconfidence) >= MIN_N_OVERCONFIDENCE);
        let recovered = value_micro(&profile, BiasAxis::Overconfidence).or_die();
        assert!(
            (recovered - 400_000).abs() <= 150_000,
            "expected ~0.4e6 miss rate, got {recovered}"
        );
        // Lane 2 must stay close to its own declared 0.5 pull despite lane
        // 3's independent miss draws sharing the same submissions.
        let anchoring = value_micro(&profile, BiasAxis::Anchoring).or_die();
        assert!(
            (anchoring - 500_000).abs() <= 60_000,
            "lane 3's miss draws leaked into lane 2's average: {anchoring}"
        );
    }

    #[test]
    fn lane3_recovers_near_zero_miss_rate_for_a_well_calibrated_bot() {
        let sessions = play_campaigns(132, anchor_bot(0));
        let profile = profile_of(&sessions);
        assert!(n_obs(&profile, BiasAxis::Overconfidence) >= MIN_N_OVERCONFIDENCE);
        let recovered = value_micro(&profile, BiasAxis::Overconfidence).or_die();
        assert!(recovered <= 100_000, "expected a low miss rate, got {recovered}");
    }

    // -----------------------------------------------------------------
    // Lane 4 — Escalation of commitment.
    // -----------------------------------------------------------------

    #[test]
    fn lane4_recovers_a_declared_escalation_bias() {
        let config = PhantomBotConfig {
            continue_baseline_micro: 300_000,
            escalation_bias_micro: 500_000,
            ..PhantomBotConfig::UNBIASED
        };
        let sessions = play_campaigns(141, config);
        let profile = profile_of(&sessions);
        assert!(n_obs(&profile, BiasAxis::EscalationCommitment) >= MIN_N_ESCALATION_COMMITMENT);
        let recovered = value_micro(&profile, BiasAxis::EscalationCommitment).or_die();
        assert!(
            (recovered - 500_000).abs() <= 200_000,
            "expected ~0.5e6 escalation, got {recovered}"
        );
        assert!(recovered > 0, "sign must show more continuation with sunk capital");
    }

    #[test]
    fn lane4_shows_no_escalation_for_the_unbiased_bot() {
        let sessions = play_campaigns(142, PhantomBotConfig::UNBIASED);
        let profile = profile_of(&sessions);
        assert!(n_obs(&profile, BiasAxis::EscalationCommitment) >= MIN_N_ESCALATION_COMMITMENT);
        let recovered = value_micro(&profile, BiasAxis::EscalationCommitment).or_die();
        assert!(
            recovered.abs() <= 200_000,
            "an equal-probability continuer must not read as escalating, got {recovered}"
        );
    }

    // -----------------------------------------------------------------
    // Lane 5 — Pressure degradation. Direction-of-effect recovery: the bot
    // never sees the true optimum's exact curvature, only its own declared
    // pricing errors, so the calibration target is sign + rough scale
    // ordering rather than an exact value.
    // -----------------------------------------------------------------

    #[test]
    fn lane5_recovers_positive_degradation_for_a_bot_that_panics_under_pressure() {
        let config = PhantomBotConfig {
            baseline_price_error_minor: 200,
            pressure_price_error_minor: 4_000,
            ..PhantomBotConfig::UNBIASED
        };
        let sessions = play_campaigns(151, config);
        let profile = profile_of(&sessions);
        assert!(n_obs(&profile, BiasAxis::PressureDegradation) >= MIN_N_PRESSURE_DEGRADATION);
        let recovered = value_micro(&profile, BiasAxis::PressureDegradation).or_die();
        assert!(recovered > 0, "a bot that panics under pressure must show positive degradation, got {recovered}");
    }

    #[test]
    fn lane5_shows_no_meaningful_degradation_for_a_bot_with_no_pressure_reaction() {
        let sessions = play_campaigns(152, PhantomBotConfig::UNBIASED);
        let profile = profile_of(&sessions);
        assert!(n_obs(&profile, BiasAxis::PressureDegradation) >= MIN_N_PRESSURE_DEGRADATION);
        let recovered = value_micro(&profile, BiasAxis::PressureDegradation).or_die();
        // The unconstrained-demand optimum still drifts tick to tick, so an
        // exact zero is not guaranteed; the gate is "far smaller than the
        // panicking bot's signal above", not literally zero.
        assert!(
            recovered.abs() <= 300_000,
            "expected small/no degradation without a pressure reaction, got {recovered}"
        );
    }

    // -----------------------------------------------------------------
    // The certificate stays sealed even from inside the passing suite.
    // -----------------------------------------------------------------

    #[test]
    fn a_passing_calibration_run_does_not_by_itself_mint_a_certificate() {
        // `CalibrationCertificate::test_only()` exists and IS reachable from
        // this file (both are `#[cfg(test)]`), which is exactly the
        // Commander's Option-A ruling: the type stays uninstantiable in
        // production while remaining usable to unit-test
        // `authoritative_projection`'s plumbing. What this test asserts is
        // narrower: nothing above this line constructed one, and running
        // every lane's recovery check above did not implicitly produce one
        // either — there is no code path from "estimate_profile succeeded"
        // to "a certificate exists".
        let cert = bias::CalibrationCertificate::test_only();
        let sessions = play_campaigns(161, PhantomBotConfig::UNBIASED);
        let profile = profile_of(&sessions);
        let projection = bias::authoritative_projection(&profile, &cert);
        // Even certificated, Phase 4 defines no weights yet (Phase 6 does):
        // the gated path is still all-N/A today (LAW-19).
        assert!(projection.scores_micro.iter().all(Option::is_none));
    }
}