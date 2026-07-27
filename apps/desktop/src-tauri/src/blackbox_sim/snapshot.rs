//! State digests and generation snapshots (SPEC §3.4, §13, §16.3; BXS-I-01).
//!
//! # What the digest must cover
//!
//! The replay identity is `digest(replay(Genesis, log)) == digest(live)`. That
//! statement is only as strong as the digest is total. A digest over the
//! visible numbers alone would pass while the two runs sat at different
//! positions in the entropy stream — identical today, divergent on the next
//! tick, and the failure would surface many ticks later with no trace of its
//! cause. So [`StateDigest`] covers the ledger, the firm, the kernel's
//! continuous state AND the exact offset of every random stream.
//!
//! What it deliberately excludes is just as important: **latency, wall-clock
//! time, and the contents of the market history ring**. Latency is observed,
//! never causal (BXS-I-01, trap BXS-W-01); including it would make replay
//! depend on how fast a human clicked. The history ring is a display cache
//! that evicts, so hashing it would make the digest depend on how long a
//! session had been running rather than on what is true.
//!
//! # Generations
//!
//! Snapshots are append-only and capped at [`MAX_GENERATIONS`]; the oldest is
//! dropped when the cap is reached, which is the one place in this simulator
//! where dropping data is correct (they are redundant checkpoints, not
//! records — contrast the journal and event log, which reject instead).
//!
//! Loading is fail-closed: a snapshot whose stored digest does not match its
//! recomputed digest is REFUSED. There is no silent fallback to an older
//! generation, because a player who is quietly rolled back to an earlier
//! financial position has been lied to about their own history (§16.3).

use serde::Serialize;
use sha2::{Digest, Sha256};

use super::firm::FirmState;
use super::ledger::{AccountCode, Balances};
use super::market::MarketKernel;

/// Retained checkpoints. Eight quarters of rollback is generous for a campaign
/// and keeps the resident cost inside the §13 budget.
pub const MAX_GENERATIONS: usize = 8;

/// Per-generation serialization ceiling (SPEC §13).
pub const MAX_SNAPSHOT_BYTES: usize = 64 * 1024;

pub const SNAPSHOT_SCHEMA: &str = "bxs.snapshot.v1";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SnapshotError {
    Serialization,
    /// The payload does not hash to the id it was stored under (§16.3:
    /// pointer and payload are bound, and a mismatch is fatal, not a hint).
    DigestMismatch,
    TooLarge { bytes: usize },
    NoGenerations,
    UnknownGeneration { generation: u32 },
}

/// A total, order-independent-free fingerprint of the authoritative state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
pub struct StateDigest(pub [u8; 32]);

impl StateDigest {
    /// The 8-byte prefix carried on every `DecisionRecord`. A cross-reference
    /// for the estimators, not a security boundary — do not treat a short
    /// digest match as proof of equality anywhere.
    #[must_use]
    pub fn short(&self) -> [u8; 8] {
        let mut out = [0_u8; 8];
        for (dst, src) in out.iter_mut().zip(self.0.iter()) {
            *dst = *src;
        }
        out
    }
}

/// Hash the full authoritative state.
///
/// Field order is part of the contract: append new sections at the end and
/// never reorder, exactly as with the genesis draw stream.
pub fn digest_state(
    kernel: &MarketKernel,
    balances: &Balances,
    firm: &FirmState,
) -> Result<StateDigest, SnapshotError> {
    let mut h = Sha256::new();
    h.update(SNAPSHOT_SCHEMA.as_bytes());
    h.update(kernel.tick().to_le_bytes());
    h.update([kernel.regime() as u8]);
    for word in kernel.raw_state_bits() {
        h.update(word.to_le_bytes());
    }
    for word in kernel.rng_position_bits() {
        h.update(word.to_le_bytes());
    }
    // Iterate the frozen account order rather than a map, so the digest cannot
    // depend on hash-map iteration order.
    for account in AccountCode::ALL {
        h.update((account as u8).to_le_bytes());
        h.update(balances.balance_minor(account).to_le_bytes());
    }
    let firm_json = serde_json::to_vec(firm).map_err(|_| SnapshotError::Serialization)?;
    h.update((firm_json.len() as u64).to_le_bytes());
    h.update(&firm_json);
    Ok(StateDigest(h.finalize().into()))
}

