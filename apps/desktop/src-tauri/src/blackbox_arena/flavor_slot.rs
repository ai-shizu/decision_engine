//! Ambient single-slot flavor delivery for the BLACKBOX arena (FLV-R-10 / F-3 T-4).
//!
//! Not piggy-backed on [`super::view::AdvanceView`] (A-4). Correlation is
//! matched on the Rust side only — FE never supplies tokens. Busy drops new
//! requests; there is no queue.

use crate::blackbox_sim::genesis::CampaignGenesis;
use crate::flavor::policy::FlavorPolicy;
use crate::flavor::request::{FlavorRequest, TemplateId};
use crate::flavor::scan;
use crate::flavor::verified::VerifiedFlavor;
use crate::llm::flavor_gen::{self, FlavorOutcome};

/// Session identity for a flavor candidate (FLV-I-13).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct FlavorCorrelation {
    pub genesis_fingerprint: [u8; 8],
    pub tick: u32,
    pub template_id: TemplateId,
}

/// Build a correlation from a live genesis (first consumer of `digest8`).
#[must_use]
pub(crate) fn correlation_from_genesis(
    genesis: &CampaignGenesis,
    tick: u32,
    template_id: TemplateId,
) -> FlavorCorrelation {
    FlavorCorrelation {
        genesis_fingerprint: genesis.digest8(),
        tick,
        template_id,
    }
}

/// Whether a begin_request was admitted or dropped because the slot is busy.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Admit {
    Started,
    DroppedBusy,
}

/// Single ambient slot: at most one in-flight generation and one held result.
#[derive(Debug, Default)]
pub(crate) struct FlavorAmbientSlot {
    held: Option<(FlavorCorrelation, VerifiedFlavor)>,
    busy: bool,
    in_flight: Option<FlavorCorrelation>,
    /// Set by [`Self::abort_campaign`]; next [`Self::finish`] discards.
    cancelled: bool,
    /// G-4 counters (Tier 3). Previously these existed only in the host-side
    /// `flavor_a1` harness, which emits with `println!` and never runs on a
    /// device — so G-4's stated criteria were unobservable on the only build
    /// that matters. These are the production equivalents.
    counters: FlavorGateCounters,
}

/// Running totals for the G-4 gate, measured on the production path.
#[derive(Debug, Default, Clone, Copy)]
pub(crate) struct FlavorGateCounters {
    pub attempts: u64,
    pub accepted: u64,
    pub discarded: u64,
    pub unavailable: u64,
    /// Accepted values that still carried a finding when re-scanned. This is a
    /// **belt**: `decide()` is supposed to make it structurally impossible, so
    /// any non-zero value means the guard did not hold on real hardware. An
    /// unaudited guard is indistinguishable from an absent one, and F-3 showed
    /// the model does emit numerals for the guard to catch.
    pub leaked: u64,
    /// Kicks refused because the slot was already in flight. Counted so that a
    /// slot stuck busy is visible: once stalled it silently drops every later
    /// kick, and without this the log is identical to "the arena never
    /// advanced".
    pub dropped_busy: u64,
}

impl FlavorAmbientSlot {
    #[must_use]
    pub(crate) fn new() -> Self {
        Self::default()
    }

    /// Queue length is always zero — busy drops; nothing is buffered.
    #[must_use]
    pub(crate) fn pending_queue_len(&self) -> usize {
        0
    }

    #[must_use]
    pub(crate) fn is_busy(&self) -> bool {
        self.busy
    }

    /// Begin a generation. If already busy, drop the new request immediately.
    pub(crate) fn begin_request(&mut self, corr: FlavorCorrelation) -> Admit {
        if self.is_busy() {
            // Invariant: nothing is buffered while busy (P-3-3).
            debug_assert_eq!(self.pending_queue_len(), 0);
            self.counters.dropped_busy += 1;
            Self::emit_one("flavor.dropped_busy", self.counters.dropped_busy);
            return Admit::DroppedBusy;
        }
        self.busy = true;
        self.in_flight = Some(corr);
        self.counters.attempts += 1;
        // Emitted at admit, not only from `finish`. A generation that is
        // admitted but never completes would otherwise leave no trace at all,
        // making "entered the path and stalled" indistinguishable from "never
        // entered the path" — an ambiguity that already cost one device
        // session, where an empty log was the only evidence available.
        Self::emit_one("flavor.attempts", self.counters.attempts);
        Admit::Started
    }

