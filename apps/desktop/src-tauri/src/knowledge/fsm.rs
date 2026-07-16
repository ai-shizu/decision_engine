//! E0b research transaction typestate FSM (move-consuming; no Clone).
//! Parent: docs/SPEC_E0B_STEP4_RUST_FSM.md
//!
//! # Typestate compile-time proofs
//!
//! Passing setup: `Txn<Pending>` exposes `transition_to_fetching`.
//! ```
//! use pkb_desktop_lib::knowledge::fsm::{Txn, Pending, Fetching};
//! fn ok_pending_to_fetching(p: Txn<Pending>) -> Txn<Fetching> {
//!     p.transition_to_fetching()
//! }
//! ```
//!
//! `Txn<Pending>` is not `Clone`.
//! ```compile_fail
//! use pkb_desktop_lib::knowledge::fsm::{Txn, Pending};
//! fn needs_clone<T: Clone>() {}
//! needs_clone::<Txn<Pending>>();
//! ```
//!
//! Passing setup: `Txn<Fetching>` exposes `transition_to_ready`.
//! ```
//! use pkb_desktop_lib::knowledge::fsm::{Txn, Fetching, ReadyToIntegrate};
//! fn ok_fetching_to_ready(f: Txn<Fetching>) -> Txn<ReadyToIntegrate> {
//!     f.transition_to_ready()
//! }
//! ```
//!
//! Double-fetch is impossible: `transition_to_fetching` exists only on `Pending`.
//! ```compile_fail
//! use pkb_desktop_lib::knowledge::fsm::{Txn, Fetching};
//! fn double_fetch(f: Txn<Fetching>) {
//!     let _ = f.transition_to_fetching();
//! }
//! ```
//!
//! Passing setup: forward-only to completed.
//! ```
//! use pkb_desktop_lib::knowledge::fsm::{Txn, ReadyToIntegrate, Completed};
//! fn ok_ready_to_completed(r: Txn<ReadyToIntegrate>) -> Txn<Completed> {
//!     r.transition_to_completed()
//! }
//! ```
//!
//! No reverse transition from `Fetching` back to `Pending`.
//! ```compile_fail
//! use pkb_desktop_lib::knowledge::fsm::{Txn, Fetching, Pending};
//! fn go_back(f: Txn<Fetching>) -> Txn<Pending> {
//!     f.transition_to_pending()
//! }
//! ```
//!
//! Passing setup: move-consuming transition returns a new owner.
//! ```
//! use pkb_desktop_lib::knowledge::fsm::{Txn, Pending, Fetching};
//! fn ok_move_once(p: Txn<Pending>) -> Txn<Fetching> {
//!     let f = p.transition_to_fetching();
//!     f
//! }
//! ```
//!
//! After move, the original `Pending` value cannot be reused.
//! ```compile_fail
//! use pkb_desktop_lib::knowledge::fsm::{Txn, Pending};
//! fn use_after_move(p: Txn<Pending>) {
//!     let _f = p.transition_to_fetching();
//!     let _again = p.transition_to_fetching();
//! }
//! ```
#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::string_slice
)]

use std::collections::HashSet;
use std::marker::PhantomData;
use std::sync::{Arc, Mutex, PoisonError};

use crate::knowledge::dual_run::AttestedIntentPayload;

/// Zero-sized state markers — never `Clone`/`Copy`.
#[derive(Debug)]
pub struct Pending;
#[derive(Debug)]
pub struct Fetching;
#[derive(Debug)]
pub struct ReadyToIntegrate;
#[derive(Debug)]
pub struct Completed;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FsmError {
    Busy,
    ReplayDetected,
    DictionaryDrift,
    MalformedNonce,
}

impl std::fmt::Display for FsmError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Busy => write!(f, "research slot busy"),
            Self::ReplayDetected => write!(f, "txn nonce replay detected"),
            Self::DictionaryDrift => write!(f, "PII dictionary hash drift"),
            Self::MalformedNonce => write!(f, "malformed txn nonce"),
        }
    }
}

