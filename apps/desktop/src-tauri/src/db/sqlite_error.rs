//! Shared SQLite error classification for the vault.
//!
//! A leaf module (depends only on `rusqlite`) so both the connection boundary
//! and the repository can classify OS-level storage denial identically without
//! a module cycle.

use rusqlite::{Error, ErrorCode};

/// True when the error signals OS-level storage access denial rather than a
/// logical or corruption failure. iOS Data Protection seals the vault file
/// while the device is locked, so subsequent I/O surfaces as one of these
/// platform-independent SQLite primary result codes. Detection uses the primary
/// code only, so it is identical on every target (no iOS-specific handling).
///
/// This is a recoverable, transient condition — distinct from a corrupt cipher
/// header — so callers must fail closed to `Locked`, never `Quarantined`.
pub(crate) fn is_data_protection_error(error: &Error) -> bool {
    matches!(
        error,
        Error::SqliteFailure(ffi_error, _)
            if matches!(
                ffi_error.code,
                ErrorCode::SystemIoFailure | ErrorCode::PermissionDenied | ErrorCode::CannotOpen
            )
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::ffi;

    fn sqlite_failure(primary_code: i32) -> Error {
        Error::SqliteFailure(ffi::Error::new(primary_code), None)
    }

    #[test]
    fn detects_os_access_denied_codes_only() {
        assert!(is_data_protection_error(&sqlite_failure(ffi::SQLITE_IOERR)));
        assert!(is_data_protection_error(&sqlite_failure(ffi::SQLITE_PERM)));
        assert!(is_data_protection_error(&sqlite_failure(
            ffi::SQLITE_CANTOPEN
        )));

        assert!(!is_data_protection_error(&sqlite_failure(ffi::SQLITE_BUSY)));
        assert!(!is_data_protection_error(&sqlite_failure(
            ffi::SQLITE_CONSTRAINT
        )));
        assert!(!is_data_protection_error(&Error::QueryReturnedNoRows));
    }
}
