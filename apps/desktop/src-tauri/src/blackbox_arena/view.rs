//! FE-facing DTOs for the BLACKBOX SIMULATOR command surface (SPEC §15).
//!
//! Every type here is the *minimal display set* (BXS-I-20 discipline): a
//! field is added only because a command genuinely needs to hand it to the
//! caller, never as a convenience mirror of an internal struct. None of these
//! types can carry Genesis truth (wall W-a) — they are built exclusively from
//! `MarketTickView`, `StimulusView`, `SessionState` and small per-tick
//! summaries, all of which are already the sim's own published boundary.

use serde::{Deserialize, Serialize};

use crate::blackbox_sim::action::Execution;
use crate::blackbox_sim::director::{DirectorError, TurnReport};
use crate::blackbox_sim::fsm::SessionState;
use crate::blackbox_sim::genesis::Difficulty;
use crate::blackbox_sim::market::MarketTickView;
use crate::blackbox_sim::settle::{CashFlowStatement, PeriodClose};
use crate::blackbox_sim::stimulus::StimulusView;

/// Wire-level difficulty. `blackbox_sim::genesis::Difficulty` deliberately
/// derives `Serialize` only (it is hashed into the fingerprint, never parsed
/// back from the wire) — this local twin is the closed, `Deserialize`-able
/// vocabulary the command surface accepts, converted at the boundary so
/// `blackbox_sim` itself gains no new derive.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ArenaDifficulty {
    Standard,
    Hard,
}

impl From<ArenaDifficulty> for Difficulty {
    fn from(value: ArenaDifficulty) -> Self {
        match value {
            ArenaDifficulty::Standard => Difficulty::Standard,
            ArenaDifficulty::Hard => Difficulty::Hard,
        }
    }
}

/// `bxs_start_campaign` request body (SPEC §15). Closed, `deny_unknown_fields`
/// per §4.12a-2 — no `serde_json::Value` anywhere on this boundary.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct StartCampaignRequest {
    pub scenario_id: u32,
    pub difficulty: ArenaDifficulty,
    pub campaign_index: u32,
    /// Strict `YYYY-MM-DD`; re-validated by `build_campaign_genesis` — this
    /// boundary does not loosen that check.
    pub created_date: String,
}

/// The market + probes the player is looking at this turn.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ObservationView {
    pub campaign_id: String,
    pub turns_completed: u32,
    pub market: MarketTickView,
    pub stimuli: Vec<StimulusView>,
    pub cash_minor: i64,
    pub state: SessionState,
}

/// What `bxs_submit_decision` hands back: whether the intent posted, nothing
/// more. Mirrors `Execution` field-for-field; kept as a separate type so the
/// wire shape does not silently change if `Execution` grows an internal-only
/// field later.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DecisionOutcomeView {
    pub ledger_effect: bool,
    pub journal_seq: Option<u64>,
}

impl From<Execution> for DecisionOutcomeView {
    fn from(execution: Execution) -> Self {
        Self {
            ledger_effect: execution.ledger_effect,
            journal_seq: execution.journal_seq,
        }
    }
}

/// Headline period-close numbers. A player-facing summary, not the full
/// `CashFlowStatement` — depreciation/interest/tax are shown because the
/// stimuli reference them narratively; the underlying account-level detail
/// stays server-side pending the FE slice's own design pass.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PeriodCloseView {
    pub net_income_minor: i64,
    pub net_cash_change_minor: i64,
    pub depreciation_minor: i64,
    pub interest_minor: i64,
    pub tax_minor: i64,
}

impl From<&PeriodClose> for PeriodCloseView {
    fn from(close: &PeriodClose) -> Self {
        let CashFlowStatement {
            net_change_minor,
            net_income_minor,
            ..
        } = close.cash_flow;
        Self {
            net_income_minor,
            net_cash_change_minor: net_change_minor,
            depreciation_minor: close.depreciation_minor,
            interest_minor: close.interest_minor,
            tax_minor: close.tax_minor,
        }
    }
}

/// What `bxs_advance` hands back: the turn just closed, plus the next
/// observation if the campaign is still live. `next_observation` is `None`
/// exactly when the campaign sealed on this turn — there is nothing left to
/// observe, and fabricating a placeholder would misreport campaign state.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AdvanceView {
    pub tick: u32,
    pub revenue_minor: i64,
    pub cogs_minor: i64,
    pub unmet_units: u32,
    pub period_close: Option<PeriodCloseView>,
    pub state: SessionState,
    pub next_observation: Option<ObservationView>,
}

impl AdvanceView {
    pub(crate) fn from_report(report: &TurnReport, next_observation: Option<ObservationView>, state: SessionState) -> Self {
        Self {
            tick: report.tick,
            revenue_minor: report.operations.revenue_minor,
            cogs_minor: report.operations.cogs_minor,
            unmet_units: report.operations.unmet_units,
            period_close: report.period_close.as_ref().map(PeriodCloseView::from),
            state,
            next_observation,
        }
    }
}

/// The two-layer error boundary (SPEC §15). Only this closed enum crosses
/// IPC; the real `DirectorError`/`SinkError`/`VaultErrorCode` is logged
/// server-side (see `commands::log_and_collapse`) before being collapsed —
/// the FE never sees a diagnostic string, but the diagnostic is never thrown
/// away either (2026-07-24 context-budget lesson, AI_SKILLS.md).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum SimUiErrorCode {
    CampaignNotFound,
    WrongPhase,
    CampaignExhausted,
    BusinessRuleRejected,
    /// Only ever constructed on the `secure-vault`+Apple resume path
    /// (`bxs_load_generation` has nowhere to load from otherwise). Kept in
    /// the enum unconditionally so the wire contract's shape does not shift
    /// across build configurations.
    #[allow(dead_code)]
    GenerationNotFound,
    Unavailable,
    InternalFault,
}

/// Narrow `DirectorError` down to the wire vocabulary. Every arm is explicit
/// (no catch-all `_ =>` hiding a future variant) except the true internal
/// faults, which have no player-actionable meaning anyway.
pub(crate) fn map_director_error(context: &'static str, error: DirectorError) -> SimUiErrorCode {
    eprintln!("blackbox_arena: {context} failed: {error:?}");
    match error {
        DirectorError::WrongPhase { .. } => SimUiErrorCode::WrongPhase,
        DirectorError::CampaignExhausted => SimUiErrorCode::CampaignExhausted,
        DirectorError::Compile(_) => SimUiErrorCode::BusinessRuleRejected,
        DirectorError::Fsm(_)
        | DirectorError::Genesis(_)
        | DirectorError::Market(_)
        | DirectorError::Settle(_)
        | DirectorError::Snapshot(_)
        | DirectorError::Telemetry(_)
        | DirectorError::Refusal(_)
        | DirectorError::Stimulus(_)
        | DirectorError::Sink(_)
        | DirectorError::Ring(_)
        | DirectorError::NoObservationYet
        | DirectorError::MalformedLog { .. } => SimUiErrorCode::InternalFault,
    }
}
