//! Vault record + profile lanes for the BLACKBOX SIMULATOR (SPEC §11, §16.3).
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
//!
//! # Profile lane (v13 / Phase 6-A)
//!
//! [`insert_profile`] / [`get_latest_profile`] / [`list_profiles`] talk to the
//! three v13 tables. The profile lane is a **snapshot**, not an append-only
//! log: re-estimating the same pool digest replaces the prior snapshot
//! (`INSERT OR REPLACE` on the parent cascades the children away). That is the
//! opposite of the record lane on purpose — a bias estimate is a derived
//! reading, not a sacred event. LAW-19 is enforced both here (Rust validation
//! refuses fabricated estimates and hard-codes the uncalibrated marker) and in
//! the schema CHECKs (a calibrated string is unstorable).

use rusqlite::{params, Connection, OptionalExtension, Transaction};

use crate::blackbox_sim::bias::{
    BiasAxis, BiasEstimate, BlackboxProfile, CALIBRATION_UNCALIBRATED, INSTRUMENT_ID, N_AXES,
    SCHEMA_BLACKBOX_PROFILE_V1,
};
use crate::blackbox_sim::bridge::pool_digest;
#[cfg(feature = "blackbox-profile-write")]
use crate::blackbox_sim::bridge::MAX_POOLED_CAMPAIGNS;
use crate::blackbox_sim::director::LoggedDecision;
use crate::blackbox_sim::money::MICRO;
use crate::blackbox_sim::persist::{DecisionBatch, FlushReceipt, BLACKBOX_LOG_SCHEMA_V1};
#[cfg(test)]
use crate::blackbox_sim::persist::{DecisionSink, SinkError};
use crate::blackbox_sim::telemetry::{ActionIntent, DecisionEvent};

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
    /// Writer handed a profile the schema CHECKs would refuse (empty pool,
    /// fabricated estimate, wrong schema marker, duplicate source, …).
    /// Fail closed in Rust so the SQL error is not the first line of defence.
    InvalidProfile,
    /// A source fingerprint is not registered in `blackbox_campaigns`. The
    /// schema FK would refuse the row; we refuse earlier with a typed error.
    /// Present only when the write feature compiles the writer that produces it.
    #[cfg(feature = "blackbox-profile-write")]
    MissingCampaign,
    /// On read: blob length wrong, lane set incomplete, or a CHECK-forbidden
    /// combination somehow reached the reader. Prefer typed failure over a
    /// half-reconstructed profile.
    CorruptRow,
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
            BlackboxRepoError::InvalidProfile => {
                write!(f, "blackbox profile lane: profile failed validation")
            }
            #[cfg(feature = "blackbox-profile-write")]
            BlackboxRepoError::MissingCampaign => {
                write!(f, "blackbox profile lane: source campaign is not registered")
            }
            BlackboxRepoError::CorruptRow => {
                write!(f, "blackbox profile lane: stored row is corrupt")
            }
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

/// A campaign row plus the one field it doesn't carry on its own
/// (`created_date`, needed to rebuild a `GenesisRequest`).
#[derive(Debug, Clone)]
pub(crate) struct LoadedCampaign {
    pub campaign: CampaignRow,
    pub created_date: String,
}

/// Resume-path loader (Phase 5, SPEC §15's `bxs_load_generation`).
///
/// Per this module's own header and `persist.rs`'s module doc: this is
/// deliberately a free function the *caller* runs, never a method on
/// `DecisionSink` — the simulator itself has no name for this module and no
/// way to call it. Read-only; never touches `blackbox_decisions`/
/// `blackbox_stimuli` beyond a plain `SELECT`.
///
/// Reconstructs `LoggedDecision`s straight from `action_json` — the same
/// representation `persist_batch` wrote them in — ordered by `seq` so the
/// replay driver in `blackbox_sim::director::replay_session` sees decisions
/// in the order they actually happened.
pub(crate) fn load_campaign_and_decisions(
    connection: &Connection,
    fingerprint: [u8; 32],
) -> Result<Option<(LoadedCampaign, Vec<LoggedDecision>)>, BlackboxRepoError> {
    let campaign: Option<(u32, u8, u32, String)> = connection
        .query_row(
            "SELECT scenario_id, difficulty, campaign_index, created_date \
             FROM blackbox_campaigns WHERE campaign_fingerprint = ?1",
            params![&fingerprint[..]],
            |row| {
                Ok((
                    row.get::<_, u32>(0)?,
                    row.get::<_, u8>(1)?,
                    row.get::<_, u32>(2)?,
                    row.get::<_, String>(3)?,
                ))
            },
        )
        .optional()
        .map_err(|_| BlackboxRepoError::Storage)?;
    let Some((scenario_id, difficulty, campaign_index, created_date)) = campaign else {
        return Ok(None);
    };
    let loaded = LoadedCampaign {
        campaign: CampaignRow {
            fingerprint,
            scenario_id,
            difficulty,
            campaign_index,
        },
        created_date,
    };

    let mut statement = connection
        .prepare(
            "SELECT tick, action_json FROM blackbox_decisions \
             WHERE campaign_fingerprint = ?1 ORDER BY seq ASC",
        )
        .map_err(|_| BlackboxRepoError::Storage)?;
    let rows = statement
        .query_map(params![&fingerprint[..]], |row| {
            Ok((row.get::<_, u32>(0)?, row.get::<_, String>(1)?))
        })
        .map_err(|_| BlackboxRepoError::Storage)?;
    let mut decisions = Vec::new();
    for row in rows {
        let (tick, action_json) = row.map_err(|_| BlackboxRepoError::Storage)?;
        let intent: ActionIntent =
            serde_json::from_str(&action_json).map_err(|_| BlackboxRepoError::Serialization)?;
        decisions.push(LoggedDecision { tick, intent });
    }
    Ok(Some((loaded, decisions)))
}

/// Decide-time digests for one campaign — the vault shape of a
/// [`crate::blackbox_sim::bridge::ReplayInput`]'s decision list.
#[derive(Debug, Clone)]
pub(crate) struct AnchoredCampaign {
    pub decisions: Vec<crate::blackbox_sim::bridge::AnchoredDecision>,
}

