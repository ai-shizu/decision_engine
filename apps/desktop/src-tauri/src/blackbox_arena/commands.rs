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

/// Non-blocking ambient flavor pull. Returns `None` when idle / mismatch /
/// already taken. Correlation is resolved in Rust from the live session.
#[cfg(feature = "flavor-live")]
#[tauri::command]
pub(crate) async fn bxs_take_flavor(
    sim: State<'_, BlackboxSimHandle>,
    campaign_id: String,
) -> Result<Option<String>, SimUiErrorCode> {
    let sim = sim.inner().clone();
    run_blocking(move || sim.take_flavor(campaign_id)).await
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

/// Estimate + persist a pooled `blackbox_profile.v1` (Phase 6-A / R-8).
///
/// Signature is identical across builds. Without `blackbox-profile-write` the
/// body refuses with [`SimUiErrorCode::BlackboxProfileWriteNotReady`] — the
/// same shape as `EGRESS_LIVE_NOT_READY` (consent alone is not enough).
#[tauri::command]
pub(crate) async fn bxs_estimate_profile(
    sim: State<'_, BlackboxSimHandle>,
) -> Result<(), SimUiErrorCode> {
    let sim = sim.inner().clone();
    run_blocking(move || sim.estimate_profile()).await
}

/// PROFILE UI / R-9 outlet: latest snapshot via `get_latest_profile` only.
///
/// Absent row → soft 「未測定」 view (never Err). Flag-off builds that lack
/// the vault blackbox lane also soft-degrade the same way.
#[cfg(all(feature = "secure-vault", target_vendor = "apple"))]
#[tauri::command]
pub(crate) async fn bxs_latest_profile(
    vault: State<'_, crate::db::VaultHandle>,
) -> Result<crate::db::blackbox_profile_outlet::BlackboxProfileView, String> {
    use crate::db::blackbox_profile_outlet::{absent_profile_view, profile_view_from_loaded};
    let vault = vault.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        match vault.blackbox_latest_profile() {
            Ok(Some(loaded)) => Ok(profile_view_from_loaded(&loaded)),
            Ok(None) => Ok(absent_profile_view()),
            Err(e) => {
                eprintln!("bxs_latest_profile vault error: {e:?}");
                Ok(absent_profile_view())
            }
        }
    })
    .await
    .map_err(|_| "bxs_latest_profile join failed".to_string())?
}

/// PROFILE UI listing — `list_profiles` only (no ad-hoc SQL).
#[cfg(all(feature = "secure-vault", target_vendor = "apple"))]
#[tauri::command]
pub(crate) async fn bxs_list_profiles(
    vault: State<'_, crate::db::VaultHandle>,
) -> Result<Vec<crate::db::blackbox_profile_outlet::BlackboxProfileMetaView>, String> {
    use crate::db::blackbox_profile_outlet::meta_view;
    let vault = vault.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        match vault.blackbox_list_profiles() {
            Ok(metas) => Ok(metas.iter().map(meta_view).collect()),
            Err(e) => {
                eprintln!("bxs_list_profiles vault error: {e:?}");
                Ok(Vec::new())
            }
        }
    })
    .await
    .map_err(|_| "bxs_list_profiles join failed".to_string())?
}
