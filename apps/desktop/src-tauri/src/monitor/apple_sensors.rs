//! Apple thermal + memory-pressure sensors (Phase 4).
//!
//! Callbacks only touch atomics / lock-free hooks — never Mutex or model Drop
//! (AI_SKILLS §1.1). Thermal uses Foundation `NSProcessInfo.thermalState` via
//! the ObjC runtime; pressure uses `DISPATCH_SOURCE_TYPE_MEMORYPRESSURE`.

use std::ffi::c_void;
use std::os::raw::{c_char, c_long, c_ulong};
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::Arc;

use super::degradation::{DegradationLevel, PressureClass};

const DISPATCH_MEMORYPRESSURE_NORMAL: c_ulong = 0x01;
const DISPATCH_MEMORYPRESSURE_WARN: c_ulong = 0x02;
const DISPATCH_MEMORYPRESSURE_CRITICAL: c_ulong = 0x04;
const DISPATCH_QUEUE_PRIORITY_DEFAULT: c_long = 0;

type DispatchObject = *mut c_void;
type DispatchQueue = *mut c_void;
type DispatchSource = *mut c_void;

#[link(name = "System", kind = "dylib")]
extern "C" {
    static _dispatch_source_type_memorypressure: c_void;
    fn dispatch_get_global_queue(identifier: c_long, flags: c_ulong) -> DispatchQueue;
    fn dispatch_source_create(
        type_: *const c_void,
        handle: c_ulong,
        mask: c_ulong,
        queue: DispatchQueue,
    ) -> DispatchSource;
    fn dispatch_source_set_event_handler_f(
        source: DispatchSource,
        handler: Option<unsafe extern "C" fn(*mut c_void)>,
    );
    fn dispatch_set_context(object: DispatchObject, context: *mut c_void);
    fn dispatch_source_get_data(source: DispatchSource) -> c_ulong;
    fn dispatch_resume(object: DispatchObject);
}

#[link(name = "objc")]
extern "C" {
    fn objc_getClass(name: *const c_char) -> *mut c_void;
    fn sel_registerName(name: *const c_char) -> *const c_void;
    fn objc_msgSend();
}

#[link(name = "Foundation", kind = "framework")]
extern "C" {}

fn pressure_store_u8(class: PressureClass) -> u8 {
    match class {
        PressureClass::Normal => 0,
        PressureClass::Warn => 1,
        PressureClass::Critical => 2,
    }
}

fn pressure_from_u8(v: u8) -> PressureClass {
    match v {
        1 => PressureClass::Warn,
        2 => PressureClass::Critical,
        _ => PressureClass::Normal,
    }
}

/// Shared sensor snapshot written by dispatch/ObjC and read by the telemetry thread.
pub struct AppleSensorState {
    pub thermal: AtomicU8,
    pub pressure: AtomicU8,
}

impl AppleSensorState {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            thermal: AtomicU8::new(DegradationLevel::Nominal.as_u8()),
            pressure: AtomicU8::new(pressure_store_u8(PressureClass::Normal)),
        })
    }

    pub fn thermal_level(&self) -> DegradationLevel {
        DegradationLevel::from_u8(self.thermal.load(Ordering::SeqCst))
    }

    pub fn pressure_class(&self) -> PressureClass {
        pressure_from_u8(self.pressure.load(Ordering::SeqCst))
    }
}

/// Read `NSProcessInfo.thermalState` (Nominal/Fair/Serious/Critical).
pub fn read_thermal_state() -> DegradationLevel {
    // SAFETY: Foundation is linked; selectors are immortal; return is NSInteger.
    unsafe {
        let cls = objc_getClass(c"NSProcessInfo".as_ptr());
        if cls.is_null() {
            return DegradationLevel::Nominal;
        }
        let process_info_sel = sel_registerName(c"processInfo".as_ptr());
        let thermal_sel = sel_registerName(c"thermalState".as_ptr());
        let msg_id: unsafe extern "C" fn(*mut c_void, *const c_void) -> *mut c_void =
            std::mem::transmute(objc_msgSend as unsafe extern "C" fn());
        let msg_isize: unsafe extern "C" fn(*mut c_void, *const c_void) -> isize =
            std::mem::transmute(objc_msgSend as unsafe extern "C" fn());
        let info = msg_id(cls, process_info_sel);
        if info.is_null() {
            return DegradationLevel::Nominal;
        }
        match msg_isize(info, thermal_sel) {
            1 => DegradationLevel::Fair,
            2 => DegradationLevel::Serious,
            3 => DegradationLevel::Critical,
            _ => DegradationLevel::Nominal,
        }
    }
}

struct PressureContext {
    source: DispatchSource,
    state: Arc<AppleSensorState>,
    on_critical: Option<Arc<dyn Fn() + Send + Sync + 'static>>,
}

/// Install a process-lifetime memory-pressure source.
/// Context is intentionally leaked — the source lives until process exit.
pub fn install_memory_pressure_watch(
    state: Arc<AppleSensorState>,
    on_critical: Option<Arc<dyn Fn() + Send + Sync + 'static>>,
) {
    let mask = DISPATCH_MEMORYPRESSURE_NORMAL
        | DISPATCH_MEMORYPRESSURE_WARN
        | DISPATCH_MEMORYPRESSURE_CRITICAL;
    // SAFETY: libdispatch symbols are process-global; handler only touches atomics.
    unsafe {
        let queue = dispatch_get_global_queue(DISPATCH_QUEUE_PRIORITY_DEFAULT, 0);
        let source = dispatch_source_create(
            &_dispatch_source_type_memorypressure,
            0,
            mask,
            queue,
        );
        if source.is_null() {
            return;
        }
        let ctx = Box::into_raw(Box::new(PressureContext {
            source,
            state,
            on_critical,
        }));
        dispatch_set_context(source, ctx.cast());
        dispatch_source_set_event_handler_f(source, Some(memory_pressure_handler));
        dispatch_resume(source);
    }
}

unsafe extern "C" fn memory_pressure_handler(ctx: *mut c_void) {
    // SAFETY: context pointer set in install_memory_pressure_watch; never freed.
    let context = &*(ctx as *const PressureContext);
    let flags = dispatch_source_get_data(context.source);
    let class = if flags & DISPATCH_MEMORYPRESSURE_CRITICAL != 0 {
        PressureClass::Critical
    } else if flags & DISPATCH_MEMORYPRESSURE_WARN != 0 {
        PressureClass::Warn
    } else {
        PressureClass::Normal
    };
    context
        .state
        .pressure
        .store(pressure_store_u8(class), Ordering::SeqCst);
    if class == PressureClass::Critical {
        if let Some(ref hook) = context.on_critical {
            hook();
        }
    }
}

/// Refresh thermal atomic from NSProcessInfo (call from slow telemetry thread).
pub fn refresh_thermal(state: &AppleSensorState) {
    let level = read_thermal_state();
    state.thermal.store(level.as_u8(), Ordering::SeqCst);
}
