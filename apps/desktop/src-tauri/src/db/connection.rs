//! M3 Phase 1-B SQLCipher connection initialization boundary.
//!
//! This module retrieves the device-bound master key, opens exactly one local
//! connection, applies the key before reading the database, and verifies that
//! SQLCipher can read the schema. The vault worker runs migrations immediately
//! after this boundary returns and before publishing the `Unlocked` state.

use std::{error::Error, fmt, path::Path};

use objc2::rc::Retained;
use objc2_local_authentication::LAContext;
use rusqlite::{Connection, OpenFlags};
use zeroize::Zeroizing;

use super::secure_vault::{SecureVault, SecureVaultError};
use super::sqlite_error::is_data_protection_error;

const KEY_LENGTH_BYTES: usize = 32;
const HEX_KEY_LENGTH: usize = KEY_LENGTH_BYTES * 2;
const RAW_KEY_PRAGMA_PREFIX: &str = "PRAGMA cipher_memory_security = ON; PRAGMA key = \"x'";
const RAW_KEY_PRAGMA_SUFFIX: &str = "'\";";
const SCHEMA_VERIFICATION_SQL: &str = "SELECT count(*) FROM sqlite_schema;";
const CIPHER_VERSION_SQL: &str = "PRAGMA cipher_version;";
const HEX_DIGITS: &[u8; 16] = b"0123456789abcdef";

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum VaultConnectionError {
    SecureVault(SecureVaultError),
    InvalidKeyLength { actual: usize },
    OpenFailed,
    KeyApplicationFailed,
    SchemaVerificationFailed,
    CipherIdentityUnavailable,
    // OS-level storage access denial (iOS Data Protection sealed the file while
    // the device is locked). Recoverable and distinct from corruption: it must
    // fail closed to `Locked`, never `Quarantined`.
    OsAccessDenied,
}

impl fmt::Display for VaultConnectionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::SecureVault(error) => write!(formatter, "secure vault failed: {error}"),
            Self::InvalidKeyLength { actual } => {
                write!(formatter, "invalid SQLCipher key length: {actual}")
            }
            Self::OpenFailed => formatter.write_str("encrypted database open failed"),
            Self::KeyApplicationFailed => formatter.write_str("SQLCipher key application failed"),
            Self::SchemaVerificationFailed => {
                formatter.write_str("encrypted database schema verification failed")
            }
            Self::CipherIdentityUnavailable => {
                formatter.write_str("SQLCipher identity is unavailable")
            }
            Self::OsAccessDenied => formatter.write_str("storage access denied by the OS"),
        }
    }
}

impl Error for VaultConnectionError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::SecureVault(error) => Some(error),
            _ => None,
        }
    }
}

impl From<SecureVaultError> for VaultConnectionError {
    fn from(error: SecureVaultError) -> Self {
        Self::SecureVault(error)
    }
}

/// Retrieve the Keychain key and open a verified SQLCipher connection.
///
/// The caller must provide an app-owned, typed LocalAuthentication context.
/// No key, SQL, or database path crosses the Tauri IPC boundary.
pub fn open_encrypted_database(
    path: &Path,
    context: &Retained<LAContext>,
) -> Result<Connection, VaultConnectionError> {
    let key = SecureVault::retrieve_or_generate_key(context)?;
    let connection = open_encrypted_database_with_key(path, key.as_slice())?;

    // Make the intended lifetime explicit: the only Rust-owned plaintext key
    // is erased before the initialized connection leaves this function.
    drop(key);
    Ok(connection)
}

fn open_encrypted_database_with_key(
    path: &Path,
    key: &[u8],
) -> Result<Connection, VaultConnectionError> {
    validate_key_length(key)?;

    // Do not enable SQLITE_OPEN_URI: the DB worker will supply a fixed local
    // app-container path, never a frontend-controlled URI.
    let flags = OpenFlags::SQLITE_OPEN_READ_WRITE
        | OpenFlags::SQLITE_OPEN_CREATE
        | OpenFlags::SQLITE_OPEN_NO_MUTEX;
    let connection = Connection::open_with_flags(path, flags)
        .map_err(|error| classify_sqlite_error(error, VaultConnectionError::OpenFailed))?;

    apply_sqlcipher_key(&connection, key)?;
    verify_encrypted_connection(&connection)?;
    Ok(connection)
}

fn apply_sqlcipher_key(connection: &Connection, key: &[u8]) -> Result<(), VaultConnectionError> {
    validate_key_length(key)?;

    // `sqlite3_key` exists in the pinned SQLCipher binding, but rusqlite only
    // exposes its handle through an unsafe raw-pointer API. The approved safe
    // fallback is therefore raw-key PRAGMA syntax. Both derived Rust buffers
    // are Zeroizing and cannot escape this scope or appear in an error value.
    {
        let hex_key = encode_hex_key(key);
        let mut key_pragma = Zeroizing::new(String::with_capacity(
            RAW_KEY_PRAGMA_PREFIX.len() + HEX_KEY_LENGTH + RAW_KEY_PRAGMA_SUFFIX.len(),
        ));
        key_pragma.push_str(RAW_KEY_PRAGMA_PREFIX);
        key_pragma.push_str(hex_key.as_str());
        key_pragma.push_str(RAW_KEY_PRAGMA_SUFFIX);

        // cipher_memory_security is enabled before SQLCipher parses/copies the
        // key specification. execute_batch borrows the zeroizing source text;
        // no secret-bearing SQL is retained by this module after return.
        connection
            .execute_batch(key_pragma.as_str())
            .map_err(|error| {
                classify_sqlite_error(error, VaultConnectionError::KeyApplicationFailed)
            })?;
    }

    Ok(())
}

