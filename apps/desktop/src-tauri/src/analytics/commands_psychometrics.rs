//! Tauri commands for M15 psychometrics (pulse / rasch / probe).

use std::collections::BTreeSet;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use tauri::State;

use crate::analytics::probe::{
    empty_store, get_probe_questions as bank_questions, probe_answer, probe_next, probe_status,
    AxisScoreHint, ProbeAnswerResultV1, ProbeQuestionOut, ProbeQuestionV1, ProbeStatusV1,
    ProbeStore,
};
use crate::analytics::rasch::{
    artifact_fingerprint, evaluate_rasch_scale as rasch_update_posterior, initial_posterior,
    rasch_select_next, ItemSelection, GRID_LEN, RASCH_SCHEMA,
};
use crate::analytics::romance_pulse::{
    calculate_interaction_pulse as compute_interaction_pulse, RomanceAnalysisV1, RomanceMetrics,
};
use crate::db::{ProbeStoreRow, PulseRunRow, RaschRunRow, VaultErrorCode, VaultHandle};

fn map_vault(err: VaultErrorCode) -> String {
    format!("{err:?}").to_ascii_lowercase()
}

fn now_unix() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

fn new_id(prefix: &str) -> String {
    format!("{prefix}-{}", now_unix())
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct CalculatePulseRequest {
    pub transcript: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct CalculatePulseResult {
    pub id: String,
    pub analysis: RomanceAnalysisV1,
    pub metrics: RomanceMetrics,
    pub input_hash: String,
}

/// Deterministic romance_analysis.v1 → vault. Raw transcript is never stored.
#[tauri::command]
pub async fn calculate_interaction_pulse(
    vault: State<'_, VaultHandle>,
    request: CalculatePulseRequest,
) -> Result<CalculatePulseResult, String> {
    let vault = vault.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let (analysis, metrics, input_hash) =
            compute_interaction_pulse(&request.transcript);
        let metrics_json = serde_json::to_string(&metrics)
            .map_err(|_| "metrics serialize failed".to_string())?;
        let id = new_id("pulse");
        vault
            .pulse_run_insert(PulseRunRow {
                id: id.clone(),
                created_at: now_unix(),
                affinity_score: analysis.affinity_score.map(|s| i64::from(s)),
                interaction_tendency: analysis.interaction_tendency.clone(),
                next_best_action: analysis.next_best_action.clone(),
                metrics_json,
                input_hash: input_hash.clone(),
            })
            .map_err(map_vault)?;
        Ok(CalculatePulseResult {
            id,
            analysis,
            metrics,
            input_hash,
        })
    })
    .await
    .map_err(|_| "calculate_interaction_pulse join failed".to_string())?
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct EvaluateRaschRequest {
    /// Optional prior posterior (len 17). Absent → uniform / initial.
    pub posterior: Option<Vec<f64>>,
    pub item_id: String,
    /// Ordinal response 0..=4.
    pub response: u8,
    /// Item ids already administered.
    pub excluded: Option<Vec<String>>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct EvaluateRaschResult {
    pub schema: String,
    pub artifact_sha256: String,
    pub posterior: Vec<f64>,
    pub excluded: Vec<String>,
    pub next: Option<ItemSelection>,
}

fn parse_posterior(raw: Option<Vec<f64>>) -> Result<[f64; GRID_LEN], String> {
    match raw {
        None => Ok(initial_posterior()),
        Some(v) => {
            if v.len() != GRID_LEN {
                return Err(format!("posterior must have length {GRID_LEN}"));
            }
            let mut arr = [0.0_f64; GRID_LEN];
            arr.copy_from_slice(&v);
            if arr.iter().any(|p| !p.is_finite() || *p < 0.0) {
                return Err("posterior entries must be finite and non-negative".into());
            }
            let sum: f64 = arr.iter().sum();
            if !(sum.is_finite() && sum > 0.0) {
                return Err("posterior mass must be positive finite".into());
            }
            Ok(arr)
        }
    }
}

/// Bayesian PCM update + next-item EIG selection. Persists filter state.
#[tauri::command]
pub async fn evaluate_rasch_scale(
    vault: State<'_, VaultHandle>,
    request: EvaluateRaschRequest,
) -> Result<EvaluateRaschResult, String> {
    let vault = vault.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let prior = parse_posterior(request.posterior)?;
        let posterior = rasch_update_posterior(&prior, &request.item_id, request.response)?;
        let mut excluded: BTreeSet<String> = request
            .excluded
            .unwrap_or_default()
            .into_iter()
            .collect();
        excluded.insert(request.item_id.clone());
        let next = rasch_select_next(&posterior, &excluded);
        let artifact = artifact_fingerprint();
        let posterior_json = serde_json::to_string(&posterior.to_vec())
            .map_err(|_| "posterior serialize failed".to_string())?;
        let excluded_vec: Vec<String> = excluded.iter().cloned().collect();
        let excluded_json = serde_json::to_string(&excluded_vec)
            .map_err(|_| "excluded serialize failed".to_string())?;
        let last_selection_json = next
            .as_ref()
            .map(serde_json::to_string)
            .transpose()
            .map_err(|_| "selection serialize failed".to_string())?;

        vault
            .rasch_run_upsert(RaschRunRow {
                id: "default".into(),
                created_at: now_unix(),
                artifact_sha256: artifact.clone(),
                posterior_json,
                excluded_json,
                last_selection_json,
            })
            .map_err(map_vault)?;

        Ok(EvaluateRaschResult {
            schema: RASCH_SCHEMA.into(),
            artifact_sha256: artifact,
            posterior: posterior.to_vec(),
            excluded: excluded_vec,
            next,
        })
    })
    .await
    .map_err(|_| "evaluate_rasch_scale join failed".to_string())?
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct SelectRaschRequest {
    pub posterior: Option<Vec<f64>>,
    pub excluded: Option<Vec<String>>,
}

#[tauri::command]
pub async fn rasch_select_next_item(
    request: SelectRaschRequest,
) -> Result<Option<ItemSelection>, String> {
    let posterior = parse_posterior(request.posterior)?;
    let excluded: BTreeSet<String> = request.excluded.unwrap_or_default().into_iter().collect();
    Ok(rasch_select_next(&posterior, &excluded))
}

#[tauri::command]
pub async fn get_probe_questions() -> Result<Vec<ProbeQuestionOut>, String> {
    Ok(bank_questions())
}

fn load_probe_store(vault: &VaultHandle) -> Result<ProbeStore, String> {
    match vault.probe_store_get().map_err(map_vault)? {
        None => Ok(empty_store()),
        Some(row) => serde_json::from_str(&row.payload_json)
            .map_err(|_| "probe store parse failed".to_string()),
    }
}

fn save_probe_store(vault: &VaultHandle, store: &ProbeStore) -> Result<(), String> {
    let payload_json =
        serde_json::to_string(store).map_err(|_| "probe store serialize failed".to_string())?;
    vault
        .probe_store_put(ProbeStoreRow {
            id: "default".into(),
            updated_at: now_unix(),
            payload_json,
        })
        .map_err(map_vault)
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct ProbeNextRequest {
    pub today: String,
    pub axis_hints: Option<Vec<AxisScoreHint>>,
}

#[tauri::command]
pub async fn probe_next_question(
    vault: State<'_, VaultHandle>,
    request: ProbeNextRequest,
) -> Result<ProbeQuestionV1, String> {
    let vault = vault.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let mut store = load_probe_store(&vault)?;
        let hints = request.axis_hints.unwrap_or_default();
        let question = probe_next(&mut store, &request.today, &hints)?;
        save_probe_store(&vault, &store)?;
        Ok(question)
    })
    .await
    .map_err(|_| "probe_next_question join failed".to_string())?
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct ProbeAnswerRequest {
    pub session_id: String,
    pub question_id: String,
    pub answer: String,
    pub today: String,
    pub axis_hints: Option<Vec<AxisScoreHint>>,
}

#[tauri::command]
pub async fn probe_submit_answer(
    vault: State<'_, VaultHandle>,
    request: ProbeAnswerRequest,
) -> Result<ProbeAnswerResultV1, String> {
    let vault = vault.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let mut store = load_probe_store(&vault)?;
        let hints = request.axis_hints.unwrap_or_default();
        let result = probe_answer(
            &mut store,
            &request.session_id,
            &request.question_id,
            &request.answer,
            &request.today,
            &hints,
        )?;
        save_probe_store(&vault, &store)?;
        Ok(result)
    })
    .await
    .map_err(|_| "probe_submit_answer join failed".to_string())?
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct ProbeStatusRequest {
    pub today: String,
}

