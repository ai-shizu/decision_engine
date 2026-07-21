//! Recursive Least Squares (RLS) online identification for Digital Twin θ.
//!
//! Linearized state increment (Phase 5 / Opus proposal 1):
//!
//! ```text
//! ΔR = ρ·(1−R)·rec − β₁·ℓ_sw − β₂·ℓ_vol − γ·frict
//! x  = [(1−R)·rec, −ℓ_sw, −ℓ_vol, −frict]ᵀ
//! θ  = [ρ, β₁, β₂, γ]ᵀ
//! ```
//!
//! Deterministic only — no RNG. 4×4 arithmetic is hand-rolled (no nalgebra).

use serde::Serialize;

use crate::analytics::digital_twin::{
    TwinScenarioResult, BSS_GATE, PRIOR_BETA1, PRIOR_BETA2, PRIOR_GAMMA, PRIOR_RHO,
};

pub const THETA_DIM: usize = 4;
/// Forgetting factor — tracks slow non-stationary user change.
pub const LAMBDA: f64 = 0.98;
/// Large diagonal prior covariance (uninformative start → prior θ).
pub const P0_SCALE: f64 = 100.0;
/// Minimum consecutive twin-run pairs before personalization may unlock.
pub const MIN_IDENTIFY_OBS: u32 = 10;

pub type Vec4 = [f64; THETA_DIM];
pub type Mat4 = [[f64; THETA_DIM]; THETA_DIM];

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct TwinIdentifyStatus {
    pub is_personalized: bool,
    pub confidence: f64,
    pub n_obs: u32,
    pub rho: f64,
    pub beta1: f64,
    pub beta2: f64,
    pub gamma: f64,
    /// `"generic"` | `"fitted"`
    pub source: String,
}

