//! M3 Phase 0 Tauri command — SQLCipher / typed objc2 Keychain link probe only.
//!
//! Feature-gated: `secure-vault`. This is **not** a production vault API.
//! No frontend caller is required for Phase 0-A. Do not manage DB connections,
//! keys, or Security / LocalAuthentication objects in Tauri State.

use crate::db;

/// Phase 0 native link probe. Returns SQLCipher identity diagnostics only.
/// Never returns key material, SQL, Keychain payloads, or file paths.
#[tauri::command]
pub fn verify_sqlcipher_link_and_keychain() -> Result<String, String> {
    db::verify_sqlcipher_link_and_keychain()
}
