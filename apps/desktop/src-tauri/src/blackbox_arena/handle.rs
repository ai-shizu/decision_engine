//! `BlackboxSimHandle` — the sim's dedicated worker thread (SPEC §13's "SimCore
//! is owned by a dedicated worker thread", mirroring `db::worker::VaultHandle`
//! and `llm::LlmHandle`).
//!
//! This is the ONE module besides `db::blackbox_repo` allowed to import both
//! `blackbox_sim` and `db` — see the module header on `db::blackbox_repo` for
//! why that asymmetry is wall W-b. `blackbox_sim` itself gains zero new
//! imports from this phase.
//!
//! # Why persistence goes through an owned-clone relay, not `VaultDecisionSink`
//!
//! `VaultDecisionSink` borrows a live `&Transaction` and is only reachable
//! when `Session` and the transaction share a call stack — true in
//! `db::blackbox_repo`'s own tests, never true here: `Session` lives on this
//! thread, the transaction lives on the vault worker's thread. So
//! `CollectingSink` (below) does the full round trip *inside* `persist()`,
//! blocking this thread on `VaultHandle::blackbox_flush` and reporting the
//! vault's real answer back to `Session::flush_to` — preserving flush_to's
//! own contract (the ring clears only once the sink truly acknowledges)
//! across the thread boundary instead of racing it.

use std::collections::HashMap;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, SyncSender, TrySendError};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use crate::blackbox_sim::director::{DirectorError, Session};
use crate::blackbox_sim::genesis::GenesisRequest;
use crate::blackbox_sim::persist::{FlushReceipt, NullSink};
use crate::blackbox_sim::telemetry::ActionIntent;

use super::view::{map_director_error, AdvanceView, ArenaLimitsView, BooksView, ObservationView, SimUiErrorCode};

#[cfg(all(feature = "secure-vault", target_vendor = "apple"))]
use crate::blackbox_sim::director::replay_session;
#[cfg(all(feature = "secure-vault", target_vendor = "apple"))]
use crate::blackbox_sim::genesis::Difficulty;
#[cfg(all(feature = "secure-vault", target_vendor = "apple"))]
use crate::blackbox_sim::persist::{DecisionBatch, DecisionSink, SinkError};
#[cfg(all(feature = "secure-vault", target_vendor = "apple"))]
use crate::blackbox_sim::settle::TICKS_PER_QUARTER;
#[cfg(all(feature = "secure-vault", target_vendor = "apple"))]
use crate::db::blackbox_repo::CampaignRow;
#[cfg(all(feature = "secure-vault", target_vendor = "apple"))]
use crate::db::{VaultErrorCode, VaultHandle};

const SIM_QUEUE_CAPACITY: usize = 8;
const SIM_REQUEST_TIMEOUT: Duration = Duration::from_secs(10);

/// Plain, always-available campaign identity (fingerprint + the fields
/// `blackbox_campaigns` keys on). A local twin of `db::blackbox_repo::CampaignRow`
/// so this struct compiles with zero cfg-gating; only the conversion into the
/// real `CampaignRow` at the vault boundary is feature-gated. Its fields are
/// read only on that gated path, so a non-`secure-vault` build sees them as
/// unread — intentional, not dead code.
#[derive(Debug, Clone, Copy)]
#[allow(dead_code)]
struct CampaignIdentity {
    fingerprint: [u8; 32],
    scenario_id: u32,
    difficulty: u8,
    campaign_index: u32,
}

#[cfg(all(feature = "secure-vault", target_vendor = "apple"))]
impl From<CampaignIdentity> for CampaignRow {
    fn from(identity: CampaignIdentity) -> Self {
        CampaignRow {
            fingerprint: identity.fingerprint,
            scenario_id: identity.scenario_id,
            difficulty: identity.difficulty,
            campaign_index: identity.campaign_index,
        }
    }
}

struct CampaignEntry {
    session: Session,
    identity: CampaignIdentity,
    created_date: String,
}

fn campaign_id_of(fingerprint: [u8; 32]) -> String {
    hex::encode(fingerprint)
}

#[cfg(all(feature = "secure-vault", target_vendor = "apple"))]
fn decode_campaign_id(campaign_id: &str) -> Result<[u8; 32], SimUiErrorCode> {
    let bytes = hex::decode(campaign_id).map_err(|_| SimUiErrorCode::CampaignNotFound)?;
    <[u8; 32]>::try_from(bytes).map_err(|_| SimUiErrorCode::CampaignNotFound)
}

#[cfg(all(feature = "secure-vault", target_vendor = "apple"))]
fn difficulty_from_u8(value: u8) -> Result<Difficulty, SimUiErrorCode> {
    match value {
        0 => Ok(Difficulty::Standard),
        1 => Ok(Difficulty::Hard),
        _ => Err(SimUiErrorCode::InternalFault),
    }
}

fn observation_view(campaign_id: &str, session: &Session) -> Result<ObservationView, SimUiErrorCode> {
    let market = session
        .market()
        .ok_or_else(|| map_director_error("observation_view", DirectorError::NoObservationYet))?;
    Ok(ObservationView {
        campaign_id: campaign_id.to_string(),
        turns_completed: session.turns_completed(),
        market,
        stimuli: session.stimulus_views(),
        books: BooksView::from_books(session.books()),
        limits: ArenaLimitsView::current(),
        state: session.state(),
    })
}

/// Sink that performs the real vault write synchronously inside `persist()`,
/// so `Session::flush_to` only drains its ring when the vault has actually
/// acknowledged (see module header).
#[cfg(all(feature = "secure-vault", target_vendor = "apple"))]
struct CollectingSink<'a> {
    vault: &'a VaultHandle,
    campaign: CampaignRow,
    created_date: String,
}

#[cfg(all(feature = "secure-vault", target_vendor = "apple"))]
impl DecisionSink for CollectingSink<'_> {
    fn persist(&mut self, batch: &DecisionBatch<'_>) -> Result<FlushReceipt, SinkError> {
        let events = batch.events.to_vec();
        let stimuli = batch.stimuli.to_vec();
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);
        self.vault
            .blackbox_flush(self.campaign, self.created_date.clone(), events, stimuli, now)
            .map_err(map_vault_error_to_sink)
    }
}

