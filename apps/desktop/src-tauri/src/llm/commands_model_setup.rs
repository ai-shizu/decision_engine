//! Offline model setup gate (AI_SKILLS §5): existence check + FE-driven import.
//!
//! Network download of models is permanently forbidden. Bytes reach the app only
//! via frontend `@tauri-apps/plugin-fs` copy into `app_data_dir()/models/`.
//! Rust never opens Security-Scoped picker paths (`std::fs` on those URLs fails
//! on iOS). Opening the recommended Hugging Face page uses the OS browser
//! (user-initiated); the app never fetches model bytes.

use serde::Serialize;
use tauri::AppHandle;

use super::model_path::{
    internal_model_present, resolve_model_path, validate_gguf_file, MODEL_RELATIVE_PATH,
};

/// Official recommended GGUF listing (guidance only — never fetched by the app).
pub const RECOMMENDED_MODEL_PAGE_URL: &str = "https://huggingface.co/Qwen/Qwen2.5-7B-Instruct-GGUF";

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelExistsStatus {
    pub exists: bool,
    /// Stable relative hint under AppData (`models/pocket-brain.gguf`).
    pub relative_path: String,
    /// Browser guidance URL (open via `open_recommended_model_page`, not fetch).
    pub recommended_page_url: String,
}

/// Absolute + relative destinations for the FE fs copy (AppData only).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelImportDest {
    pub absolute_path: String,
    pub relative_path: String,
}

/// True when `<app_data_dir>/models/pocket-brain.gguf` is a non-empty file.
#[tauri::command]
pub fn check_model_exists(app: AppHandle) -> Result<ModelExistsStatus, String> {
    let _ = resolve_model_path(&app)?;
    Ok(ModelExistsStatus {
        exists: internal_model_present(&app)?,
        relative_path: MODEL_RELATIVE_PATH.to_string(),
        recommended_page_url: RECOMMENDED_MODEL_PAGE_URL.to_string(),
    })
}

/// Ensure AppData `models/` exists and return the canonical dest paths for FE copy.
#[tauri::command]
pub fn prepare_model_import_dest(app: AppHandle) -> Result<ModelImportDest, String> {
    let path = resolve_model_path(&app)?;
    Ok(ModelImportDest {
        absolute_path: path.to_string_lossy().into_owned(),
        relative_path: MODEL_RELATIVE_PATH.to_string(),
    })
}

/// After FE copy: verify the internal GGUF is present (never accepts an external path).
#[tauri::command]
pub fn confirm_model_imported(app: AppHandle) -> Result<(), String> {
    let path = resolve_model_path(&app)?;
    validate_gguf_file(&path)
}

/// Open the recommended model page in the system browser (no in-app fetch).
#[tauri::command]
pub fn open_recommended_model_page(app: AppHandle) -> Result<(), String> {
    open_https_url(&app, RECOMMENDED_MODEL_PAGE_URL)
}

fn require_https_guidance_url(url: &str) -> Result<(), String> {
    if !url.starts_with("https://") {
        return Err("案内先 URL が不正です。".into());
    }
    Ok(())
}

fn open_https_url(app: &AppHandle, url: &str) -> Result<(), String> {
    require_https_guidance_url(url)?;
    #[cfg(target_os = "macos")]
    {
        let _ = app;
        std::process::Command::new("open")
            .arg(url)
            .spawn()
            .map_err(|_| "ブラウザを開けませんでした。".to_string())?;
        return Ok(());
    }
    #[cfg(all(feature = "secure-vault", target_os = "ios"))]
    {
        return ios_open_https(app, url);
    }
    #[cfg(all(target_os = "ios", not(feature = "secure-vault")))]
    {
        let _ = (app, url);
        return Err(
            "ブラウザ自動起動には secure-vault ビルドが必要です。案内 URL を手動で開いてください。"
                .into(),
        );
    }
    #[cfg(target_os = "windows")]
    {
        let _ = app;
        std::process::Command::new("cmd")
            .args(["/C", "start", "", url])
            .spawn()
            .map_err(|_| "ブラウザを開けませんでした。".to_string())?;
        return Ok(());
    }
    #[cfg(all(unix, not(target_os = "macos"), not(target_os = "ios")))]
    {
        let _ = app;
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
        let _ = (app, url);
        Err("この OS では案内ページを開けません。".into())
    }
}

#[cfg(all(feature = "secure-vault", target_os = "ios"))]
fn ios_open_https(app: &AppHandle, url: &str) -> Result<(), String> {
    let url = url.to_string();
    let (tx, rx) = std::sync::mpsc::channel();
    app.run_on_main_thread(move || {
        let _ = tx.send(ios_open_https_on_main(&url));
    })
    .map_err(|_| "ブラウザ起動をメインスレッドへ渡せませんでした。".to_string())?;
    rx.recv()
        .map_err(|_| "ブラウザ起動の応答が失われました。".to_string())?
}

#[cfg(all(feature = "secure-vault", target_os = "ios"))]
fn ios_open_https_on_main(url: &str) -> Result<(), String> {
    use objc2::runtime::AnyObject;
    use objc2::MainThreadMarker;
    use objc2_foundation::{NSDictionary, NSString, NSURL};
    use objc2_ui_kit::{UIApplication, UIApplicationOpenExternalURLOptionsKey};

    let Some(mtm) = MainThreadMarker::new() else {
        return Err("ブラウザ起動はメインスレッド必須です。".into());
    };
    let ns = NSString::from_str(url);
    let Some(ns_url) = NSURL::URLWithString(&ns) else {
        return Err("案内先 URL が不正です。".into());
    };
    let shared = UIApplication::sharedApplication(mtm);
    if !shared.canOpenURL(&ns_url) {
        return Err("Safari を開けませんでした。案内 URL を手動で開いてください。".into());
    }
    let options = NSDictionary::<UIApplicationOpenExternalURLOptionsKey, AnyObject>::new();
    // Non-deprecated API; completion is optional (fire-and-forget from IPC).
    unsafe {
        shared.openURL_options_completionHandler(&ns_url, &options, None);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recommended_url_is_https_only() {
        assert!(RECOMMENDED_MODEL_PAGE_URL.starts_with("https://"));
        assert!(require_https_guidance_url("http://example.com").is_err());
        assert!(require_https_guidance_url(RECOMMENDED_MODEL_PAGE_URL).is_ok());
    }
}
