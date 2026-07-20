//! M9: statically activate the `sqlite-vec` extension with SQLCipher.
//!
//! iOS cannot rely on `load_extension` / a shipped `.dylib`. The `sqlite-vec`
//! crate compiles `sqlite-vec.c` with `SQLITE_CORE` and links it into the app.
//!
//! Activation strategy (belt + suspenders):
//! 1. Process-wide `sqlite3_auto_extension` so future opens inherit `vec0`.
//! 2. Per-connection explicit `sqlite3_vec_init` via the raw DB handle, because
//!    SQLCipher open paths have historically skipped auto-extensions in some
//!    embeddings — calling init after open makes activation deterministic.

use std::sync::OnceLock;

use rusqlite::auto_extension::RawAutoExtension;
use rusqlite::{ffi, Connection};
use sqlite_vec::sqlite3_vec_init;

/// Process-wide registration result.
static SQLITE_VEC_STATE: OnceLock<Result<(), String>> = OnceLock::new();

fn vec_init_entry() -> RawAutoExtension {
    // SAFETY: C ABI of `sqlite3_vec_init` matches `RawAutoExtension`. The crate
    // FFI declares a zero-arg stub; we transmute the symbol address to the real
    // SQLite extension entry signature (same pattern as sqlite-vec's own docs).
    unsafe { std::mem::transmute(sqlite3_vec_init as *const ()) }
}

/// Register `sqlite3_vec_init` once for this process (auto-extension table).
pub(crate) fn ensure_sqlite_vec_loaded() -> Result<(), String> {
    match SQLITE_VEC_STATE.get_or_init(|| {
        // SAFETY: same ABI contract as `register_auto_extension`.
        unsafe {
            let rc = ffi::sqlite3_auto_extension(Some(vec_init_entry()));
            if rc != ffi::SQLITE_OK {
                return Err(format!("sqlite3_auto_extension failed: {rc}"));
            }
        }
        Ok(())
    }) {
        Ok(()) => Ok(()),
        Err(message) => Err(message.clone()),
    }
}

/// Ensure vec0 is live on this connection (auto-extension + explicit init).
pub(crate) fn activate_sqlite_vec(connection: &Connection) -> Result<(), String> {
    ensure_sqlite_vec_loaded()?;
    // SAFETY: `handle` is only used to invoke the statically linked init; we do
    // not close or retain the pointer beyond this call.
    unsafe {
        let db = connection.handle();
        let mut err_msg: *mut std::ffi::c_char = std::ptr::null_mut();
        let rc = vec_init_entry()(db, &mut err_msg, std::ptr::null());
        if !err_msg.is_null() {
            ffi::sqlite3_free(err_msg.cast());
        }
        if rc != ffi::SQLITE_OK {
            return Err(format!("sqlite3_vec_init failed: {rc}"));
        }
    }
    verify_vec_extension(connection)
}

/// Smoke-check that the extension is live on an already-open connection.
pub(crate) fn verify_vec_extension(connection: &Connection) -> Result<(), String> {
    let version: String = connection
        .query_row("SELECT vec_version();", [], |row| row.get(0))
        .map_err(|error| format!("vec_version unavailable: {error}"))?;
    if version.trim().is_empty() {
        return Err("vec_version empty".to_string());
    }
    Ok(())
}