/// Load decisions with `state_digest` for estimator replay (Phase 6-A step 5).
///
/// Verifies `seq` is contiguous from 0 with no gaps; a hole is
/// [`BlackboxRepoError::CorruptRow`] (MalformedLog-equivalent at the storage
/// boundary). Does not judge seal status — that authority stays with
/// [`crate::blackbox_sim::bridge::replay_verified`].
pub(crate) fn load_anchored_campaign(
    connection: &Connection,
    fingerprint: [u8; 32],
) -> Result<Option<AnchoredCampaign>, BlackboxRepoError> {
    if load_campaign_and_decisions(connection, fingerprint)?.is_none() {
        return Ok(None);
    }

    let mut statement = connection
        .prepare(
            "SELECT seq, tick, action_json, state_digest FROM blackbox_decisions \
             WHERE campaign_fingerprint = ?1 ORDER BY seq ASC",
        )
        .map_err(|_| BlackboxRepoError::Storage)?;
    let rows = statement
        .query_map(params![&fingerprint[..]], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, u32>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, Vec<u8>>(3)?,
            ))
        })
        .map_err(|_| BlackboxRepoError::Storage)?;

    let mut decisions = Vec::new();
    for (expected_seq, row) in rows.enumerate() {
        let (seq_i, tick, action_json, digest_blob) =
            row.map_err(|_| BlackboxRepoError::Storage)?;
        let seq = u32_from_i64(seq_i)?;
        let expected = u32::try_from(expected_seq).map_err(|_| BlackboxRepoError::CorruptRow)?;
        if seq != expected {
            return Err(BlackboxRepoError::CorruptRow);
        }
        let intent: ActionIntent =
            serde_json::from_str(&action_json).map_err(|_| BlackboxRepoError::Serialization)?;
        let expected_digest = blob_to_array8(&digest_blob)?;
        decisions.push(crate::blackbox_sim::bridge::AnchoredDecision {
            tick,
            intent,
            expected_digest,
        });
    }

    Ok(Some(AnchoredCampaign { decisions }))
}

/// Newest-first campaigns whose decision count equals [`CAMPAIGN_TICKS`].
///
/// Efficiency pre-filter only (R-10): incomplete runs are excluded early, but
/// seal truth remains `replay_verified` → `CampaignNotSealed`.
pub(crate) fn list_full_decision_campaigns(
    connection: &Connection,
) -> Result<Vec<LoadedCampaign>, BlackboxRepoError> {
    use crate::blackbox_sim::director::CAMPAIGN_TICKS;

    let ticks = i64::from(CAMPAIGN_TICKS);
    let mut statement = connection
        .prepare(
            "SELECT c.campaign_fingerprint, c.scenario_id, c.difficulty, \
                    c.campaign_index, c.created_date \
             FROM blackbox_campaigns c \
             INNER JOIN ( \
                 SELECT campaign_fingerprint, COUNT(*) AS n \
                 FROM blackbox_decisions \
                 GROUP BY campaign_fingerprint \
                 HAVING n = ?1 \
             ) d ON d.campaign_fingerprint = c.campaign_fingerprint \
             ORDER BY c.started_at DESC, c.campaign_fingerprint DESC",
        )
        .map_err(|_| BlackboxRepoError::Storage)?;
    let rows = statement
        .query_map(params![ticks], |row| {
            Ok((
                row.get::<_, Vec<u8>>(0)?,
                row.get::<_, u32>(1)?,
                row.get::<_, u8>(2)?,
                row.get::<_, u32>(3)?,
                row.get::<_, String>(4)?,
            ))
        })
        .map_err(|_| BlackboxRepoError::Storage)?;

    let mut out = Vec::new();
    for row in rows {
        let (blob, scenario_id, difficulty, campaign_index, created_date) =
            row.map_err(|_| BlackboxRepoError::Storage)?;
        let fingerprint = blob_to_array32(&blob)?;
        out.push(LoadedCampaign {
            campaign: CampaignRow {
                fingerprint,
                scenario_id,
                difficulty,
                campaign_index,
            },
            created_date,
        });
    }
    Ok(out)
}

/// Ordered pool entry: one [`ReplayInput`] and the fingerprint that must appear
/// in `insert_profile`'s `sources` at the same index (W-26).
#[cfg(feature = "blackbox-profile-write")]
#[derive(Debug, Clone)]
pub(crate) struct PoolMember {
    pub input: crate::blackbox_sim::bridge::ReplayInput,
    pub fingerprint: [u8; 32],
}

/// Build the sealed pool (newest-first, ≤ [`MAX_POOLED_CAMPAIGNS`]) from vault
/// rows. Fingerprint nail-down is fail-closed; not-sealed logs are skipped.
#[cfg(feature = "blackbox-profile-write")]
pub(crate) fn compose_sealed_pool(
    connection: &Connection,
) -> Result<Vec<PoolMember>, ProfileWriteError> {
    use crate::blackbox_sim::bridge::{
        replay_verified, BridgeError, ReplayInput, MAX_POOLED_CAMPAIGNS,
    };
    use crate::blackbox_sim::director::Session;
    use crate::blackbox_sim::genesis::{Difficulty, GenesisRequest};

    let candidates = list_full_decision_campaigns(connection)?;
    let mut pool = Vec::new();

    for candidate in candidates {
        if pool.len() >= MAX_POOLED_CAMPAIGNS {
            break;
        }
        let difficulty = match candidate.campaign.difficulty {
            0 => Difficulty::Standard,
            1 => Difficulty::Hard,
            _ => return Err(ProfileWriteError::CorruptRow),
        };
        let request = GenesisRequest {
            scenario_id: candidate.campaign.scenario_id,
            difficulty,
            campaign_index: candidate.campaign.campaign_index,
            created_date: candidate.created_date.clone(),
        };
        // W-a nail: restored request must regenerate the stored fingerprint.
        let probe = Session::start(request.clone()).map_err(ProfileWriteError::from)?;
        if probe.campaign_fingerprint() != candidate.campaign.fingerprint {
            return Err(ProfileWriteError::FingerprintMismatch);
        }

        let Some(anchored) =
            load_anchored_campaign(connection, candidate.campaign.fingerprint)?
        else {
            continue;
        };
        let input = ReplayInput {
            request,
            decisions: anchored.decisions,
        };
        match replay_verified(&input, pool.len()) {
            Ok(_) => {
                pool.push(PoolMember {
                    input,
                    fingerprint: candidate.campaign.fingerprint,
                });
            }
            // Pre-filter is best-effort; seal authority may still refuse.
            Err(BridgeError::CampaignNotSealed { .. }) => continue,
            Err(other) => return Err(ProfileWriteError::Bridge(other)),
        }
    }

    Ok(pool)
}

