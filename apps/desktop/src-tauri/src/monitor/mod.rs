//! [B] Thermal / Jetsam Monitor (docs/architecture_blueprint.md §3.3 + Phase 4).
//!
//! Apple: event-driven memory pressure + slow thermal/footprint telemetry.
//! Non-Apple: adaptive low-frequency footprint polling (no fixed 500ms spin).

mod degradation;
mod probe;

#[cfg(target_vendor = "apple")]
mod apple_sensors;

use std::sync::atomic::{AtomicBool, AtomicU64, AtomicU8, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use serde::Serialize;
use tauri::ipc::Channel;

pub use degradation::DegradationLevel;
use degradation::combine_degradation;
#[cfg(not(target_vendor = "apple"))]
use degradation::PressureClass;
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
    /// Progressive degradation ladder (Nominal→Critical).
    pub degradation: DegradationLevel,
}

/// Lock-free hook invoked on Critical / over-threshold rising edge.
/// Typically wired to `LlmMemoryGovernor::request_purge` (two atomic stores).
pub type OverThresholdHook = Arc<dyn Fn() + Send + Sync + 'static>;

/// Invoked when the ladder enters Serious (FE warning / throttle).
pub type DegradationHook = Arc<dyn Fn(DegradationLevel) + Send + Sync + 'static>;

/// Background footprint / thermal monitor.
pub struct MemoryMonitor {
    running: Arc<AtomicBool>,
    phase: Arc<AtomicU8>,
    baseline: Arc<AtomicU64>,
    degradation: Arc<AtomicU8>,
}

impl MemoryMonitor {
    pub fn new() -> Self {
        Self {
            running: Arc::new(AtomicBool::new(false)),
            phase: Arc::new(AtomicU8::new(MemPhase::Baseline.as_u8())),
            baseline: Arc::new(AtomicU64::new(0)),
            degradation: Arc::new(AtomicU8::new(DegradationLevel::Nominal.as_u8())),
        }
    }

    /// Spawn the monitor. Apple uses pressure events + slow telemetry; others use
    /// adaptive polling. `interval_ms` from the FE is a hint only (clamped).
    pub fn start(
        &self,
        channel: Channel<MemSample>,
        interval_ms: u64,
        threshold_bytes: u64,
        over_threshold_hook: Option<OverThresholdHook>,
        degradation_hook: Option<DegradationHook>,
    ) {
        self.running.store(false, Ordering::SeqCst);
        let base = phys_footprint_bytes().unwrap_or(0);
        self.baseline.store(base, Ordering::SeqCst);
        self.phase.store(MemPhase::Baseline.as_u8(), Ordering::SeqCst);
        self.degradation
            .store(DegradationLevel::Nominal.as_u8(), Ordering::SeqCst);
        self.running.store(true, Ordering::SeqCst);

        let running = Arc::clone(&self.running);
        let phase = Arc::clone(&self.phase);
        let baseline = Arc::clone(&self.baseline);
        let degradation = Arc::clone(&self.degradation);

        #[cfg(target_vendor = "apple")]
        {
            let sensors = apple_sensors::AppleSensorState::new();
            apple_sensors::install_memory_pressure_watch(
                Arc::clone(&sensors),
                over_threshold_hook.clone(),
            );
            let sensors_thread = Arc::clone(&sensors);
            thread::spawn(move || {
                apple_telemetry_loop(
                    running,
                    phase,
                    baseline,
                    degradation,
                    sensors_thread,
                    channel,
                    threshold_bytes,
                    interval_ms,
                    over_threshold_hook,
                    degradation_hook,
                );
            });
        }

        #[cfg(not(target_vendor = "apple"))]
        {
            thread::spawn(move || {
                adaptive_poll_loop(
                    running,
                    phase,
                    baseline,
                    degradation,
                    channel,
                    threshold_bytes,
                    interval_ms,
                    over_threshold_hook,
                    degradation_hook,
                );
            });
        }
    }

    pub fn set_phase(&self, phase: MemPhase) {
        self.phase.store(phase.as_u8(), Ordering::SeqCst);
    }

    pub fn stop(&self) {
        self.running.store(false, Ordering::SeqCst);
    }
}

impl Default for MemoryMonitor {
    fn default() -> Self {
        Self::new()
    }
}

