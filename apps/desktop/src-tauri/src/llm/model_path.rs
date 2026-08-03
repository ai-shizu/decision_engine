//! GGUF path resolution for on-device Pocket Brain (docs/architecture_blueprint.md §3.5).
//!
//! ## Load priority
//! 1. **Bundled resource** — `$RESOURCE/models/pocket-brain.gguf` (iOS App Bundle;
//!    V1 ships the 1.5B GGUF via `bundle.resources` for offline CONSULT).
//! 2. **AppData import** — `app.path().app_data_dir()/models/pocket-brain.gguf`
//!    (FE `plugin-fs` copy / Simulator inject). Never read Security-Scoped picker
//!    paths from Rust (AI_SKILLS §5.1 / §4.72).

use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};

use tauri::{AppHandle, Manager};

/// Fixed filename the operator places (or FE-imports) inside `<app_data>/models/`.
pub const MODEL_FILENAME: &str = "pocket-brain.gguf";

/// Relative path under AppData **and** under `$RESOURCE/` when bundled.
pub const MODEL_RELATIVE_PATH: &str = "models/pocket-brain.gguf";
const GGUF_MAGIC_LE: u32 = 0x4655_4747;

/// Writable AppData destination for FE import (`prepare_model_import_dest`).
///
/// Fail-closed if `app_data_dir` cannot be resolved — never fall back to CWD
/// or an externally supplied path. Creates the parent `models/` directory.
pub fn resolve_model_path(app: &AppHandle) -> Result<PathBuf, String> {
    let root = app
        .path()
        .app_data_dir()
        .map_err(|e| {
            log::error!("app_data_dir unresolved: {e}");
            "アプリデータ領域を解決できません。".to_string()
        })?;
    let dir = root.join("models");
    if let Err(e) = std::fs::create_dir_all(&dir) {
        log::error!("models dir create failed at {}: {e}", dir.display());
        return Err("モデル保存先を作成できません。".to_string());
    }
    Ok(dir.join(MODEL_FILENAME))
}

/// Prefer the App Bundle resource (iOS offline ship), else AppData import path.
pub fn resolve_loadable_model_path(app: &AppHandle) -> Result<PathBuf, String> {
    // `resolve_model_path` is passed as a thunk so the AppData `models/` dir is
    // only created when the bundled resource is absent or fails validation.
    select_loadable_path(try_bundled_model_path(app), || resolve_model_path(app))
}

/// Pure load-priority selector shared by [`resolve_loadable_model_path`], kept
/// `AppHandle`-free so the resource-first / AppData-fallback contract is
/// unit-testable without a Tauri app.
///
/// `bundled` is the resolved `$RESOURCE/...` candidate (`None` on desktop, where
/// the resource is absent). Returns the first path that passes GGUF validation:
/// bundled first, then whatever `appdata` yields. `appdata` is a thunk so its
/// side effects (directory creation) run only on fallback. When neither path
/// validates, the AppData error is surfaced — that is the writable location the
/// user is instructed to import into.
fn select_loadable_path(
    bundled: Option<PathBuf>,
    appdata: impl FnOnce() -> Result<PathBuf, String>,
) -> Result<PathBuf, String> {
    if let Some(bundled) = bundled {
        match validate_gguf_file(&bundled) {
            Ok(()) => {
                log::info!("using bundled GGUF at {}", bundled.display());
                return Ok(bundled);
            }
            Err(e) => {
                log::error!(
                    "bundled GGUF present but invalid at {}: {e}",
                    bundled.display()
                );
            }
        }
    }

    let path = appdata()?;
    match validate_gguf_file(&path) {
        Ok(()) => {
            log::info!("using AppData GGUF at {}", path.display());
            Ok(path)
        }
        Err(e) => {
            log::error!("AppData GGUF unavailable at {}: {e}", path.display());
            Err(e)
        }
    }
}

