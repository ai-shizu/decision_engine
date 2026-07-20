//! M3 SQLCipher vault boundary.
//!
//! The Phase 0 link probe remains available for diagnostics. Production access
//! goes through a cloneable [`VaultHandle`]; the dedicated worker exclusively
//! owns LocalAuthentication objects and the single SQLCipher connection.

#[cfg(target_vendor = "apple")]
pub(crate) mod connection;
#[cfg(target_vendor = "apple")]
#[allow(dead_code)] // Retained Phase 0 diagnostic; no longer exposed over IPC.
mod keychain_probe;
// Background auto-lock is iOS-only: it observes UIKit lifecycle notifications,
// and UIKit does not exist on macOS (docs/m3_action_plan.md §0, §8).
#[cfg(target_os = "ios")]
pub(crate) mod lifecycle;
#[cfg(target_vendor = "apple")]
mod analytics_repo;
#[cfg(target_vendor = "apple")]
mod knowledge_repo;
#[cfg(target_vendor = "apple")]
mod oracle_repo;
#[cfg(target_vendor = "apple")]
mod psychometrics_repo;
#[cfg(target_vendor = "apple")]
mod migrations;
#[cfg(target_vendor = "apple")]
mod repository;
#[cfg(target_vendor = "apple")]
pub(crate) mod secure_vault;
#[cfg(target_vendor = "apple")]
mod sqlite_error;
#[cfg(target_vendor = "apple")]
mod sqlite_vec_ext;
#[cfg(target_vendor = "apple")]
mod worker;

#[cfg(target_vendor = "apple")]
pub(crate) use analytics_repo::{GapAnalysisRow, TensorProfileRow};
#[cfg(target_vendor = "apple")]
pub(crate) use knowledge_repo::{KnowledgeChunkRow, KnowledgeSearchHit};
#[cfg(target_vendor = "apple")]
pub(crate) use oracle_repo::{OracleRunRow, TwinRunRow};
#[cfg(target_vendor = "apple")]
pub(crate) use psychometrics_repo::{PulseRunRow, ProbeStoreRow, RaschRunRow};
#[cfg(target_vendor = "apple")]
pub(crate) use repository::{ChatCreate, ChatRecord, MessageAppend, MessageCursor, MessageRecord};
#[cfg(target_vendor = "apple")]
pub(crate) use worker::{
    VaultErrorCode, VaultHandle, VaultLifecycleEvent, VaultStatus, REPOSITORY_CONTENT_MAX_BYTES,
    VAULT_DATABASE_FILENAME,
};

use rusqlite::Connection;
use zeroize::Zeroizing;

#[allow(dead_code)]
const ERR_OPEN: &str = "secure_vault_link_probe: open_in_memory failed";
#[allow(dead_code)]
const ERR_KEY: &str = "secure_vault_link_probe: pragma key failed";
#[allow(dead_code)]
const ERR_CIPHER_QUERY: &str = "secure_vault_link_probe: cipher_version query failed";
#[allow(dead_code)]
const ERR_CIPHER_EMPTY: &str = "secure_vault_link_probe: cipher_version empty";

/// Phase 0-A link probe only — not a production vault API.
///
/// Returns a safe diagnostic string containing the SQLCipher identity
/// (`cipher_version`). Never returns key material.
#[allow(dead_code)]
pub fn verify_sqlcipher_link_and_keychain() -> Result<String, String> {
    let passphrase = Zeroizing::new(String::from("pkb-m3-phase0a-link-probe"));

    let conn = Connection::open_in_memory().map_err(|_| ERR_OPEN.to_string())?;

    // Do not interpolate secrets into SQL strings.
    conn.pragma_update(None, "key", passphrase.as_str())
        .map_err(|_| ERR_KEY.to_string())?;

    let cipher_version: String = conn
        .query_row("PRAGMA cipher_version;", [], |row| row.get(0))
        .map_err(|_| ERR_CIPHER_QUERY.to_string())?;

    if cipher_version.trim().is_empty() {
        return Err(ERR_CIPHER_EMPTY.to_string());
    }

    #[cfg(target_vendor = "apple")]
    {
        keychain_probe::verify_typed_keychain_link_probe()?;
    }

    Ok(format!("sqlcipher_link_ok:{cipher_version}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Link / SQLCipher identity check only.
    /// Does **not** prove encrypted-at-rest or Keychain authentication.
    #[test]
    fn phase0a_in_memory_pragma_key_and_cipher_version() -> Result<(), String> {
        let report = verify_sqlcipher_link_and_keychain()?;
        assert!(
            report.starts_with("sqlcipher_link_ok:"),
            "unexpected report prefix"
        );
        assert!(
            report.len() > "sqlcipher_link_ok:".len(),
            "cipher_version must be non-empty"
        );
        Ok(())
    }
}
