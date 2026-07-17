//! STEP 8: user consent for external research (Rust-owned persistence).
#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::string_slice
)]

use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use crate::knowledge::NetworkPolicy;
use crate::paths::user_data_root;

const SCHEMA: &str = "knowledge_policy.v1";
const FILE_NAME: &str = "knowledge_policy.json";

#[derive(Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct PersistedPolicy {
    schema: String,
    enabled: bool,
}

/// Managed Tauri state: in-memory policy + disk round-trip (default Off).
///
/// Persistence root is captured at construction (`user_data_root()` in production).
/// Tests inject an isolated temp directory via [`NetworkPolicyStore::from_root`] so
/// they never touch process-global `LOCALAPPDATA`.
pub struct NetworkPolicyStore {
    inner: Mutex<NetworkPolicy>,
    root: PathBuf,
}

impl Default for NetworkPolicyStore {
    fn default() -> Self {
        Self::new()
    }
}

impl NetworkPolicyStore {
    pub fn new() -> Self {
        Self::from_root(user_data_root())
    }

    /// Construct against an explicit data root (production: `user_data_root()`;
    /// tests: per-case temp dir — no env mutation).
    fn from_root(root: PathBuf) -> Self {
        let policy = Self::load_from_disk(&root).unwrap_or(NetworkPolicy::Off);
        Self {
            inner: Mutex::new(policy),
            root,
        }
    }

    pub fn get(&self) -> NetworkPolicy {
        match self.inner.lock() {
            Ok(guard) => *guard,
            Err(poisoned) => *poisoned.into_inner(),
        }
    }

    pub fn enabled(&self) -> bool {
        matches!(self.get(), NetworkPolicy::Live)
    }

    pub fn set_enabled(&self, enabled: bool) -> Result<(), String> {
        let policy = if enabled {
            NetworkPolicy::Live
        } else {
            NetworkPolicy::Off
        };
        {
            let mut guard = self
                .inner
                .lock()
                .map_err(|_| "policy store lock poisoned".to_string())?;
            *guard = policy;
        }
        Self::save_to_disk(&self.root, policy)
    }

    fn policy_path(root: &Path) -> PathBuf {
        root.join(FILE_NAME)
    }

    fn load_from_disk(root: &Path) -> Option<NetworkPolicy> {
        let path = Self::policy_path(root);
        let bytes = fs::read(&path).ok()?;
        let parsed: PersistedPolicy = serde_json::from_slice(&bytes).ok()?;
        if parsed.schema != SCHEMA {
            return None;
        }
        Some(if parsed.enabled {
            NetworkPolicy::Live
        } else {
            NetworkPolicy::Off
        })
    }

    fn save_to_disk(root: &Path, policy: NetworkPolicy) -> Result<(), String> {
        let path = Self::policy_path(root);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|_| "policy persist failed".to_string())?;
        }
        let enabled = matches!(policy, NetworkPolicy::Live);
        let body = serde_json::json!({
            "schema": SCHEMA,
            "enabled": enabled,
        });
        let serialized = serde_json::to_vec(&body).map_err(|_| "policy persist failed".to_string())?;
        let tmp = path.with_extension("json.tmp");
        let _ = fs::remove_file(&tmp);
        {
            let mut file = File::create(&tmp).map_err(|_| "policy persist failed".to_string())?;
            file.write_all(&serialized)
                .map_err(|_| "policy persist failed".to_string())?;
            file.sync_all()
                .map_err(|_| "policy persist failed".to_string())?;
        }
        if path.exists() {
            fs::remove_file(&path).map_err(|_| "policy persist failed".to_string())?;
        }
        fs::rename(&tmp, &path).map_err(|_| "policy persist failed".to_string())?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    // Test scope: `.expect()` / `.unwrap()` are the sanctioned idiom for test
    // fixtures; relax the crate-level `#![deny]` here without weakening it for
    // production code above.
    #![allow(clippy::expect_used, clippy::unwrap_used)]
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    static TEST_DIR_SEQ: AtomicUsize = AtomicUsize::new(0);

    fn isolated_root() -> PathBuf {
        let seq = TEST_DIR_SEQ.fetch_add(1, Ordering::SeqCst);
        let dir = std::env::temp_dir().join(format!("pkb_policy_test_{seq}"));
        let _ = fs::remove_dir_all(&dir);
        // Mirror production layout: policy file lives under <root>/PKB/ only when
        // user_data_root() is used; for DI tests the injected root *is* the data
        // directory (file written directly as knowledge_policy.json).
        fs::create_dir_all(&dir).expect("mkdir");
        dir
    }

    fn isolated_store() -> NetworkPolicyStore {
        NetworkPolicyStore::from_root(isolated_root())
    }

    #[test]
    fn default_is_off() {
        let store = isolated_store();
        assert_eq!(store.get(), NetworkPolicy::Off);
        assert!(!store.enabled());
    }

    #[test]
    fn set_true_promotes_live() {
        let store = isolated_store();
        store.set_enabled(true).expect("set");
        assert_eq!(store.get(), NetworkPolicy::Live);
        assert!(store.enabled());
    }

    #[test]
    fn set_false_demotes_to_off() {
        let store = isolated_store();
        store.set_enabled(true).expect("set");
        store.set_enabled(false).expect("unset");
        assert_eq!(store.get(), NetworkPolicy::Off);
    }

    #[test]
    fn persistence_round_trip() {
        let root = isolated_root();

        {
            let store = NetworkPolicyStore::from_root(root.clone());
            store.set_enabled(true).expect("set");
        }
        let reloaded = NetworkPolicyStore::from_root(root.clone());
        assert_eq!(reloaded.get(), NetworkPolicy::Live);

        reloaded.set_enabled(false).expect("unset");
        let again = NetworkPolicyStore::from_root(root);
        assert_eq!(again.get(), NetworkPolicy::Off);
    }
}