/// `$RESOURCE/models/pocket-brain.gguf` when the file exists on disk.
///
/// Uses [`tauri::path::BaseDirectory::Resource`] so iOS resolves under
/// `${exe_dir}/assets/...` while desktop uses the platform resource root.
fn try_bundled_model_path(app: &AppHandle) -> Option<PathBuf> {
    use tauri::path::BaseDirectory;

    let path = match app
        .path()
        .resolve(MODEL_RELATIVE_PATH, BaseDirectory::Resource)
    {
        Ok(path) => path,
        Err(e) => {
            log::error!("BaseDirectory::Resource resolve failed for {MODEL_RELATIVE_PATH}: {e}");
            return None;
        }
    };
    if path.is_file() {
        let bytes = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
        log::info!(
            "bundled model candidate {} ({} bytes)",
            path.display(),
            bytes
        );
        Some(path)
    } else {
        // Expected on desktop default builds; noisy only when we are on iOS
        // where V1 ships the resource.
        #[cfg(target_os = "ios")]
        log::error!("bundled model missing at {}", path.display());
        #[cfg(not(target_os = "ios"))]
        log::debug!("bundled model not present at {}", path.display());
        None
    }
}

fn validate_gguf_reader(reader: &mut impl Read) -> Result<(), String> {
    let mut magic = [0_u8; 4];
    reader
        .read_exact(&mut magic)
        .map_err(|_| "GGUFヘッダーを読み取れません。".to_string())?;
    if u32::from_le_bytes(magic) != GGUF_MAGIC_LE {
        return Err("選択されたファイルはGGUF形式ではありません。".into());
    }
    Ok(())
}

/// Minimum format guard used both after import and immediately before load.
/// This intentionally validates only the canonical four-byte GGUF magic; full
/// tensor/metadata validation remains llama.cpp's responsibility.
pub fn validate_gguf_file(path: &Path) -> Result<(), String> {
    let metadata = match std::fs::metadata(path) {
        Ok(m) => m,
        Err(e) => {
            log::error!("model metadata failed at {}: {e}", path.display());
            return Err("モデルファイルを確認できません。".to_string());
        }
    };
    if !metadata.is_file() || metadata.len() < 4 {
        log::error!(
            "model file empty/short at {} (len={})",
            path.display(),
            metadata.len()
        );
        return Err("モデルファイルが空または短すぎます。".into());
    }
    let mut file = match File::open(path) {
        Ok(f) => f,
        Err(e) => {
            log::error!("model open failed at {}: {e}", path.display());
            return Err("モデルファイルを開けません。".to_string());
        }
    };
    validate_gguf_reader(&mut file)
}

/// True when a loadable GGUF exists (bundled resource **or** AppData import).
pub fn internal_model_present(app: &AppHandle) -> Result<bool, String> {
    Ok(resolve_loadable_model_path(app).is_ok())
}

