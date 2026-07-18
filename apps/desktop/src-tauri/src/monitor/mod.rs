//! [B] Jetsam Monitor — M4 Phase 0 stubs (docs/architecture_blueprint.md §3.3).
//!
//! **STUBS ONLY — NO LOGIC.** Bodies are `todo!()`. Compiled only under the
//! `pocket-brain` feature (wired in `lib.rs`) per the M4 directive's isolation rule.
//!
//! Phase 1 fills these in using `proc_pid_rusage(RUSAGE_INFO_V2).ri_phys_footprint`
//! (the exact jetsam-ledger metric; `MACH_TASK_BASIC_INFO` was rejected in the M0
//! spike because it only exposes `resident_size`). `probe.rs` (the raw libc probe)
//! and the sampler thread land with that logic, not in Phase 0.

use serde::Serialize;

/// Lifecycle phase used to attribute footprint deltas (blueprint §3.3).
#[derive(Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum MemPhase {
    Baseline,
    ModelLoaded,
    CtxCreated,
    Inference,
    Idle,
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

/// Background footprint monitor. Phase 0 stub — no sampler thread yet.
pub struct MemoryMonitor;

impl MemoryMonitor {
    /// Phase 1: construct the monitor (atomics for phase/baseline).
    pub fn new() -> Self {
        todo!("M4 Phase 1: MemoryMonitor::new")
    }

    /// Phase 1: worker calls this on each phase transition.
    pub fn set_phase(&self, _phase: MemPhase) {
        todo!("M4 Phase 1: set_phase")
    }

    /// Phase 1: stop the sampler thread.
    pub fn stop(&self) {
        todo!("M4 Phase 1: stop")
    }
}
