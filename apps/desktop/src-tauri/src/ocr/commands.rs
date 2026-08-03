//! Tauri command: Vision OCR → layout text (Phase 11).

use serde::Serialize;
use tauri::AppHandle;

use super::vision::recognize_layout_text;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct OcrLayoutResult {
    pub text: String,
    pub char_count: usize,
}

/// OCR a receipt/image (base64 or raw bytes via number array) into layout text.
///
/// `image_bytes` is the JPEG/PNG payload as a JS `number[]` / `Uint8Array`.
#[tauri::command]
pub async fn ocr_recognize_layout(
    _app: AppHandle,
    image_bytes: Vec<u8>,
) -> Result<OcrLayoutResult, String> {
    let handle = tauri::async_runtime::spawn_blocking(move || recognize_layout_text(&image_bytes));
    let text = handle
        .await
        .map_err(|_| "ocr join failed".to_string())??;
    Ok(OcrLayoutResult {
        char_count: text.chars().count(),
        text,
    })
}