/// Retryable environmental failures (locked/busy/timeout/self-locked/storage
/// hiccup) map to `Unavailable` — `Session::flush_to` keeps every record and
/// the next flush attempt tries again. Failures that will not change on retry
/// (bad key, unsupported schema, malformed input, identity conflicts) map to
/// `Rejected`.
#[cfg(all(feature = "secure-vault", target_vendor = "apple"))]
fn map_vault_error_to_sink(error: VaultErrorCode) -> SinkError {
    match error {
        VaultErrorCode::Locked
        | VaultErrorCode::Busy
        | VaultErrorCode::Timeout
        | VaultErrorCode::AuthenticationCancelled
        | VaultErrorCode::AuthenticationFailed
        | VaultErrorCode::InteractionNotAllowed
        | VaultErrorCode::KeychainUnavailable
        | VaultErrorCode::VaultQuarantined
        | VaultErrorCode::OsLockEngaged
        | VaultErrorCode::StorageFailed
        | VaultErrorCode::Unavailable => SinkError::Unavailable,
        VaultErrorCode::CorruptOrWrongKey
        | VaultErrorCode::UnsupportedSchema
        | VaultErrorCode::InvalidInput
        | VaultErrorCode::NotFound
        | VaultErrorCode::Conflict
        | VaultErrorCode::IdentityAmbiguous => SinkError::Rejected,
    }
}

#[cfg(all(feature = "secure-vault", target_vendor = "apple"))]
fn attempt_flush(
    session: &mut Session,
    identity: CampaignIdentity,
    created_date: &str,
    vault: Option<&VaultHandle>,
) -> Result<FlushReceipt, DirectorError> {
    match vault {
        Some(vault) => {
            let mut sink = CollectingSink {
                vault,
                campaign: identity.into(),
                created_date: created_date.to_string(),
            };
            session.flush_to(&mut sink)
        }
        None => session.flush_to(&mut NullSink),
    }
}

#[cfg(not(all(feature = "secure-vault", target_vendor = "apple")))]
fn attempt_flush(
    session: &mut Session,
    _identity: CampaignIdentity,
    _created_date: &str,
) -> Result<FlushReceipt, DirectorError> {
    session.flush_to(&mut NullSink)
}

enum SimRequest {
    Start {
        request: GenesisRequest,
        reply: SyncSender<Result<ObservationView, SimUiErrorCode>>,
    },
    GetView {
        campaign_id: String,
        reply: SyncSender<Result<ObservationView, SimUiErrorCode>>,
    },
    SubmitDecision {
        campaign_id: String,
        intent: ActionIntent,
        latency_ms: Option<u32>,
        reply: SyncSender<Result<super::view::DecisionOutcomeView, SimUiErrorCode>>,
    },
    Advance {
        campaign_id: String,
        reply: SyncSender<Result<AdvanceView, SimUiErrorCode>>,
    },
    Abort {
        campaign_id: String,
        reply: SyncSender<Result<(), SimUiErrorCode>>,
    },
    LoadGeneration {
        campaign_id: String,
        generation_index: u32,
        reply: SyncSender<Result<ObservationView, SimUiErrorCode>>,
    },
    #[cfg(feature = "flavor-live")]
    TakeFlavor {
        campaign_id: String,
        reply: SyncSender<Result<Option<String>, SimUiErrorCode>>,
    },
    /// Off-turn ambient delivery (A-4). No reply — never blocks `advance`.
    /// Delegates model work to [`crate::llm::LlmHandle`] (関所 H); this
    /// message only builds an owned prompt and enqueues.
    #[cfg(feature = "flavor-live")]
    DeliverAmbientFlavor {
        campaign_id: String,
    },
    /// Completion relay from the LLM worker back onto the sim queue (関所 H).
    #[cfg(feature = "flavor-live")]
    AmbientFlavorReady {
        campaign_id: String,
        /// `None` = model absent / error / empty → `Unavailable` via deliver.
        completion: Option<String>,
    },
}

struct Worker {
    sessions: HashMap<String, CampaignEntry>,
    #[cfg(all(feature = "secure-vault", target_vendor = "apple"))]
    vault: Arc<Mutex<Option<VaultHandle>>>,
    #[cfg(feature = "flavor-live")]
    flavor: super::flavor_slot::FlavorAmbientSlot,
    /// Self-queue for off-turn flavor delivery kick + LLM completion relay.
    #[cfg(feature = "flavor-live")]
    flavor_tx: SyncSender<SimRequest>,
    /// LLM handle for ambient generation (関所 H). Attached after spawn.
    #[cfg(feature = "flavor-live")]
    llm: Arc<Mutex<Option<crate::llm::LlmHandle>>>,
}

impl Worker {
    fn run(mut self, receiver: Receiver<SimRequest>) {
        while let Ok(request) = receiver.recv() {
            match request {
                SimRequest::Start { request, reply } => {
                    let _ = reply.send(self.start(request));
                }
                SimRequest::GetView { campaign_id, reply } => {
                    let _ = reply.send(self.get_view(&campaign_id));
                }
                SimRequest::SubmitDecision {
                    campaign_id,
                    intent,
                    latency_ms,
                    reply,
                } => {
                    let _ = reply.send(self.submit_decision(&campaign_id, intent, latency_ms));
                }
                SimRequest::Advance { campaign_id, reply } => {
                    let _ = reply.send(self.advance(&campaign_id));
                }
                SimRequest::Abort { campaign_id, reply } => {
                    let _ = reply.send(self.abort(&campaign_id));
                }
                SimRequest::LoadGeneration {
                    campaign_id,
                    generation_index,
                    reply,
                } => {
                    let _ = reply.send(self.load_generation(&campaign_id, generation_index));
                }
                #[cfg(feature = "flavor-live")]
                SimRequest::TakeFlavor { campaign_id, reply } => {
                    let _ = reply.send(self.take_flavor(&campaign_id));
                }
                #[cfg(feature = "flavor-live")]
                SimRequest::DeliverAmbientFlavor { campaign_id } => {
                    self.deliver_ambient_flavor(&campaign_id);
                }
                #[cfg(feature = "flavor-live")]
                SimRequest::AmbientFlavorReady {
                    campaign_id,
                    completion,
                } => {
                    self.finish_ambient_flavor(&campaign_id, completion);
                }
            }
        }
    }

    #[cfg(all(feature = "secure-vault", target_vendor = "apple"))]
    fn vault_handle(&self) -> Option<VaultHandle> {
        self.vault.lock().ok().and_then(|guard| guard.clone())
    }

    /// Best-effort campaign registration. A failure here does not stop play —
    /// the same "flush is best-effort, gameplay never blocks on the vault"
    /// principle as `attempt_flush` — it only means the eventual decision
    /// flush will also fail to find its parent row, which `blackbox_flush`
    /// tolerates by upserting the campaign itself before persisting.
    fn start(&mut self, request: GenesisRequest) -> Result<ObservationView, SimUiErrorCode> {
        let created_date = request.created_date.clone();
        let scenario_id = request.scenario_id;
        let difficulty = request.difficulty as u8;
        let campaign_index = request.campaign_index;
        let mut session =
            Session::start(request).map_err(|e| map_director_error("start", e))?;
        session
            .observe()
            .map_err(|e| map_director_error("observe", e))?;
        let fingerprint = session.campaign_fingerprint();
        let campaign_id = campaign_id_of(fingerprint);
        let identity = CampaignIdentity {
            fingerprint,
            scenario_id,
            difficulty,
            campaign_index,
        };
        let view = observation_view(&campaign_id, &session)?;
        self.sessions.insert(
            campaign_id,
            CampaignEntry {
                session,
                identity,
                created_date,
            },
        );
        Ok(view)
    }

