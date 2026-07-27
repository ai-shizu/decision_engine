//! Vault v12 record lane for the BLACKBOX SIMULATOR (SPEC §11, §16.3).
//!
//! # Why the adapter lives here and not in the simulator
//!
//! `blackbox_sim` may not name `crate::db` — that is wall W-b, and the contract
//! test scans for it. So the dependency is inverted: the simulator declares the
//! write-only [`DecisionSink`] port, and this module, which is allowed to know
//! about both worlds, implements it.
//!
//! Read the direction carefully. This file imports from `blackbox_sim`; nothing
//! in `blackbox_sim` imports from here. That asymmetry is the wall. The
//! simulator can be handed a writer and can push records out; it has no name
//! for this module, no way to construct one, and no method on the port that
//! would let one hand anything back.
//!
//! # Append-only in the storage layer too (第八律)
//!
//! [`persist_batch`] only ever `INSERT`s. There is no `UPDATE` and no `DELETE`
//! against `blackbox_decisions` or `blackbox_stimuli` anywhere in this module,
//! and adding one would break the record sanctity that the ring buffer, the
//! flush contract and the schema's primary keys all exist to protect.
//!
//! Re-flushing the same records is legal and idempotent: the primary key makes
//! a duplicate a no-op via `INSERT OR IGNORE`, so a retry after a crash between
//! "vault committed" and "ring cleared" cannot double-count a decision. What
//! it must never do is silently *replace* a row, because that would let a
//! second, different version of history overwrite the first — hence IGNORE
//! rather than REPLACE.

use rusqlite::{params, Transaction};

use crate::blackbox_sim::persist::{
    DecisionBatch, DecisionSink, FlushReceipt, SinkError, BLACKBOX_LOG_SCHEMA_V1,
};
use crate::blackbox_sim::telemetry::DecisionEvent;

/// Wall W-c: gap_analysis selects by channel, and simulator behaviour is
/// behaviour under a controlled instrument, not a life resource allocation.
/// Merging this lane into the subjective or objective corpora would make the
/// profile read the game as if it were the player's life.
pub(crate) const BLACKBOX_CHANNEL: &str = "blackbox_sim";

/// The campaign this batch belongs to, as the caller knows it.
///
/// Deliberately no `campaign_seed`. Storing the seed next to the analysis would
/// let any reader regenerate the world's true parameters and recover the answer
/// key for every anchor in the campaign (wall W-a).
#[derive(Debug, Clone, Copy)]
pub(crate) struct CampaignRow {
    pub fingerprint: [u8; 32],
    pub scenario_id: u32,
    pub difficulty: u8,
    pub campaign_index: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BlackboxRepoError {
    Storage,
    Serialization,
    /// A stimulus row's parameters do not hash to the digest stored with them.
    /// Refused at the boundary rather than written: an answer key that does not
    /// match its pointer is a corrupted measurement (SPEC §16.3).
    BindingBroken { seq: u32 },
}

impl std::fmt::Display for BlackboxRepoError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BlackboxRepoError::Storage => write!(f, "blackbox record lane: storage failed"),
            BlackboxRepoError::Serialization => {
                write!(f, "blackbox record lane: payload would not serialise")
            }
            BlackboxRepoError::BindingBroken { seq } => write!(
                f,
                "blackbox record lane: stimulus {seq} does not hash to its digest"
            ),
        }
    }
}

impl std::error::Error for BlackboxRepoError {}

/// Register the campaign. Idempotent; the fingerprint is the identity.
pub(crate) fn upsert_campaign(
    transaction: &Transaction<'_>,
    campaign: &CampaignRow,
    created_date: &str,
    now: i64,
) -> Result<(), BlackboxRepoError> {
    transaction
        .execute(
            "INSERT INTO blackbox_campaigns (campaign_fingerprint, schema_version, scenario_id, \
             difficulty, campaign_index, created_date, started_at, updated_at) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?7) \
             ON CONFLICT(campaign_fingerprint) DO UPDATE SET updated_at = ?7",
            params![
                &campaign.fingerprint[..],
                BLACKBOX_LOG_SCHEMA_V1,
                campaign.scenario_id,
                campaign.difficulty,
                campaign.campaign_index,
                created_date,
                now,
            ],
        )
        .map_err(|_| BlackboxRepoError::Storage)?;
    Ok(())
}