/// A checkpoint taken at a period close.
///
/// Two digests, doing two different jobs. `state_digest` is the replay anchor:
/// it fingerprints the live state and is what a replayed run must reproduce.
/// `payload_digest` binds this record to its own bytes, so corruption of the
/// stored blob is detectable without reconstructing anything (§16.3 — the
/// pointer and the payload are one unit). Collapsing them into one field would
/// mean a tampered payload still "matched" its record.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Generation {
    pub schema: &'static str,
    pub generation: u32,
    pub tick: u32,
    pub state_digest: StateDigest,
    pub payload_digest: StateDigest,
    /// Serialized payload. Kept as bytes so the store enforces the size budget
    /// on the thing that is actually persisted, not on a guess.
    #[serde(skip)]
    pub payload: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
struct SnapshotPayload<'a> {
    tick: u32,
    regime: u8,
    market_state: [u64; 4],
    rng_position: [u64; 8],
    balances: Vec<(u8, i64)>,
    firm: &'a FirmState,
}

/// Bounded, append-only ring of checkpoints.
#[derive(Debug, Clone, Default)]
pub struct GenerationStore {
    generations: Vec<Generation>,
    next_generation: u32,
}

impl GenerationStore {
    #[must_use]
    pub fn new() -> Self {
        Self {
            generations: Vec::new(),
            next_generation: 0,
        }
    }

    /// Capture the current state. The digest is computed from the live state
    /// and the payload is built from the same values, so a later load can
    /// re-derive the digest and detect any corruption in between.
    pub fn capture(
        &mut self,
        kernel: &MarketKernel,
        balances: &Balances,
        firm: &FirmState,
    ) -> Result<u32, SnapshotError> {
        let digest = digest_state(kernel, balances, firm)?;
        let payload = SnapshotPayload {
            tick: kernel.tick(),
            regime: kernel.regime() as u8,
            market_state: kernel.raw_state_bits(),
            rng_position: kernel.rng_position_bits(),
            balances: AccountCode::ALL
                .iter()
                .map(|a| (*a as u8, balances.balance_minor(*a)))
                .collect(),
            firm,
        };
        let bytes = serde_json::to_vec(&payload).map_err(|_| SnapshotError::Serialization)?;
        if bytes.len() > MAX_SNAPSHOT_BYTES {
            return Err(SnapshotError::TooLarge { bytes: bytes.len() });
        }
        let generation = self.next_generation;
        self.next_generation = self.next_generation.saturating_add(1);
        // Checkpoints are redundant by construction, so evicting the oldest is
        // safe here in a way it never is for the journal or the event log.
        if self.generations.len() == MAX_GENERATIONS {
            self.generations.remove(0);
        }
        self.generations.push(Generation {
            schema: SNAPSHOT_SCHEMA,
            generation,
            tick: kernel.tick(),
            state_digest: digest,
            payload_digest: payload_binding(kernel.tick(), &bytes),
            payload: bytes,
        });
        Ok(generation)
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.generations.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.generations.is_empty()
    }

    #[must_use]
    pub fn resident_bytes(&self) -> usize {
        self.generations
            .iter()
            .map(|g| g.payload.len())
            .fold(0, usize::saturating_add)
    }

    pub fn latest(&self) -> Result<&Generation, SnapshotError> {
        self.generations.last().ok_or(SnapshotError::NoGenerations)
    }

    pub fn get(&self, generation: u32) -> Result<&Generation, SnapshotError> {
        self.generations
            .iter()
            .find(|g| g.generation == generation)
            .ok_or(SnapshotError::UnknownGeneration { generation })
    }

