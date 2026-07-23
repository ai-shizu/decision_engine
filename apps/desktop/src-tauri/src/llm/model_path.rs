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
    if let Some(bundled) = try_bundled_model_path(app) {
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

    let path = resolve_model_path(app)?;
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn relative_path_matches_filename() {
        assert!(MODEL_RELATIVE_PATH.ends_with(MODEL_FILENAME));
        assert!(MODEL_RELATIVE_PATH.starts_with("models/"));
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
}
