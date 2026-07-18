//! M3 Phase 0 — SQLCipher / typed objc2 Keychain link probe.
//!
//! Scope: prove `bundled-sqlcipher` and the single-stack objc2 Security /
//! LocalAuthentication bindings link into the final binary. This is **not**
//! encrypted-at-rest proof and **not** Keychain operational authentication.
//! Gated behind `secure-vault` (docs/m3_action_plan.md §4.1 / §4.2.1).

#[cfg(target_vendor = "apple")]
mod keychain_probe;
// Phase 1-A defines the production boundary; Phase 1-B will connect it to the
// DB worker. Keep this scoped allowance until that caller is introduced.
#[cfg(target_vendor = "apple")]
#[allow(dead_code)]
pub(crate) mod secure_vault;

use rusqlite::Connection;
use zeroize::Zeroizing;

const ERR_OPEN: &str = "secure_vault_link_probe: open_in_memory failed";
const ERR_KEY: &str = "secure_vault_link_probe: pragma key failed";
const ERR_CIPHER_QUERY: &str = "secure_vault_link_probe: cipher_version query failed";
const ERR_CIPHER_EMPTY: &str = "secure_vault_link_probe: cipher_version empty";

/// Phase 0-A link probe only — not a production vault API.
///
/// Returns a safe diagnostic string containing the SQLCipher identity
/// (`cipher_version`). Never returns key material.
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
