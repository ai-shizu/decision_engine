//! GGUF path resolution for on-device Pocket Brain (docs/architecture_blueprint.md §3.5).
//!
//! A+1 data root (`paths::user_data_root`):
//!   macOS/Windows/Linux → `…/PKB/models/pocket-brain.gguf`
//!   iOS → `…/Application Support/com.ai-shizu.pkb/models/pocket-brain.gguf`
//!
//! Never guess from CWD. Network download of this file is permanently forbidden
//! (AI_SKILLS §5); only offline local import may create it.

use std::path::PathBuf;

use tauri::AppHandle;

use crate::paths::user_data_root;

/// Fixed filename the operator places (or imports) inside `<data_root>/models/`.
pub const MODEL_FILENAME: &str = "pocket-brain.gguf";

/// Returns `<user_data_root>/models/pocket-brain.gguf`, creating the parent dir.
///
/// `AppHandle` is retained for call-site stability with Tauri commands; the path
/// itself is derived from the A+1 `user_data_root` (not a second ad-hoc root).
pub fn resolve_model_path(_app: &AppHandle) -> Result<PathBuf, String> {
    let dir = user_data_root().join("models");
    std::fs::create_dir_all(&dir).map_err(|_| "モデル保存先を作成できません。".to_string())?;
    Ok(dir.join(MODEL_FILENAME))
}