    fn get_view(&self, campaign_id: &str) -> Result<ObservationView, SimUiErrorCode> {
        let entry = self
            .sessions
            .get(campaign_id)
            .ok_or(SimUiErrorCode::CampaignNotFound)?;
        observation_view(campaign_id, &entry.session)
    }

    fn submit_decision(
        &mut self,
        campaign_id: &str,
        intent: ActionIntent,
        latency_ms: Option<u32>,
    ) -> Result<super::view::DecisionOutcomeView, SimUiErrorCode> {
        let entry = self
            .sessions
            .get_mut(campaign_id)
            .ok_or(SimUiErrorCode::CampaignNotFound)?;
        entry
            .session
            .submit(intent, latency_ms)
            .map(super::view::DecisionOutcomeView::from)
            .map_err(|e| map_director_error("submit_decision", e))
    }

    fn advance(&mut self, campaign_id: &str) -> Result<AdvanceView, SimUiErrorCode> {
        let entry = self
            .sessions
            .get_mut(campaign_id)
            .ok_or(SimUiErrorCode::CampaignNotFound)?;
        entry
            .session
            .execute()
            .map_err(|e| map_director_error("advance.execute", e))?;
        entry
            .session
            .settle()
            .map_err(|e| map_director_error("advance.settle", e))?;
        let report = entry
            .session
            .report()
            .map_err(|e| map_director_error("advance.report", e))?;

        // SPEC §9.2: flush at Settle. A quarter close is exactly when a
        // `PeriodClose` is `Some`; best-effort, never fatal to the turn.
        if report.period_close.is_some() {
            if let Err(e) = self.flush(campaign_id) {
                eprintln!("blackbox_arena: settle-time flush failed for {campaign_id}: {e:?}");
            }
        }

        let entry = self
            .sessions
            .get_mut(campaign_id)
            .ok_or(SimUiErrorCode::CampaignNotFound)?;
        let state = entry.session.state();
        // Read while `entry` is still borrowed: the terminal branch calls
        // `self.flush` first, which needs `&mut self` and ends this borrow.
        #[cfg(target_os = "ios")]
        let turns_completed = entry.session.turns_completed();
        let next_observation = if matches!(state, crate::blackbox_sim::fsm::SessionState::Active { .. }) {
            entry
                .session
                .observe()
                .map_err(|e| map_director_error("advance.observe_next", e))?;
            Some(observation_view(campaign_id, &entry.session)?)
        } else {
            // Sealed (campaign completed) or Dead (invariant breach): nothing
            // further to observe. A final best-effort flush covers whatever
            // the Report step just produced.
            if let Err(e) = self.flush(campaign_id) {
                eprintln!("blackbox_arena: final flush failed for {campaign_id}: {e:?}");
            }
            #[cfg(target_os = "ios")]
            log_arena_terminal_ios(state, turns_completed);
            None
        };
        #[cfg(feature = "flavor-live")]
        self.kick_ambient_flavor(campaign_id);
        Ok(AdvanceView::from_report(&report, next_observation, state))
    }

    /// Turn-path ambient kick (A-4): admit the slot only. Generation runs on
    /// [`Self::deliver_ambient_flavor`] via a queued worker message — never
    /// inline here (関所 G / FLV-I-14).
    #[cfg(feature = "flavor-live")]
    fn kick_ambient_flavor(&mut self, campaign_id: &str) {
        use crate::flavor::request::TemplateId;
        use super::flavor_slot::{correlation_from_genesis, Admit};

        let Some(entry) = self.sessions.get(campaign_id) else {
            return;
        };
        let corr = correlation_from_genesis(
            entry.session.genesis(),
            entry.session.turns_completed(),
            TemplateId::ArenaEventHeadline,
        );
        match self.flavor.begin_request(corr) {
            Admit::DroppedBusy => {}
            Admit::Started => {
                // Queue delivery off this call stack. On full/disconnect, clear
                // busy without invoking the model on the turn path.
                if self
                    .flavor_tx
                    .try_send(SimRequest::DeliverAmbientFlavor {
                        campaign_id: campaign_id.to_string(),
                    })
                    .is_err()
                {
                    self.flavor
                        .finish(crate::llm::flavor_gen::FlavorOutcome::Unavailable);
                }
            }
        }
    }

    /// Worker path (関所 H): build owned prompt, enqueue on `LlmHandle`, return.
    /// Must **not** call the model or `flavor_gen::generate` here — completion
    /// arrives as [`SimRequest::AmbientFlavorReady`].
    #[cfg(feature = "flavor-live")]
    fn deliver_ambient_flavor(&mut self, campaign_id: &str) {
        use crate::llm::flavor_gen;

        let Some(entry) = self.sessions.get(campaign_id) else {
            // Aborted between kick and deliver — drop without panic.
            if self.flavor.is_busy() {
                self.flavor
                    .finish(crate::llm::flavor_gen::FlavorOutcome::Unavailable);
            }
            return;
        };
        // Read the market the player is looking at, so the prompt stops being
        // the same sentence on every turn.
        let request = ambient_flavor_request(mood_for_market(entry.session.market()));
        // Read before the borrow ends: an independent draw per turn is the
        // other half of breaking the constant output (Tier 3 §14.3).
        let seed = ambient_flavor_seed(
            entry.session.genesis_digest8(),
            entry.session.turns_completed(),
        );
        // Owned prompt crosses the thread boundary (AI_SKILLS §20-5 relay).
        let prompt = flavor_gen::render_prompt(&request);
        let llm = self
            .llm
            .lock()
            .ok()
            .and_then(|guard| guard.clone());
        let Some(llm) = llm else {
            self.flavor
                .finish(crate::llm::flavor_gen::FlavorOutcome::Unavailable);
            return;
        };
        let tx = self.flavor_tx.clone();
        let cid = campaign_id.to_string();
        // Headline budget is 48 chars; 64 tokens is ample headroom.
        const FLAVOR_MAX_TOKENS: u32 = 64;
        if llm
            .enqueue_flavor_generate_seeded(
                prompt,
                FLAVOR_MAX_TOKENS,
                seed,
                Box::new(move |result| {
                    let completion = result.ok().filter(|s| !s.trim().is_empty());
                    // Block on the sim queue if full — never leave the slot busy.
                    let _ = tx.send(SimRequest::AmbientFlavorReady {
                        campaign_id: cid,
                        completion,
                    });
                }),
            )
            .is_err()
        {
            self.flavor
                .finish(crate::llm::flavor_gen::FlavorOutcome::Unavailable);
        }
    }

