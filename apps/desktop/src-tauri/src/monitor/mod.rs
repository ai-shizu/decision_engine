//! [B] Thermal / Jetsam Monitor (docs/architecture_blueprint.md §3.3 + Phase 4).
//!
//! Apple: event-driven memory pressure + slow thermal/footprint telemetry.
//! Non-Apple: adaptive low-frequency footprint polling (no fixed 500ms spin).

mod degradation;
mod probe;

#[cfg(target_vendor = "apple")]
mod apple_sensors;

use std::sync::atomic::{AtomicBool, AtomicU64, AtomicU8, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use serde::Serialize;
use tauri::ipc::Channel;

use degradation::combine_degradation;
pub use degradation::DegradationLevel;
#[cfg(not(target_vendor = "apple"))]
use degradation::PressureClass;
pub use probe::{os_proc_available_memory_bytes, phys_footprint_bytes};

/// Lifecycle phase used to attribute footprint deltas.
#[derive(Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum MemPhase {
    Baseline,
    ModelLoaded,
    CtxCreated,
    Inference,
    Idle,
    EdinetFetch,
    EdinetExtract,
}

impl MemPhase {
    fn as_u8(self) -> u8 {
        match self {
            Self::Baseline => 0,
            Self::ModelLoaded => 1,
            Self::CtxCreated => 2,
            Self::Inference => 3,
            Self::Idle => 4,
            Self::EdinetFetch => 5,
            Self::EdinetExtract => 6,
        }
    }

    fn from_u8(v: u8) -> Self {
        match v {
            0 => Self::Baseline,
            1 => Self::ModelLoaded,
            2 => Self::CtxCreated,
            3 => Self::Inference,
            4 => Self::Idle,
            5 => Self::EdinetFetch,
            6 => Self::EdinetExtract,
            _ => Self::Idle,
        }
    }
}

