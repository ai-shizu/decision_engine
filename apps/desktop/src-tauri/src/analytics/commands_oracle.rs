//! Tauri commands for M16 Digital Twin + Oracle orchestration.

use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use tauri::State;

use crate::analytics::coupling::{coupling_matrix, CouplingMatrix, MAX_LAG};
use crate::analytics::digital_twin::{
    evaluate_digital_twin_scenario_with_identify, ScenarioModifiers, TwinScenarioResult,
    TwinSnapshotInput, MC_HORIZON_DEFAULT,
};
use crate::analytics::oracle::{
    generate_oracle_payload as assemble_oracle_payload, render_oracle_consult, OracleProvenance,
    ORACLE_SCHEMA,
};
use crate::analytics::tensor::{authoritative_profile, TensorProfile};
use crate::analytics::twin_identify::{
    resolve_identify_status, warm_rls_from_twin_payloads, TwinIdentifyStatus,
};
use crate::db::{OracleRunRow, TwinRunRow, VaultErrorCode, VaultHandle};

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
pub struct EvaluateTwinRequest {
    pub today: String,
    pub horizon_days: Option<u32>,
    pub scenario: Option<ScenarioModifiers>,
    pub tensor: Option<TensorProfile>,
    pub pulse_affinity: Option<u8>,
    pub rasch_posterior: Option<Vec<f64>>,
    pub gap_data_sufficiency: Option<f64>,
    pub gap_count: Option<usize>,
    /// Optional row-major lane series for Echo coupling (n_rows * n_lanes).
    pub lane_values: Option<Vec<f64>>,
    pub lane_mask: Option<Vec<bool>>,
    pub n_rows: Option<usize>,
    pub n_lanes: Option<usize>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct EvaluateTwinResult {
    pub id: String,
    pub twin: TwinScenarioResult,
    pub coupling: Option<CouplingMatrix>,
}

fn load_snapshot_from_vault(
    vault: &VaultHandle,
    request: &EvaluateTwinRequest,
) -> Result<(TwinSnapshotInput, OracleProvenance), String> {
    let mut provenance = OracleProvenance {
        gap_run_id: None,
        tensor_run_id: None,
        pulse_run_id: None,
        rasch_run_id: None,
        twin_run_id: None,
        rag_hit_count: None,
        interview_session_id: None,
    };

    let gap = vault.gap_analysis_latest().map_err(map_vault)?;
    let (gap_suf, gap_count, gap_id) = match gap {
        Some(row) => {
            provenance.gap_run_id = Some(row.id.clone());
            let payload: Value = serde_json::from_str(&row.payload_json).unwrap_or(Value::Null);
            let count = payload
                .get("gaps")
                .and_then(|g| g.as_array())
                .map(|a| a.len())
                .unwrap_or(0);
            (Some(row.data_sufficiency), Some(count), Some(row.id))
        }
        None => (None, None, None),
    };
    let _ = gap_id;

    let tensor_row = vault.tensor_profile_latest().map_err(map_vault)?;
    let tensor = match (&request.tensor, tensor_row) {
        (Some(t), _) => Some(t.clone()),
        (None, Some(row)) => {
            provenance.tensor_run_id = Some(row.id.clone());
            serde_json::from_str(&row.payload_json).ok()
        }
        (None, None) => Some(authoritative_profile()),
    };

    let pulse = vault.pulse_run_latest().map_err(map_vault)?;
    let pulse_affinity = match (request.pulse_affinity, pulse) {
        (Some(a), _) => Some(a),
        (None, Some(row)) => {
            provenance.pulse_run_id = Some(row.id);
            row.affinity_score.and_then(|s| u8::try_from(s).ok())
        }
        (None, None) => None,
    };

    let rasch = vault.rasch_run_latest().map_err(map_vault)?;
    let rasch_posterior = match (&request.rasch_posterior, rasch) {
        (Some(p), _) => Some(p.clone()),
        (None, Some(row)) => {
            provenance.rasch_run_id = Some(row.id);
            serde_json::from_str(&row.posterior_json).ok()
        }
        (None, None) => None,
    };

    Ok((
        TwinSnapshotInput {
            tensor,
            pulse_affinity,
            rasch_posterior,
            gap_data_sufficiency: request.gap_data_sufficiency.or(gap_suf),
            gap_count: request.gap_count.or(gap_count),
            today: request.today.clone(),
            horizon_days: request.horizon_days.unwrap_or(MC_HORIZON_DEFAULT),
            scenario: request.scenario.clone().unwrap_or_default(),
        },
        provenance,
    ))
}

fn optional_coupling(request: &EvaluateTwinRequest) -> Result<Option<CouplingMatrix>, String> {
    match (
        &request.lane_values,
        &request.lane_mask,
        request.n_rows,
        request.n_lanes,
    ) {
        (Some(values), Some(mask), Some(n_rows), Some(n_lanes)) => {
            match coupling_matrix(values, mask, n_rows, n_lanes, MAX_LAG) {
                Ok(m) => Ok(Some(m)),
                Err(e) => Err(e.to_string()),
            }
        }
        _ => Ok(None),
    }
}

fn warm_identify(vault: &VaultHandle) -> TwinIdentifyStatus {
    match vault.twin_run_list_payloads(256) {
        Ok(payloads) => {
            let filter = warm_rls_from_twin_payloads(&payloads);
            resolve_identify_status(&filter)
        }
        Err(_) => TwinIdentifyStatus::generic_prior(),
    }
}

#[tauri::command]
pub async fn evaluate_digital_twin_scenario(
    vault: State<'_, VaultHandle>,
    request: EvaluateTwinRequest,
) -> Result<EvaluateTwinResult, String> {
    let vault = vault.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let identify = warm_identify(&vault);
        let (snapshot, _prov) = load_snapshot_from_vault(&vault, &request)?;
        let twin = evaluate_digital_twin_scenario_with_identify(snapshot, Some(&identify));
        let coupling = optional_coupling(&request)?;
        let id = new_id("twin");
        let payload_json =
            serde_json::to_string(&twin).map_err(|_| "twin serialize failed".to_string())?;
        vault
            .twin_run_insert(TwinRunRow {
                id: id.clone(),
                created_at: now_unix(),
                schema_version: twin.schema.clone(),
                gate_passed: i64::from(twin.params.gate_passed),
                coverage_score: twin.params.coverage_score,
                payload_json,
            })
            .map_err(map_vault)?;
        Ok(EvaluateTwinResult {
            id,
            twin,
            coupling,
        })
    })
    .await
    .map_err(|_| "evaluate_digital_twin_scenario join failed".to_string())?
}