    /// Single numeric OSLog line. Reuses the `%{public}llu` path P0-6 proved
    /// survives Release; a no-op off iOS, where the host tests read `counters()`
    /// directly instead.
    fn emit_one(label: &str, value: u64) {
        #[cfg(target_os = "ios")]
        crate::ios_oslog::log_model_u64(label, value);
        #[cfg(not(target_os = "ios"))]
        let _ = (label, value);
    }

    /// G-4 evidence accessor (also the seam host tests assert on).
    #[must_use]
    pub(crate) fn counters(&self) -> FlavorGateCounters {
        self.counters
    }

    /// Complete the in-flight request. Latest Accepted overwrites `held`.
    /// After [`Self::abort_campaign`], the outcome is discarded.
    pub(crate) fn finish(&mut self, outcome: FlavorOutcome) {
        let Some(corr) = self.in_flight.take() else {
            self.busy = false;
            return;
        };
        self.busy = false;
        if self.cancelled {
            self.cancelled = false;
            return;
        }
        match outcome {
            FlavorOutcome::Accepted(flavor) => {
                self.counters.accepted += 1;
                // Belt: re-scan what the guard already accepted. `decide()` only
                // returns Accepted when verify passes, so this should always be
                // clean — which is exactly why it is worth measuring rather than
                // asserting. Mirrors the harness check in `flavor_a1`.
                let policy = FlavorPolicy::for_template(corr.template_id);
                if !scan::scan(flavor.as_str(), &policy).is_clean() {
                    self.counters.leaked += 1;
                }
                // Latest-wins overwrite (P-8).
                self.held = Some((corr, flavor));
            }
            FlavorOutcome::Discarded(findings) => {
                self.counters.discarded += 1;
                // Telemetry material for §14; keep the field live.
                let _finding_count = findings.len();
            }
            FlavorOutcome::Unavailable => {
                self.counters.unavailable += 1;
            }
        }
        self.emit_gate_counters();
    }

    /// Stamp the running totals so G-4 can be judged from a Release device log.
    /// Numeric OSLog only — the same `%{public}llu` path P0-6 proved survives a
    /// Release build, reused rather than reinvented.
    fn emit_gate_counters(&self) {
        #[cfg(target_os = "ios")]
        {
            use crate::ios_oslog::log_model_u64;
            log_model_u64("flavor.attempts", self.counters.attempts);
            log_model_u64("flavor.accepted", self.counters.accepted);
            log_model_u64("flavor.discarded", self.counters.discarded);
            log_model_u64("flavor.unavailable", self.counters.unavailable);
            log_model_u64("flavor.leaked", self.counters.leaked);
            log_model_u64("flavor.dropped_busy", self.counters.dropped_busy);
        }
    }

    /// Worker path only: run `generate` once, then [`Self::finish`].
    /// Must never be reachable from the turn path (`kick_ambient_flavor` / `advance`).
    pub(crate) fn deliver_completion(
        &mut self,
        request: &FlavorRequest,
        completion: Option<&str>,
    ) {
        if !self.busy {
            return;
        }
        let outcome = flavor_gen::generate(request, completion);
        self.finish(outcome);
    }

    /// Test-only sync helper (begin → generate → finish). The production turn
    /// path must not call this — 関所 G scans `kick_ambient_flavor` for it.
    #[cfg(test)]
    pub(crate) fn request_generate(
        &mut self,
        corr: FlavorCorrelation,
        request: &FlavorRequest,
        completion: Option<&str>,
    ) -> Admit {
        match self.begin_request(corr) {
            Admit::DroppedBusy => Admit::DroppedBusy,
            Admit::Started => {
                self.deliver_completion(request, completion);
                Admit::Started
            }
        }
    }

    /// Non-blocking pull with exact correlation match + take semantics.
    pub(crate) fn take(&mut self, current: &FlavorCorrelation) -> Option<VerifiedFlavor> {
        let (corr, flavor) = self.held.take()?;
        if &corr == current {
            Some(flavor)
        } else {
            // Mismatch: discard (do not put back). Design-impossible newer
            // ticks and stale/foreign campaigns all land here (P-2-2…5).
            None
        }
    }

    /// Campaign abort: in-flight results must not land; clear held prose.
    pub(crate) fn abort_campaign(&mut self) {
        self.cancelled = true;
        self.held = None;
    }
}

#[cfg(test)]
mod tests {
    //! Frozen populations P-2 / P-3 / P-8 (LAW-25).

    use super::*;
    use crate::flavor::request::{
        FlavorLocale, FlavorSchema, FlavorSlot, SlotId, SlotValue, TemplateId,
    };

