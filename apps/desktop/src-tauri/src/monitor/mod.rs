//! [B] Jetsam Monitor (docs/architecture_blueprint.md §3.3).
//!
//! Sampler thread + phase attribution. Tauri command lives in
//! `llm::commands_llm::memory_monitor_start` and is registered from `lib.rs`
//! under `#[cfg(feature = "pocket-brain")]` only.

mod probe;

use std::sync::atomic::{AtomicBool, AtomicU64, AtomicU8, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use serde::Serialize;
use tauri::ipc::Channel;

pub use probe::phys_footprint_bytes;

/// Lifecycle phase used to attribute footprint deltas.
#[derive(Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum MemPhase {
    Baseline,
    ModelLoaded,
    CtxCreated,
    Inference,
    Idle,
}

impl MemPhase {
    fn as_u8(self) -> u8 {
        match self {
            Self::Baseline => 0,
            Self::ModelLoaded => 1,
            Self::CtxCreated => 2,
            Self::Inference => 3,
            Self::Idle => 4,
        }
    }

    fn from_u8(v: u8) -> Self {
        match v {
            0 => Self::Baseline,
            1 => Self::ModelLoaded,
            2 => Self::CtxCreated,
            3 => Self::Inference,
            _ => Self::Idle,
        }
    }
}

/// One footprint sample streamed to the frontend over `tauri::ipc::Channel`.
#[derive(Clone, Serialize)]
pub struct MemSample {
    pub phase: MemPhase,
    pub phys_footprint_bytes: u64,
    pub delta_from_baseline_bytes: i64,
    pub threshold_bytes: u64,
    pub over_threshold: bool,
    pub headroom_bytes: i64,
    pub t_ms: u64,
}

/// Lock-free hook invoked on the rising edge of `over_threshold`.
/// Typically wired to `LlmMemoryGovernor::request_purge` (two atomic stores).
pub type OverThresholdHook = Arc<dyn Fn() + Send + Sync + 'static>;

/// Background footprint monitor. `phase`/`baseline`/`running` are shared with the
/// sampler thread via atomics so the LLM worker can mark phase transitions without
/// locking.
pub struct MemoryMonitor {
    running: Arc<AtomicBool>,
    phase: Arc<AtomicU8>,
    baseline: Arc<AtomicU64>,
}

impl MemoryMonitor {
    pub fn new() -> Self {
        Self {
            running: Arc::new(AtomicBool::new(false)),
            phase: Arc::new(AtomicU8::new(MemPhase::Baseline.as_u8())),
            baseline: Arc::new(AtomicU64::new(0)),
        }
    }

    /// Spawn the sampler thread. Records the baseline footprint from the first
    /// reading, then emits a `MemSample` every `interval_ms` until `stop()`.
    /// `threshold_bytes` is the jetsam budget (e.g. A17 Pro/8GB ≈ 4.8 GB = 60%).
    ///
    /// On the rising edge of `over_threshold`, `over_threshold_hook` is invoked
    /// (lock-free expected). Caller should `stop()` a previous run before
    /// starting a new one.
    pub fn start(
        &self,
        channel: Channel<MemSample>,
        interval_ms: u64,
        threshold_bytes: u64,
        over_threshold_hook: Option<OverThresholdHook>,
    ) {
        let base = phys_footprint_bytes().unwrap_or(0);
        self.baseline.store(base, Ordering::SeqCst);
        self.phase.store(MemPhase::Baseline.as_u8(), Ordering::SeqCst);
        self.running.store(true, Ordering::SeqCst);

        let running = Arc::clone(&self.running);
        let phase = Arc::clone(&self.phase);
        let baseline = Arc::clone(&self.baseline);
        let interval = Duration::from_millis(interval_ms.max(1));

        thread::spawn(move || {
            let t0 = Instant::now();
            let mut was_over = false;
            while running.load(Ordering::SeqCst) {
                let cur = phys_footprint_bytes().unwrap_or(0);
                let base = baseline.load(Ordering::SeqCst);
                let over = cur >= threshold_bytes;
                // Rising-edge only: continuous over-threshold must not spam purge.
                if over && !was_over {
                    if let Some(ref hook) = over_threshold_hook {
                        hook();
                    }
                }
                was_over = over;
                let sample = MemSample {
                    phase: MemPhase::from_u8(phase.load(Ordering::SeqCst)),
                    phys_footprint_bytes: cur,
                    delta_from_baseline_bytes: cur as i64 - base as i64,
                    threshold_bytes,
                    over_threshold: over,
                    headroom_bytes: threshold_bytes as i64 - cur as i64,
                    t_ms: t0.elapsed().as_millis() as u64,
                };
                // Frontend hung up (channel closed) → stop sampling.
                if channel.send(sample).is_err() {
                    break;
                }
                thread::sleep(interval);
            }
        });
    }

    /// The LLM worker calls this on each phase transition.
    pub fn set_phase(&self, phase: MemPhase) {
        self.phase.store(phase.as_u8(), Ordering::SeqCst);
    }

    /// Stop the sampler thread (it exits on its next tick).
    pub fn stop(&self) {
        self.running.store(false, Ordering::SeqCst);
    }
}

impl Default for MemoryMonitor {
    fn default() -> Self {
        Self::new()
    }
}