/// Estimate from the sealed pool and persist in the caller's transaction.
///
/// Sources order is taken from the same [`PoolMember`] vec that fed
/// `estimate_pooled` (W-26). Caller owns commit/rollback (M-2).
#[cfg(feature = "blackbox-profile-write")]
pub(crate) fn estimate_and_insert_profile(
    transaction: &Transaction<'_>,
    now: i64,
) -> Result<[u8; 8], ProfileWriteError> {
    use crate::blackbox_sim::bridge::{estimate_pooled, BridgeError};

    let pool = compose_sealed_pool(transaction)?;
    if pool.is_empty() {
        return Err(ProfileWriteError::EmptyPool);
    }
    let inputs: Vec<_> = pool.iter().map(|m| m.input.clone()).collect();
    let sources: Vec<[u8; 32]> = pool.iter().map(|m| m.fingerprint).collect();
    let profile = match estimate_pooled(&inputs) {
        Ok(p) => p,
        Err(BridgeError::EmptyPool) => return Err(ProfileWriteError::EmptyPool),
        Err(e) => return Err(ProfileWriteError::Bridge(e)),
    };
    insert_profile(transaction, &profile, &sources, now)?;
    Ok(profile.campaign_digest)
}

/// Errors from the live profile-write path (step 5). Distinct from ordinary
/// record-lane [`BlackboxRepoError`] so the worker can map EmptyPool →
/// Unavailable without conflating storage faults.
#[cfg(feature = "blackbox-profile-write")]
#[derive(Debug)]
pub(crate) enum ProfileWriteError {
    EmptyPool,
    FingerprintMismatch,
    CorruptRow,
    Bridge(crate::blackbox_sim::bridge::BridgeError),
    Repo(BlackboxRepoError),
}

#[cfg(feature = "blackbox-profile-write")]
impl std::fmt::Display for ProfileWriteError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ProfileWriteError::EmptyPool => write!(f, "blackbox profile write: sealed pool is empty"),
            ProfileWriteError::FingerprintMismatch => {
                write!(f, "blackbox profile write: restored genesis fingerprint mismatch")
            }
            ProfileWriteError::CorruptRow => {
                write!(f, "blackbox profile write: campaign log row is corrupt")
            }
            ProfileWriteError::Bridge(e) => write!(f, "blackbox profile write: bridge: {e:?}"),
            ProfileWriteError::Repo(e) => write!(f, "blackbox profile write: repo: {e}"),
        }
    }
}

#[cfg(feature = "blackbox-profile-write")]
impl std::error::Error for ProfileWriteError {}

#[cfg(feature = "blackbox-profile-write")]
impl From<BlackboxRepoError> for ProfileWriteError {
    fn from(e: BlackboxRepoError) -> Self {
        match e {
            BlackboxRepoError::CorruptRow => ProfileWriteError::CorruptRow,
            other => ProfileWriteError::Repo(other),
        }
    }
}

#[cfg(feature = "blackbox-profile-write")]
impl From<crate::blackbox_sim::director::DirectorError> for ProfileWriteError {
    fn from(e: crate::blackbox_sim::director::DirectorError) -> Self {
        ProfileWriteError::Bridge(crate::blackbox_sim::bridge::BridgeError::Replay(e))
    }
}

/// Snapshot metadata for one pooled estimate (v13 profile lane).
///
/// Deliberately free of lane values and source fingerprints: those belong on
/// [`LoadedProfile`]. Listing is for provenance / UI chrome; reconstruction
/// of the estimate itself goes through [`get_latest_profile`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ProfileMeta {
    pub pool_digest: [u8; 8],
    pub schema_version: String,
    pub instrument: String,
    pub calibration: String,
    pub pooled_campaigns: u32,
    pub estimated_at: i64,
    pub updated_at: i64,
}

impl ProfileMeta {
    /// Construct from a stored row, fail-closing on any LAW-19 marker drift.
    ///
    /// Schema CHECKs prevent an honest writer from storing a calibrated claim,
    /// but a tampered DB is exactly the threat `CorruptRow` exists for — so
    /// every reader path ([`list_profiles`], [`get_latest_profile`]) goes
    /// through this constructor rather than trusting the TEXT columns.
    fn from_row(
        pool_digest: [u8; 8],
        schema_version: String,
        instrument: String,
        calibration: String,
        pooled_campaigns: u32,
        estimated_at: i64,
        updated_at: i64,
    ) -> Result<Self, BlackboxRepoError> {
        if schema_version != SCHEMA_BLACKBOX_PROFILE_V1
            || instrument != INSTRUMENT_ID
            || calibration != CALIBRATION_UNCALIBRATED
        {
            return Err(BlackboxRepoError::CorruptRow);
        }
        Ok(Self {
            pool_digest,
            schema_version,
            instrument,
            calibration,
            pooled_campaigns,
            estimated_at,
            updated_at,
        })
    }
}

/// Full reconstruction of a stored profile: axes + ordered pool sources.
///
/// `profile.campaign_digest` is the pool digest (W-26). Instrument and
/// calibration are duplicated on [`ProfileMeta`] so a reader can assert the
/// LAW-19 markers without reaching into constants — but the only values this
/// module ever writes are the sealed literals.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LoadedProfile {
    pub meta: ProfileMeta,
    pub profile: BlackboxProfile,
    /// Campaign fingerprints in pool-input order (`ordinal` ascending).
    pub sources: Vec<[u8; 32]>,
}