fn emit_sample(
    channel: &Channel<MemSample>,
    phase: &AtomicU8,
    baseline: &AtomicU64,
    degradation: DegradationLevel,
    threshold_bytes: u64,
    t0: Instant,
) -> bool {
    let cur = phys_footprint_bytes().unwrap_or(0);
    let base = baseline.load(Ordering::SeqCst);
    let over = cur >= threshold_bytes;
    let sample = MemSample {
        phase: MemPhase::from_u8(phase.load(Ordering::SeqCst)),
        phys_footprint_bytes: cur,
        delta_from_baseline_bytes: cur as i64 - base as i64,
        threshold_bytes,
        over_threshold: over,
        headroom_bytes: threshold_bytes as i64 - cur as i64,
        t_ms: t0.elapsed().as_millis() as u64,
        degradation,
    };
    channel.send(sample).is_ok()
}

fn apply_level(
    degradation: &AtomicU8,
    level: DegradationLevel,
    was_critical: &mut bool,
    was_serious: &mut bool,
    over_threshold_hook: &Option<OverThresholdHook>,
    degradation_hook: &Option<DegradationHook>,
) {
    let prev = DegradationLevel::from_u8(degradation.swap(level.as_u8(), Ordering::SeqCst));
    if level != prev {
        if let Some(ref hook) = degradation_hook {
            hook(level);
        }
    }
    if level == DegradationLevel::Critical && !*was_critical {
        *was_critical = true;
        if let Some(ref hook) = over_threshold_hook {
            hook();
        }
    }
    if level < DegradationLevel::Critical {
        *was_critical = false;
    }
    if level >= DegradationLevel::Serious && (prev < DegradationLevel::Serious || !*was_serious)
    {
        *was_serious = true;
        // Serious+ already notified via degradation_hook; keep flag for rising-edge.
    }
    if level < DegradationLevel::Serious {
        *was_serious = false;
    }
}

/// Apple: ≥5s telemetry; pressure events drive Critical independently.
#[cfg(target_vendor = "apple")]
fn apple_telemetry_loop(
    running: Arc<AtomicBool>,
    phase: Arc<AtomicU8>,
    baseline: Arc<AtomicU64>,
    degradation: Arc<AtomicU8>,
    sensors: Arc<apple_sensors::AppleSensorState>,
    channel: Channel<MemSample>,
    threshold_bytes: u64,
    interval_ms: u64,
    over_threshold_hook: Option<OverThresholdHook>,
    degradation_hook: Option<DegradationHook>,
) {
    // Floor at 5s — never honor a 500ms battery-burning request on Apple.
    let tick = Duration::from_millis(interval_ms.max(5_000));
    let t0 = Instant::now();
    let mut was_critical = false;
    let mut was_serious = false;

    while running.load(Ordering::SeqCst) {
        apple_sensors::refresh_thermal(&sensors);
        let cur = phys_footprint_bytes().unwrap_or(0);
        let ratio = if threshold_bytes == 0 {
            0.0
        } else {
            cur as f64 / threshold_bytes as f64
        };
        let level = combine_degradation(
            sensors.thermal_level(),
            sensors.pressure_class(),
            ratio,
        );
        apply_level(
            &degradation,
            level,
            &mut was_critical,
            &mut was_serious,
            &over_threshold_hook,
            &degradation_hook,
        );
        if !emit_sample(
            &channel,
            &phase,
            &baseline,
            level,
            threshold_bytes,
            t0,
        ) {
            break;
        }
        thread::sleep(tick);
    }
}

/// Non-Apple fallback: adaptive interval from footprint ratio (1s–5s).
#[cfg(not(target_vendor = "apple"))]
fn adaptive_poll_loop(
    running: Arc<AtomicBool>,
    phase: Arc<AtomicU8>,
    baseline: Arc<AtomicU64>,
    degradation: Arc<AtomicU8>,
    channel: Channel<MemSample>,
    threshold_bytes: u64,
    _interval_ms: u64,
    over_threshold_hook: Option<OverThresholdHook>,
    degradation_hook: Option<DegradationHook>,
) {
    let t0 = Instant::now();
    let mut was_critical = false;
    let mut was_serious = false;

    while running.load(Ordering::SeqCst) {
        let cur = phys_footprint_bytes().unwrap_or(0);
        let ratio = if threshold_bytes == 0 {
            0.0
        } else {
            cur as f64 / threshold_bytes as f64
        };
        let level = combine_degradation(
            DegradationLevel::Nominal,
            PressureClass::Normal,
            ratio,
        );
        apply_level(
            &degradation,
            level,
            &mut was_critical,
            &mut was_serious,
            &over_threshold_hook,
            &degradation_hook,
        );
        if !emit_sample(
            &channel,
            &phase,
            &baseline,
            level,
            threshold_bytes,
            t0,
        ) {
            break;
        }
        let sleep_ms = if ratio >= 0.90 {
            1_000
        } else if ratio >= 0.70 {
            2_000
        } else {
            5_000
        };
        thread::sleep(Duration::from_millis(sleep_ms));
    }
}
