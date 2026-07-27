//! Tauri command surface for the BLACKBOX SIMULATOR (SPEC §15, Phase 5).
//!
//! Every command is `async fn` + `State<'_, BlackboxSimHandle>` +
//! `spawn_blocking`, the same shape as `analytics::commands::calculate_gap_analysis`.
//! `BlackboxSimHandle` carries its own vault capability internally (attached
//! once at app setup via `attach_vault`), so no command here needs a separate
//! `State<'_, VaultHandle>` parameter — that would require a different
//! signature per `secure-vault` build, which the attach-once design avoids.

use tauri::State;

use crate::blackbox_sim::genesis::GenesisRequest;
use crate::blackbox_sim::telemetry::ActionIntent;

use super::handle::BlackboxSimHandle;
use super::view::{
    AdvanceView, DecisionOutcomeView, ObservationView, SimUiErrorCode, StartCampaignRequest,
};

async fn run_blocking<T: Send + 'static>(
    task: impl FnOnce() -> Result<T, SimUiErrorCode> + Send + 'static,
) -> Result<T, SimUiErrorCode> {
    match tauri::async_runtime::spawn_blocking(task).await {
        Ok(result) => result,
        Err(_) => {
            eprintln!("blackbox_arena: command task join failed");
            Err(SimUiErrorCode::InternalFault)
        }
    }
}

#[tauri::command]
pub(crate) async fn bxs_start_campaign(
    sim: State<'_, BlackboxSimHandle>,
    request: StartCampaignRequest,
) -> Result<ObservationView, SimUiErrorCode> {
    let sim = sim.inner().clone();
    let genesis_request = GenesisRequest {
        scenario_id: request.scenario_id,
        difficulty: request.difficulty.into(),
        campaign_index: request.campaign_index,
        created_date: request.created_date,
    };
    run_blocking(move || sim.start_campaign(genesis_request)).await
}

#[tauri::command]
pub(crate) async fn bxs_get_view(
    sim: State<'_, BlackboxSimHandle>,
    campaign_id: String,
) -> Result<ObservationView, SimUiErrorCode> {
    let sim = sim.inner().clone();
    run_blocking(move || sim.get_view(campaign_id)).await
}

#[tauri::command]
pub(crate) async fn bxs_submit_decision(
    sim: State<'_, BlackboxSimHandle>,
    campaign_id: String,
    intent: ActionIntent,
    latency_ms: Option<u32>,
) -> Result<DecisionOutcomeView, SimUiErrorCode> {
    let sim = sim.inner().clone();
    run_blocking(move || sim.submit_decision(campaign_id, intent, latency_ms)).await
}

#[tauri::command]
pub(crate) async fn bxs_advance(
    sim: State<'_, BlackboxSimHandle>,
    campaign_id: String,
) -> Result<AdvanceView, SimUiErrorCode> {
    let sim = sim.inner().clone();
    run_blocking(move || sim.advance(campaign_id)).await
}

#[tauri::command]
pub(crate) async fn bxs_abort(
    sim: State<'_, BlackboxSimHandle>,
    campaign_id: String,
) -> Result<(), SimUiErrorCode> {
    let sim = sim.inner().clone();
    run_blocking(move || sim.abort(campaign_id)).await
}

#[tauri::command]
pub(crate) async fn bxs_load_generation(
    sim: State<'_, BlackboxSimHandle>,
    campaign_id: String,
    generation_index: u32,
) -> Result<ObservationView, SimUiErrorCode> {
    let sim = sim.inner().clone();
    run_blocking(move || sim.load_generation(campaign_id, generation_index)).await
}