/// Discriminant of the intent, stored alongside the payload so the estimators
/// can filter by act without parsing JSON for every row.
fn action_kind(event: &DecisionEvent) -> i64 {
    use crate::blackbox_sim::telemetry::ActionIntent as A;
    match event.action {
        A::SetPrice { .. } => 0,
        A::OrderInventory { .. } => 1,
        A::AcceptOffer { .. } => 2,
        A::DeclineOffer { .. } => 3,
        A::ClosePosition { .. } => 4,
        A::OpenHedge { .. } => 5,
        A::Invest { .. } => 6,
        A::ContinueProject { .. } => 7,
        A::AbandonProject { .. } => 8,
        A::Borrow { .. } => 9,
        A::Repay { .. } => 10,
        A::ForecastInterval { .. } => 11,
        A::Abstain => 12,
        A::ForcedDefault => 13,
    }
}

/// Write one flush. All-or-nothing: the caller runs this inside the worker's
/// IMMEDIATE transaction, so a mid-batch failure rolls the whole batch back and
/// the simulator's ring keeps every record.
pub(crate) fn persist_batch(
    transaction: &Transaction<'_>,
    campaign: &CampaignRow,
    batch: &DecisionBatch<'_>,
    now: i64,
) -> Result<FlushReceipt, BlackboxRepoError> {
    if campaign.fingerprint != batch.campaign_fingerprint {
        return Err(BlackboxRepoError::Storage);
    }

    // Verify every binding BEFORE writing anything. A batch that carries a
    // mismatched answer key is refused whole rather than half-stored.
    for row in batch.stimuli {
        if !row.binding_holds() {
            return Err(BlackboxRepoError::BindingBroken { seq: row.seq });
        }
    }

    let mut stimuli_persisted: u32 = 0;
    for row in batch.stimuli {
        let params_json =
            serde_json::to_string(&row.params).map_err(|_| BlackboxRepoError::Serialization)?;
        transaction
            .execute(
                "INSERT OR IGNORE INTO blackbox_stimuli (campaign_fingerprint, stimulus_seq, \
                 tick, kind, params_json, params_digest, persisted_at) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![
                    &campaign.fingerprint[..],
                    row.seq,
                    row.tick,
                    row.params.kind() as u8,
                    params_json,
                    &row.params_digest[..],
                    now,
                ],
            )
            .map_err(|_| BlackboxRepoError::Storage)?;
        stimuli_persisted = stimuli_persisted.saturating_add(1);
    }

    let mut events_persisted: u32 = 0;
    for event in batch.events {
        let action_json =
            serde_json::to_string(&event.action).map_err(|_| BlackboxRepoError::Serialization)?;
        let (stimulus_seq, stimulus_kind, stimulus_digest) = match event.stimulus {
            Some(reference) => (
                Some(reference.stimulus_seq),
                Some(reference.kind as u8),
                Some(reference.params_digest.to_vec()),
            ),
            None => (None, None, None),
        };
        transaction
            .execute(
                "INSERT OR IGNORE INTO blackbox_decisions (campaign_fingerprint, seq, tick, \
                 phase, action_kind, action_json, stimulus_seq, stimulus_kind, stimulus_digest, \
                 latency_ms, forced_default, state_digest, channel, persisted_at) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)",
                params![
                    &campaign.fingerprint[..],
                    i64::try_from(event.seq).unwrap_or(i64::MAX),
                    event.tick,
                    event.phase as u8,
                    action_kind(event),
                    action_json,
                    stimulus_seq,
                    stimulus_kind,
                    stimulus_digest,
                    event.latency_ms,
                    i64::from(event.forced_default),
                    &event.state_digest[..],
                    BLACKBOX_CHANNEL,
                    now,
                ],
            )
            .map_err(|_| BlackboxRepoError::Storage)?;
        events_persisted = events_persisted.saturating_add(1);
    }

    Ok(FlushReceipt {
        events_persisted,
        stimuli_persisted,
    })
}

/// Binds a live transaction to the simulator's write-only port.
///
/// Short-lived by design: it borrows the transaction, so it cannot outlive the
/// unit of work and cannot be stashed somewhere that would let the simulator
/// keep a handle on the database.
pub(crate) struct VaultDecisionSink<'a, 'conn> {
    transaction: &'a Transaction<'conn>,
    campaign: CampaignRow,
    now: i64,
}

impl<'a, 'conn> VaultDecisionSink<'a, 'conn> {
    pub(crate) fn new(
        transaction: &'a Transaction<'conn>,
        campaign: CampaignRow,
        now: i64,
    ) -> Self {
        Self {
            transaction,
            campaign,
            now,
        }
    }
}

