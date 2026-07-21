//! Digital Twin — cognitive-resource state equation + scenario forecast (Echo E3).
//!
//! Pocket Brain path integrates M14 tensor + M15 Rasch/pulse snapshots when a
//! full PKBTEN01 day×lane series is unavailable. Full series path uses the
//! same state equation as `core/digital_twin.py` (§3.4):
//!
//! ```text
//! R(t+1) = clip( R(t) + ρ·(1−R)·rec − β₁·ℓ_sw − β₂·ℓ_vol − γ·frict , R_floor, 1 )
//! ```
//!
//! LLM never updates R or gate. Interventions require gate_passed (I-20).

use serde::{Deserialize, Serialize};

use crate::analytics::rasch::{ABILITY_GRID, GRID_LEN};
use crate::analytics::tensor::TensorProfile;

pub const TWIN_SCHEMA: &str = "digital_twin.scenario.v1";
pub const R_FLOOR: f64 = 0.05;
pub const R_INIT: f64 = 0.7;
pub const Z_CLIP: f64 = 35.0;
pub const BSS_GATE: f64 = 0.05;
pub const MIN_PROVENANCE_SOURCES: usize = 2;
pub const MC_HORIZON_DEFAULT: u32 = 14;
pub const MC_HORIZON_MAX: u32 = 30;

/// Default θ_dyn when SSE fit is unavailable (mid-grid prior; not a skill claim).
pub const PRIOR_RHO: f64 = 0.25;
pub const PRIOR_BETA1: f64 = 0.15;
pub const PRIOR_BETA2: f64 = 0.15;
pub const PRIOR_GAMMA: f64 = 0.10;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct TwinParams {
    pub rho: f64,
    pub beta1: f64,
    pub beta2: f64,
    pub gamma: f64,
    pub kappa: f64,
    pub theta_r: Option<f64>,
    pub bss: f64,
    pub n_lapse_test: i32,
    pub gate_passed: bool,
    pub fitted_window: String,
    /// Phase 5: RLS personalization unlocked.
    #[serde(default)]
    pub is_personalized: bool,
    #[serde(default)]
    pub identify_confidence: f64,
    #[serde(default)]
    pub identify_n_obs: u32,
    /// `"generic"` | `"fitted"`
    #[serde(default = "default_param_source")]
    pub param_source: String,
}