#[tauri::command]
pub async fn get_probe_status(
    vault: State<'_, VaultHandle>,
    request: ProbeStatusRequest,
) -> Result<ProbeStatusV1, String> {
    let vault = vault.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let store = load_probe_store(&vault)?;
        Ok(probe_status(&store, &request.today))
    })
    .await
    .map_err(|_| "get_probe_status join failed".to_string())?
}

#[tauri::command]
pub async fn get_latest_rasch_state(
    vault: State<'_, VaultHandle>,
) -> Result<Option<Value>, String> {
    let vault = vault.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        match vault.rasch_run_latest().map_err(map_vault)? {
            None => Ok(None),
            Some(row) => {
                let posterior: Value = serde_json::from_str(&row.posterior_json)
                    .map_err(|_| "posterior parse failed".to_string())?;
                let excluded: Value = serde_json::from_str(&row.excluded_json)
                    .map_err(|_| "excluded parse failed".to_string())?;
                let last_selection: Option<Value> = row
                    .last_selection_json
                    .as_ref()
                    .map(|s| serde_json::from_str(s))
                    .transpose()
                    .map_err(|_| "selection parse failed".to_string())?;
                Ok(Some(serde_json::json!({
                    "id": row.id,
                    "created_at": row.created_at,
                    "artifact_sha256": row.artifact_sha256,
                    "posterior": posterior,
                    "excluded": excluded,
                    "last_selection": last_selection,
                })))
            }
        }
    })
    .await
    .map_err(|_| "get_latest_rasch_state join failed".to_string())?
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn posterior_rejects_negative_entry_even_when_total_is_positive() {
        let mut posterior = vec![1.0 / GRID_LEN as f64; GRID_LEN];
        posterior[0] = -0.25;
        posterior[1] += 0.25;
        assert_eq!(
            parse_posterior(Some(posterior)),
            Err("posterior entries must be finite and non-negative".into())
        );
    }

    #[test]
    fn posterior_rejects_non_finite_entry() {
        let mut posterior = vec![1.0 / GRID_LEN as f64; GRID_LEN];
        posterior[0] = f64::INFINITY;
        assert!(parse_posterior(Some(posterior)).is_err());
    }
}
