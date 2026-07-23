//! GGUF path resolution for on-device Pocket Brain (docs/architecture_blueprint.md §3.5).
//!
//! Canonical root = Tauri `app.path().app_data_dir()` (iOS Application Support /
//! desktop app-data). Never read Security-Scoped picker paths from Rust —
//! the frontend copies into this directory first (AI_SKILLS §5.1 / §4.72).

use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};

use tauri::{AppHandle, Manager};

/// Fixed filename the operator places (or FE-imports) inside `<app_data>/models/`.
pub const MODEL_FILENAME: &str = "pocket-brain.gguf";

/// Relative path under AppData (matches FE `BaseDirectory.AppData` write target).
pub const MODEL_RELATIVE_PATH: &str = "models/pocket-brain.gguf";
const GGUF_MAGIC_LE: u32 = 0x4655_4747;

/// Returns `<app_data_dir>/models/pocket-brain.gguf`, creating the parent dir.
///
/// Fail-closed if `app_data_dir` cannot be resolved — never fall back to CWD
/// or an externally supplied path.
pub fn resolve_model_path(app: &AppHandle) -> Result<PathBuf, String> {
    let root = app
        .path()
        .app_data_dir()
        .map_err(|_| "アプリデータ領域を解決できません。".to_string())?;
    let dir = root.join("models");
    std::fs::create_dir_all(&dir).map_err(|_| "モデル保存先を作成できません。".to_string())?;
    Ok(dir.join(MODEL_FILENAME))
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
    let metadata =
        std::fs::metadata(path).map_err(|_| "モデルファイルを確認できません。".to_string())?;
    if !metadata.is_file() || metadata.len() < 4 {
        return Err("モデルファイルが空または短すぎます。".into());
    }
    let mut file = File::open(path).map_err(|_| "モデルファイルを開けません。".to_string())?;
    validate_gguf_reader(&mut file)
}

/// True when the canonical internal model is a file with valid GGUF magic.
pub fn internal_model_present(app: &AppHandle) -> Result<bool, String> {
    let path = resolve_model_path(app)?;
    Ok(validate_gguf_file(&path).is_ok())
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