    /// LLM-thread completion landed on the sim queue (関所 H).
    #[cfg(feature = "flavor-live")]
    fn finish_ambient_flavor(&mut self, campaign_id: &str, completion: Option<String>) {
        let Some(entry) = self.sessions.get(campaign_id) else {
            if self.flavor.is_busy() {
                self.flavor
                    .finish(crate::llm::flavor_gen::FlavorOutcome::Unavailable);
            }
            return;
        };
        // Same builder as the prompt side. Only `template_id` reaches `decide`,
        // so a market that moved since the kick cannot change the verdict.
        let request = ambient_flavor_request(mood_for_market(entry.session.market()));
        self.flavor
            .deliver_completion(&request, completion.as_deref());
    }

    #[cfg(feature = "flavor-live")]
    fn take_flavor(&mut self, campaign_id: &str) -> Result<Option<String>, SimUiErrorCode> {
        use crate::flavor::request::TemplateId;
        use super::flavor_slot::correlation_from_genesis;

        let entry = self
            .sessions
            .get(campaign_id)
            .ok_or(SimUiErrorCode::CampaignNotFound)?;
        let current = correlation_from_genesis(
            entry.session.genesis(),
            entry.session.turns_completed(),
            TemplateId::ArenaEventHeadline,
        );
        Ok(self
            .flavor
            .take(&current)
            .map(|v| v.as_str().to_string()))
    }

    fn abort(&mut self, campaign_id: &str) -> Result<(), SimUiErrorCode> {
        {
            let entry = self
                .sessions
                .get_mut(campaign_id)
                .ok_or(SimUiErrorCode::CampaignNotFound)?;
            entry
                .session
                .abort()
                .map_err(|e| map_director_error("abort", e))?;
        }
        if let Err(e) = self.flush(campaign_id) {
            eprintln!("blackbox_arena: abort-time flush failed for {campaign_id}: {e:?}");
        }
        self.sessions.remove(campaign_id);
        #[cfg(feature = "flavor-live")]
        self.flavor.abort_campaign();
        Ok(())
    }

    #[cfg(all(feature = "secure-vault", target_vendor = "apple"))]
    fn flush(&mut self, campaign_id: &str) -> Result<FlushReceipt, DirectorError> {
        let vault = self.vault_handle();
        let entry = match self.sessions.get_mut(campaign_id) {
            Some(entry) => entry,
            None => return Ok(FlushReceipt::default()),
        };
        attempt_flush(
            &mut entry.session,
            entry.identity,
            &entry.created_date,
            vault.as_ref(),
        )
    }

    #[cfg(not(all(feature = "secure-vault", target_vendor = "apple")))]
    fn flush(&mut self, campaign_id: &str) -> Result<FlushReceipt, DirectorError> {
        let entry = match self.sessions.get_mut(campaign_id) {
            Some(entry) => entry,
            None => return Ok(FlushReceipt::default()),
        };
        attempt_flush(&mut entry.session, entry.identity, &entry.created_date)
    }

    #[cfg(all(feature = "secure-vault", target_vendor = "apple"))]
    fn load_generation(
        &mut self,
        campaign_id: &str,
        generation_index: u32,
    ) -> Result<ObservationView, SimUiErrorCode> {
        let fingerprint = decode_campaign_id(campaign_id)?;
        let vault = self.vault_handle().ok_or(SimUiErrorCode::Unavailable)?;
        let (loaded, decisions) = vault
            .blackbox_load_campaign(fingerprint)
            .map_err(|e| {
                eprintln!("blackbox_arena: load_generation vault read failed: {e:?}");
                SimUiErrorCode::Unavailable
            })?
            .ok_or(SimUiErrorCode::CampaignNotFound)?;
        // Ticks are 0-indexed (`market.rs`'s `step()` reports the pre-increment
        // value), so a closed quarter's decisions carry ticks
        // `[q*TICKS_PER_QUARTER, (q+1)*TICKS_PER_QUARTER)`; the tick *count*
        // (one decision per turn, `ForcedDefault` fills any gap) is what maps
        // cleanly onto "how many quarters have fully closed" — the max tick
        // value itself is off by one for this purpose.
        let available_generations = (decisions.len() as u32) / TICKS_PER_QUARTER;
        if generation_index >= available_generations {
            return Err(SimUiErrorCode::GenerationNotFound);
        }
        let boundary_tick = generation_index.saturating_add(1).saturating_mul(TICKS_PER_QUARTER);
        let truncated: Vec<_> = decisions
            .into_iter()
            .filter(|d| d.tick <= boundary_tick)
            .collect();
        let difficulty = difficulty_from_u8(loaded.campaign.difficulty)?;
        let genesis_request = GenesisRequest {
            scenario_id: loaded.campaign.scenario_id,
            difficulty,
            campaign_index: loaded.campaign.campaign_index,
            created_date: loaded.created_date.clone(),
        };
        let session = replay_session(genesis_request, &truncated)
            .map_err(|e| map_director_error("load_generation.replay", e))?;
        let view = observation_view(campaign_id, &session)?;
        self.sessions.insert(
            campaign_id.to_string(),
            CampaignEntry {
                identity: CampaignIdentity {
                    fingerprint,
                    scenario_id: loaded.campaign.scenario_id,
                    difficulty: loaded.campaign.difficulty,
                    campaign_index: loaded.campaign.campaign_index,
                },
                created_date: loaded.created_date,
                session,
            },
        );
        Ok(view)
    }

    #[cfg(not(all(feature = "secure-vault", target_vendor = "apple")))]
    fn load_generation(
        &mut self,
        _campaign_id: &str,
        _generation_index: u32,
    ) -> Result<ObservationView, SimUiErrorCode> {
        // No vault in this build, so there is nowhere a prior generation
        // could have been persisted to. Fail closed rather than pretend.
        Err(SimUiErrorCode::Unavailable)
    }
}

#[derive(Clone)]
pub(crate) struct BlackboxSimHandle {
    inner: Arc<Inner>,
}

struct Inner {
    sender: SyncSender<SimRequest>,
    #[cfg(all(feature = "secure-vault", target_vendor = "apple"))]
    vault: Arc<Mutex<Option<VaultHandle>>>,
    #[cfg(feature = "flavor-live")]
    llm: Arc<Mutex<Option<crate::llm::LlmHandle>>>,
}