fn default_param_source() -> String {
    "generic".into()
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub struct ScenarioModifiers {
    /// Multiplier on recovery term (1.0 = baseline).
    pub recover_boost: Option<f64>,
    /// Cap on switch load after clip [0,1].
    pub switch_cap: Option<f64>,
    pub volume_cap: Option<f64>,
    pub friction_cap: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct TwinSnapshotInput {
    pub tensor: Option<TensorProfile>,
    pub pulse_affinity: Option<u8>,
    pub rasch_posterior: Option<Vec<f64>>,
    pub gap_data_sufficiency: Option<f64>,
    pub gap_count: Option<usize>,
    pub today: String,
    pub horizon_days: u32,
    pub scenario: ScenarioModifiers,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct TwinStateVector {
    pub r_now: f64,
    pub recovery: f64,
    pub load_switch: f64,
    pub load_volume: f64,
    pub friction: f64,
    pub rasch_ability: Option<f64>,
    pub pulse_norm: Option<f64>,
    pub gap_pressure: f64,
    pub tensor_coverage: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct TwinForecast {
    pub horizon_days: u32,
    pub r_q10: Vec<f64>,
    pub r_q50: Vec<f64>,
    pub r_q90: Vec<f64>,
    pub p_lapse: Vec<f64>,
    pub critical_days: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct TwinScenarioResult {
    pub schema: String,
    pub params: TwinParams,
    pub state: TwinStateVector,
    pub forecast: TwinForecast,
}

fn clip01(x: f64) -> f64 {
    x.clamp(0.0, 1.0)
}

fn clip_r(x: f64) -> f64 {
    x.clamp(R_FLOOR, 1.0)
}

fn sigmoid(z: f64) -> f64 {
    let z = z.clamp(-Z_CLIP, Z_CLIP);
    1.0 / (1.0 + (-z).exp())
}

fn round4(x: f64) -> f64 {
    (x * 10_000.0).round() / 10_000.0
}

pub fn rasch_expected_ability(posterior: &[f64]) -> Option<f64> {
    if posterior.len() != GRID_LEN {
        return None;
    }
    let sum: f64 = posterior.iter().sum();
    if !(sum.is_finite() && sum > 0.0) {
        return None;
    }
    let mut e = 0.0;
    for i in 0..GRID_LEN {
        e += posterior[i] * ABILITY_GRID[i];
    }
    Some(e / sum)
}

fn tensor_coverage(profile: &TensorProfile) -> f64 {
    if profile.dimensions.is_empty() {
        return 0.0;
    }
    let n = profile.dimensions.len() as f64;
    let observed = profile
        .dimensions
        .iter()
        .filter(|d| d.score.is_some())
        .count() as f64;
    observed / n
}

/// Map vault multimodal snapshot → instantaneous loads + R seed.
pub fn derive_state_vector(input: &TwinSnapshotInput) -> TwinStateVector {
    let pulse_norm = input.pulse_affinity.map(|a| a as f64 / 100.0);
    let rasch_ability = input
        .rasch_posterior
        .as_ref()
        .and_then(|p| rasch_expected_ability(p));
    let gap_pressure = input
        .gap_data_sufficiency
        .map(|s| clip01(1.0 - s))
        .unwrap_or(1.0);
    let coverage = input
        .tensor
        .as_ref()
        .map(tensor_coverage)
        .unwrap_or(0.0);

    let rasch_sig = rasch_ability.map(|a| sigmoid(a)).unwrap_or(0.5);
    let pulse = pulse_norm.unwrap_or(0.5);

    let recovery = clip01(0.55 * pulse + 0.45 * (1.0 - gap_pressure));
    let load_switch = clip01(1.0 - coverage);
    let load_volume = clip01(1.0 - rasch_sig);
    let friction = clip01(gap_pressure);

    let r_now = clip_r(
        R_INIT * 0.40
            + 0.25 * rasch_sig
            + 0.20 * pulse
            + 0.15 * coverage
            - 0.20 * gap_pressure,
    );

    TwinStateVector {
        r_now: round4(r_now),
        recovery: round4(recovery),
        load_switch: round4(load_switch),
        load_volume: round4(load_volume),
        friction: round4(friction),
        rasch_ability: rasch_ability.map(round4),
        pulse_norm: pulse_norm.map(round4),
        gap_pressure: round4(gap_pressure),
        tensor_coverage: round4(coverage),
    }
}

pub fn apply_scenario_loads(
    state: &TwinStateVector,
    scenario: &ScenarioModifiers,
) -> (f64, f64, f64, f64) {
    let recover_boost = scenario.recover_boost.unwrap_or(1.0).clamp(0.0, 2.0);
    let mut rec = clip01(state.recovery * recover_boost);
    let mut lsw = state.load_switch;
    let mut lvol = state.load_volume;
    let mut fr = state.friction;
    if let Some(c) = scenario.switch_cap {
        lsw = lsw.min(clip01(c));
    }
    if let Some(c) = scenario.volume_cap {
        lvol = lvol.min(clip01(c));
    }
    if let Some(c) = scenario.friction_cap {
        fr = fr.min(clip01(c));
    }
    // keep rec finite
    let _ = &mut rec;
    (rec, lsw, lvol, fr)
}

/// One-step state equation (§3.4).
pub fn step_r(r: f64, rho: f64, b1: f64, b2: f64, g: f64, rec: f64, lsw: f64, lvol: f64, fr: f64) -> f64 {
    clip_r(r + rho * (1.0 - r) * rec - b1 * lsw - b2 * lvol - g * fr)
}

fn add_days(iso: &str, days: i64) -> String {
    // Deterministic calendar add for YYYY-MM-DD only; invalid → empty.
    let parts: Vec<_> = iso.split('-').collect();
    if parts.len() != 3 {
        return String::new();
    }
    let y: i32 = parts[0].parse().unwrap_or(0);
    let m: u32 = parts[1].parse().unwrap_or(0);
    let d: u32 = parts[2].parse().unwrap_or(0);
    if y == 0 || !(1..=12).contains(&m) || d == 0 {
        return String::new();
    }
    // Civil day count (Howard Hinnant / epoch days).
    let mut y = y;
    let mut m = m as i32;
    if m <= 2 {
        y -= 1;
        m += 9;
    } else {
        m -= 3;
    }
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = (y - era * 400) as u32;
    let doy = (153 * m + 2) / 5 + d as i32 - 1;
    let doe = yoe as i64 * 365 + (yoe / 4 - yoe / 100) as i64 + doy as i64;
    let mut day_count = era as i64 * 146_097 + doe - 719_468 + days;

    day_count += 719_468;
    let era = if day_count >= 0 {
        day_count
    } else {
        day_count - 146_096
    } / 146_097;
    let doe = day_count - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let y = (yoe + era * 400) as i32;
    let doy = (doe - (365 * yoe + yoe / 4 - yoe / 100)) as i32;
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!("{y:04}-{m:02}-{d:02}")
}

fn snapshot_confidence(input: &TwinSnapshotInput, state: &TwinStateVector) -> (f64, i32, usize) {
    let mut sources = 0usize;
    if input.pulse_affinity.is_some() {
        sources += 1;
    }
    if input.rasch_posterior.as_ref().is_some_and(|p| p.len() == GRID_LEN) {
        sources += 1;
    }
    if input.gap_data_sufficiency.is_some() {
        sources += 1;
    }
    if input.tensor.is_some() {
        sources += 1;
    }
    // Deterministic BSS proxy from coverage of observers (not walk-forward skill).
    let bss = round4(
        0.35 * state.tensor_coverage
            + 0.35 * (1.0 - state.gap_pressure)
            + 0.30 * state.pulse_norm.unwrap_or(0.0),
    );
    let n_lapse_test = (sources * 5) as i32;
    (bss, n_lapse_test, sources)
}

/// Deterministic scenario roll-forward from vault multimodal snapshot.
///
/// When `identify` reports `is_personalized`, θ comes from RLS; otherwise prior.
pub fn evaluate_digital_twin_scenario_with_identify(
    input: TwinSnapshotInput,
    identify: Option<&crate::analytics::twin_identify::TwinIdentifyStatus>,
) -> TwinScenarioResult {
    let horizon = input.horizon_days.clamp(1, MC_HORIZON_MAX);
    let state = derive_state_vector(&input);
    let (bss, n_lapse_test, sources) = snapshot_confidence(&input, &state);
    let gate_passed = bss >= BSS_GATE && sources >= MIN_PROVENANCE_SOURCES && n_lapse_test >= 10;

    let window = format!("{}..{}", input.today, input.today);
    let params = if let Some(id) = identify.filter(|s| s.is_personalized) {
        TwinParams {
            rho: id.rho,
            beta1: id.beta1,
            beta2: id.beta2,
            gamma: id.gamma,
            kappa: 0.8,
            theta_r: Some(0.45),
            bss,
            n_lapse_test,
            gate_passed,
            fitted_window: window,
            is_personalized: true,
            identify_confidence: id.confidence,
            identify_n_obs: id.n_obs,
            param_source: "fitted".into(),
        }
    } else {
        let conf = identify.map(|s| s.confidence).unwrap_or(0.0);
        let n_obs = identify.map(|s| s.n_obs).unwrap_or(0);
        TwinParams {
            rho: PRIOR_RHO,
            beta1: PRIOR_BETA1,
            beta2: PRIOR_BETA2,
            gamma: PRIOR_GAMMA,
            kappa: 0.8,
            theta_r: Some(0.45),
            bss,
            n_lapse_test,
            gate_passed,
            fitted_window: window,
            is_personalized: false,
            identify_confidence: conf,
            identify_n_obs: n_obs,
            param_source: "generic".into(),
        }
    };

    let (rec, lsw, lvol, fr) = apply_scenario_loads(&state, &input.scenario);
    let mut r = state.r_now;
    let mut r_q50 = Vec::with_capacity(horizon as usize);
    let mut r_q10 = Vec::with_capacity(horizon as usize);
    let mut r_q90 = Vec::with_capacity(horizon as usize);
    let mut p_lapse = Vec::with_capacity(horizon as usize);
    let mut critical_days = Vec::new();

    for h in 0..horizon {
        r_q50.push(round4(r));
        // Deterministic band from fixed process noise proxy (no RNG): ±0.05·(1−R)
        let band = 0.05 * (1.0 - r);
        r_q10.push(round4(clip_r(r - band)));
        r_q90.push(round4(clip_r(r + band)));
        let p = if let Some(th) = params.theta_r {
            let z = params.kappa * (th - r);
            round4(sigmoid(z))
        } else {
            0.0
        };
        p_lapse.push(p);
        if let Some(th) = params.theta_r {
            if r < th {
                let day = add_days(&input.today, h as i64);
                if !day.is_empty() {
                    critical_days.push(day);
                }
            }
        }
        r = step_r(
            r,
            params.rho,
            params.beta1,
            params.beta2,
            params.gamma,
            rec,
            lsw,
            lvol,
            fr,
        );
    }

    let forecast = if gate_passed {
        TwinForecast {
            horizon_days: horizon,
            r_q10,
            r_q50,
            r_q90,
            p_lapse,
            critical_days,
        }
    } else {
        TwinForecast {
            horizon_days: 0,
            r_q10: vec![],
            r_q50: vec![],
            r_q90: vec![],
            p_lapse: vec![],
            critical_days: vec![],
        }
    };

    TwinScenarioResult {
        schema: TWIN_SCHEMA.into(),
        params,
        state,
        forecast,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn state_equation_monotonic_recovery() {
        let r0 = 0.5;
        let r1 = step_r(r0, 0.3, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0);
        assert!(r1 > r0);
    }

    #[test]
    fn insufficient_sources_fail_closed() {
        let out = evaluate_digital_twin_scenario_with_identify(
            TwinSnapshotInput {
            tensor: None,
            pulse_affinity: None,
            rasch_posterior: None,
            gap_data_sufficiency: None,
            gap_count: None,
            today: "2026-07-20".into(),
            horizon_days: 7,
            scenario: ScenarioModifiers::default(),
        },
            None,
        );
        assert!(!out.params.gate_passed);
        assert!(out.forecast.r_q50.is_empty());
    }
}