impl std::error::Error for FsmError {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AbortReason {
    Explicit,
}

struct ResearchSlotInner {
    occupied: bool,
    used_nonces: HashSet<[u8; 32]>,
    current_dict_hash: String,
}

/// Shared research manager (Arc-shareable). Holds the single-flight slot + nonce ledger.
pub struct ResearchSlot {
    inner: Mutex<ResearchSlotInner>,
}

/// RAII guard: Drop always clears `occupied` (terminal / abort / mid-drop).
pub struct SlotGuard {
    slot: Arc<ResearchSlot>,
}

impl Drop for SlotGuard {
    fn drop(&mut self) {
        let mut guard = self
            .slot
            .inner
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        guard.occupied = false;
    }
}

/// Typestate transaction value. Move-only; never `Clone`/`Copy`.
pub struct Txn<S> {
    _state: PhantomData<S>,
    #[allow(dead_code)]
    guard: SlotGuard,
    #[allow(dead_code)]
    payload: AttestedIntentPayload,
}

impl ResearchSlot {
    pub fn new(dict_hash: impl Into<String>) -> Arc<Self> {
        Arc::new(Self {
            inner: Mutex::new(ResearchSlotInner {
                occupied: false,
                used_nonces: HashSet::new(),
                current_dict_hash: dict_hash.into(),
            }),
        })
    }

    pub fn set_dictionary(&self, new_hash: impl Into<String>) {
        let mut guard = self.inner.lock().unwrap_or_else(PoisonError::into_inner);
        guard.current_dict_hash = new_hash.into();
    }

    /// Atomic begin (§1.3): Busy → Drift → parse nonce → Replay → occupy+consume.
    pub fn begin(
        self: &Arc<Self>,
        payload: AttestedIntentPayload,
    ) -> Result<Txn<Pending>, FsmError> {
        let mut guard = self.inner.lock().unwrap_or_else(PoisonError::into_inner);

        // 1. single-flight
        if guard.occupied {
            return Err(FsmError::Busy);
        }
        // 2. dictionary revision lock
        if payload.dict_hash != guard.current_dict_hash {
            return Err(FsmError::DictionaryDrift);
        }
        // 3. parse nonce (fail-closed; does not occupy)
        let nonce = parse_nonce(&payload.txn_nonce)?;
        // 4. anti-replay
        if guard.used_nonces.contains(&nonce) {
            return Err(FsmError::ReplayDetected);
        }
        // 5. consume nonce + occupy (only on full success)
        guard.used_nonces.insert(nonce);
        guard.occupied = true;
        drop(guard);

        Ok(Txn {
            _state: PhantomData,
            guard: SlotGuard {
                slot: Arc::clone(self),
            },
            payload,
        })
    }
}

fn parse_nonce(hex_str: &str) -> Result<[u8; 32], FsmError> {
    if hex_str.len() != 64 {
        return Err(FsmError::MalformedNonce);
    }
    if !hex_str.chars().all(|c| matches!(c, '0'..='9' | 'a'..='f')) {
        return Err(FsmError::MalformedNonce);
    }
    let bytes = hex::decode(hex_str).map_err(|_| FsmError::MalformedNonce)?;
    <[u8; 32]>::try_from(bytes).map_err(|_| FsmError::MalformedNonce)
}

impl Txn<Pending> {
    pub fn transition_to_fetching(self) -> Txn<Fetching> {
        Txn {
            _state: PhantomData,
            guard: self.guard,
            payload: self.payload,
        }
    }
}

impl Txn<Fetching> {
    pub fn transition_to_ready(self) -> Txn<ReadyToIntegrate> {
        Txn {
            _state: PhantomData,
            guard: self.guard,
            payload: self.payload,
        }
    }
}

impl Txn<ReadyToIntegrate> {
    pub fn transition_to_completed(self) -> Txn<Completed> {
        Txn {
            _state: PhantomData,
            guard: self.guard,
            payload: self.payload,
        }
    }
}

impl<S> Txn<S> {
    /// Consume the transaction and release the slot via `SlotGuard::drop`.
    pub fn abort(self, reason: AbortReason) -> AbortReason {
        drop(self);
        reason
    }

    pub fn payload(&self) -> &AttestedIntentPayload {
        &self.payload
    }
}
