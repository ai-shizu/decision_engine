//! Offline model setup gate (AI_SKILLS §5): existence check + local GGUF import.
//!
//! Network download of models is permanently forbidden. The only write path is a
//! user-picked local file, chunk-copied into `resolve_model_path`. Opening the
//! recommended Hugging Face page uses the OS browser (user-initiated navigation);
//! the app never fetches model bytes.

use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use serde::Serialize;
use tauri::{AppHandle, Emitter};

use super::model_path::{resolve_model_path, MODEL_FILENAME};

/// Event name for chunk-copy progress (frontend `listen`).
pub const MODEL_IMPORT_PROGRESS_EVENT: &str = "model-import-progress";

/// Official recommended GGUF listing (guidance only — never fetched by the app).
pub const RECOMMENDED_MODEL_PAGE_URL: &str =
    "https://huggingface.co/Qwen/Qwen2.5-7B-Instruct-GGUF";

const COPY_CHUNK_BYTES: usize = 1024 * 1024;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelExistsStatus {
    pub exists: bool,
    /// Stable relative hint under the A+1 data root (`models/pocket-brain.gguf`).
    pub relative_path: String,
    /// Browser guidance URL (open via `open_recommended_model_page`, not fetch).
    pub recommended_page_url: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelImportProgress {
    pub percent: u8,
    pub bytes_copied: u64,
    pub total_bytes: u64,
}

/// True when `<user_data_root>/models/pocket-brain.gguf` is a non-empty file.
#[tauri::command]
pub fn check_model_exists(app: AppHandle) -> Result<ModelExistsStatus, String> {
    let path = resolve_model_path(&app)?;
    let exists = path.is_file()
        && fs::metadata(&path)
            .map(|m| m.len() > 0)
            .unwrap_or(false);
    Ok(ModelExistsStatus {
        exists,
        relative_path: format!("models/{MODEL_FILENAME}"),
        recommended_page_url: RECOMMENDED_MODEL_PAGE_URL.to_string(),
    })
}

/// Native file picker for a local `.gguf`. Returns `None` if the user cancels.
/// Unavailable on iOS (use Files / inject script); never downloads.
#[tauri::command]
pub async fn pick_local_gguf() -> Result<Option<String>, String> {
    #[cfg(target_os = "ios")]
    {
        return Err(
            "この端末ではファイル選択ダイアログを使えません。Files または inject スクリプトで models へ配置してください。"
                .into(),
        );
    }
    #[cfg(not(target_os = "ios"))]
    {
        let picked = tauri::async_runtime::spawn_blocking(|| {
            rfd::FileDialog::new()
                .add_filter("GGUF model", &["gguf"])
                .set_title("Coraxis — ローカル GGUF を選択")
                .pick_file()
        })
        .await
        .map_err(|_| "ファイル選択を開始できませんでした。".to_string())?;

        Ok(picked.map(|p| p.to_string_lossy().into_owned()))
    }
}

/// Open the recommended model page in the system browser (no in-app fetch).
#[tauri::command]
pub fn open_recommended_model_page() -> Result<(), String> {
    open_https_url(RECOMMENDED_MODEL_PAGE_URL)
}

/// Chunk-copy a user-selected local GGUF into the A+1 model path with progress events.
#[tauri::command]
pub async fn import_local_model(app: AppHandle, source_path: String) -> Result<(), String> {
    let dest = resolve_model_path(&app)?;
    let source = validate_gguf_source(&source_path)?;

    let app_progress = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        copy_gguf_with_progress(&source, &dest, &app_progress)
    })
    .await
    .map_err(|_| "モデルの取り込みを開始できませんでした。".to_string())?
}

fn validate_gguf_source(raw: &str) -> Result<PathBuf, String> {
    let path = PathBuf::from(raw);
    if !path.is_absolute() {
        return Err("モデルファイルのパスが不正です。".into());
    }
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase());
    if ext.as_deref() != Some("gguf") {
        return Err("GGUF ファイル（.gguf）を選択してください。".into());
    }
    let meta = fs::metadata(&path).map_err(|_| "選択したファイルを読み取れません。".to_string())?;
    if !meta.is_file() || meta.len() == 0 {
        return Err("選択したファイルが空か、通常のファイルではありません。".into());
    }
    // Reject obvious directory traversal / non-canonical oddities softly.
    let canonical = path
        .canonicalize()
        .map_err(|_| "選択したファイルを解決できません。".to_string())?;
    if !canonical.is_file() {
        return Err("選択したファイルを読み取れません。".into());
    }
    Ok(canonical)
}