#[cfg(target_os = "ios")]
fn phase_label(phase: MemPhase) -> &'static str {
    match phase {
        MemPhase::Baseline => "phase.baseline",
        MemPhase::ModelLoaded => "phase.model_loaded",
        MemPhase::CtxCreated => "phase.ctx_created",
        MemPhase::Inference => "phase.inference",
        MemPhase::Idle => "phase.idle",
        MemPhase::EdinetFetch => "phase.edinet_fetch",
        MemPhase::EdinetExtract => "phase.edinet_extract",
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
/// Typically wired to `LlmMemoryGovernor::request_purge` (three atomic updates).
pub type OverThresholdHook = Arc<dyn Fn() + Send + Sync + 'static>;

/// Invoked when the ladder enters Serious (FE warning / throttle).
pub type DegradationHook = Arc<dyn Fn(DegradationLevel) + Send + Sync + 'static>;

struct MonitorControl {
    running: AtomicBool,
    gate: Mutex<()>,
    wake: Condvar,
}

impl MonitorControl {
    fn new() -> Self {
        Self {
            running: AtomicBool::new(false),
            gate: Mutex::new(()),
            wake: Condvar::new(),
        }
    }

    fn start(&self) {
        if let Ok(_guard) = self.gate.lock() {
            self.running.store(true, Ordering::SeqCst);
        }
    }

    fn stop(&self) {
        if let Ok(_guard) = self.gate.lock() {
            self.running.store(false, Ordering::SeqCst);
            self.wake.notify_all();
        } else {
            self.running.store(false, Ordering::SeqCst);
            self.wake.notify_all();
        }
    }

    fn is_running(&self) -> bool {
        self.running.load(Ordering::SeqCst)
    }

    /// Interruptible replacement for `thread::sleep`; stop/restart never waits
    /// for the 5-second telemetry cadence to expire.
    fn wait(&self, duration: Duration) -> bool {
        let Ok(guard) = self.gate.lock() else {
            return false;
        };
        if !self.is_running() {
            return false;
        }
        match self
            .wake
            .wait_timeout_while(guard, duration, |_| self.is_running())
        {
            Ok(_) => self.is_running(),
            Err(_) => false,
        }
    }
}

/// Background footprint / thermal monitor.
pub struct MemoryMonitor {
    control: Arc<MonitorControl>,
    worker: Mutex<Option<JoinHandle<()>>>,
    phase: Arc<AtomicU8>,
    baseline: Arc<AtomicU64>,
    degradation: Arc<AtomicU8>,
    #[cfg(target_vendor = "apple")]
    apple_sensors: Arc<apple_sensors::AppleSensorState>,
    #[cfg(target_vendor = "apple")]
    pressure_watch_installed: AtomicBool,
}

impl MemoryMonitor {
    pub fn new() -> Self {
        Self {
            control: Arc::new(MonitorControl::new()),
            worker: Mutex::new(None),
            phase: Arc::new(AtomicU8::new(MemPhase::Baseline.as_u8())),
            baseline: Arc::new(AtomicU64::new(0)),
            degradation: Arc::new(AtomicU8::new(DegradationLevel::Nominal.as_u8())),
            #[cfg(target_vendor = "apple")]
            apple_sensors: apple_sensors::AppleSensorState::new(),
            #[cfg(target_vendor = "apple")]
            pressure_watch_installed: AtomicBool::new(false),
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
    ) -> Result<(), String> {
        let mut worker = self
            .worker
            .lock()
            .map_err(|_| "memory monitor lifecycle poisoned".to_string())?;
        self.control.stop();
        if let Some(previous) = worker.take() {
            previous
                .join()
                .map_err(|_| "previous memory monitor thread panicked".to_string())?;
        }

        let base = phys_footprint_bytes().unwrap_or(0);
        self.baseline.store(base, Ordering::SeqCst);
        self.phase
            .store(MemPhase::Baseline.as_u8(), Ordering::SeqCst);
        self.degradation
            .store(DegradationLevel::Nominal.as_u8(), Ordering::SeqCst);
        self.control.start();

        let control = Arc::clone(&self.control);
        let phase = Arc::clone(&self.phase);
        let baseline = Arc::clone(&self.baseline);
        let degradation = Arc::clone(&self.degradation);

        #[cfg(target_vendor = "apple")]
        let spawned = {
            let sensors = Arc::clone(&self.apple_sensors);
            if !self.pressure_watch_installed.swap(true, Ordering::SeqCst) {
                // The dispatch source is process-lifetime; installing it for
                // every React remount leaks callbacks and repeats purge signals.
                apple_sensors::install_memory_pressure_watch(
                    Arc::clone(&sensors),
                    over_threshold_hook.clone(),
                );
            }
            let sensors_thread = Arc::clone(&sensors);
            thread::Builder::new()
                .name("coraxis-memory-monitor".into())
                .spawn(move || {
                    apple_telemetry_loop(
                        control,
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
                })
        };

        #[cfg(not(target_vendor = "apple"))]
        let spawned = thread::Builder::new()
            .name("coraxis-memory-monitor".into())
            .spawn(move || {
                adaptive_poll_loop(
                    control,
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

        match spawned {
            Ok(handle) => {
                *worker = Some(handle);
                Ok(())
            }
            Err(error) => {
                self.control.stop();
                Err(format!("memory monitor spawn failed: {error}"))
            }
        }
    }

    pub fn set_phase(&self, phase: MemPhase) {
        self.phase.store(phase.as_u8(), Ordering::SeqCst);
        #[cfg(target_os = "ios")]
        {
            if let Some(bytes) = phys_footprint_bytes() {
                crate::ios_oslog::log_footprint_bytes(phase_label(phase), bytes);
            }
        }
    }

    pub fn stop(&self) -> Result<(), String> {
        let mut worker = self
            .worker
            .lock()
            .map_err(|_| "memory monitor lifecycle poisoned".to_string())?;
        self.control.stop();
        if let Some(handle) = worker.take() {
            handle
                .join()
                .map_err(|_| "memory monitor thread panicked".to_string())?;
        }
        Ok(())
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
    if level >= DegradationLevel::Serious && (prev < DegradationLevel::Serious || !*was_serious) {
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
    control: Arc<MonitorControl>,
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

    while control.is_running() {
        apple_sensors::refresh_thermal(&sensors);
        let cur = phys_footprint_bytes().unwrap_or(0);
        let ratio = if threshold_bytes == 0 {
            0.0
        } else {
            cur as f64 / threshold_bytes as f64
        };
        let level = combine_degradation(sensors.thermal_level(), sensors.pressure_class(), ratio);
        apply_level(
            &degradation,
            level,
            &mut was_critical,
            &mut was_serious,
            &over_threshold_hook,
            &degradation_hook,
        );
        if !emit_sample(&channel, &phase, &baseline, level, threshold_bytes, t0) {
            break;
        }
        if !control.wait(tick) {
            break;
        }
    }
}

/// Non-Apple fallback: adaptive interval from footprint ratio (1s–5s).
#[cfg(not(target_vendor = "apple"))]
fn adaptive_poll_loop(
    control: Arc<MonitorControl>,
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

    while control.is_running() {
        let cur = phys_footprint_bytes().unwrap_or(0);
        let ratio = if threshold_bytes == 0 {
            0.0
        } else {
            cur as f64 / threshold_bytes as f64
        };
        let level = combine_degradation(DegradationLevel::Nominal, PressureClass::Normal, ratio);
        apply_level(
            &degradation,
            level,
            &mut was_critical,
            &mut was_serious,
            &over_threshold_hook,
            &degradation_hook,
        );
        if !emit_sample(&channel, &phase, &baseline, level, threshold_bytes, t0) {
            break;
        }
        let sleep_ms = if ratio >= 0.90 {
            1_000
        } else if ratio >= 0.70 {
            2_000
        } else {
            5_000
        };
        if !control.wait(Duration::from_millis(sleep_ms)) {
            break;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn monitor_wait_is_interrupted_by_stop() {
        let control = Arc::new(MonitorControl::new());
        control.start();
        let worker_control = Arc::clone(&control);
        let (ready, ready_rx) = std::sync::mpsc::sync_channel(1);
        let worker = thread::spawn(move || {
            let _ = ready.send(());
            let started = Instant::now();
            let still_running = worker_control.wait(Duration::from_secs(5));
            (still_running, started.elapsed())
        });
        ready_rx.recv().expect("waiter ready");
        control.stop();
        let (still_running, elapsed) = worker.join().expect("waiter joins");
        assert!(!still_running);
        assert!(elapsed < Duration::from_secs(1));
    }
}
