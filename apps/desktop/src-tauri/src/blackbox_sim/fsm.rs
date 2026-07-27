//! Session/turn FSM — a literal transcription of the SPEC §7 decision table
//! (FLR-3 / BXS-I-07). Do not invent transitions, do not round unknown pairs:
//! every (state, event) combination outside the table is a typed
//! `IllegalTransition`, and `Sealed` / `Dead` are absorbing.
//!
//! Timeout-forced decisions are NOT a separate transition: the Director
//! submits `DecisionSubmitted` carrying `ForcedDefault` as a first-class
//! logged decision (SPEC §3.4, trap BXS-W-07).

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TurnPhase {
    Observe,
    Decide,
    Execute,
    Settle,
    Report,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FailureReason {
    AccountingBreach,
    SnapshotDigestMismatch,
    ReplayDivergence,
    InternalInvariantBroken,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", rename_all_fields = "camelCase")]
pub enum SessionState {
    Genesis,
    Active { phase: TurnPhase },
    Sealed,
    Dead { reason: FailureReason },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", rename_all_fields = "camelCase")]
pub enum SessionEvent {
    StartConfirmed,
    ObservationClosed,
    DecisionSubmitted,
    ExecutionApplied,
    SettlementVerified,
    ReportAcknowledged,
    CampaignCompleted,
    AbortRequested,
    CorruptionDetected { reason: FailureReason },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FsmError {
    IllegalTransition {
        from: SessionState,
        event: SessionEvent,
    },
}

/// SPEC §7 decision table, transcribed 1:1. Any edit here without the matching
/// SPEC table edit (or vice versa) is a violation of FLR-1/FLR-3.
pub fn advance(state: SessionState, event: SessionEvent) -> Result<SessionState, FsmError> {
    use SessionEvent as E;
    use SessionState as S;
    use TurnPhase as P;
    match (state, event) {
        (S::Genesis, E::StartConfirmed) => Ok(S::Active { phase: P::Observe }),
        (S::Genesis, E::AbortRequested) => Ok(S::Sealed),
        (S::Active { phase: P::Observe }, E::ObservationClosed) => {
            Ok(S::Active { phase: P::Decide })
        }
        (S::Active { phase: P::Decide }, E::DecisionSubmitted) => {
            Ok(S::Active { phase: P::Execute })
        }
        (S::Active { phase: P::Execute }, E::ExecutionApplied) => {
            Ok(S::Active { phase: P::Settle })
        }
        (S::Active { phase: P::Settle }, E::SettlementVerified) => {
            Ok(S::Active { phase: P::Report })
        }
        (S::Active { phase: P::Report }, E::ReportAcknowledged) => {
            Ok(S::Active { phase: P::Observe })
        }
        (S::Active { phase: P::Report }, E::CampaignCompleted) => Ok(S::Sealed),
        (S::Active { .. }, E::AbortRequested) => Ok(S::Sealed),
        (S::Active { .. }, E::CorruptionDetected { reason }) => Ok(S::Dead { reason }),
        (from, event) => Err(FsmError::IllegalTransition { from, event }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PHASES: [TurnPhase; 5] = [
        TurnPhase::Observe,
        TurnPhase::Decide,
        TurnPhase::Execute,
        TurnPhase::Settle,
        TurnPhase::Report,
    ];

    fn all_states() -> Vec<SessionState> {
        let mut v = vec![SessionState::Genesis, SessionState::Sealed];
        for phase in PHASES {
            v.push(SessionState::Active { phase });
        }
        v.push(SessionState::Dead {
            reason: FailureReason::AccountingBreach,
        });
        v
    }

    fn all_events() -> Vec<SessionEvent> {
        vec![
            SessionEvent::StartConfirmed,
            SessionEvent::ObservationClosed,
            SessionEvent::DecisionSubmitted,
            SessionEvent::ExecutionApplied,
            SessionEvent::SettlementVerified,
            SessionEvent::ReportAcknowledged,
            SessionEvent::CampaignCompleted,
            SessionEvent::AbortRequested,
            SessionEvent::CorruptionDetected {
                reason: FailureReason::ReplayDivergence,
            },
        ]
    }

    /// The expected legal-transition table — an independent transcription of
    /// SPEC §7 (kept deliberately separate from `advance` so the test is a
    /// control group, not a wrapper of the implementation — §16.6).
    fn expected(state: SessionState, event: SessionEvent) -> Option<SessionState> {
        use SessionEvent as E;
        use SessionState as S;
        use TurnPhase as P;
        match (state, event) {
            (S::Genesis, E::StartConfirmed) => Some(S::Active { phase: P::Observe }),
            (S::Genesis, E::AbortRequested) => Some(S::Sealed),
            (S::Active { phase: P::Observe }, E::ObservationClosed) => {
                Some(S::Active { phase: P::Decide })
            }
            (S::Active { phase: P::Decide }, E::DecisionSubmitted) => {
                Some(S::Active { phase: P::Execute })
            }
            (S::Active { phase: P::Execute }, E::ExecutionApplied) => {
                Some(S::Active { phase: P::Settle })
            }
            (S::Active { phase: P::Settle }, E::SettlementVerified) => {
                Some(S::Active { phase: P::Report })
            }
            (S::Active { phase: P::Report }, E::ReportAcknowledged) => {
                Some(S::Active { phase: P::Observe })
            }
            (S::Active { phase: P::Report }, E::CampaignCompleted) => Some(S::Sealed),
            (S::Active { .. }, E::AbortRequested) => Some(S::Sealed),
            (S::Active { .. }, E::CorruptionDetected { reason }) => Some(S::Dead { reason }),
            _ => None,
        }
    }

    #[test]
    fn decision_table_exhaustive() {
        let mut legal = 0usize;
        let mut illegal = 0usize;
        for state in all_states() {
            for event in all_events() {
                match (advance(state, event), expected(state, event)) {
                    (Ok(next), Some(want)) => {
                        assert_eq!(next, want, "wrong target for {state:?} × {event:?}");
                        legal += 1;
                    }
                    (Err(FsmError::IllegalTransition { from, event: ev }), None) => {
                        assert_eq!(from, state);
                        assert_eq!(ev, event);
                        illegal += 1;
                    }
                    (got, want) => {
                        unreachable!(
                            "table mismatch for {state:?} × {event:?}: got {got:?}, want {want:?}"
                        );
                    }
                }
            }
        }
        // 8 states × 9 events = 72 pairs; the SPEC table defines exactly 18
        // legal ones (2 from Genesis + 5 phase advances + CampaignCompleted
        // + 5 aborts + 5 corruption exits — Sealed/Dead absorb everything).
        assert_eq!(legal + illegal, 72);
        assert_eq!(legal, 18);
    }

    #[test]
    fn sealed_and_dead_are_absorbing() {
        for event in all_events() {
            assert!(matches!(
                advance(SessionState::Sealed, event),
                Err(FsmError::IllegalTransition { .. })
            ));
            assert!(matches!(
                advance(
                    SessionState::Dead {
                        reason: FailureReason::SnapshotDigestMismatch
                    },
                    event
                ),
                Err(FsmError::IllegalTransition { .. })
            ));
        }
    }
}