fn copy_gguf_with_progress(source: &Path, dest: &Path, app: &AppHandle) -> Result<(), String> {
    let total = fs::metadata(source)
        .map_err(|_| "選択したファイルを読み取れません。".to_string())?
        .len();
    if total == 0 {
        return Err("選択したファイルが空です。".into());
    }

    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent).map_err(|_| "モデル保存先を作成できません。".to_string())?;
    }

    let partial = dest.with_extension("gguf.partial");
    let _ = fs::remove_file(&partial);

    let result = (|| {
        let mut reader =
            File::open(source).map_err(|_| "選択したファイルを開けません。".to_string())?;
        let mut writer =
            File::create(&partial).map_err(|_| "モデルの一時ファイルを作成できません。".to_string())?;

        let mut buf = vec![0u8; COPY_CHUNK_BYTES];
        let mut copied: u64 = 0;
        let mut last_emitted: u8 = 255;

        emit_progress(app, 0, 0, total);

        loop {
            let n = reader
                .read(&mut buf)
                .map_err(|_| "モデルの読み込み中に失敗しました。".to_string())?;
            if n == 0 {
                break;
            }
            writer
                .write_all(&buf[..n])
                .map_err(|_| "モデルの書き込み中に失敗しました。".to_string())?;
            copied = copied.saturating_add(n as u64);
            let percent = ((copied as u128 * 100) / total as u128).min(99) as u8;
            if percent != last_emitted {
                emit_progress(app, percent, copied, total);
                last_emitted = percent;
            }
        }

        writer
            .flush()
            .map_err(|_| "モデルの書き込み完了に失敗しました。".to_string())?;
        drop(writer);

        fs::rename(&partial, dest).map_err(|_| "モデルの確定に失敗しました。".to_string())?;
        emit_progress(app, 100, total, total);
        Ok(())
    })();

    if result.is_err() {
        let _ = fs::remove_file(&partial);
        let _ = fs::remove_file(dest);
    }
    result
}

fn emit_progress(app: &AppHandle, percent: u8, bytes_copied: u64, total_bytes: u64) {
    let _ = app.emit(
        MODEL_IMPORT_PROGRESS_EVENT,
        ModelImportProgress {
            percent,
            bytes_copied,
            total_bytes,
        },
    );
}

fn open_https_url(url: &str) -> Result<(), String> {
    if !url.starts_with("https://") {
        return Err("案内先 URL が不正です。".into());
    }
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open")
            .arg(url)
            .spawn()
            .map_err(|_| "ブラウザを開けませんでした。".to_string())?;
        return Ok(());
    }
    #[cfg(target_os = "ios")]
    {
        // iOS: `open` CLI unavailable; surface the URL via Err so UI can show copy text.
        let _ = url;
        return Err(
            "ブラウザ自動起動はこの端末では未対応です。案内 URL を手動で開いてください。".into(),
        );
    }
    #[cfg(target_os = "windows")]
    {
        std::process::Command::new("cmd")
            .args(["/C", "start", "", url])
            .spawn()
            .map_err(|_| "ブラウザを開けませんでした。".to_string())?;
        return Ok(());
    }
    #[cfg(all(unix, not(target_os = "macos"), not(target_os = "ios")))]
    {
        std::process::Command::new("xdg-open")
            .arg(url)
            .spawn()
            .map_err(|_| "ブラウザを開けませんでした。".to_string())?;
        return Ok(());
    }
    #[cfg(not(any(
        target_os = "macos",
        target_os = "ios",
        target_os = "windows",
        all(unix, not(target_os = "macos"), not(target_os = "ios"))
    )))]
    {
        let _ = url;
        Err("この OS では案内ページを開けません。".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn validate_rejects_non_gguf_and_relative() {
        assert!(validate_gguf_source("model.bin").is_err());
        assert!(validate_gguf_source("relative.gguf").is_err());
    }

    #[test]
    fn validate_accepts_absolute_gguf() {
        let dir = std::env::temp_dir().join(format!(
            "pkb-model-setup-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        fs::create_dir_all(&dir).expect("tmpdir");
        let path = dir.join("sample.gguf");
        {
            let mut f = File::create(&path).expect("create");
            f.write_all(b"GGUF-test").expect("write");
        }
        let ok = validate_gguf_source(&path.to_string_lossy());
        let _ = fs::remove_dir_all(&dir);
        assert!(ok.is_ok());
    }

    #[test]
    fn recommended_url_is_https_only() {
        assert!(RECOMMENDED_MODEL_PAGE_URL.starts_with("https://"));
        assert!(open_https_url("http://example.com").is_err());
    }
}