impl DecisionSink for VaultDecisionSink<'_, '_> {
    fn persist(&mut self, batch: &DecisionBatch<'_>) -> Result<FlushReceipt, SinkError> {
        match persist_batch(self.transaction, &self.campaign, batch, self.now) {
            Ok(receipt) => Ok(receipt),
            // A broken binding is the batch's fault and will not fix itself on
            // retry, so it is `Rejected` rather than `Unavailable`. The
            // simulator keeps its records either way.
            Err(BlackboxRepoError::BindingBroken { .. } | BlackboxRepoError::Serialization) => {
                Err(SinkError::Rejected)
            }
            Err(BlackboxRepoError::Storage) => Err(SinkError::Unavailable),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::blackbox_sim::director::{Session, CAMPAIGN_TICKS};
    use crate::blackbox_sim::genesis::{Difficulty, GenesisRequest};
    use crate::blackbox_sim::telemetry::ActionIntent;
    use crate::db::migrations::run_migrations;
    use rusqlite::Connection;
    use std::error::Error;

    fn request() -> GenesisRequest {
        GenesisRequest {
            scenario_id: 3,
            difficulty: Difficulty::Standard,
            campaign_index: 1,
            created_date: "2026-07-27".to_string(),
        }
    }

    fn play(turns: u32) -> Session {
        let mut session = match Session::start(request()) {
            Ok(s) => s,
            Err(e) => unreachable!("session start: {e:?}"),
        };
        for _ in 0..turns {
            if session.observe().is_err() {
                break;
            }
            let intent = session
                .stimulus_views()
                .into_iter()
                .find_map(|v| v.offer_id)
                .map_or(ActionIntent::Abstain, |offer_id| ActionIntent::AcceptOffer {
                    offer_id,
                });
            if session.submit(intent, Some(120)).is_err() {
                let _ = session.submit(ActionIntent::Abstain, Some(120));
            }
            let _ = session.execute();
            let _ = session.settle();
            let _ = session.report();
        }
        session
    }

    fn campaign_of(session: &Session) -> CampaignRow {
        CampaignRow {
            fingerprint: session.campaign_fingerprint(),
            scenario_id: 3,
            difficulty: 0,
            campaign_index: 1,
        }
    }

    fn migrated() -> Result<Connection, Box<dyn Error>> {
        let mut connection = Connection::open_in_memory()?;
        run_migrations(&mut connection)?;
        Ok(connection)
    }

    #[test]
    fn a_campaign_round_trips_through_the_vault() -> Result<(), Box<dyn Error>> {
        let mut session = play(20);
        let campaign = campaign_of(&session);
        let mut connection = migrated()?;
        let transaction = connection.transaction()?;
        upsert_campaign(&transaction, &campaign, "2026-07-27", 1_000)?;
        let expected = session.unflushed_records();
        {
            let mut sink = VaultDecisionSink::new(&transaction, campaign, 1_000);
            let receipt = match session.flush_to(&mut sink) {
                Ok(r) => r,
                Err(e) => unreachable!("flush: {e:?}"),
            };
            assert_eq!(receipt.events_persisted as usize, expected);
        }
        let stored: i64 = transaction.query_row(
            "SELECT count(*) FROM blackbox_decisions WHERE campaign_fingerprint = ?1",
            params![&campaign.fingerprint[..]],
            |row| row.get(0),
        )?;
        assert_eq!(stored as usize, expected);

        // Every attribution resolves to a stored answer-key row, and the
        // pointer/payload binding survived serialisation.
        let dangling: i64 = transaction.query_row(
            "SELECT count(*) FROM blackbox_decisions d \
             LEFT JOIN blackbox_stimuli s \
               ON s.campaign_fingerprint = d.campaign_fingerprint \
              AND s.stimulus_seq = d.stimulus_seq \
             WHERE d.stimulus_seq IS NOT NULL \
               AND (s.stimulus_seq IS NULL OR s.params_digest != d.stimulus_digest)",
            [],
            |row| row.get(0),
        )?;
        assert_eq!(dangling, 0, "a decision points at a missing or wrong row");
        Ok(())
    }

    #[test]
    fn every_row_is_tagged_with_the_simulator_channel() -> Result<(), Box<dyn Error>> {
        let mut session = play(10);
        let campaign = campaign_of(&session);
        let mut connection = migrated()?;
        let transaction = connection.transaction()?;
        upsert_campaign(&transaction, &campaign, "2026-07-27", 1)?;
        let mut sink = VaultDecisionSink::new(&transaction, campaign, 1);
        let _ = session.flush_to(&mut sink);
        let foreign: i64 = transaction.query_row(
            "SELECT count(*) FROM blackbox_decisions WHERE channel != 'blackbox_sim'",
            [],
            |row| row.get(0),
        )?;
        assert_eq!(foreign, 0, "wall W-c: the lane must be self-identifying");
        Ok(())
    }

    #[test]
    fn reflushing_the_same_records_cannot_duplicate_history() -> Result<(), Box<dyn Error>> {
        let mut session = play(12);
        let campaign = campaign_of(&session);
        let mut connection = migrated()?;
        let transaction = connection.transaction()?;
        upsert_campaign(&transaction, &campaign, "2026-07-27", 1)?;
        // Capture what a flush would hand over, then replay that same batch.
        let mut captured = crate::blackbox_sim::persist::testing::RecordingSink::default();
        let _ = session.flush_to(&mut captured);
        let events = captured.events;
        let stimuli = captured.stimuli;
        assert!(!events.is_empty(), "the campaign produced records");
        let batch = DecisionBatch {
            schema: BLACKBOX_LOG_SCHEMA_V1,
            campaign_fingerprint: campaign.fingerprint,
            events: &events,
            stimuli: &stimuli,
        };
        // Simulate a crash between "vault committed" and "ring cleared": the
        // identical batch arrives twice.
        persist_batch(&transaction, &campaign, &batch, 1)?;
        persist_batch(&transaction, &campaign, &batch, 2)?;
        let stored: i64 =
            transaction.query_row("SELECT count(*) FROM blackbox_decisions", [], |r| r.get(0))?;
        assert_eq!(stored as usize, events.len(), "a retry double-counted");
        // And the first write won: IGNORE, never REPLACE.
        let first: i64 = transaction.query_row(
            "SELECT min(persisted_at) FROM blackbox_decisions",
            [],
            |r| r.get(0),
        )?;
        assert_eq!(first, 1, "history was overwritten by the retry");
        Ok(())
    }

    #[test]
    fn a_broken_binding_is_refused_and_writes_nothing() -> Result<(), Box<dyn Error>> {
        use crate::blackbox_sim::stimulus::{PlantedStimulus, StimulusParams};
        let campaign = CampaignRow {
            fingerprint: [7; 32],
            scenario_id: 3,
            difficulty: 0,
            campaign_index: 1,
        };
        let mut connection = migrated()?;
        let transaction = connection.transaction()?;
        upsert_campaign(&transaction, &campaign, "2026-07-27", 1)?;
        // A row whose parameters were swapped after the digest was taken.
        let mut tampered = PlantedStimulus {
            seq: 0,
            tick: 1,
            params: StimulusParams::ForecastElicitation {
                reference_minor: 100,
            },
            params_digest: [0; 8],
        };
        tampered.params_digest = tampered.params.digest(1, 0);
        tampered.params = StimulusParams::ForecastElicitation {
            reference_minor: 999,
        };
        let batch = DecisionBatch {
            schema: BLACKBOX_LOG_SCHEMA_V1,
            campaign_fingerprint: campaign.fingerprint,
            events: &[],
            stimuli: &[tampered],
        };
        assert_eq!(
            persist_batch(&transaction, &campaign, &batch, 1),
            Err(BlackboxRepoError::BindingBroken { seq: 0 })
        );
        let stored: i64 =
            transaction.query_row("SELECT count(*) FROM blackbox_stimuli", [], |r| r.get(0))?;
        assert_eq!(stored, 0, "a refused batch must write nothing");
        Ok(())
    }

    #[test]
    fn a_full_campaign_persists_enough_to_estimate_from() -> Result<(), Box<dyn Error>> {
        let mut session = play(CAMPAIGN_TICKS);
        let campaign = campaign_of(&session);
        let mut connection = migrated()?;
        let transaction = connection.transaction()?;
        upsert_campaign(&transaction, &campaign, "2026-07-27", 1)?;
        let mut sink = VaultDecisionSink::new(&transaction, campaign, 1);
        let _ = session.flush_to(&mut sink);
        // Lane 0 reads accept/decline against gamble probes. Kind 0 is
        // GamblePair (frozen id, BXS-I-13).
        let gambles: i64 = transaction.query_row(
            "SELECT count(*) FROM blackbox_decisions WHERE stimulus_kind = 0",
            [],
            |r| r.get(0),
        )?;
        assert!(gambles >= 12, "lane 0 would be starved on load: {gambles}");
        Ok(())
    }
}