pub(crate) fn verify_encrypted_connection(
    connection: &Connection,
) -> Result<(), VaultConnectionError> {
    let _: i64 = connection
        .query_row(SCHEMA_VERIFICATION_SQL, [], |row| row.get(0))
        .map_err(|error| {
            classify_sqlite_error(error, VaultConnectionError::SchemaVerificationFailed)
        })?;

    let cipher_version: String = connection
        .query_row(CIPHER_VERSION_SQL, [], |row| row.get(0))
        .map_err(|error| {
            classify_sqlite_error(error, VaultConnectionError::CipherIdentityUnavailable)
        })?;
    if cipher_version.trim().is_empty() {
        return Err(VaultConnectionError::CipherIdentityUnavailable);
    }

    Ok(())
}

fn validate_key_length(key: &[u8]) -> Result<(), VaultConnectionError> {
    if key.len() == KEY_LENGTH_BYTES {
        Ok(())
    } else {
        Err(VaultConnectionError::InvalidKeyLength { actual: key.len() })
    }
}

/// Prefer the recoverable `OsAccessDenied` classification when the OS sealed the
/// file (iOS Data Protection); otherwise use the caller's fallback. The raw
/// error is inspected by primary code only and then dropped, so no SQL or key
/// material is ever retained or surfaced.
fn classify_sqlite_error(
    error: rusqlite::Error,
    fallback: VaultConnectionError,
) -> VaultConnectionError {
    if is_data_protection_error(&error) {
        VaultConnectionError::OsAccessDenied
    } else {
        fallback
    }
}

fn encode_hex_key(key: &[u8]) -> Zeroizing<String> {
    let mut hex = Zeroizing::new(String::with_capacity(key.len() * 2));
    for byte in key {
        hex.push(char::from(HEX_DIGITS[(byte >> 4) as usize]));
        hex.push(char::from(HEX_DIGITS[(byte & 0x0f) as usize]));
    }
    hex
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        path::{Path, PathBuf},
        process,
        time::{SystemTime, UNIX_EPOCH},
    };

    use super::*;

    struct TemporaryDatabase {
        path: PathBuf,
    }

    impl TemporaryDatabase {
        fn new() -> Result<Self, Box<dyn Error>> {
            let nonce = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
            let path = std::env::temp_dir()
                .join(format!("pkb-m3-phase1b-{}-{nonce}.sqlite3", process::id()));
            Ok(Self { path })
        }

        fn path(&self) -> &Path {
            &self.path
        }
    }

    impl Drop for TemporaryDatabase {
        fn drop(&mut self) {
            let _ = fs::remove_file(&self.path);
            let _ = fs::remove_file(self.path.with_extension("sqlite3-wal"));
            let _ = fs::remove_file(self.path.with_extension("sqlite3-shm"));
        }
    }

    #[test]
    fn encodes_fixed_width_lowercase_hex() {
        let mut key = [0u8; KEY_LENGTH_BYTES];
        key[0] = 0x01;
        key[1] = 0xaf;
        key[KEY_LENGTH_BYTES - 1] = 0xff;

        let encoded = encode_hex_key(&key);
        assert_eq!(encoded.len(), HEX_KEY_LENGTH);
        assert!(encoded.starts_with("01af"));
        assert!(encoded.ends_with("ff"));
    }

    #[test]
    fn rejects_non_256_bit_keys_before_opening() -> Result<(), Box<dyn Error>> {
        let database = TemporaryDatabase::new()?;
        let error = open_encrypted_database_with_key(database.path(), &[0u8; 31])
            .err()
            .ok_or("invalid key unexpectedly succeeded")?;

        assert_eq!(error, VaultConnectionError::InvalidKeyLength { actual: 31 });
        assert!(!database.path().exists());
        Ok(())
    }

    #[test]
    fn encrypted_file_reopens_with_same_key_and_rejects_wrong_key() -> Result<(), Box<dyn Error>> {
        let database = TemporaryDatabase::new()?;
        let key = Zeroizing::new(vec![0x3a; KEY_LENGTH_BYTES]);
        let wrong_key = Zeroizing::new(vec![0xc7; KEY_LENGTH_BYTES]);

        {
            let connection = open_encrypted_database_with_key(database.path(), key.as_slice())?;
            connection.execute_batch(
                "CREATE TABLE phase1b_probe(value TEXT NOT NULL);\
                 INSERT INTO phase1b_probe(value) VALUES ('encrypted-marker');",
            )?;
        }

        let header = fs::read(database.path())?;
        assert!(!header.starts_with(b"SQLite format 3\0"));
        assert!(!header
            .windows(b"encrypted-marker".len())
            .any(|window| window == b"encrypted-marker"));

        {
            let connection = open_encrypted_database_with_key(database.path(), key.as_slice())?;
            let marker: String =
                connection.query_row("SELECT value FROM phase1b_probe LIMIT 1;", [], |row| {
                    row.get(0)
                })?;
            assert_eq!(marker, "encrypted-marker");
        }

        let wrong_key_result =
            open_encrypted_database_with_key(database.path(), wrong_key.as_slice());
        assert!(matches!(
            wrong_key_result,
            Err(VaultConnectionError::SchemaVerificationFailed)
        ));

        Ok(())
    }
}
