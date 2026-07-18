//! M3 Phase 0-A — SQLCipher / Security.framework link probe.
//!
//! Scope: prove `bundled-sqlcipher` and Apple Security.framework symbols link
//! into the final binary. This is **not** encrypted-at-rest proof and **not**
//! Keychain operational authentication proof.
//! Gated behind `secure-vault` (docs/m3_action_plan.md §4.1 / Phase 0-A).

use rusqlite::Connection;
use zeroize::Zeroizing;

const ERR_OPEN: &str = "secure_vault_link_probe: open_in_memory failed";
const ERR_KEY: &str = "secure_vault_link_probe: pragma key failed";
const ERR_CIPHER_QUERY: &str = "secure_vault_link_probe: cipher_version query failed";
const ERR_CIPHER_EMPTY: &str = "secure_vault_link_probe: cipher_version empty";
const ERR_SECRANDOM: &str = "secure_vault_link_probe: SecRandom failed";
const ERR_ACCESS_CONTROL: &str = "secure_vault_link_probe: SecAccessControl failed";

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
        probe_security_framework_symbols()?;
    }

    Ok(format!("sqlcipher_link_ok:{cipher_version}"))
}

/// Force-link Security.framework via confirmed high-level APIs.
///
/// Phase 0-A does **not** create, store, or retrieve Keychain items.
/// Success here means constructors returned `Ok` (symbols resolved and ran);
/// it does **not** mean userPresence authentication or Keychain round-trip.
#[cfg(target_vendor = "apple")]
fn probe_security_framework_symbols() -> Result<(), String> {
    use security_framework::access_control::{ProtectionMode, SecAccessControl};
    use security_framework::passwords::AccessControlOptions;
    use security_framework::random::SecRandom;

    let mut key_buf = Zeroizing::new([0u8; 32]);
    SecRandom::default()
        .copy_bytes(key_buf.as_mut())
        .map_err(|_| ERR_SECRANDOM.to_string())?;

    let access = SecAccessControl::create_with_protection(
        Some(ProtectionMode::AccessibleWhenPasscodeSetThisDeviceOnly),
        AccessControlOptions::USER_PRESENCE.bits(),
    )
    .map_err(|_| ERR_ACCESS_CONTROL.to_string())?;

    // Drop without Keychain I/O. Plaintext key_buf is zeroized on drop.
    drop(access);
    Ok(())
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
