//! Decision telemetry — the record ledger (SPEC §9.2, 第八律: 記録は聖域).
//!
//! Append-only, reject-on-full, seq-monotonic (BXS-I-08). There are no
//! free-text fields: every field is an enum, an integer, or a digest, so PII
//! cannot exist here by construction. `latency_ms` is an observation only —
//! it must never become an input to state evolution (BXS-I-01 / BXS-W-01).
//!
//! Channel discipline (wall W-c): everything recorded here carries the
//! `blackbox_sim` channel and never merges into the subjective/objective
//! gap-analysis corpora — the simulator measures behavior under controlled
//! conditions, not life resource allocation.

use serde::{Deserialize, Serialize};

use super::fsm::TurnPhase;
use super::ring::{FixedRing, RingConfigError};

/// Wall W-c channel tag. Persisted with every flushed batch (Phase 3).
pub const TELEMETRY_CHANNEL: &str = "blackbox_sim";

pub const EVENT_LOG_CAPACITY: usize = 512;

/// Closed intent catalog — the ONLY vocabulary the UI may submit (wall W-d).
/// The Phase 2 ActionCompiler expands these into ledger transactions; neither
/// the UI nor the LLM can author postings directly.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", rename_all_fields = "camelCase", deny_unknown_fields)]
pub enum ActionIntent {
    SetPrice { sku: u8, tick_price: i64 },
    OrderInventory { sku: u8, units: u32 },
    AcceptOffer { offer_id: u32 },
    DeclineOffer { offer_id: u32 },
    ClosePosition { position_id: u32 },
    OpenHedge { instrument: u8, notional_minor: i64 },
    Invest { project_id: u32, amount_minor: i64 },
    ContinueProject { project_id: u32 },
    AbandonProject { project_id: u32 },
    Borrow { facility: u8, amount_minor: i64 },
    Repay { facility: u8, amount_minor: i64 },
    ForecastInterval { lo_minor: i64, hi_minor: i64 },
    Abstain,
    /// Timeout default injected by the Director — a first-class decision
    /// (BXS-W-07): silently unrecorded timeouts would bias the pressure and
    /// escalation lanes.
    ForcedDefault,
}