impl BlackboxSimHandle {
    /// Always constructible, independent of `secure-vault`/Apple: play with
    /// no back-end is a hard requirement (SPEC §14's precedent). The vault
    /// capability, when the build has one, is attached afterward via
    /// `attach_vault`.
    pub(crate) fn spawn() -> Self {
        let (sender, receiver) = mpsc::sync_channel(SIM_QUEUE_CAPACITY);
        #[cfg(all(feature = "secure-vault", target_vendor = "apple"))]
        let vault: Arc<Mutex<Option<VaultHandle>>> = Arc::new(Mutex::new(None));
        #[cfg(feature = "flavor-live")]
        let flavor_tx = sender.clone();
        #[cfg(feature = "flavor-live")]
        let llm: Arc<Mutex<Option<crate::llm::LlmHandle>>> = Arc::new(Mutex::new(None));
        let worker = Worker {
            sessions: HashMap::new(),
            #[cfg(all(feature = "secure-vault", target_vendor = "apple"))]
            vault: Arc::clone(&vault),
            #[cfg(feature = "flavor-live")]
            flavor: super::flavor_slot::FlavorAmbientSlot::new(),
            #[cfg(feature = "flavor-live")]
            flavor_tx,
            #[cfg(feature = "flavor-live")]
            llm: Arc::clone(&llm),
        };
        let spawned = thread::Builder::new()
            .name("bxs-sim-worker".to_string())
            .spawn(move || worker.run(receiver));
        if spawned.is_err() {
            eprintln!("blackbox_arena: sim worker thread failed to start; sim is unavailable");
            // `sender`'s receiver was dropped along with `worker`/`receiver`
            // above having never been taken by a live thread — every
            // subsequent `try_send` observes `Disconnected` and every public
            // method maps that to `SimUiErrorCode::Unavailable`. Fail closed.
        }
        Self {
            inner: Arc::new(Inner {
                sender,
                #[cfg(all(feature = "secure-vault", target_vendor = "apple"))]
                vault,
                #[cfg(feature = "flavor-live")]
                llm,
            }),
        }
    }

    #[cfg(all(feature = "secure-vault", target_vendor = "apple"))]
    pub(crate) fn attach_vault(&self, vault: VaultHandle) {
        if let Ok(mut slot) = self.inner.vault.lock() {
            *slot = Some(vault);
        }
    }

    /// Wire the pocket-brain worker so ambient flavor generation leaves the
    /// sim thread (関所 H / A-4).
    #[cfg(feature = "flavor-live")]
    pub(crate) fn attach_llm(&self, handle: crate::llm::LlmHandle) {
        if let Ok(mut slot) = self.inner.llm.lock() {
            *slot = Some(handle);
        }
    }

    fn send<T>(
        &self,
        build: impl FnOnce(SyncSender<Result<T, SimUiErrorCode>>) -> SimRequest,
    ) -> Result<T, SimUiErrorCode> {
        let (reply, receiver) = mpsc::sync_channel(1);
        let request = build(reply);
        match self.inner.sender.try_send(request) {
            Ok(()) => {}
            Err(TrySendError::Full(_)) => return Err(SimUiErrorCode::Unavailable),
            Err(TrySendError::Disconnected(_)) => return Err(SimUiErrorCode::Unavailable),
        }
        match receiver.recv_timeout(SIM_REQUEST_TIMEOUT) {
            Ok(result) => result,
            Err(RecvTimeoutError::Timeout) => Err(SimUiErrorCode::Unavailable),
            Err(RecvTimeoutError::Disconnected) => Err(SimUiErrorCode::Unavailable),
        }
    }

    pub(crate) fn start_campaign(
        &self,
        request: GenesisRequest,
    ) -> Result<ObservationView, SimUiErrorCode> {
        self.send(|reply| SimRequest::Start { request, reply })
    }

    pub(crate) fn get_view(&self, campaign_id: String) -> Result<ObservationView, SimUiErrorCode> {
        self.send(|reply| SimRequest::GetView { campaign_id, reply })
    }

    pub(crate) fn submit_decision(
        &self,
        campaign_id: String,
        intent: ActionIntent,
        latency_ms: Option<u32>,
    ) -> Result<super::view::DecisionOutcomeView, SimUiErrorCode> {
        self.send(|reply| SimRequest::SubmitDecision {
            campaign_id,
            intent,
            latency_ms,
            reply,
        })
    }

    pub(crate) fn advance(&self, campaign_id: String) -> Result<AdvanceView, SimUiErrorCode> {
        self.send(|reply| SimRequest::Advance { campaign_id, reply })
    }

    pub(crate) fn abort(&self, campaign_id: String) -> Result<(), SimUiErrorCode> {
        self.send(|reply| SimRequest::Abort { campaign_id, reply })
    }

    pub(crate) fn load_generation(
        &self,
        campaign_id: String,
        generation_index: u32,
    ) -> Result<ObservationView, SimUiErrorCode> {
        self.send(|reply| SimRequest::LoadGeneration {
            campaign_id,
            generation_index,
            reply,
        })
    }

    /// Non-blocking ambient flavor pull (FLV-R-10). Correlation is resolved
    /// on the Rust worker from the live session — FE never supplies tokens.
    #[cfg(feature = "flavor-live")]
    pub(crate) fn take_flavor(&self, campaign_id: String) -> Result<Option<String>, SimUiErrorCode> {
        self.send(|reply| SimRequest::TakeFlavor { campaign_id, reply })
    }

    /// R-8 two-factor write gate. Signature identical across builds.
    ///
    /// - Without `blackbox-profile-write`: refuse with
    ///   [`SimUiErrorCode::BlackboxProfileWriteNotReady`].
    /// - With the flag + vault: seal-filter campaigns, `estimate_pooled`,
    ///   `insert_profile` in one vault transaction. Empty sealed pool →
    ///   [`SimUiErrorCode::Unavailable`] (never silent Ok / no-op).
    pub(crate) fn estimate_profile(&self) -> Result<(), SimUiErrorCode> {
        #[cfg(not(feature = "blackbox-profile-write"))]
        {
            let _ = self;
            Err(SimUiErrorCode::BlackboxProfileWriteNotReady)
        }
        #[cfg(all(
            feature = "blackbox-profile-write",
            feature = "secure-vault",
            target_vendor = "apple"
        ))]
        {
            let vault = self
                .inner
                .vault
                .lock()
                .ok()
                .and_then(|guard| guard.clone())
                .ok_or(SimUiErrorCode::Unavailable)?;
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs() as i64)
                .unwrap_or(0);
            match vault.blackbox_estimate_and_persist(now) {
                Ok(_) => Ok(()),
                Err(VaultErrorCode::NotFound) | Err(VaultErrorCode::Unavailable) => {
                    Err(SimUiErrorCode::Unavailable)
                }
                Err(VaultErrorCode::Timeout)
                | Err(VaultErrorCode::Busy)
                | Err(VaultErrorCode::Locked)
                | Err(VaultErrorCode::StorageFailed) => Err(SimUiErrorCode::Unavailable),
                Err(_) => Err(SimUiErrorCode::InternalFault),
            }
        }
        #[cfg(all(
            feature = "blackbox-profile-write",
            not(all(feature = "secure-vault", target_vendor = "apple"))
        ))]
        {
            let _ = self;
            Err(SimUiErrorCode::Unavailable)
        }
    }
}