    const F1: [u8; 8] = [1, 0, 0, 0, 0, 0, 0, 0];
    const F2: [u8; 8] = [2, 0, 0, 0, 0, 0, 0, 0];

    fn corr(fp: [u8; 8], tick: u32, tid: TemplateId) -> FlavorCorrelation {
        FlavorCorrelation {
            genesis_fingerprint: fp,
            tick,
            template_id: tid,
        }
    }

    fn current() -> FlavorCorrelation {
        corr(F1, 5, TemplateId::ArenaEventHeadline)
    }

    fn accepted_flavor() -> VerifiedFlavor {
        let raw = "あ".repeat(8);
        match flavor_gen::decide(&raw, TemplateId::ArenaEventHeadline) {
            FlavorOutcome::Accepted(v) => v,
            other => panic!("setup Accepted failed: {other:?}"),
        }
    }

    fn store(slot: &mut FlavorAmbientSlot, c: FlavorCorrelation) {
        assert_eq!(slot.begin_request(c), Admit::Started);
        slot.finish(FlavorOutcome::Accepted(accepted_flavor()));
    }

    /// G-4 counters are measured on the production path, so they must be
    /// verifiable where tests actually run. The device only *reports* these
    /// numbers; nothing on a device proves the arithmetic behind them, and an
    /// iOS-gated test would run nowhere at all.
    #[test]
    fn gate_counters_track_each_outcome_and_belt_stays_clean() {
        let mut slot = FlavorAmbientSlot::new();
        assert_eq!(slot.counters().attempts, 0);

        store(&mut slot, current());
        let c = slot.counters();
        assert_eq!((c.attempts, c.accepted), (1, 1));
        // The belt: `decide()` accepted it, so a re-scan must find nothing.
        assert_eq!(c.leaked, 0, "guard accepted a value that fails re-scan");

        assert_eq!(slot.begin_request(current()), Admit::Started);
        slot.finish(FlavorOutcome::Unavailable);
        assert_eq!(slot.counters().unavailable, 1);

        assert_eq!(slot.begin_request(current()), Admit::Started);
        slot.finish(FlavorOutcome::Discarded(Vec::new()));
        let c = slot.counters();
        assert_eq!((c.attempts, c.discarded, c.leaked), (3, 1, 0));
    }

    /// A dropped request is not an attempt: busy-drops never reach `generate`,
    /// so counting them would inflate `attempts` and make G-4's `attempts > 0`
    /// satisfiable without a single generation having run.
    #[test]
    fn busy_drop_is_not_counted_as_an_attempt() {
        let mut slot = FlavorAmbientSlot::new();
        assert_eq!(slot.begin_request(current()), Admit::Started);
        assert_eq!(slot.begin_request(current()), Admit::DroppedBusy);
        assert_eq!(slot.counters().attempts, 1);
        // Counted separately so a slot stuck in flight is still visible: every
        // later kick is dropped, and `attempts` alone would stay frozen at 1
        // exactly as it would if the arena had never advanced again.
        assert_eq!(slot.counters().dropped_busy, 1);
    }

    fn req(tid: TemplateId) -> FlavorRequest {
        FlavorRequest {
            schema: FlavorSchema::V1,
            template_id: tid,
            slots: vec![FlavorSlot {
                id: SlotId::Mood,
                tag: SlotValue::MoodCalm,
            }],
            locale: FlavorLocale::Ja,
        }
    }

    #[test]
    fn p2_1_matching_correlation_delivers() {
        let mut slot = FlavorAmbientSlot::new();
        store(&mut slot, current());
        assert!(slot.take(&current()).is_some());
    }

    #[test]
    fn p2_2_older_tick_discarded() {
        let mut slot = FlavorAmbientSlot::new();
        store(&mut slot, corr(F1, 4, TemplateId::ArenaEventHeadline));
        assert!(slot.take(&current()).is_none());
    }

    #[test]
    fn p2_3_newer_tick_discarded() {
        let mut slot = FlavorAmbientSlot::new();
        store(&mut slot, corr(F1, 6, TemplateId::ArenaEventHeadline));
        assert!(slot.take(&current()).is_none());
    }

    #[test]
    fn p2_4_foreign_campaign_discarded() {
        let mut slot = FlavorAmbientSlot::new();
        store(&mut slot, corr(F2, 5, TemplateId::ArenaEventHeadline));
        assert!(slot.take(&current()).is_none());
    }

    #[test]
    fn p2_5_other_template_discarded() {
        let mut slot = FlavorAmbientSlot::new();
        store(&mut slot, corr(F1, 5, TemplateId::ArenaEventAside));
        assert!(slot.take(&current()).is_none());
    }