impl TwinIdentifyStatus {
    pub fn generic_prior() -> Self {
        Self {
            is_personalized: false,
            confidence: 0.0,
            n_obs: 0,
            rho: PRIOR_RHO,
            beta1: PRIOR_BETA1,
            beta2: PRIOR_BETA2,
            gamma: PRIOR_GAMMA,
            source: "generic".into(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct RlsFilter {
    theta: Vec4,
    p: Mat4,
    n_obs: u32,
    sum_y: f64,
    sum_y2: f64,
    sum_e2: f64,
}

impl RlsFilter {
    pub fn with_prior() -> Self {
        Self {
            theta: [PRIOR_RHO, PRIOR_BETA1, PRIOR_BETA2, PRIOR_GAMMA],
            p: scale_eye(P0_SCALE),
            n_obs: 0,
            sum_y: 0.0,
            sum_y2: 0.0,
            sum_e2: 0.0,
        }
    }

    #[cfg(test)]
    pub fn theta(&self) -> &Vec4 {
        &self.theta
    }

    #[cfg(test)]
    pub fn n_obs(&self) -> u32 {
        self.n_obs
    }

    /// One RLS step with forgetting λ. Returns absolute residual |e|.
    pub fn update(&mut self, x: &Vec4, y: f64) -> f64 {
        // K = P x / (λ + xᵀ P x)
        let px = mat_vec(&self.p, x);
        let denom = LAMBDA + dot(&px, x);
        if !denom.is_finite() || denom.abs() < 1e-18 {
            return 0.0;
        }
        let mut k = [0.0; THETA_DIM];
        for i in 0..THETA_DIM {
            k[i] = px[i] / denom;
        }

        let y_hat = dot(&self.theta, x);
        let e = y - y_hat;

        for i in 0..THETA_DIM {
            self.theta[i] += k[i] * e;
        }
        clamp_theta(&mut self.theta);

        // P ← (1/λ) (P − K (xᵀ P))
        // xᵀ P = (Pᵀ x)ᵀ; P is kept symmetric.
        let xt_p = mat_vec_transpose_approx(&self.p, x);
        let kxtp = outer(&k, &xt_p);
        let mut p_new = [[0.0; THETA_DIM]; THETA_DIM];
        for i in 0..THETA_DIM {
            for j in 0..THETA_DIM {
                p_new[i][j] = (self.p[i][j] - kxtp[i][j]) / LAMBDA;
            }
        }
        // Symmetrize to curb numerical drift.
        for i in 0..THETA_DIM {
            for j in i..THETA_DIM {
                let v = 0.5 * (p_new[i][j] + p_new[j][i]);
                p_new[i][j] = v;
                p_new[j][i] = v;
            }
        }
        self.p = p_new;

        self.n_obs = self.n_obs.saturating_add(1);
        self.sum_y += y;
        self.sum_y2 += y * y;
        self.sum_e2 += e * e;
        e.abs()
    }

    /// Coefficient of determination on accumulated residuals (BSS-comparable).
    pub fn confidence(&self) -> f64 {
        if self.n_obs < 2 {
            return 0.0;
        }
        let n = self.n_obs as f64;
        let mean_y = self.sum_y / n;
        let tss = (self.sum_y2 - n * mean_y * mean_y).max(0.0);
        if tss < 1e-12 {
            return 0.0;
        }
        (1.0 - self.sum_e2 / tss).clamp(0.0, 1.0)
    }

    pub fn status(&self) -> TwinIdentifyStatus {
        let confidence = round4(self.confidence());
        let is_personalized = confidence >= BSS_GATE && self.n_obs >= MIN_IDENTIFY_OBS;
        TwinIdentifyStatus {
            is_personalized,
            confidence,
            n_obs: self.n_obs,
            rho: round4(self.theta[0]),
            beta1: round4(self.theta[1]),
            beta2: round4(self.theta[2]),
            gamma: round4(self.theta[3]),
            source: if is_personalized {
                "fitted".into()
            } else {
                "generic".into()
            },
        }
    }
}

fn clamp_theta(theta: &mut Vec4) {
    theta[0] = theta[0].clamp(0.0, 1.0);
    theta[1] = theta[1].clamp(0.0, 1.0);
    theta[2] = theta[2].clamp(0.0, 1.0);
    theta[3] = theta[3].clamp(0.0, 1.0);
}

fn round4(x: f64) -> f64 {
    (x * 10_000.0).round() / 10_000.0
}

fn scale_eye(s: f64) -> Mat4 {
    let mut m = [[0.0; THETA_DIM]; THETA_DIM];
    for i in 0..THETA_DIM {
        m[i][i] = s;
    }
    m
}

fn dot(a: &Vec4, b: &Vec4) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2] + a[3] * b[3]
}

fn mat_vec(m: &Mat4, x: &Vec4) -> Vec4 {
    let mut out = [0.0; THETA_DIM];
    for i in 0..THETA_DIM {
        out[i] = m[i][0] * x[0] + m[i][1] * x[1] + m[i][2] * x[2] + m[i][3] * x[3];
    }
    out
}

/// For symmetric P, Pᵀx = Px; kept as a named helper for the RLS formula.
fn mat_vec_transpose_approx(m: &Mat4, x: &Vec4) -> Vec4 {
    mat_vec(m, x)
}

fn outer(a: &Vec4, b: &Vec4) -> Mat4 {
    let mut m = [[0.0; THETA_DIM]; THETA_DIM];
    for i in 0..THETA_DIM {
        for j in 0..THETA_DIM {
            m[i][j] = a[i] * b[j];
        }
    }
    m
}

/// Feature vector x and target ΔR from consecutive twin snapshots.
pub fn observation_from_states(
    r: f64,
    recovery: f64,
    load_switch: f64,
    load_volume: f64,
    friction: f64,
    r_next: f64,
) -> (Vec4, f64) {
    let x = [
        (1.0 - r) * recovery,
        -load_switch,
        -load_volume,
        -friction,
    ];
    let y = r_next - r;
    (x, y)
}

/// Warm RLS from chronological twin scenario payloads (oldest → newest).
pub fn warm_rls_from_twin_payloads(payloads_oldest_first: &[String]) -> RlsFilter {
    let mut filter = RlsFilter::with_prior();
    let mut prev: Option<TwinScenarioResult> = None;
    for raw in payloads_oldest_first {
        let Ok(cur) = serde_json::from_str::<TwinScenarioResult>(raw) else {
            continue;
        };
        if let Some(p) = prev.as_ref() {
            let (x, y) = observation_from_states(
                p.state.r_now,
                p.state.recovery,
                p.state.load_switch,
                p.state.load_volume,
                p.state.friction,
                cur.state.r_now,
            );
            if x.iter().all(|v| v.is_finite()) && y.is_finite() {
                filter.update(&x, y);
            }
        }
        prev = Some(cur);
    }
    filter
}

/// Resolve effective θ: fitted when gate opens, else generic prior.
pub fn resolve_identify_status(filter: &RlsFilter) -> TwinIdentifyStatus {
    let mut status = filter.status();
    if !status.is_personalized {
        // Fail closed to prior magnitudes while still reporting n_obs/confidence.
        status.rho = PRIOR_RHO;
        status.beta1 = PRIOR_BETA1;
        status.beta2 = PRIOR_BETA2;
        status.gamma = PRIOR_GAMMA;
        status.source = "generic".into();
    }
    status
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rls_recovers_known_theta_no_noise() {
        let theta_star = [0.3, 0.1, 0.2, 0.05];
        let mut filter = RlsFilter::with_prior();
        let mut r = 0.55;
        // Persistently excite all four regressors (fixed loads → rank-deficient).
        for t in 0..160u32 {
            let rec = 0.35 + 0.55 * ((t % 7) as f64) / 6.0;
            let lsw = 0.05 + 0.70 * ((t % 5) as f64) / 4.0;
            let lvol = 0.10 + 0.60 * ((t % 11) as f64) / 10.0;
            let fr = 0.02 + 0.50 * ((t % 9) as f64) / 8.0;
            let x = [(1.0 - r) * rec, -lsw, -lvol, -fr];
            let y = dot(&theta_star, &x);
            filter.update(&x, y);
            r = (r + y).clamp(0.05, 1.0);
        }
        let t = filter.theta();
        assert!((t[0] - 0.3).abs() < 0.08, "rho {:?}", t);
        assert!((t[1] - 0.1).abs() < 0.08, "beta1 {:?}", t);
        assert!((t[2] - 0.2).abs() < 0.08, "beta2 {:?}", t);
        assert!((t[3] - 0.05).abs() < 0.08, "gamma {:?}", t);
        assert!(filter.n_obs() >= MIN_IDENTIFY_OBS);
        assert!(filter.confidence() >= BSS_GATE);
    }

    #[test]
    fn generic_until_enough_obs() {
        let filter = RlsFilter::with_prior();
        let status = resolve_identify_status(&filter);
        assert!(!status.is_personalized);
        assert_eq!(status.source, "generic");
        assert_eq!(status.rho, PRIOR_RHO);
    }

    #[test]
    fn observation_signs_match_linearization() {
        let (x, y) = observation_from_states(0.5, 1.0, 0.0, 0.0, 0.0, 0.7);
        assert!((x[0] - 0.5).abs() < 1e-12);
        assert_eq!(x[1], 0.0);
        assert!((y - 0.2).abs() < 1e-12);
    }
}