/// Mood tag derived from the market the player is actually looking at.
///
/// This was hardcoded to `MoodCalm`, in both the prompt and the decide-side
/// request, so `render_prompt` produced a byte-identical prompt on every turn.
/// Together with the sampler seed fixed at 0 that collapsed the output space to
/// a single point: 52 generations on device produced 52 identical strings, and
/// the numeral guard was never once handed anything to reject (Tier 3 §14).
///
/// A jump outranks the regime — a shock is volatile whether or not the regime
/// has caught up with it yet.
#[cfg_attr(not(feature = "flavor-live"), allow(dead_code))]
pub(crate) fn mood_for_market(
    market: Option<crate::blackbox_sim::market::MarketTickView>,
) -> crate::flavor::request::SlotValue {
    use crate::blackbox_sim::market::Regime;
    use crate::flavor::request::SlotValue;
    match market {
        Some(m) if m.jump_occurred => SlotValue::MoodVolatile,
        Some(m) => match m.regime {
            Regime::Stress => SlotValue::MoodTense,
            Regime::Calm => SlotValue::MoodCalm,
        },
        // Before the first observation there is no market to read; calm is the
        // same default this code already had.
        None => SlotValue::MoodCalm,
    }
}

/// The ambient request, built in one place so the prompt side and the
/// decide side cannot drift apart.
///
/// Only `template_id` is load-bearing at decide time (`decide` takes the
/// template, never the slots), so a market that moves between the kick and the
/// completion changes the wording of the prompt and nothing about the verdict.
#[cfg_attr(not(feature = "flavor-live"), allow(dead_code))]
pub(crate) fn ambient_flavor_request(
    mood: crate::flavor::request::SlotValue,
) -> crate::flavor::request::FlavorRequest {
    use crate::flavor::request::{
        FlavorLocale, FlavorRequest, FlavorSchema, FlavorSlot, SlotId, TemplateId,
    };
    FlavorRequest {
        schema: FlavorSchema::V1,
        template_id: TemplateId::ArenaEventHeadline,
        slots: vec![FlavorSlot {
            id: SlotId::Mood,
            tag: mood,
        }],
        locale: FlavorLocale::Ja,
    }
}

/// Sampler seed for one ambient generation.
///
/// The live path pinned this at 0 on every call (`enqueue_flavor_generate`),
/// so a given prompt yielded exactly one draw — 52 generations on device
/// produced 52 identical strings and the numeral guard was never handed
/// anything to reject. The seeded entry point already existed for precisely
/// this reason; its own doc says T-8's discard-rate arms "need independent
/// draws". It had simply never been wired to the arena.
///
/// Deterministic in (campaign, tick) rather than random, on purpose. Varying
/// is what breaks the collapse; reproducible is what keeps a surprising line
/// investigable instead of a one-off nobody can retrieve.
#[cfg_attr(not(feature = "flavor-live"), allow(dead_code))]
pub(crate) fn ambient_flavor_seed(genesis_digest8: [u8; 8], turns: u32) -> u32 {
    let g = u32::from_le_bytes([
        genesis_digest8[0],
        genesis_digest8[1],
        genesis_digest8[2],
        genesis_digest8[3],
    ]);
    g.wrapping_mul(2_654_435_761)
        .wrapping_add(turns.wrapping_mul(9973).wrapping_add(1))
}

/// Numeric codes for the arena's terminal state (Tier 3 follow-up to §13.5).
///
/// Returns `(sealed, dead_reason)`. `dead_reason` is `0` unless the session is
/// `Dead`, which is **not** an ordinary ending: it is reachable only via
/// `SessionEvent::CorruptionDetected`, i.e. an invariant breach. A campaign
/// runs `CAMPAIGN_TICKS` (13 × 4 = 52), so a stop well before that is a fault
/// rather than exhaustion — and `turns_completed` is logged beside these codes
/// precisely so the two can be told apart without inference.
///
/// Kept free of `cfg(target_os)` so the mapping is asserted by host tests. The
/// same shape as `sanitize_one_liner` had, whose iOS-only test never ran once
/// and asserted something false for months.
pub(crate) const fn arena_terminal_codes(
    state: crate::blackbox_sim::fsm::SessionState,
) -> (u64, u64) {
    use crate::blackbox_sim::fsm::{FailureReason, SessionState};
    match state {
        SessionState::Genesis | SessionState::Active { .. } => (0, 0),
        SessionState::Sealed => (1, 0),
        SessionState::Dead { reason } => (
            1,
            match reason {
                FailureReason::AccountingBreach => 1,
                FailureReason::SnapshotDigestMismatch => 2,
                FailureReason::ReplayDivergence => 3,
                FailureReason::InternalInvariantBroken => 4,
            },
        ),
    }
}

