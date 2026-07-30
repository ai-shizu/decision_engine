//! iOS-only OSLog backend for the `log` facade (Tier 3 P0-1 / P0-5).
//!
//! Bypasses Tauri's stdout/stderr pipe (Release discards it). Numbers go out
//! as `%{public}llu` via the C shim; free-form text is sanitized + truncated
//! and emitted only as an allowlisted `%{public}s` one-liner — never as a
//! whole-record `%{public}@`, and never as the format string itself.
//! `install()` always emits `instrument.alive=<phys_footprint>` (category `app`)
//! so cold start proves the numeric path without waiting for LLM phases.
//!
//! # Limits
//!
//! Foreign `abort` / Jetsam kill the process without running the Rust panic
//! hook. Those events are recovered from crash / Jetsam logs, not from this
//! backend.

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::PathBuf;
use std::sync::Mutex;

use crate::paths::user_data_root;

/// Bundle identifier — also the OSLog subsystem (matches `tauri.conf.json`).
pub const SUBSYSTEM: &str = "com.ai-shizu.pkb";

/// Categories for `log show --predicate 'subsystem == "…" AND category == "…"'`.
pub const CAT_MEMORY: &str = "memory";
pub const CAT_MODEL: &str = "model";
pub const CAT_PANIC: &str = "panic";
pub const CAT_APP: &str = "app";

const TYPE_DEFAULT: u8 = 0x00;
const TYPE_INFO: u8 = 0x01;
const TYPE_DEBUG: u8 = 0x02;
const TYPE_ERROR: u8 = 0x10;
const TYPE_FAULT: u8 = 0x11;

const MSG_MAX: usize = 192;
const JSONL_MAX_BYTES: u64 = 256 * 1024;

use std::os::raw::c_char;

#[link(name = "pkb_ios_oslog", kind = "static")]
extern "C" {
    fn pkb_oslog_u64(
        subsystem: *const c_char,
        category: *const c_char,
        type_: u8,
        label: *const c_char,
        value: u64,
    );
    fn pkb_oslog_msg(
        subsystem: *const c_char,
        category: *const c_char,
        type_: u8,
        msg: *const c_char,
    );
}

fn c_str(s: &str) -> std::ffi::CString {
    // Strip interior NULs so CString::new never fails for log payloads.
    let cleaned: String = s.chars().filter(|c| *c != '\0').collect();
    std::ffi::CString::new(cleaned).unwrap_or_else(|_| std::ffi::CString::new("").unwrap())
}

/// Footprint / evidence checkpoint — numbers are always public (`%{public}llu`).
pub fn log_footprint_bytes(label: &str, bytes: u64) {
    emit_u64(CAT_MEMORY, label, bytes);
    jsonl_checkpoint("footprint", label, Some(bytes));
}

/// Model-identity / device-arm evidence (Tier 3 P0-6). Category `model`, Default.
pub fn log_model_u64(label: &str, value: u64) {
    emit_u64(CAT_MODEL, label, value);
    jsonl_checkpoint("model", label, Some(value));
}

/// Searchable survival label (P0-5). Console.app message-body search target.
/// Value is boot-time `phys_footprint` bytes (model not resident).
pub const ALIVE_LABEL: &str = "instrument.alive";

/// G-1 identity labels (P0-6-1). Values ride `%{public}llu` only.
pub const MODEL_N_LAYER: &str = "model.n_layer";
pub const MODEL_N_PARAMS: &str = "model.n_params";
pub const MODEL_SIZE: &str = "model.size";
pub const MODEL_META_COUNT: &str = "model.meta_count";
pub const MODEL_N_VOCAB: &str = "model.n_vocab";
/// 0=unknown, 1=bundled resource, 2=AppData import — never the raw path.
pub const MODEL_ORIGIN: &str = "model.origin";
/// 0=Metal path (default), 1=`CORAXIS_FORCE_CPU=1` oracle arm.
pub const MODEL_FORCE_CPU: &str = "model.force_cpu";

fn emit_u64(category: &str, label: &str, value: u64) {
    let sub = c_str(SUBSYSTEM);
    let cat = c_str(category);
    let lab = c_str(label);
    unsafe {
        pkb_oslog_u64(
            sub.as_ptr(),
            cat.as_ptr(),
            TYPE_DEFAULT,
            lab.as_ptr(),
            value,
        );
    }
}

fn boot_phys_footprint_bytes() -> u64 {
    #[cfg(feature = "pocket-brain")]
    {
        crate::monitor::phys_footprint_bytes().unwrap_or(0)
    }
    #[cfg(not(feature = "pocket-brain"))]
    {
        0
    }
}

/// P0-5: one Default/`app` line right after `install()` so cold start proves
/// the numeric OSLog path is alive (distinct from LLM-phase footprints).
fn emit_instrument_alive() {
    let bytes = boot_phys_footprint_bytes();
    emit_u64(CAT_APP, ALIVE_LABEL, bytes);
    jsonl_checkpoint("alive", ALIVE_LABEL, Some(bytes));
}

fn level_to_type(level: log::Level) -> u8 {
    match level {
        log::Level::Error => TYPE_ERROR,
        log::Level::Warn => TYPE_DEFAULT,
        log::Level::Info => TYPE_DEFAULT, // evidence-grade persistence
        log::Level::Debug => TYPE_INFO,
        log::Level::Trace => TYPE_DEBUG,
    }
}