/// Persist one pooled estimate across the three v13 tables.
///
/// `sources` must be the exact ordered fingerprint list the bridge hashed into
/// `profile.campaign_digest` (W-26): a digest that does not recompute from
/// `sources` is refused as [`BlackboxRepoError::InvalidProfile`]. Each
/// fingerprint must already exist in `blackbox_campaigns` (FK). Re-inserting
/// the same digest replaces the prior snapshot (parent `INSERT OR REPLACE`
/// cascades children away).
///
/// Hard-codes `instrument` / `calibration` to the sealed literals — callers
/// cannot smuggle a calibrated marker through this API (LAW-19 / R-7).
///
/// **Transaction contract:** this function does not open a savepoint. A
/// mid-function failure can leave a half-written parent row inside the
/// caller's open transaction; the caller must roll the transaction back
/// (never commit) on any `Err`.
///
/// **Timestamps:** `estimated_at` and `updated_at` are written to the same
/// `now` value on every insert/replace. Under the snapshot policy a replace
/// is a new estimate of the same pool, so the two clocks intentionally
/// coincide — there is no "first estimated at" retained across replaces.
///
/// **R-8:** absent from binaries that do not enable `blackbox-profile-write`
/// (shape defence — the writer does not exist, not merely an `if` that skips).
///
/// Production caller: [`estimate_and_insert_profile`] ← vault
/// `blackbox_estimate_and_persist` ← `bxs_estimate_profile` (step 5).
#[cfg(feature = "blackbox-profile-write")]
pub(crate) fn insert_profile(
    transaction: &Transaction<'_>,
    profile: &BlackboxProfile,
    sources: &[[u8; 32]],
    now: i64,
) -> Result<(), BlackboxRepoError> {
    validate_profile_write(profile, sources)?;

    for fingerprint in sources {
        let exists: bool = transaction
            .query_row(
                "SELECT 1 FROM blackbox_campaigns WHERE campaign_fingerprint = ?1",
                params![&fingerprint[..]],
                |_| Ok(true),
            )
            .optional()
            .map_err(|_| BlackboxRepoError::Storage)?
            .unwrap_or(false);
        if !exists {
            return Err(BlackboxRepoError::MissingCampaign);
        }
    }

    let pooled = i64::try_from(sources.len()).map_err(|_| BlackboxRepoError::InvalidProfile)?;
    transaction
        .execute(
            "INSERT OR REPLACE INTO blackbox_profiles (\
                 pool_digest, schema_version, instrument, calibration, \
                 pooled_campaigns, estimated_at, updated_at\
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?6)",
            params![
                &profile.campaign_digest[..],
                SCHEMA_BLACKBOX_PROFILE_V1,
                INSTRUMENT_ID,
                CALIBRATION_UNCALIBRATED,
                pooled,
                now,
            ],
        )
        .map_err(|_| BlackboxRepoError::Storage)?;

    for axis in BiasAxis::ALL {
        let estimate = profile.axis(axis);
        transaction
            .execute(
                "INSERT INTO blackbox_profile_lanes (\
                     pool_digest, lane, value_micro, n_obs, sufficiency_micro\
                 ) VALUES (?1, ?2, ?3, ?4, ?5)",
                params![
                    &profile.campaign_digest[..],
                    i64::from(axis.lane()),
                    estimate.value_micro,
                    i64::from(estimate.n_obs),
                    estimate.sufficiency_micro,
                ],
            )
            .map_err(|_| BlackboxRepoError::Storage)?;
    }

    for (ordinal, fingerprint) in sources.iter().enumerate() {
        let ordinal_i = i64::try_from(ordinal).map_err(|_| BlackboxRepoError::InvalidProfile)?;
        transaction
            .execute(
                "INSERT INTO blackbox_profile_sources (\
                     pool_digest, ordinal, campaign_fingerprint\
                 ) VALUES (?1, ?2, ?3)",
                params![
                    &profile.campaign_digest[..],
                    ordinal_i,
                    &fingerprint[..],
                ],
            )
            .map_err(|_| BlackboxRepoError::Storage)?;
    }

    Ok(())
}

/// Newest profile by `(estimated_at DESC, pool_digest DESC)`, fully loaded.
///
/// R-9 outlets (consult / 講評 / PROFILE UI) must obtain rows only through
/// this function or [`list_profiles`] — never via ad-hoc SQL.
pub(crate) fn get_latest_profile(
    connection: &Connection,
) -> Result<Option<LoadedProfile>, BlackboxRepoError> {
    let meta = match load_latest_meta(connection)? {
        Some(m) => m,
        None => return Ok(None),
    };
    Ok(Some(load_profile_body(connection, meta)?))
}

/// All profile snapshots, newest first. Metadata only — no lanes / sources.
///
/// PROFILE UI listing (R-9) goes through [`crate::db::blackbox_profile_outlet`].
pub(crate) fn list_profiles(
    connection: &Connection,
) -> Result<Vec<ProfileMeta>, BlackboxRepoError> {
    let mut statement = connection
        .prepare(
            "SELECT pool_digest, schema_version, instrument, calibration, \
                    pooled_campaigns, estimated_at, updated_at \
             FROM blackbox_profiles \
             ORDER BY estimated_at DESC, pool_digest DESC",
        )
        .map_err(|_| BlackboxRepoError::Storage)?;
    let rows = statement
        .query_map([], |row| {
            Ok((
                row.get::<_, Vec<u8>>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, i64>(4)?,
                row.get::<_, i64>(5)?,
                row.get::<_, i64>(6)?,
            ))
        })
        .map_err(|_| BlackboxRepoError::Storage)?;
    let mut out = Vec::new();
    for row in rows {
        let (digest, schema, instrument, calibration, pooled, estimated_at, updated_at) =
            row.map_err(|_| BlackboxRepoError::Storage)?;
        out.push(ProfileMeta::from_row(
            blob_to_array8(&digest)?,
            schema,
            instrument,
            calibration,
            u32_from_i64(pooled)?,
            estimated_at,
            updated_at,
        )?);
    }
    Ok(out)
}

#[cfg(feature = "blackbox-profile-write")]
fn validate_profile_write(
    profile: &BlackboxProfile,
    sources: &[[u8; 32]],
) -> Result<(), BlackboxRepoError> {
    if profile.schema != SCHEMA_BLACKBOX_PROFILE_V1 {
        return Err(BlackboxRepoError::InvalidProfile);
    }
    if sources.is_empty() || sources.len() > MAX_POOLED_CAMPAIGNS {
        return Err(BlackboxRepoError::InvalidProfile);
    }
    // UNIQUE (pool_digest, campaign_fingerprint) — refuse duplicates in Rust.
    for (i, left) in sources.iter().enumerate() {
        for right in sources.iter().skip(i.saturating_add(1)) {
            if left == right {
                return Err(BlackboxRepoError::InvalidProfile);
            }
        }
    }
    for axis in BiasAxis::ALL {
        validate_estimate(&profile.axis(axis))?;
    }
    // W-26: the primary key is the content hash of `sources` in order.
    if pool_digest(sources) != profile.campaign_digest {
        return Err(BlackboxRepoError::InvalidProfile);
    }
    Ok(())
}

/// Mirror of the v13 lane CHECK: honest N/A or measured-with-observations.
fn validate_estimate(estimate: &BiasEstimate) -> Result<(), BlackboxRepoError> {
    if estimate.sufficiency_micro < 0 || estimate.sufficiency_micro > MICRO {
        return Err(BlackboxRepoError::InvalidProfile);
    }
    match estimate.value_micro {
        None => {
            if estimate.n_obs == 0 && estimate.sufficiency_micro == 0 {
                Ok(())
            } else {
                Err(BlackboxRepoError::InvalidProfile)
            }
        }
        Some(_) => {
            if estimate.n_obs > 0 {
                Ok(())
            } else {
                Err(BlackboxRepoError::InvalidProfile)
            }
        }
    }
}