    #[test]
    fn p2_6_empty_is_none() {
        let mut slot = FlavorAmbientSlot::new();
        assert!(slot.take(&current()).is_none());
    }

    #[test]
    fn p2_7_take_empties_slot() {
        let mut slot = FlavorAmbientSlot::new();
        store(&mut slot, current());
        assert!(slot.take(&current()).is_some());
        assert!(slot.take(&current()).is_none());
    }

    #[test]
    fn p3_1_idle_request_starts() {
        let mut slot = FlavorAmbientSlot::new();
        assert_eq!(
            slot.begin_request(current()),
            Admit::Started
        );
        assert!(slot.is_busy());
    }

    #[test]
    fn p3_2_busy_drops_immediately() {
        let mut slot = FlavorAmbientSlot::new();
        assert_eq!(slot.begin_request(current()), Admit::Started);
        let t0 = std::time::Instant::now();
        assert_eq!(
            slot.begin_request(corr(F1, 6, TemplateId::ArenaEventHeadline)),
            Admit::DroppedBusy
        );
        assert!(
            t0.elapsed() < std::time::Duration::from_millis(50),
            "busy drop must return immediately"
        );
    }

    #[test]
    fn p3_3_hundred_busy_drops_queue_always_zero() {
        let mut slot = FlavorAmbientSlot::new();
        assert_eq!(slot.begin_request(current()), Admit::Started);
        for _ in 0..100 {
            assert_eq!(
                slot.begin_request(corr(F1, 6, TemplateId::ArenaEventHeadline)),
                Admit::DroppedBusy
            );
            assert_eq!(slot.pending_queue_len(), 0);
        }
    }

    #[test]
    fn p3_4_abort_then_finish_discards_without_panic() {
        let mut slot = FlavorAmbientSlot::new();
        assert_eq!(slot.begin_request(current()), Admit::Started);
        slot.abort_campaign();
        slot.finish(FlavorOutcome::Accepted(accepted_flavor()));
        assert!(slot.take(&current()).is_none());
    }

    #[test]
    fn p8_1_latest_overwrite_keeps_only_newer() {
        let mut slot = FlavorAmbientSlot::new();
        store(&mut slot, corr(F1, 5, TemplateId::ArenaEventHeadline));
        store(&mut slot, corr(F1, 6, TemplateId::ArenaEventHeadline));
        // Pull with T5 must miss (old gone); proven via p8_3. Here: held equals T6.
        assert!(slot.take(&corr(F1, 6, TemplateId::ArenaEventHeadline)).is_some());
    }

    #[test]
    fn p8_2_pull_at_t6_after_overwrite_delivers() {
        let mut slot = FlavorAmbientSlot::new();
        store(&mut slot, corr(F1, 5, TemplateId::ArenaEventHeadline));
        store(&mut slot, corr(F1, 6, TemplateId::ArenaEventHeadline));
        assert!(slot.take(&corr(F1, 6, TemplateId::ArenaEventHeadline)).is_some());
    }

    #[test]
    fn p8_3_pull_at_t5_after_overwrite_discards() {
        let mut slot = FlavorAmbientSlot::new();
        store(&mut slot, corr(F1, 5, TemplateId::ArenaEventHeadline));
        store(&mut slot, corr(F1, 6, TemplateId::ArenaEventHeadline));
        assert!(slot.take(&corr(F1, 5, TemplateId::ArenaEventHeadline)).is_none());
    }

    #[test]
    fn request_generate_wires_flavor_gen() {
        let mut slot = FlavorAmbientSlot::new();
        let r = req(TemplateId::ArenaEventHeadline);
        assert_eq!(
            slot.request_generate(current(), &r, Some(&"あ".repeat(8))),
            Admit::Started
        );
        assert!(slot.take(&current()).is_some());
    }

    #[test]
    fn correlation_from_genesis_calls_digest8() {
        use crate::blackbox_sim::genesis::{build_campaign_genesis, Difficulty, GenesisRequest};
        let g = build_campaign_genesis(GenesisRequest {
            scenario_id: 1,
            difficulty: Difficulty::Standard,
            campaign_index: 0,
            created_date: "2026-07-29".into(),
        })
        .expect("genesis");
        let c = correlation_from_genesis(&g, 3, TemplateId::SettlementRipple);
        assert_eq!(c.genesis_fingerprint, g.digest8());
        assert_eq!(c.tick, 3);
        assert_eq!(c.template_id, TemplateId::SettlementRipple);
    }
}