/// Measurement stimulus taxonomy (SPEC §9.1). Frozen ids (BXS-I-13).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[repr(u8)]
pub enum StimulusKind {
    GamblePair = 0,
    AnchorProbe = 1,
    ForecastElicitation = 2,
    SunkCostPair = 3,
    CrisisCountdown = 4,
    DispositionWindow = 5,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct StimulusRef {
    pub kind: StimulusKind,
    pub stimulus_seq: u32,
    /// Truncated digest of the planted stimulus parameters — a cross
    /// reference for the estimators, not a security boundary.
    pub params_digest: [u8; 8],
}

/// Everything the engine knows about one decision, minus the sequence number
/// (which only the log may assign — no two-phase construction of `seq`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DecisionRecord {
    pub tick: u32,
    pub phase: TurnPhase,
    pub action: ActionIntent,
    pub stimulus: Option<StimulusRef>,
    /// Monotonic-clock observation in milliseconds. NEVER an input to state
    /// evolution — replay ignores it (BXS-I-01).
    pub latency_ms: Option<u32>,
    pub forced_default: bool,
    /// Digest of the authoritative state at decision time (replay anchor).
    pub state_digest: [u8; 8],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DecisionEvent {
    pub seq: u64,
    pub tick: u32,
    pub phase: TurnPhase,
    pub action: ActionIntent,
    pub stimulus: Option<StimulusRef>,
    pub latency_ms: Option<u32>,
    pub forced_default: bool,
    pub state_digest: [u8; 8],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TelemetryError {
    LogFull,
    SeqExhausted,
    Config(RingConfigError),
}

/// Append-only decision log with an explicit flush contract: when the ring is
/// full, `record` REJECTS (typed) until `drain_for_flush` moves the batch to
/// the vault (Phase 3). Records are never silently evicted (第八律).
#[derive(Debug, Clone)]
pub struct EventLog {
    ring: FixedRing<DecisionEvent>,
    next_seq: u64,
}

impl EventLog {
    pub fn new() -> Result<Self, TelemetryError> {
        let ring = FixedRing::new(EVENT_LOG_CAPACITY).map_err(TelemetryError::Config)?;
        Ok(Self { ring, next_seq: 0 })
    }

    /// All-or-nothing: on rejection neither the ring nor `next_seq` moves.
    pub fn record(&mut self, r: DecisionRecord) -> Result<u64, TelemetryError> {
        let seq = self.next_seq;
        let next = seq.checked_add(1).ok_or(TelemetryError::SeqExhausted)?;
        let event = DecisionEvent {
            seq,
            tick: r.tick,
            phase: r.phase,
            action: r.action,
            stimulus: r.stimulus,
            latency_ms: r.latency_ms,
            forced_default: r.forced_default,
            state_digest: r.state_digest,
        };
        self.ring
            .try_push(event)
            .map_err(|_| TelemetryError::LogFull)?;
        self.next_seq = next;
        Ok(seq)
    }

    /// Explicit Settle-time flush (vault persistence in Phase 3).
    pub fn drain_for_flush(&mut self) -> Vec<DecisionEvent> {
        self.ring.drain_all()
    }

    /// Read the pending records without consuming them. Read-only by return
    /// type: `&DecisionEvent` gives a caller no way to edit or delete, which is
    /// the whole contract of this log (第八律).
    pub fn records(&self) -> impl Iterator<Item = &DecisionEvent> {
        self.ring.iter()
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.ring.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.ring.is_empty()
    }

    #[must_use]
    pub fn is_full(&self) -> bool {
        self.ring.is_full()
    }

    #[must_use]
    pub fn next_seq(&self) -> u64 {
        self.next_seq
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ok<T, E: std::fmt::Debug>(r: Result<T, E>) -> T {
        match r {
            Ok(v) => v,
            Err(e) => unreachable!("test setup failed: {e:?}"),
        }
    }

    fn record(tick: u32) -> DecisionRecord {
        DecisionRecord {
            tick,
            phase: TurnPhase::Decide,
            action: ActionIntent::Abstain,
            stimulus: None,
            latency_ms: Some(420),
            forced_default: false,
            state_digest: [7; 8],
        }
    }

    #[test]
    fn record_assigns_monotonic_seq() {
        let mut log = ok(EventLog::new());
        for expected in 0..8u64 {
            let seq = ok(log.record(record(expected as u32)));
            assert_eq!(seq, expected);
        }
        assert_eq!(log.next_seq(), 8);
        assert_eq!(log.len(), 8);
    }

    #[test]
    fn full_log_rejects_and_preserves_state() {
        let mut log = ok(EventLog::new());
        for tick in 0..EVENT_LOG_CAPACITY as u32 {
            let _ = ok(log.record(record(tick)));
        }
        assert!(log.is_full());
        let before_seq = log.next_seq();
        assert!(matches!(
            log.record(record(9_999)),
            Err(TelemetryError::LogFull)
        ));
        assert_eq!(log.next_seq(), before_seq, "seq must not advance on reject");
        let drained = log.drain_for_flush();
        assert_eq!(drained.len(), EVENT_LOG_CAPACITY);
        assert!(matches!(drained.first(), Some(DecisionEvent { seq: 0, .. })));
        // After the explicit flush the log accepts again, seq continues.
        let seq = ok(log.record(record(1)));
        assert_eq!(seq, EVENT_LOG_CAPACITY as u64);
    }

    #[test]
    fn stimulus_kind_ids_frozen() {
        assert_eq!(StimulusKind::GamblePair as u8, 0);
        assert_eq!(StimulusKind::AnchorProbe as u8, 1);
        assert_eq!(StimulusKind::ForecastElicitation as u8, 2);
        assert_eq!(StimulusKind::SunkCostPair as u8, 3);
        assert_eq!(StimulusKind::CrisisCountdown as u8, 4);
        assert_eq!(StimulusKind::DispositionWindow as u8, 5);
    }

    #[test]
    fn decision_event_fits_memory_budget() {
        // SPEC §13: 512 events must stay ≈64 KiB; guard against field bloat.
        assert!(std::mem::size_of::<DecisionEvent>() <= 128);
    }

    #[test]
    fn wire_shape_is_camel_case_and_strict() {
        let ev = DecisionEvent {
            seq: 1,
            tick: 2,
            phase: TurnPhase::Report,
            action: ActionIntent::ForecastInterval {
                lo_minor: 100,
                hi_minor: 900,
            },
            stimulus: Some(StimulusRef {
                kind: StimulusKind::ForecastElicitation,
                stimulus_seq: 3,
                params_digest: [1, 2, 3, 4, 5, 6, 7, 8],
            }),
            latency_ms: None,
            forced_default: false,
            state_digest: [0; 8],
        };
        let json = match serde_json::to_string(&ev) {
            Ok(s) => s,
            Err(e) => unreachable!("serialize failed: {e}"),
        };
        // Serde contract (§4.12a-2): fields camelCase, variants snake_case.
        assert!(json.contains("\"latencyMs\""));
        assert!(json.contains("\"forcedDefault\""));
        assert!(json.contains("\"forecast_interval\""));
        assert!(json.contains("\"loMinor\""));
        // Unknown fields must be rejected on the way back in (§16.2).
        let tampered = json.replacen("{\"seq\":1", "{\"seq\":1,\"extra\":1", 1);
        let back: Result<DecisionEvent, _> = serde_json::from_str(&tampered);
        assert!(back.is_err(), "unknown field must hard-fail, not be ignored");
        let clean: Result<DecisionEvent, _> = serde_json::from_str(&json);
        assert!(matches!(clean, Ok(e2) if e2 == ev));
    }
}