fn load_latest_meta(connection: &Connection) -> Result<Option<ProfileMeta>, BlackboxRepoError> {
    // Named fields keep the SELECT shape readable without a 7-tuple clippy hit.
    struct MetaRow {
        pool_digest: Vec<u8>,
        schema_version: String,
        instrument: String,
        calibration: String,
        pooled_campaigns: i64,
        estimated_at: i64,
        updated_at: i64,
    }
    let row: Option<MetaRow> = connection
        .query_row(
            "SELECT pool_digest, schema_version, instrument, calibration, \
                    pooled_campaigns, estimated_at, updated_at \
             FROM blackbox_profiles \
             ORDER BY estimated_at DESC, pool_digest DESC \
             LIMIT 1",
            [],
            |row| {
                Ok(MetaRow {
                    pool_digest: row.get(0)?,
                    schema_version: row.get(1)?,
                    instrument: row.get(2)?,
                    calibration: row.get(3)?,
                    pooled_campaigns: row.get(4)?,
                    estimated_at: row.get(5)?,
                    updated_at: row.get(6)?,
                })
            },
        )
        .optional()
        .map_err(|_| BlackboxRepoError::Storage)?;
    let Some(row) = row else {
        return Ok(None);
    };
    Ok(Some(ProfileMeta::from_row(
        blob_to_array8(&row.pool_digest)?,
        row.schema_version,
        row.instrument,
        row.calibration,
        u32_from_i64(row.pooled_campaigns)?,
        row.estimated_at,
        row.updated_at,
    )?))
}

fn load_profile_body(
    connection: &Connection,
    meta: ProfileMeta,
) -> Result<LoadedProfile, BlackboxRepoError> {
    // Markers already verified by ProfileMeta::from_row.

    let n_axes_i64 = i64::try_from(N_AXES).map_err(|_| BlackboxRepoError::CorruptRow)?;
    let mut statement = connection
        .prepare(
            "SELECT lane, value_micro, n_obs, sufficiency_micro \
             FROM blackbox_profile_lanes WHERE pool_digest = ?1 ORDER BY lane ASC",
        )
        .map_err(|_| BlackboxRepoError::Storage)?;
    let rows = statement
        .query_map(params![&meta.pool_digest[..]], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, Option<i64>>(1)?,
                row.get::<_, i64>(2)?,
                row.get::<_, i64>(3)?,
            ))
        })
        .map_err(|_| BlackboxRepoError::Storage)?;

    let mut profile = BlackboxProfile::empty(meta.pool_digest);
    let mut seen = [false; N_AXES];
    for row in rows {
        let (lane_i, value_micro, n_obs_i, sufficiency_micro) =
            row.map_err(|_| BlackboxRepoError::Storage)?;
        if lane_i < 0 || lane_i >= n_axes_i64 {
            return Err(BlackboxRepoError::CorruptRow);
        }
        let lane = u8::try_from(lane_i).map_err(|_| BlackboxRepoError::CorruptRow)?;
        let axis = match BiasAxis::ALL.iter().copied().find(|a| a.lane() == lane) {
            Some(a) => a,
            None => return Err(BlackboxRepoError::CorruptRow),
        };
        let index = usize::from(axis.lane());
        let slot = match seen.get_mut(index) {
            Some(s) => s,
            None => return Err(BlackboxRepoError::CorruptRow),
        };
        if *slot {
            return Err(BlackboxRepoError::CorruptRow);
        }
        *slot = true;
        let n_obs = u32_from_i64(n_obs_i)?;
        let estimate = BiasEstimate {
            value_micro,
            n_obs,
            sufficiency_micro,
        };
        validate_estimate(&estimate).map_err(|_| BlackboxRepoError::CorruptRow)?;
        profile.set_axis(axis, estimate);
    }
    if seen.iter().any(|present| !*present) {
        return Err(BlackboxRepoError::CorruptRow);
    }

    let mut source_stmt = connection
        .prepare(
            "SELECT ordinal, campaign_fingerprint FROM blackbox_profile_sources \
             WHERE pool_digest = ?1 ORDER BY ordinal ASC",
        )
        .map_err(|_| BlackboxRepoError::Storage)?;
    let source_rows = source_stmt
        .query_map(params![&meta.pool_digest[..]], |row| {
            Ok((row.get::<_, i64>(0)?, row.get::<_, Vec<u8>>(1)?))
        })
        .map_err(|_| BlackboxRepoError::Storage)?;
    let mut sources = Vec::new();
    for (expected_ordinal, row) in source_rows.enumerate() {
        let (ordinal, blob) = row.map_err(|_| BlackboxRepoError::Storage)?;
        let expected_i =
            i64::try_from(expected_ordinal).map_err(|_| BlackboxRepoError::CorruptRow)?;
        if ordinal != expected_i {
            return Err(BlackboxRepoError::CorruptRow);
        }
        sources.push(blob_to_array32(&blob)?);
    }
    if sources.len() != meta.pooled_campaigns as usize {
        return Err(BlackboxRepoError::CorruptRow);
    }
    // W-26 at rest: stored digest must still be the content hash of sources.
    if pool_digest(&sources) != meta.pool_digest {
        return Err(BlackboxRepoError::CorruptRow);
    }

    Ok(LoadedProfile {
        meta,
        profile,
        sources,
    })
}

fn blob_to_array8(bytes: &[u8]) -> Result<[u8; 8], BlackboxRepoError> {
    if bytes.len() != 8 {
        return Err(BlackboxRepoError::CorruptRow);
    }
    let mut out = [0_u8; 8];
    for (dst, src) in out.iter_mut().zip(bytes.iter()) {
        *dst = *src;
    }
    Ok(out)
}

fn blob_to_array32(bytes: &[u8]) -> Result<[u8; 32], BlackboxRepoError> {
    if bytes.len() != 32 {
        return Err(BlackboxRepoError::CorruptRow);
    }
    let mut out = [0_u8; 32];
    for (dst, src) in out.iter_mut().zip(bytes.iter()) {
        *dst = *src;
    }
    Ok(out)
}