/// Path-origin discriminator for OSLog (P0-6-1). Never logs the path itself.
///
/// - `1` = bundled App resource (`…/assets/models/…` or inside `.app`)
/// - `2` = AppData / container import (`Application Support`)
/// - `0` = unrecognized layout
#[cfg_attr(not(target_os = "ios"), allow(dead_code))]
pub fn model_origin_code(path: &Path) -> u64 {
    let s = path.to_string_lossy();
    if s.contains("Application Support") {
        return 2;
    }
    if s.contains("/assets/models/") || s.contains(".app/") {
        return 1;
    }
    0
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;
    use std::sync::atomic::{AtomicUsize, Ordering};

    static TEST_DIR_SEQ: AtomicUsize = AtomicUsize::new(0);

    /// Fresh, empty temp dir following the project convention (no `tempfile`
    /// dev-dep); mirrors `knowledge::policy_store` test isolation.
    fn isolated_root() -> PathBuf {
        let seq = TEST_DIR_SEQ.fetch_add(1, Ordering::SeqCst);
        let dir = std::env::temp_dir().join(format!("pkb_model_path_test_{seq}"));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("mkdir");
        dir
    }

    /// Write `contents` to `<root>/<name>` and return the path.
    fn write_file(root: &Path, name: &str, contents: &[u8]) -> PathBuf {
        let path = root.join(name);
        std::fs::write(&path, contents).expect("write");
        path
    }

    #[test]
    fn relative_path_matches_filename() {
        assert!(MODEL_RELATIVE_PATH.ends_with(MODEL_FILENAME));
        assert!(MODEL_RELATIVE_PATH.starts_with("models/"));
    }

    #[test]
    fn bundled_valid_is_preferred_and_appdata_thunk_not_run() {
        let root = isolated_root();
        let bundled = write_file(&root, "bundled.gguf", b"GGUFpayload");
        // If the bundled resource validates, the AppData fallback thunk must not
        // run (no directory creation side effect on the resource-hit path).
        let appdata_ran = Cell::new(false);
        let chosen = select_loadable_path(Some(bundled.clone()), || {
            appdata_ran.set(true);
            Ok(write_file(&root, "appdata.gguf", b"GGUFpayload"))
        })
        .expect("bundled path should be chosen");
        assert_eq!(chosen, bundled);
        assert!(!appdata_ran.get(), "AppData thunk must be skipped on bundled hit");
    }

    #[test]
    fn falls_back_to_appdata_when_no_bundled() {
        let root = isolated_root();
        let appdata = write_file(&root, "appdata.gguf", b"GGUFpayload");
        let chosen = select_loadable_path(None, || Ok(appdata.clone()))
            .expect("AppData path should be chosen");
        assert_eq!(chosen, appdata);
    }

    #[test]
    fn falls_back_to_appdata_when_bundled_invalid() {
        let root = isolated_root();
        // Bundled file exists but is not GGUF (fails magic) -> must fall through.
        let bundled = write_file(&root, "bundled.gguf", b"ZIP!payload");
        let appdata = write_file(&root, "appdata.gguf", b"GGUFpayload");
        let chosen = select_loadable_path(Some(bundled), || Ok(appdata.clone()))
            .expect("should fall back to valid AppData model");
        assert_eq!(chosen, appdata);
    }

    #[test]
    fn falls_back_to_appdata_when_bundled_missing_on_disk() {
        let root = isolated_root();
        // Candidate path was resolved but the file is absent (metadata fails).
        let bundled = root.join("does-not-exist.gguf");
        let appdata = write_file(&root, "appdata.gguf", b"GGUFpayload");
        let chosen = select_loadable_path(Some(bundled), || Ok(appdata.clone()))
            .expect("missing bundled file must not shadow a valid AppData model");
        assert_eq!(chosen, appdata);
    }

    #[test]
    fn errors_when_neither_bundled_nor_appdata_valid() {
        let root = isolated_root();
        let missing_appdata = root.join("absent.gguf");
        let err = select_loadable_path(None, || Ok(missing_appdata.clone()))
            .expect_err("no loadable model should surface the AppData error");
        // Surfaces the fixed user-facing wording, never a raw path/exception.
        assert_eq!(err, "モデルファイルを確認できません。");
    }

    #[test]
    fn errors_when_appdata_thunk_itself_fails() {
        // e.g. app_data_dir unresolved / models dir uncreatable — the thunk's
        // own error must propagate unchanged.
        let err = select_loadable_path(None, || Err("アプリデータ領域を解決できません。".into()))
            .expect_err("thunk failure must propagate");
        assert_eq!(err, "アプリデータ領域を解決できません。");
    }

    #[test]
    fn gguf_magic_accepts_little_endian_signature() {
        let mut bytes = std::io::Cursor::new(b"GGUFpayload");
        assert!(validate_gguf_reader(&mut bytes).is_ok());
    }

    #[test]
    fn gguf_magic_rejects_non_gguf_and_short_files() {
        let mut wrong = std::io::Cursor::new(b"ZIP!payload");
        assert!(validate_gguf_reader(&mut wrong).is_err());
        let mut short = std::io::Cursor::new(b"GGU");
        assert!(validate_gguf_reader(&mut short).is_err());
    }

    #[test]
    fn model_origin_code_distinguishes_bundle_and_appdata_without_raw_path() {
        assert_eq!(
            model_origin_code(Path::new(
                "/var/containers/Bundle/Application/X/Coraxis.app/assets/models/pocket-brain.gguf"
            )),
            1
        );
        assert_eq!(
            model_origin_code(Path::new(
                "/var/mobile/Containers/Data/Application/Y/Library/Application Support/com.ai-shizu.pkb/models/pocket-brain.gguf"
            )),
            2
        );
        assert_eq!(model_origin_code(Path::new("/tmp/scratch.gguf")), 0);
    }
}
