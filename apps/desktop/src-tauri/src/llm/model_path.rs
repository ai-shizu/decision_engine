//! GGUF path resolution against the iOS App Container (docs/architecture_blueprint.md §3.5).
//!
//! The path is derived from Tauri's validated `app_data_dir` (the app sandbox's
//! Library/Application Support on iOS) — never guessed from CWD or `HOME`.

use std::path::PathBuf;

use tauri::{AppHandle, Manager};

/// Fixed filename the operator places inside `<app_data_dir>/models/`.
pub const MODEL_FILENAME: &str = "pocket-brain.gguf";

/// Returns `<app_data_dir>/models/pocket-brain.gguf`, creating the parent dir.
pub fn resolve_model_path(app: &AppHandle) -> Result<PathBuf, String> {
    let dir = app
        .path()
        .app_data_dir()
        .map_err(|e| format!("app_data_dir resolve failed: {e}"))?
        .join("models");
    std::fs::create_dir_all(&dir).map_err(|e| format!("mkdir {}: {e}", dir.display()))?;
    Ok(dir.join(MODEL_FILENAME))
}