fn category_for_target(target: &str) -> &'static str {
    let t = target.to_ascii_lowercase();
    if t.contains("monitor") || t.contains("memory") || t.contains("jetsam") {
        CAT_MEMORY
    } else if t.contains("llm") || t.contains("model") || t.contains("gguf") || t.contains("brain")
    {
        CAT_MODEL
    } else if t.contains("panic") {
        CAT_PANIC
    } else {
        CAT_APP
    }
}

/// Strip absolute-looking path segments and truncate — never emit raw panic /
/// IO error text that embeds filesystem paths (§5.1 invariant).
fn sanitize_one_liner(level: log::Level, target: &str, args: &str) -> String {
    let mut out = String::new();
    out.push_str(level.as_str());
    out.push(' ');
    out.push_str(target);
    out.push(' ');
    let mut depth = 0i32;
    for ch in args.chars() {
        if ch == '/' {
            depth += 1;
            if depth >= 2 {
                // Collapse `/Users/...` / container paths to a single marker.
                if !out.ends_with("<path>") {
                    out.push_str("<path>");
                }
                continue;
            }
        } else {
            depth = 0;
        }
        if ch == '\n' || ch == '\r' {
            out.push(' ');
        } else {
            out.push(ch);
        }
        if out.len() >= MSG_MAX {
            out.push('…');
            break;
        }
    }
    out
}

fn emit_msg(category: &str, type_: u8, msg: &str) {
    let sub = c_str(SUBSYSTEM);
    let cat = c_str(category);
    let m = c_str(msg);
    unsafe {
        pkb_oslog_msg(sub.as_ptr(), cat.as_ptr(), type_, m.as_ptr());
    }
}

static JSONL_LOCK: Mutex<()> = Mutex::new(());

fn jsonl_path() -> PathBuf {
    user_data_root().join("logs").join("tier3-checkpoints.jsonl")
}

fn jsonl_checkpoint(kind: &str, label: &str, bytes: Option<u64>) {
    let _guard = JSONL_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let path = jsonl_path();
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    // Rotate before write if over cap (Jetsam may kill mid-write — keep records short).
    if let Ok(meta) = fs::metadata(&path) {
        if meta.len() >= JSONL_MAX_BYTES {
            let bak = path.with_extension("jsonl.1");
            let _ = fs::rename(&path, &bak);
        }
    }
    let mut line = format!(
        "{{\"k\":{kind:?},\"l\":{label:?},\"t_ms\":{}}}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or(0)
    );
    if let Some(b) = bytes {
        // Rewrite with bytes — keep one short JSON object per checkpoint.
        line = format!(
            "{{\"k\":{kind:?},\"l\":{label:?},\"bytes\":{b},\"t_ms\":{}}}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_millis())
                .unwrap_or(0)
        );
    }
    if let Ok(mut f) = OpenOptions::new().create(true).append(true).open(&path) {
        let _ = writeln!(f, "{line}");
        let _ = f.flush();
    }
}

struct IosOsLogLogger;

impl log::Log for IosOsLogLogger {
    fn enabled(&self, _metadata: &log::Metadata) -> bool {
        true
    }

    fn log(&self, record: &log::Record) {
        if !self.enabled(record.metadata()) {
            return;
        }
        let cat = category_for_target(record.target());
        let ty = level_to_type(record.level());
        let args = format!("{}", record.args());
        let line = sanitize_one_liner(record.level(), record.target(), &args);
        emit_msg(cat, ty, &line);
    }

    fn flush(&self) {}
}

static IOS_LOGGER: IosOsLogLogger = IosOsLogLogger;

pub fn install() {
    if log::set_logger(&IOS_LOGGER).is_ok() {
        log::set_max_level(log::LevelFilter::Debug);
    }
    install_panic_hook();
    // Cold-start survival proof (P0-5): must run even if no LLM phase fires.
    emit_instrument_alive();
}

fn install_panic_hook() {
    let prev = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let msg = if let Some(s) = info.payload().downcast_ref::<&str>() {
            (*s).to_string()
        } else if let Some(s) = info.payload().downcast_ref::<String>() {
            s.clone()
        } else {
            "panic".to_string()
        };
        let loc = info
            .location()
            .map(|l| format!("{}:{}", l.file(), l.line()))
            .unwrap_or_else(|| "unknown".into());
        // Sanitize location (may contain source paths) and payload.
        let line = sanitize_one_liner(log::Level::Error, "panic", &format!("{loc} {msg}"));
        emit_msg(CAT_PANIC, TYPE_FAULT, &line);
        jsonl_checkpoint("panic", "hook", None);
        // Still invoke previous hook (may write stderr — discarded in Release).
        prev(info);
    }));
}

#[cfg(test)]
mod tests {
    use super::sanitize_one_liner;

    #[test]
    fn sanitize_collapses_absolute_paths() {
        let s = sanitize_one_liner(
            log::Level::Error,
            "llm::model_path",
            "failed /Users/atsu/secret/models/x.gguf open",
        );
        assert!(s.contains("<path>"), "{s}");
        assert!(!s.contains("/Users/atsu"), "{s}");
    }
}
