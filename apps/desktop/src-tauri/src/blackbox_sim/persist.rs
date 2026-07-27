//! Persistence port — how records leave L2 without letting anything back in.
//!
//! # The problem this module solves
//!
//! Phase 3 has to write the decision log and the answer key to the vault, but
//! `blackbox_sim` is forbidden from naming the database layer at all — the
//! contract test `test_no_profile_dependency` scans this directory for those
//! names, comments included, and that scan is the structural form of wall W-b.
//!
//! The tempting shortcut is to relax the scan "just for the writer". Do not.
//! The scan is not the guarantee; it is a proxy for the guarantee, which is
//! that **the simulator cannot read a profile even by accident**. A module that
//! holds a live vault handle can call every read method on it, and the wall is
//! then one careless line away from falling, with no test able to tell.
//!
//! So the dependency is inverted. This module declares what a persistence
//! back-end must *do*; the back-end that knows about SQLCipher lives outside
//! `blackbox_sim` and implements the trait. The simulator depends on a promise,
//! not on a database.
//!
//! # Why [`DecisionSink`] has no read method
//!
//! Look at the trait and notice what is missing: there is no `load`, no `get`,
//! no `query`. That is deliberate and it is the strongest guarantee in this
//! file. Wall W-b stops being a rule that reviewers must remember and becomes a
//! fact about the type system — an implementor may write records outward and
//! has no channel to hand anything back except a count of what it stored.
//!
//! If a future phase needs to reload a campaign, do not add a read method here.
//! Add a separate loader that the *caller* runs, and pass the result into
//! `Session::start` as data. The sim must never be able to pull.
//!
//! # Failure means "not yet", never "gone"
//!
//! [`Session::flush_to`] hands the sink a borrowed view of the pending records
//! and only clears the ring once the sink reports success. A vault that is
//! locked, busy or quarantined therefore costs nothing but a retry: the records
//! stay in the ring, the ring keeps rejecting new writes when it fills, and the
//! campaign keeps playing. Dropping records to make room would be the exact
//! failure 第八律 exists to forbid, and it is the failure that a naive
//! `drain_all(); sink.write(...)` would produce on the first locked vault.

use super::stimulus::PlantedStimulus;
use super::telemetry::DecisionEvent;

/// Wire schema of a persisted batch. Bump this, never reinterpret it.
pub const BLACKBOX_LOG_SCHEMA_V1: &str = "blackbox_log.v1";

/// One flush: the records produced since the previous successful flush.
///
/// Borrowed rather than owned so that a failed flush costs no allocation and,
/// more importantly, so the sink cannot take ownership of records that the log
/// has not yet been told it may forget.
#[derive(Debug, Clone, Copy)]
pub struct DecisionBatch<'a> {
    pub schema: &'static str,
    /// Identifies the campaign these records belong to. The Genesis
    /// fingerprint, not the seed: the seed would let a reader regenerate the
    /// world's true parameters, and that is wall W-a's whole concern.
    pub campaign_fingerprint: [u8; 32],
    pub events: &'a [DecisionEvent],
    /// Answer-key rows not yet persisted. Append-only upstream, so this is
    /// always a suffix of the ledger.
    pub stimuli: &'a [PlantedStimulus],
}

impl DecisionBatch<'_> {
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.events.is_empty() && self.stimuli.is_empty()
    }
}

/// What the back-end reports back. Counts only — a receipt is an
/// acknowledgement, not a channel.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct FlushReceipt {
    pub events_persisted: u32,
    pub stimuli_persisted: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SinkError {
    /// The store is not accepting writes right now (locked, busy, absent).
    /// Retryable: the caller keeps its records.
    Unavailable,
    /// The store rejected the batch itself. Not retryable as-is.
    Rejected,
    /// The store accepted fewer records than it was given. Treated as a
    /// failure on purpose: a partial write that reported success would let the
    /// ring clear records that were never stored.
    Partial,
}

/// Write-only outbound port. See the module header before adding a method.
pub trait DecisionSink {
    fn persist(&mut self, batch: &DecisionBatch<'_>) -> Result<FlushReceipt, SinkError>;
}

/// A sink that stores nothing and always succeeds.
///
/// This is the default, not a test double. Per SPEC §14's precedent for the LLM
/// layer, the simulator must be fully playable with no back-end at all; a
/// campaign is 52 turns against a 512-entry ring, so a session that never
/// flushes never fills. Using this means telemetry is discarded at the end of
/// the session, which is the honest outcome of having nowhere to put it.
#[derive(Debug, Default, Clone, Copy)]
pub struct NullSink;

impl DecisionSink for NullSink {
    fn persist(&mut self, batch: &DecisionBatch<'_>) -> Result<FlushReceipt, SinkError> {
        Ok(FlushReceipt {
            events_persisted: u32::try_from(batch.events.len()).unwrap_or(u32::MAX),
            stimuli_persisted: u32::try_from(batch.stimuli.len()).unwrap_or(u32::MAX),
        })
    }
}

#[cfg(test)]
pub(crate) mod testing {
    use super::*;

    /// Records what it was handed, and can be told to fail.
    #[derive(Debug, Default)]
    pub struct RecordingSink {
        pub events: Vec<DecisionEvent>,
        pub stimuli: Vec<PlantedStimulus>,
        pub calls: u32,
        pub fail_with: Option<SinkError>,
    }

    impl DecisionSink for RecordingSink {
        fn persist(&mut self, batch: &DecisionBatch<'_>) -> Result<FlushReceipt, SinkError> {
            self.calls = self.calls.saturating_add(1);
            if let Some(error) = self.fail_with {
                return Err(error);
            }
            self.events.extend_from_slice(batch.events);
            self.stimuli.extend_from_slice(batch.stimuli);
            Ok(FlushReceipt {
                events_persisted: u32::try_from(batch.events.len()).unwrap_or(u32::MAX),
                stimuli_persisted: u32::try_from(batch.stimuli.len()).unwrap_or(u32::MAX),
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_empty_batch_is_recognisable() {
        let batch = DecisionBatch {
            schema: BLACKBOX_LOG_SCHEMA_V1,
            campaign_fingerprint: [0; 32],
            events: &[],
            stimuli: &[],
        };
        assert!(batch.is_empty());
        assert_eq!(
            NullSink.persist(&batch),
            Ok(FlushReceipt {
                events_persisted: 0,
                stimuli_persisted: 0
            })
        );
    }
}