    /// Re-bind a checkpoint's pointer to its payload. A caller must run this
    /// before trusting a loaded generation; failure is terminal for that
    /// generation and MUST NOT fall through to an older one.
    pub fn verify(&self, generation: u32) -> Result<(), SnapshotError> {
        let g = self.get(generation)?;
        if payload_binding(g.tick, &g.payload) == g.payload_digest {
            Ok(())
        } else {
            Err(SnapshotError::DigestMismatch)
        }
    }
}

fn payload_binding(tick: u32, payload: &[u8]) -> StateDigest {
    let mut h = Sha256::new();
    h.update(SNAPSHOT_SCHEMA.as_bytes());
    h.update(tick.to_le_bytes());
    h.update(payload);
    StateDigest(h.finalize().into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::blackbox_sim::action::{execute_intent, ActionContext, SimBooks};
    use crate::blackbox_sim::genesis::{build_campaign_genesis, Difficulty, GenesisRequest};
    use crate::blackbox_sim::telemetry::ActionIntent;

    fn ok<T, E: std::fmt::Debug>(r: Result<T, E>) -> T {
        match r {
            Ok(v) => v,
            Err(e) => unreachable!("snapshot test setup failed: {e:?}"),
        }
    }

    fn fixture() -> (ActionContext, SimBooks, MarketKernel) {
        let g = ok(build_campaign_genesis(GenesisRequest {
            scenario_id: 11,
            difficulty: Difficulty::Standard,
            campaign_index: 0,
            created_date: "2026-07-27".to_string(),
        }));
        let mut kernel = ok(MarketKernel::new(&g));
        let market = ok(kernel.step());
        let mut books = ok(SimBooks::new(&g.firm));
        ok(books.seed_capital(g.firm.opening_capital_minor));
        (
            ActionContext {
                market,
                firm_params: g.firm,
            },
            books,
            kernel,
        )
    }

    #[test]
    fn identical_states_digest_identically() {
        let (_ctx, books, kernel) = fixture();
        let a = ok(digest_state(&kernel, &books.balances, &books.firm));
        let b = ok(digest_state(&kernel, &books.balances, &books.firm));
        assert_eq!(a, b);
    }

    #[test]
    fn a_ledger_change_changes_the_digest() {
        let (ctx, mut books, kernel) = fixture();
        let before = ok(digest_state(&kernel, &books.balances, &books.firm));
        let _ = ok(execute_intent(
            ActionIntent::Borrow {
                facility: 0,
                amount_minor: 1,
            },
            &ctx,
            &mut books,
        ));
        let after = ok(digest_state(&kernel, &books.balances, &books.firm));
        assert_ne!(before, after);
    }

    #[test]
    fn a_firm_only_change_changes_the_digest() {
        let (ctx, mut books, kernel) = fixture();
        let before = ok(digest_state(&kernel, &books.balances, &books.firm));
        let _ = ok(execute_intent(
            ActionIntent::SetPrice {
                sku: 0,
                tick_price: 1_234,
            },
            &ctx,
            &mut books,
        ));
        let after = ok(digest_state(&kernel, &books.balances, &books.firm));
        assert_ne!(
            before, after,
            "state outside the ledger is still state; a digest that ignores \
             it would let replay diverge undetected"
        );
    }

    /// The reason `rng_position_bits` exists. Two kernels can agree on every
    /// visible number and still be different states.
    #[test]
    fn stream_position_is_part_of_the_state() {
        let g = ok(build_campaign_genesis(GenesisRequest {
            scenario_id: 11,
            difficulty: Difficulty::Standard,
            campaign_index: 0,
            created_date: "2026-07-27".to_string(),
        }));
        let mut a = ok(MarketKernel::new(&g));
        let mut b = ok(MarketKernel::new(&g));
        let _ = ok(a.step());
        let _ = ok(b.step());
        assert_eq!(a.raw_state_bits(), b.raw_state_bits());
        assert_eq!(a.rng_position_bits(), b.rng_position_bits());
        let _ = ok(b.step());
        assert_ne!(a.rng_position_bits(), b.rng_position_bits());
    }

    #[test]
    fn a_captured_generation_verifies() {
        let (_ctx, books, kernel) = fixture();
        let mut store = GenerationStore::new();
        let id = ok(store.capture(&kernel, &books.balances, &books.firm));
        ok(store.verify(id));
        assert_eq!(ok(store.latest()).generation, id);
        assert_eq!(
            ok(store.latest()).state_digest,
            ok(digest_state(&kernel, &books.balances, &books.firm))
        );
    }

    #[test]
    fn tampering_with_a_payload_is_caught() {
        let (_ctx, books, kernel) = fixture();
        let mut store = GenerationStore::new();
        let id = ok(store.capture(&kernel, &books.balances, &books.firm));
        ok(store.verify(id));
        match store.generations.first_mut() {
            Some(g) => g.payload.push(b' '),
            None => unreachable!("a generation was just captured"),
        }
        assert!(
            matches!(store.verify(id), Err(SnapshotError::DigestMismatch)),
            "a corrupted payload must be refused, never loaded"
        );
    }

    /// §16.3: a broken latest generation must NOT silently resolve to an older
    /// one. Rolling a player back without telling them is the failure mode.
    #[test]
    fn a_broken_latest_does_not_fall_back_to_an_older_one() {
        let (ctx, mut books, kernel) = fixture();
        let mut store = GenerationStore::new();
        let first = ok(store.capture(&kernel, &books.balances, &books.firm));
        let _ = ok(execute_intent(
            ActionIntent::Borrow {
                facility: 0,
                amount_minor: 5_000,
            },
            &ctx,
            &mut books,
        ));
        let second = ok(store.capture(&kernel, &books.balances, &books.firm));
        match store.generations.last_mut() {
            Some(g) => g.payload.clear(),
            None => unreachable!("two generations were just captured"),
        }
        assert!(matches!(
            store.verify(second),
            Err(SnapshotError::DigestMismatch)
        ));
        // The older one is still intact and still addressable BY NAME — the
        // point is that recovering to it must be an explicit act.
        ok(store.verify(first));
        assert_ne!(
            ok(store.get(first)).state_digest,
            ok(store.get(second)).state_digest
        );
    }

    #[test]
    fn the_store_keeps_the_newest_generations_only() {
        let (ctx, mut books, kernel) = fixture();
        let mut store = GenerationStore::new();
        for _ in 0..(MAX_GENERATIONS + 3) {
            let _ = ok(execute_intent(
                ActionIntent::Borrow {
                    facility: 0,
                    amount_minor: 1_000,
                },
                &ctx,
                &mut books,
            ));
            let _ = ok(store.capture(&kernel, &books.balances, &books.firm));
        }
        assert_eq!(store.len(), MAX_GENERATIONS);
        let expected_latest = ok(u32::try_from(MAX_GENERATIONS + 2));
        assert_eq!(ok(store.latest()).generation, expected_latest);
        // The evicted ones are gone by name, not silently aliased to another.
        assert!(matches!(
            store.get(0),
            Err(SnapshotError::UnknownGeneration { generation: 0 })
        ));
    }

    #[test]
    fn an_empty_store_refuses_rather_than_inventing_a_generation() {
        let store = GenerationStore::new();
        assert!(matches!(store.latest(), Err(SnapshotError::NoGenerations)));
        assert!(matches!(
            store.get(0),
            Err(SnapshotError::UnknownGeneration { generation: 0 })
        ));
    }

    #[test]
    fn generations_stay_inside_the_size_budget() {
        let (_ctx, books, kernel) = fixture();
        let mut store = GenerationStore::new();
        let id = ok(store.capture(&kernel, &books.balances, &books.firm));
        let g = ok(store.get(id));
        assert!(
            g.payload.len() <= MAX_SNAPSHOT_BYTES,
            "generation payload is {} bytes",
            g.payload.len()
        );
        for _ in 0..MAX_GENERATIONS {
            let _ = ok(store.capture(&kernel, &books.balances, &books.firm));
        }
        assert!(
            store.resident_bytes() <= MAX_GENERATIONS * MAX_SNAPSHOT_BYTES,
            "generation store is over budget at {} bytes",
            store.resident_bytes()
        );
    }
}
