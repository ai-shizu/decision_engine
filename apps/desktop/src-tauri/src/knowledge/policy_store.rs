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
use std::path::PathBuf;
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
pub struct NetworkPolicyStore {
    inner: Mutex<NetworkPolicy>,
}

impl Default for NetworkPolicyStore {
    fn default() -> Self {
        Self::new()
    }
}

impl NetworkPolicyStore {
    pub fn new() -> Self {
        let policy = Self::load_from_disk().unwrap_or(NetworkPolicy::Off);
        Self {
            inner: Mutex::new(policy),
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
        Self::save_to_disk(policy)
    }

    fn policy_path() -> PathBuf {
        user_data_root().join(FILE_NAME)
    }

    fn load_from_disk() -> Option<NetworkPolicy> {
        let path = Self::policy_path();
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

    fn save_to_disk(policy: NetworkPolicy) -> Result<(), String> {
        let path = Self::policy_path();
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
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Mutex;

    static POLICY_TEST_LOCK: Mutex<()> = Mutex::new(());

    static TEST_DIR_SEQ: AtomicUsize = AtomicUsize::new(0);

    fn isolated_store() -> NetworkPolicyStore {
        let _guard = POLICY_TEST_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let seq = TEST_DIR_SEQ.fetch_add(1, Ordering::SeqCst);
        let dir = std::env::temp_dir().join(format!("pkb_policy_test_{seq}"));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("mkdir");
        std::env::set_var("LOCALAPPDATA", &dir);
        NetworkPolicyStore::new()
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
        let _guard = POLICY_TEST_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let dir = std::env::temp_dir().join(format!(
            "pkb_policy_rt_{}",
            TEST_DIR_SEQ.fetch_add(1, Ordering::SeqCst)
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("mkdir");
        std::env::set_var("LOCALAPPDATA", &dir);

        {
            let store = NetworkPolicyStore::new();
            store.set_enabled(true).expect("set");
        }
        let reloaded = NetworkPolicyStore::new();
        assert_eq!(reloaded.get(), NetworkPolicy::Live);

        reloaded.set_enabled(false).expect("unset");
        let again = NetworkPolicyStore::new();
        assert_eq!(again.get(), NetworkPolicy::Off);
    }
}