fn u32_from_i64(value: i64) -> Result<u32, BlackboxRepoError> {
    u32::try_from(value).map_err(|_| BlackboxRepoError::CorruptRow)
}

/// Binds a live transaction to the simulator's write-only port.
///
/// Short-lived by design: it borrows the transaction, so it cannot outlive the
/// unit of work and cannot be stashed somewhere that would let the simulator
/// keep a handle on the database.
///
/// `#[cfg(test)]` only: it requires `Session` and the `Transaction` on the
/// same call stack, which is true in this file's own integration tests but
/// never true in production — there, `Session` lives on the sim worker
/// thread and the transaction lives on the vault worker thread. The
/// production path (Phase 5, `blackbox_arena::handle`) calls
/// `upsert_campaign`/`persist_batch` directly from inside `db::worker`'s own
/// `write_repository` transaction instead of going through this adapter.
#[cfg(test)]
pub(crate) struct VaultDecisionSink<'a, 'conn> {
    transaction: &'a Transaction<'conn>,
    campaign: CampaignRow,
    now: i64,
}

#[cfg(test)]
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

#[cfg(test)]
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
            Err(BlackboxRepoError::Storage | BlackboxRepoError::CorruptRow) => {
                Err(SinkError::Unavailable)
            }
            Err(BlackboxRepoError::InvalidProfile) => Err(SinkError::Unavailable),
            #[cfg(feature = "blackbox-profile-write")]
            Err(BlackboxRepoError::MissingCampaign) => Err(SinkError::Unavailable),
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

    #[test]
    fn get_latest_profile_returns_none_on_an_empty_lane() -> Result<(), Box<dyn Error>> {
        let connection = migrated()?;
        assert_eq!(get_latest_profile(&connection)?, None);
        assert!(list_profiles(&connection)?.is_empty());
        Ok(())
    }

    /// Profile-lane write tests require `blackbox-profile-write` (R-8): the
    /// writer symbol is absent from binaries that do not enable the flag.
    #[cfg(feature = "blackbox-profile-write")]
    mod profile_write_tests {
        use super::*;
        use crate::blackbox_sim::bias::{
            BiasAxis, BiasEstimate, BlackboxProfile, CALIBRATION_UNCALIBRATED, INSTRUMENT_ID,
            N_AXES, SCHEMA_BLACKBOX_PROFILE_V1,
        };
        use crate::blackbox_sim::bridge::pool_digest;

        fn register_campaign(
            transaction: &Transaction<'_>,
            fingerprint: [u8; 32],
            campaign_index: u32,
            now: i64,
        ) -> Result<(), BlackboxRepoError> {
            upsert_campaign(
                transaction,
                &CampaignRow {
                    fingerprint,
                    scenario_id: 9,
                    difficulty: 0,
                    campaign_index,
                },
                "2026-07-28",
                now,
            )
        }

        fn sample_profile(digest: [u8; 8]) -> BlackboxProfile {
            let mut profile = BlackboxProfile::empty(digest);
            profile.set_axis(
                BiasAxis::LossAversion,
                BiasEstimate {
                    value_micro: Some(2_000_000),
                    n_obs: 12,
                    sufficiency_micro: 750_000,
                },
            );
            profile.set_axis(
                BiasAxis::Anchoring,
                BiasEstimate {
                    value_micro: Some(500_000),
                    n_obs: 8,
                    sufficiency_micro: 400_000,
                },
            );
            profile
        }

        #[test]
        fn a_profile_round_trips_through_the_three_v13_tables() -> Result<(), Box<dyn Error>> {
            let fp_a = [0x11; 32];
            let fp_b = [0x22; 32];
            let digest = pool_digest(&[fp_a, fp_b]);
            let profile = sample_profile(digest);

            let mut connection = migrated()?;
            {
                let transaction = connection.transaction()?;
                register_campaign(&transaction, fp_a, 0, 1_000)?;
                register_campaign(&transaction, fp_b, 1, 1_000)?;
                insert_profile(&transaction, &profile, &[fp_a, fp_b], 2_000)?;
                transaction.commit()?;
            }

            let loaded = match get_latest_profile(&connection)? {
                Some(p) => p,
                None => unreachable!("profile was just inserted"),
            };
            assert_eq!(loaded.meta.pool_digest, digest);
            assert_eq!(loaded.meta.schema_version, SCHEMA_BLACKBOX_PROFILE_V1);
            assert_eq!(loaded.meta.instrument, INSTRUMENT_ID);
            assert_eq!(loaded.meta.calibration, CALIBRATION_UNCALIBRATED);
            assert_eq!(loaded.meta.pooled_campaigns, 2);
            assert_eq!(loaded.meta.estimated_at, 2_000);
            assert_eq!(loaded.meta.updated_at, 2_000);
            assert_eq!(loaded.sources, vec![fp_a, fp_b]);
            assert_eq!(loaded.profile, profile);
            for axis in BiasAxis::ALL {
                assert_eq!(loaded.profile.axis(axis), profile.axis(axis));
            }

            let listed = list_profiles(&connection)?;
            assert_eq!(listed.len(), 1);
            assert_eq!(listed[0], loaded.meta);
            Ok(())
        }

        #[test]
        fn reinserting_the_same_digest_replaces_the_snapshot() -> Result<(), Box<dyn Error>> {
            let fp = [0x33; 32];
            let digest = pool_digest(&[fp]);
            let first = sample_profile(digest);
            let mut second = BlackboxProfile::empty(digest);
            second.set_axis(
                BiasAxis::Overconfidence,
                BiasEstimate {
                    value_micro: Some(300_000),
                    n_obs: 9,
                    sufficiency_micro: 500_000,
                },
            );

            let mut connection = migrated()?;
            {
                let transaction = connection.transaction()?;
                register_campaign(&transaction, fp, 0, 1)?;
                insert_profile(&transaction, &first, &[fp], 10)?;
                insert_profile(&transaction, &second, &[fp], 20)?;
                transaction.commit()?;
            }

            let loaded = match get_latest_profile(&connection)? {
                Some(p) => p,
                None => unreachable!("profile missing after replace"),
            };
            assert_eq!(loaded.meta.estimated_at, 20);
            assert_eq!(
                loaded.profile.axis(BiasAxis::Overconfidence).value_micro,
                Some(300_000)
            );
            assert_eq!(
                loaded.profile.axis(BiasAxis::LossAversion),
                BiasEstimate::NOT_MEASURED
            );
            assert_eq!(first.axis(BiasAxis::LossAversion).value_micro, Some(2_000_000));

            let count: i64 =
                connection.query_row("SELECT count(*) FROM blackbox_profiles", [], |r| r.get(0))?;
            assert_eq!(count, 1, "same digest must not accumulate rows");
            let lanes: i64 = connection.query_row(
                "SELECT count(*) FROM blackbox_profile_lanes WHERE pool_digest = ?1",
                params![&digest[..]],
                |r| r.get(0),
            )?;
            assert_eq!(lanes, i64::try_from(N_AXES)?);
            Ok(())
        }

        #[test]
        fn list_profiles_orders_newest_first() -> Result<(), Box<dyn Error>> {
            let fp = [0x44; 32];
            let older_digest = pool_digest(&[fp]);
            let fp_b = [0x45; 32];
            let newer_digest = pool_digest(&[fp_b]);
            let older = sample_profile(older_digest);
            let newer = sample_profile(newer_digest);

            let mut connection = migrated()?;
            {
                let transaction = connection.transaction()?;
                register_campaign(&transaction, fp, 0, 1)?;
                register_campaign(&transaction, fp_b, 1, 1)?;
                insert_profile(&transaction, &older, &[fp], 100)?;
                insert_profile(&transaction, &newer, &[fp_b], 200)?;
                transaction.commit()?;
            }

            let listed = list_profiles(&connection)?;
            assert_eq!(listed.len(), 2);
            assert_eq!(listed[0].pool_digest, newer_digest);
            assert_eq!(listed[0].estimated_at, 200);
            assert_eq!(listed[1].pool_digest, older_digest);
            assert_eq!(listed[1].estimated_at, 100);

            let latest = match get_latest_profile(&connection)? {
                Some(p) => p,
                None => unreachable!("expected a latest profile"),
            };
            assert_eq!(latest.meta.pool_digest, newer_digest);
            Ok(())
        }

        #[test]
        fn insert_profile_refuses_an_unregistered_source_campaign() -> Result<(), Box<dyn Error>> {
            let mut connection = migrated()?;
            let transaction = connection.transaction()?;
            let fp = [0x99; 32];
            let profile = sample_profile(pool_digest(&[fp]));
            assert_eq!(
                insert_profile(&transaction, &profile, &[fp], 1),
                Err(BlackboxRepoError::MissingCampaign)
            );
            let count: i64 =
                transaction.query_row("SELECT count(*) FROM blackbox_profiles", [], |r| r.get(0))?;
            assert_eq!(count, 0, "a refused write must leave the lane empty");
            Ok(())
        }

        #[test]
        fn insert_profile_refuses_a_fabricated_estimate() -> Result<(), Box<dyn Error>> {
            let fp = [0x66; 32];
            let mut connection = migrated()?;
            let transaction = connection.transaction()?;
            register_campaign(&transaction, fp, 0, 1)?;

            let mut fabricated = BlackboxProfile::empty(pool_digest(&[fp]));
            fabricated.set_axis(
                BiasAxis::DispositionEffect,
                BiasEstimate {
                    value_micro: Some(0),
                    n_obs: 0,
                    sufficiency_micro: 0,
                },
            );
            assert_eq!(
                insert_profile(&transaction, &fabricated, &[fp], 1),
                Err(BlackboxRepoError::InvalidProfile)
            );

            let mut half = BlackboxProfile::empty(pool_digest(&[fp]));
            half.set_axis(
                BiasAxis::DispositionEffect,
                BiasEstimate {
                    value_micro: None,
                    n_obs: 5,
                    sufficiency_micro: 100_000,
                },
            );
            assert_eq!(
                insert_profile(&transaction, &half, &[fp], 1),
                Err(BlackboxRepoError::InvalidProfile)
            );

            let ok_profile = sample_profile(pool_digest(&[fp]));
            assert_eq!(
                insert_profile(&transaction, &ok_profile, &[], 1),
                Err(BlackboxRepoError::InvalidProfile)
            );
            assert_eq!(
                insert_profile(&transaction, &ok_profile, &[fp, fp], 1),
                Err(BlackboxRepoError::InvalidProfile)
            );
            Ok(())
        }

        #[test]
        fn insert_profile_refuses_a_digest_that_does_not_match_sources() -> Result<(), Box<dyn Error>>
        {
            let fp_a = [0x71; 32];
            let fp_b = [0x72; 32];
            let mut connection = migrated()?;
            let transaction = connection.transaction()?;
            register_campaign(&transaction, fp_a, 0, 1)?;
            register_campaign(&transaction, fp_b, 1, 1)?;
            let forged = sample_profile([0xAB; 8]);
            assert_eq!(
                insert_profile(&transaction, &forged, &[fp_a, fp_b], 1),
                Err(BlackboxRepoError::InvalidProfile)
            );
            let count: i64 =
                transaction.query_row("SELECT count(*) FROM blackbox_profiles", [], |r| r.get(0))?;
            assert_eq!(count, 0);
            Ok(())
        }

        #[test]
        fn swapping_source_ordinals_fail_closes_on_read() -> Result<(), Box<dyn Error>> {
            let fp_a = [0x81; 32];
            let fp_b = [0x82; 32];
            let digest = pool_digest(&[fp_a, fp_b]);
            let profile = sample_profile(digest);

            let mut connection = migrated()?;
            {
                let transaction = connection.transaction()?;
                register_campaign(&transaction, fp_a, 0, 1)?;
                register_campaign(&transaction, fp_b, 1, 1)?;
                insert_profile(&transaction, &profile, &[fp_a, fp_b], 1)?;
                transaction.commit()?;
            }
            connection.execute(
                "UPDATE blackbox_profile_sources SET ordinal = 30 \
                 WHERE pool_digest = ?1 AND ordinal = 0",
                params![&digest[..]],
            )?;
            connection.execute(
                "UPDATE blackbox_profile_sources SET ordinal = 0 \
                 WHERE pool_digest = ?1 AND ordinal = 1",
                params![&digest[..]],
            )?;
            connection.execute(
                "UPDATE blackbox_profile_sources SET ordinal = 1 \
                 WHERE pool_digest = ?1 AND ordinal = 30",
                params![&digest[..]],
            )?;

            assert_eq!(
                get_latest_profile(&connection),
                Err(BlackboxRepoError::CorruptRow)
            );
            Ok(())
        }

        #[test]
        fn list_profiles_fail_closes_on_a_tampered_calibration_marker() -> Result<(), Box<dyn Error>>
        {
            let fp = [0x91; 32];
            let digest = pool_digest(&[fp]);
            let profile = sample_profile(digest);

            let mut connection = migrated()?;
            {
                let transaction = connection.transaction()?;
                register_campaign(&transaction, fp, 0, 1)?;
                insert_profile(&transaction, &profile, &[fp], 1)?;
                transaction.commit()?;
            }
            connection.execute_batch(
                "PRAGMA foreign_keys = OFF;
                 CREATE TABLE blackbox_profiles_tampered (
                     pool_digest BLOB NOT NULL PRIMARY KEY,
                     schema_version TEXT NOT NULL,
                     instrument TEXT NOT NULL,
                     calibration TEXT NOT NULL,
                     pooled_campaigns INTEGER NOT NULL,
                     estimated_at INTEGER NOT NULL,
                     updated_at INTEGER NOT NULL
                 );
                 INSERT INTO blackbox_profiles_tampered
                     SELECT pool_digest, schema_version, instrument, 'calibrated',
                            pooled_campaigns, estimated_at, updated_at
                     FROM blackbox_profiles;
                 DROP TABLE blackbox_profiles;
                 ALTER TABLE blackbox_profiles_tampered RENAME TO blackbox_profiles;
                 PRAGMA foreign_keys = ON;",
            )?;

            assert_eq!(
                list_profiles(&connection),
                Err(BlackboxRepoError::CorruptRow)
            );
            assert_eq!(
                get_latest_profile(&connection),
                Err(BlackboxRepoError::CorruptRow)
            );
            Ok(())
        }

        /// H-1: W-a nail must fail-close the whole compose, not skip one row.
        #[test]
        fn tampered_campaign_identity_aborts_compose_with_fingerprint_mismatch()
        -> Result<(), Box<dyn Error>> {
            let mut connection = migrated()?;
            let fp = flush_play(&mut connection, 7, CAMPAIGN_TICKS, 1_000)?;
            // Append-only contract covers decisions/stimuli only — campaign
            // identity columns may be UPDATE'd in a test to forge a mismatch.
            connection.execute(
                "UPDATE blackbox_campaigns SET created_date = ?1 \
                 WHERE campaign_fingerprint = ?2",
                params!["2099-01-01", &fp[..]],
            )?;
            match compose_sealed_pool(&connection) {
                Err(ProfileWriteError::FingerprintMismatch) => Ok(()),
                other => Err(format!("expected FingerprintMismatch, got {other:?}").into()),
            }
        }

        fn play_indexed(campaign_index: u32, turns: u32) -> Session {
            let mut session = match Session::start(GenesisRequest {
                scenario_id: 3,
                difficulty: Difficulty::Standard,
                campaign_index,
                created_date: "2026-07-27".to_string(),
            }) {
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

        fn flush_play(
            connection: &mut Connection,
            campaign_index: u32,
            turns: u32,
            now: i64,
        ) -> Result<[u8; 32], Box<dyn Error>> {
            let mut session = play_indexed(campaign_index, turns);
            let campaign = CampaignRow {
                fingerprint: session.campaign_fingerprint(),
                scenario_id: 3,
                difficulty: 0,
                campaign_index,
            };
            let fingerprint = campaign.fingerprint;
            let transaction = connection.transaction()?;
            upsert_campaign(&transaction, &campaign, "2026-07-27", now)?;
            {
                let mut sink = VaultDecisionSink::new(&transaction, campaign, now);
                session
                    .flush_to(&mut sink)
                    .map_err(|e| format!("flush: {e:?}"))?;
            }
            transaction.commit()?;
            Ok(fingerprint)
        }

        #[test]
        fn live_write_round_trips_a_sealed_campaign() -> Result<(), Box<dyn Error>> {
            let mut connection = migrated()?;
            let fp = flush_play(&mut connection, 1, CAMPAIGN_TICKS, 1_000)?;
            let digest = {
                let transaction = connection.transaction()?;
                let d = estimate_and_insert_profile(&transaction, 2_000)?;
                transaction.commit()?;
                d
            };
            let loaded = match get_latest_profile(&connection)? {
                Some(p) => p,
                None => unreachable!("profile missing after live write"),
            };
            assert_eq!(loaded.meta.pool_digest, digest);
            assert_eq!(loaded.sources, vec![fp]);
            assert_eq!(pool_digest(&loaded.sources), digest);
            Ok(())
        }

        #[test]
        fn interrupted_campaigns_are_excluded_from_the_pool() -> Result<(), Box<dyn Error>> {
            let mut connection = migrated()?;
            let _partial = flush_play(&mut connection, 1, 10, 100)?;
            let sealed = flush_play(&mut connection, 2, CAMPAIGN_TICKS, 200)?;
            let listed = list_full_decision_campaigns(&connection)?;
            assert_eq!(listed.len(), 1);
            assert_eq!(listed[0].campaign.fingerprint, sealed);

            let pool = compose_sealed_pool(&connection)?;
            assert_eq!(pool.len(), 1);
            assert_eq!(pool[0].fingerprint, sealed);
            Ok(())
        }

        #[test]
        fn pool_truncates_to_max_pooled_newest_first() -> Result<(), Box<dyn Error>> {
            use crate::blackbox_sim::bridge::MAX_POOLED_CAMPAIGNS;

            let mut connection = migrated()?;
            let mut fingerprints = Vec::new();
            // MAX+1 sealed campaigns; later `now` = newer.
            for i in 0..=MAX_POOLED_CAMPAIGNS as u32 {
                let fp = flush_play(
                    &mut connection,
                    i.saturating_add(1),
                    CAMPAIGN_TICKS,
                    i64::from(i).saturating_add(1),
                )?;
                fingerprints.push(fp);
            }
            let listed = list_full_decision_campaigns(&connection)?;
            assert_eq!(listed.len(), MAX_POOLED_CAMPAIGNS + 1);
            assert_eq!(
                listed[0].campaign.fingerprint,
                fingerprints[MAX_POOLED_CAMPAIGNS],
                "newest started_at must lead"
            );

            let pool = compose_sealed_pool(&connection)?;
            assert_eq!(pool.len(), MAX_POOLED_CAMPAIGNS);
            assert_eq!(
                pool[0].fingerprint,
                fingerprints[MAX_POOLED_CAMPAIGNS],
                "compose must keep newest-first order"
            );
            assert!(
                !pool
                    .iter()
                    .any(|m| m.fingerprint == fingerprints[0]),
                "oldest of MAX+1 must be truncated away"
            );
            Ok(())
        }
    }
}