#[tauri::command]
pub async fn get_twin_identify_status(
    vault: State<'_, VaultHandle>,
) -> Result<TwinIdentifyStatus, String> {
    let vault = vault.inner().clone();
    tauri::async_runtime::spawn_blocking(move || Ok(warm_identify(&vault)))
        .await
        .map_err(|_| "get_twin_identify_status join failed".to_string())?
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct GenerateOracleRequest {
    pub today: String,
    pub horizon_days: Option<u32>,
    pub scenario: Option<ScenarioModifiers>,
    pub tensor: Option<TensorProfile>,
    pub pulse_affinity: Option<u8>,
    pub rasch_posterior: Option<Vec<f64>>,
    pub gap_data_sufficiency: Option<f64>,
    pub gap_count: Option<usize>,
    pub lane_values: Option<Vec<f64>>,
    pub lane_mask: Option<Vec<bool>>,
    pub n_rows: Option<usize>,
    pub n_lanes: Option<usize>,
    pub rag_hit_count: Option<u32>,
    pub interview_session_id: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct GenerateOracleResult {
    pub id: String,
    pub payload: Value,
    pub provenance: OracleProvenance,
    pub languageization_prompt: String,
}

#[tauri::command]
pub async fn generate_oracle_payload(
    vault: State<'_, VaultHandle>,
    request: GenerateOracleRequest,
) -> Result<GenerateOracleResult, String> {
    let vault = vault.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let eval_req = EvaluateTwinRequest {
            today: request.today.clone(),
            horizon_days: request.horizon_days,
            scenario: request.scenario,
            tensor: request.tensor,
            pulse_affinity: request.pulse_affinity,
            rasch_posterior: request.rasch_posterior,
            gap_data_sufficiency: request.gap_data_sufficiency,
            gap_count: request.gap_count,
            lane_values: request.lane_values,
            lane_mask: request.lane_mask,
            n_rows: request.n_rows,
            n_lanes: request.n_lanes,
        };
        let (snapshot, mut provenance) = load_snapshot_from_vault(&vault, &eval_req)?;
        provenance.rag_hit_count = request.rag_hit_count;
        provenance.interview_session_id = request.interview_session_id;

        let identify = warm_identify(&vault);
        let twin = evaluate_digital_twin_scenario_with_identify(snapshot, Some(&identify));
        let coupling = optional_coupling(&eval_req)?;
        let twin_id = new_id("twin");
        let twin_json =
            serde_json::to_string(&twin).map_err(|_| "twin serialize failed".to_string())?;
        vault
            .twin_run_insert(TwinRunRow {
                id: twin_id.clone(),
                created_at: now_unix(),
                schema_version: twin.schema.clone(),
                gate_passed: i64::from(twin.params.gate_passed),
                coverage_score: twin.params.coverage_score,
                payload_json: twin_json,
            })
            .map_err(map_vault)?;
        provenance.twin_run_id = Some(twin_id);

        let days_observed = coupling.as_ref().map(|c| c.n_rows as i32).unwrap_or(0);
        let coverage = if twin.state.tensor_coverage > 0.0 {
            twin.state.tensor_coverage
        } else {
            0.0
        };
        let payload = assemble_oracle_payload(
            &request.today,
            &twin,
            coupling.as_ref(),
            days_observed,
            coverage,
        )?;
        let languageization_prompt = render_oracle_consult(&payload);
        let id = new_id("oracle");
        let provenance_json = serde_json::to_string(&provenance)
            .map_err(|_| "provenance serialize failed".to_string())?;
        let payload_json =
            serde_json::to_string(&payload).map_err(|_| "oracle serialize failed".to_string())?;
        vault
            .oracle_run_insert(OracleRunRow {
                id: id.clone(),
                created_at: now_unix(),
                schema_version: ORACLE_SCHEMA.into(),
                gate_passed: i64::from(
                    payload
                        .pointer("/sufficiency/gate_passed")
                        .and_then(|v| v.as_bool())
                        .unwrap_or(false),
                ),
                payload_json,
                provenance_json,
            })
            .map_err(map_vault)?;

        Ok(GenerateOracleResult {
            id,
            payload,
            provenance,
            languageization_prompt,
        })
    })
    .await
    .map_err(|_| "generate_oracle_payload join failed".to_string())?
}