/// Release-visible terminal evidence. The arena's own diagnostics are
/// `eprintln!`, which iOS Release discards (G-0R) — so a campaign that died of
/// an invariant breach was indistinguishable, on device, from one the operator
/// simply stopped advancing. This is the same hole G-5's import and G-4's
/// flavor each fell into, closed the same way: numeric OSLog through the path
/// P0-6 proved survives.
#[cfg(target_os = "ios")]
fn log_arena_terminal_ios(state: crate::blackbox_sim::fsm::SessionState, turns: u32) {
    use crate::ios_oslog::log_model_u64;
    let (sealed, reason) = arena_terminal_codes(state);
    log_model_u64("arena.sealed", sealed);
    log_model_u64("arena.dead_reason", reason);
    log_model_u64("arena.turns_completed", u64::from(turns));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::blackbox_sim::fsm::SessionState;
    use crate::blackbox_sim::genesis::Difficulty as GenesisDifficulty;

    fn ok<T, E: std::fmt::Debug>(r: Result<T, E>) -> T {
        match r {
            Ok(v) => v,
            Err(e) => unreachable!("test setup failed: {e:?}"),
        }
    }

    /// The mood was hardcoded, so the prompt never changed and — with the
    /// sampler seed also fixed — 52 device generations produced one string.
    /// Every branch must be reachable, or the collapse simply moves.
    #[test]
    fn mood_is_read_from_the_market_and_every_branch_is_reachable() {
        use crate::blackbox_sim::market::{MarketTickView, Regime};
        use crate::flavor::request::SlotValue;

        fn view(regime: Regime, jump_occurred: bool) -> MarketTickView {
            MarketTickView {
                tick: 1,
                regime,
                commodity_price_minor: 0,
                equity_index_centi: 0,
                demand_index_micro: 0,
                rate_bp: 0,
                jump_occurred,
            }
        }

        assert_eq!(
            mood_for_market(Some(view(Regime::Calm, false))),
            SlotValue::MoodCalm
        );
        assert_eq!(
            mood_for_market(Some(view(Regime::Stress, false))),
            SlotValue::MoodTense
        );
        // A jump outranks the regime in both directions — a shock is volatile
        // even while the regime still reads calm.
        assert_eq!(
            mood_for_market(Some(view(Regime::Calm, true))),
            SlotValue::MoodVolatile
        );
        assert_eq!(
            mood_for_market(Some(view(Regime::Stress, true))),
            SlotValue::MoodVolatile
        );
        // Before the first observation: unchanged default.
        assert_eq!(mood_for_market(None), SlotValue::MoodCalm);
    }

    /// Seed 0 on every call is the defect this replaces, so the property under
    /// test is that consecutive turns never draw the same seed. Reproducibility
    /// is asserted too: the same campaign and tick must give the same seed, or
    /// an odd line on device could never be retrieved.
    #[test]
    fn ambient_seed_varies_per_turn_and_repeats_only_for_the_same_turn() {
        let g: [u8; 8] = [7, 200, 3, 91, 0, 0, 0, 0];
        let seeds: Vec<u32> = (0..64).map(|t| ambient_flavor_seed(g, t)).collect();

        let mut sorted = seeds.clone();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(sorted.len(), seeds.len(), "64 turns must give 64 seeds");

        // Reproducible for the same (campaign, tick).
        assert_eq!(ambient_flavor_seed(g, 17), ambient_flavor_seed(g, 17));
        // A different campaign must not replay the same prose sequence.
        let other: [u8; 8] = [8, 200, 3, 91, 0, 0, 0, 0];
        assert_ne!(ambient_flavor_seed(g, 17), ambient_flavor_seed(other, 17));
    }

    /// Distinct moods must reach the prompt as distinct text. If they rendered
    /// identically the derivation would be decorative and the output space
    /// would stay collapsed.
    #[test]
    fn distinct_moods_render_distinct_prompts() {
        use crate::flavor::request::SlotValue;
        use crate::llm::flavor_gen::render_prompt;

        let calm = render_prompt(&ambient_flavor_request(SlotValue::MoodCalm));
        let tense = render_prompt(&ambient_flavor_request(SlotValue::MoodTense));
        let volatile = render_prompt(&ambient_flavor_request(SlotValue::MoodVolatile));
        assert_ne!(calm, tense, "calm and tense must not share a prompt");
        assert_ne!(calm, volatile, "calm and volatile must not share a prompt");
        assert_ne!(tense, volatile, "tense and volatile must not share a prompt");
    }

    /// The device reports these codes; nothing on hardware proves the mapping
    /// behind them. Asserted here, where tests actually run.
    #[test]
    fn arena_terminal_codes_separate_a_clean_end_from_an_invariant_breach() {
        use crate::blackbox_sim::fsm::{FailureReason, TurnPhase};

        // Running: no terminal evidence at all.
        assert_eq!(arena_terminal_codes(SessionState::Genesis), (0, 0));
        assert_eq!(
            arena_terminal_codes(SessionState::Active {
                phase: TurnPhase::Observe
            }),
            (0, 0)
        );

        // Completed the campaign: sealed, but nothing died.
        assert_eq!(arena_terminal_codes(SessionState::Sealed), (1, 0));

        // Every failure reason must be distinguishable — a single "it died"
        // bit would leave the device log unable to name which invariant broke.
        for (reason, code) in [
            (FailureReason::AccountingBreach, 1),
            (FailureReason::SnapshotDigestMismatch, 2),
            (FailureReason::ReplayDivergence, 3),
            (FailureReason::InternalInvariantBroken, 4),
        ] {
            assert_eq!(
                arena_terminal_codes(SessionState::Dead { reason }),
                (1, code),
                "{reason:?} must map to its own code"
            );
        }
    }

    fn request() -> GenesisRequest {
        GenesisRequest {
            scenario_id: 7,
            difficulty: GenesisDifficulty::Standard,
            campaign_index: 1,
            created_date: "2026-07-27".to_string(),
        }
    }

    #[test]
    fn campaign_id_round_trips_through_hex() {
        let fingerprint = [9u8; 32];
        let id = campaign_id_of(fingerprint);
        assert_eq!(id.len(), 64, "32 bytes must encode to 64 hex chars");
    }

    /// No vault attached anywhere in this test: exercises the play-with-no-
    /// backend requirement end to end through every command surface method,
    /// including a settle-time flush attempt that must fall back to
    /// `NullSink` and never fail the turn.
    #[test]
    fn full_lifecycle_without_a_vault_never_errors() {
        let sim = BlackboxSimHandle::spawn();
        let view = ok(sim.start_campaign(request()));
        let campaign_id = view.campaign_id.clone();
        assert_eq!(view.turns_completed, 0);
        assert!(matches!(view.state, SessionState::Active { .. }));

        let refetched = ok(sim.get_view(campaign_id.clone()));
        assert_eq!(refetched.campaign_id, campaign_id);

        // Drive through a full quarter (13 turns) so the settle-time flush
        // path (NullSink here) is actually exercised, not just started.
        for turn in 0..13 {
            let outcome = ok(sim.submit_decision(
                campaign_id.clone(),
                ActionIntent::Abstain,
                Some(50),
            ));
            // Abstain never posts a transaction.
            assert!(!outcome.ledger_effect);
            let advanced = ok(sim.advance(campaign_id.clone()));
            if turn == 12 {
                assert!(
                    advanced.period_close.is_some(),
                    "the 13th turn must close a quarter"
                );
            }
            assert!(matches!(advanced.state, SessionState::Active { .. }));
            assert!(advanced.next_observation.is_some());
        }

        ok(sim.abort(campaign_id.clone()));
        assert_eq!(
            sim.get_view(campaign_id),
            Err(SimUiErrorCode::CampaignNotFound),
            "abort must remove the session"
        );
    }

    #[test]
    fn unknown_campaign_id_is_reported_not_panicked_on() {
        let sim = BlackboxSimHandle::spawn();
        assert_eq!(
            sim.get_view("not-a-real-campaign".to_string()),
            Err(SimUiErrorCode::CampaignNotFound)
        );
        assert_eq!(
            sim.submit_decision("nope".to_string(), ActionIntent::Abstain, None),
            Err(SimUiErrorCode::CampaignNotFound)
        );
        assert_eq!(
            sim.advance("nope".to_string()),
            Err(SimUiErrorCode::CampaignNotFound)
        );
    }

    #[cfg(all(feature = "secure-vault", target_vendor = "apple"))]
    mod with_vault {
        use super::*;

        fn attached_sim() -> (BlackboxSimHandle, VaultHandle) {
            let sim = BlackboxSimHandle::spawn();
            let vault = match VaultHandle::spawn_test_unlocked() {
                Ok(v) => v,
                Err(e) => unreachable!("test vault setup failed: {e:?}"),
            };
            sim.attach_vault(vault.clone());
            (sim, vault)
        }

        /// The end-to-end round trip this whole phase exists for: a settle
        /// boundary triggers `attempt_flush`, which must reach the vault via
        /// `CollectingSink` and actually persist rows — not merely drain the
        /// session's own ring.
        #[test]
        fn a_quarter_close_flushes_decisions_to_the_vault() {
            let (sim, vault) = attached_sim();
            let view = ok(sim.start_campaign(request()));
            let campaign_id = view.campaign_id.clone();
            for _ in 0..13 {
                let _ = ok(sim.submit_decision(
                    campaign_id.clone(),
                    ActionIntent::Abstain,
                    None,
                ));
                let _ = ok(sim.advance(campaign_id.clone()));
            }
            let fingerprint = decode_campaign_id(&campaign_id).expect("valid hex id");
            let (loaded, decisions) = ok(vault.blackbox_load_campaign(fingerprint))
                .expect("campaign must have been registered and flushed");
            assert_eq!(loaded.campaign.scenario_id, 7);
            assert_eq!(loaded.created_date, "2026-07-27");
            assert_eq!(
                decisions.len(),
                13,
                "every decision in the closed quarter must have persisted"
            );
            assert!(decisions.iter().all(|d| d.intent == ActionIntent::Abstain));
        }

        /// `bxs_load_generation`'s whole reason to exist: rebuild a live
        /// session purely from the vault's persisted log via
        /// `replay_session`, with no snapshot bytes involved.
        #[test]
        fn load_generation_replays_a_persisted_campaign() {
            let (sim, vault) = attached_sim();
            let view = ok(sim.start_campaign(request()));
            let campaign_id = view.campaign_id.clone();
            for _ in 0..13 {
                let _ = ok(sim.submit_decision(
                    campaign_id.clone(),
                    ActionIntent::Abstain,
                    None,
                ));
                let _ = ok(sim.advance(campaign_id.clone()));
            }
            // A fresh handle with no in-memory session at all — this must
            // come purely from the vault.
            let fresh_sim = BlackboxSimHandle::spawn();
            fresh_sim.attach_vault(vault);
            let resumed = ok(fresh_sim.load_generation(campaign_id.clone(), 0));
            assert_eq!(resumed.campaign_id, campaign_id);
            assert_eq!(resumed.turns_completed, 13);
        }

        /// Exactly one quarter (generation 0) has closed and flushed; asking
        /// for generation 1 (which has not happened yet) must be reported,
        /// not silently served as if it existed.
        #[test]
        fn load_generation_beyond_what_was_persisted_is_reported() {
            let (sim, _vault) = attached_sim();
            let view = ok(sim.start_campaign(request()));
            let campaign_id = view.campaign_id.clone();
            for _ in 0..13 {
                let _ = ok(sim.submit_decision(
                    campaign_id.clone(),
                    ActionIntent::Abstain,
                    None,
                ));
                let _ = ok(sim.advance(campaign_id.clone()));
            }
            assert_eq!(
                sim.load_generation(campaign_id, 1),
                Err(SimUiErrorCode::GenerationNotFound)
            );
        }

        /// A partial quarter (no settle has happened yet) never reaches the
        /// vault at all — best-effort registration happens at flush time, so
        /// this must read as "nothing persisted", not a generation error.
        #[test]
        fn load_generation_before_any_flush_is_campaign_not_found() {
            let (sim, _vault) = attached_sim();
            let view = ok(sim.start_campaign(request()));
            let campaign_id = view.campaign_id.clone();
            for _ in 0..3 {
                let _ = ok(sim.submit_decision(
                    campaign_id.clone(),
                    ActionIntent::Abstain,
                    None,
                ));
                let _ = ok(sim.advance(campaign_id.clone()));
            }
            assert_eq!(
                sim.load_generation(campaign_id, 0),
                Err(SimUiErrorCode::CampaignNotFound)
            );
        }

        #[test]
        fn load_generation_without_a_vault_is_unavailable() {
            let sim = BlackboxSimHandle::spawn();
            // A well-formed (64 hex char) id so the failure genuinely comes
            // from the missing vault, not from id-shape validation.
            let campaign_id = campaign_id_of([3u8; 32]);
            assert_eq!(
                sim.load_generation(campaign_id, 0),
                Err(SimUiErrorCode::Unavailable)
            );
        }

        fn play_to_seal(sim: &BlackboxSimHandle, campaign_id: &str) {
            for _ in 0..crate::blackbox_sim::director::CAMPAIGN_TICKS {
                let _ = ok(sim.submit_decision(
                    campaign_id.to_string(),
                    ActionIntent::Abstain,
                    None,
                ));
                let advanced = ok(sim.advance(campaign_id.to_string()));
                if matches!(advanced.state, SessionState::Sealed) {
                    break;
                }
            }
        }

        #[cfg(feature = "blackbox-profile-write")]
        #[test]
        fn estimate_profile_persists_from_sealed_vault_campaigns() {
            let (sim, vault) = attached_sim();
            let view = ok(sim.start_campaign(request()));
            let campaign_id = view.campaign_id.clone();
            play_to_seal(&sim, &campaign_id);
            ok(sim.estimate_profile());
            let loaded = ok(vault.blackbox_latest_profile());
            assert!(
                loaded.is_some(),
                "live write must leave a profile row"
            );
        }

        #[cfg(not(feature = "blackbox-profile-write"))]
        #[test]
        fn estimate_profile_refuses_without_write_feature_and_writes_nothing() {
            let (sim, vault) = attached_sim();
            let view = ok(sim.start_campaign(request()));
            let campaign_id = view.campaign_id.clone();
            play_to_seal(&sim, &campaign_id);
            assert_eq!(
                sim.estimate_profile(),
                Err(SimUiErrorCode::BlackboxProfileWriteNotReady)
            );
            let latest = ok(vault.blackbox_latest_profile());
            assert!(
                latest.is_none(),
                "flag-off must not leave a profile row"
            );
        }
    }
}
