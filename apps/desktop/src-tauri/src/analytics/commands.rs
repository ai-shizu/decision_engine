//! Tauri commands for Gap Analysis + Tensor Profile (M14).

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::Serialize;
use serde_json::Value;
use tauri::State;

use crate::analytics::gap::analyze_gaps;
use crate::analytics::input::{validate_request, CalculateGapRequest};
use crate::analytics::prompt::build_gap_languageization_prompt;
use crate::analytics::tensor::{authoritative_profile, tensor_to_json, TensorProfile};
use crate::db::{GapAnalysisRow, TensorProfileRow, VaultErrorCode, VaultHandle};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct CalculateGapAnalysisResult {
    pub id: String,
    pub data_sufficiency: f64,
    pub gap_count: usize,
    pub gaps: Vec<Value>,
    pub languageization_prompt: String,
    pub payload: Value,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct LatestGapAnalysisResult {
    pub id: String,
    pub created_at: i64,
    pub data_sufficiency: f64,
    pub payload: Value,
}

fn map_vault(err: VaultErrorCode) -> String {
    format!("{err:?}").to_ascii_lowercase()
}

fn now_unix() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// Process-wide sequence appended to every id — `now_unix()` is second-
/// precision, so two ids minted within the same second (e.g.
/// `calculate_gap_analysis`'s internal tensor snapshot insert immediately
/// followed by `commands::ensure_authoritative_tensor_profile`'s own insert,
/// both from the on-device profile-rebuild fallback) previously collided on
/// `tensor_profiles.id TEXT PRIMARY KEY`, surfacing as a `StorageFailed`
/// device-log error. This counter makes every id unique regardless of timing,
/// with no change to `created_at` (still plain unix seconds, relied on
/// elsewhere for JST date bucketing).
static ID_SEQ: AtomicU64 = AtomicU64::new(0);

fn new_id(prefix: &str) -> String {
    let seq = ID_SEQ.fetch_add(1, Ordering::Relaxed);
    format!("{prefix}-{}-{seq}", now_unix())
}

/// Deterministic gap analysis → vault persist. Does not call the LLM.
#[tauri::command]
pub async fn calculate_gap_analysis(
    vault: State<'_, VaultHandle>,
    request: CalculateGapRequest,
) -> Result<CalculateGapAnalysisResult, String> {
    validate_request(&request)?;
    let vault = vault.inner().clone();
    let days = request.days;

    tauri::async_runtime::spawn_blocking(move || {
        let result = analyze_gaps(&days);
        let id = new_id("gap");
        let payload_json = serde_json::to_string(&result.payload)
            .map_err(|_| "gap payload serialize failed".to_string())?;
        vault
            .gap_analysis_insert(GapAnalysisRow {
                id: id.clone(),
                created_at: now_unix(),
                schema_version: result.schema.clone(),
                data_sufficiency: result.data_sufficiency,
                payload_json,
            })
            .map_err(map_vault)?;

        // Persist authoritative N/A tensor snapshot alongside gap runs.
        let tensor = authoritative_profile();
        let tensor_json = serde_json::to_string(&tensor)
            .map_err(|_| "tensor serialize failed".to_string())?;
        vault
            .tensor_profile_insert(TensorProfileRow {
                id: new_id("tensor"),
                created_at: now_unix(),
                schema_version: tensor.schema.clone(),
                model_hash: tensor.model_hash.clone(),
                payload_json: tensor_json,
            })
            .map_err(map_vault)?;

        let languageization_prompt = build_gap_languageization_prompt(&result);
        Ok(CalculateGapAnalysisResult {
            id,
            data_sufficiency: result.data_sufficiency,
            gap_count: result.gaps.len(),
            gaps: result.gaps,
            languageization_prompt,
            payload: result.payload,
        })
    })
    .await
    .map_err(|_| "gap analysis task join failed".to_string())?
}

#[tauri::command]
pub async fn get_latest_gap_analysis(
    vault: State<'_, VaultHandle>,
) -> Result<Option<LatestGapAnalysisResult>, String> {
    let vault = vault.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let row = vault.gap_analysis_latest().map_err(map_vault)?;
        match row {
            None => Ok(None),
            Some(r) => {
                let payload: Value = serde_json::from_str(&r.payload_json)
                    .map_err(|_| "gap payload parse failed".to_string())?;
                Ok(Some(LatestGapAnalysisResult {
                    id: r.id,
                    created_at: r.created_at,
                    data_sufficiency: r.data_sufficiency,
                    payload,
                }))
            }
        }
    })
    .await
    .map_err(|_| "get_latest_gap_analysis join failed".to_string())?
}

#[tauri::command]
pub async fn get_latest_tensor_profile(
    vault: State<'_, VaultHandle>,
) -> Result<TensorProfile, String> {
    let vault = vault.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        match vault.tensor_profile_latest().map_err(map_vault)? {
            Some(row) => serde_json::from_str(&row.payload_json)
                .map_err(|_| "tensor payload parse failed".to_string()),
            None => {
                // Fail closed to authoritative N/A rather than inventing scores.
                Ok(authoritative_profile())
            }
        }
    })
    .await
    .map_err(|_| "get_latest_tensor_profile join failed".to_string())?
}

#[tauri::command]
pub async fn ensure_authoritative_tensor_profile(
    vault: State<'_, VaultHandle>,
) -> Result<Value, String> {
    let vault = vault.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let profile = authoritative_profile();
        let payload = tensor_to_json(&profile);
        let payload_json = serde_json::to_string(&profile)
            .map_err(|_| "tensor serialize failed".to_string())?;
        vault
            .tensor_profile_insert(TensorProfileRow {
                id: new_id("tensor"),
                created_at: now_unix(),
                schema_version: profile.schema,
                model_hash: profile.model_hash,
                payload_json,
            })
            .map_err(map_vault)?;
        Ok(payload)
    })
    .await
    .map_err(|_| "ensure_authoritative_tensor_profile join failed".to_string())?
}
