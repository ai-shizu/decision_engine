//! Orthogonal coupling matrix — FFT cross-correlation + far-lag null (Echo E2).
//!
//! Ports `core/coupling.py` contracts (SPEC §3.3 / W-7‥W-14). Pure Rust,
//! no RNG. Missing values use mask only (I-18).

use serde::{Deserialize, Serialize};

pub const MAX_LAG: i32 = 14;
pub const RHO_MIN: f64 = 0.15;
pub const N_MIN: f64 = 100.0;
pub const NULL_LAG_LO: i32 = 45;
pub const NULL_LAG_HI: i32 = 365;
pub const MIN_NULL_SAMPLES: usize = 50;
pub const MAX_PAIRS_REPORTED: usize = 64;

pub const FEATURE_LANES: &[&str] = &[
    "diary_chars",
    "abstract_idx",
    "guilt_idx",
    "productivity_idx",
    "consult_count",
    "spend_total",
    "spend_hedonic",
    "spend_invest",
    "cal_event_count",
    "cal_private_hours",
    "cal_switch_count",
    "line_out_msgs",
    "line_in_msgs",
    "line_out_chars",
    "line_reply_med_min",
    "line_initiations",
    "line_night_out",
    "friction_events",
    "task_declared",
    "task_executed",
    "github_commits",
    "leetcode_solved",
];

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct CouplingPair {
    pub src: usize,
    pub dst: usize,
    pub lag: i32,
    pub rho: f64,
    pub n_eff: i32,
    pub null_q99: Option<f64>,
    pub sig: bool,
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct CouplingMatrix {
    pub pairs: Vec<CouplingPair>,
    pub n_rows: usize,
    pub max_lag: i32,
    pub n_lanes: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CouplingError {
    ShapeMismatch,
    SeriesTooShort { n_rows: usize, minimum: usize },
}

impl std::fmt::Display for CouplingError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ShapeMismatch => write!(f, "values/mask shape mismatch"),
            Self::SeriesTooShort { n_rows, minimum } => {
                write!(f, "series too short: n_rows={n_rows} need>={minimum}")
            }
        }
    }
}

#[derive(Clone, Copy)]
struct C64 {
    re: f64,
    im: f64,
}

impl C64 {
    fn new(re: f64, im: f64) -> Self {
        Self { re, im }
    }
    fn zero() -> Self {
        Self { re: 0.0, im: 0.0 }
    }
    fn mul(self, o: Self) -> Self {
        Self {
            re: self.re * o.re - self.im * o.im,
            im: self.re * o.im + self.im * o.re,
        }
    }
    fn add(self, o: Self) -> Self {
        Self {
            re: self.re + o.re,
            im: self.im + o.im,
        }
    }
    fn sub(self, o: Self) -> Self {
        Self {
            re: self.re - o.re,
            im: self.im - o.im,
        }
    }
    fn scale(self, s: f64) -> Self {
        Self {
            re: self.re * s,
            im: self.im * s,
        }
    }
    fn conj(self) -> Self {
        Self {
            re: self.re,
            im: -self.im,
        }
    }
}

fn next_pow2(n: usize) -> usize {
    let mut m = 1usize;
    while m < n {
        m <<= 1;
    }
    m
}

/// In-place radix-2 Cooley–Tukey FFT. `invert=true` → IFFT (unnormalized).
fn fft_inplace(a: &mut [C64], invert: bool) {
    let n = a.len();
    debug_assert!(n.is_power_of_two());
    let mut j = 0usize;
    for i in 1..n {
        let mut bit = n >> 1;
        while j & bit != 0 {
            j ^= bit;
            bit >>= 1;
        }
        j ^= bit;
        if i < j {
            a.swap(i, j);
        }
    }
    let mut len = 2;
    while len <= n {
        let ang = std::f64::consts::TAU / (len as f64) * if invert { 1.0 } else { -1.0 };
        let wlen = C64::new(ang.cos(), ang.sin());
        let mut i = 0;
        while i < n {
            let mut w = C64::new(1.0, 0.0);
            for j in 0..(len / 2) {
                let u = a[i + j];
                let v = a[i + j + len / 2].mul(w);
                a[i + j] = u.add(v);
                a[i + j + len / 2] = u.sub(v);
                w = w.mul(wlen);
            }
            i += len;
        }
        len <<= 1;
    }
    if invert {
        let inv = 1.0 / n as f64;
        for x in a.iter_mut() {
            *x = x.scale(inv);
        }
    }
}

fn rfft_pad(signal: &[f64], m: usize) -> Vec<C64> {
    let mut buf = vec![C64::zero(); m];
    for (i, &v) in signal.iter().enumerate() {
        buf[i] = C64::new(v, 0.0);
    }
    fft_inplace(&mut buf, false);
    buf
}

fn circular_xcorr(a_fft: &[C64], b_fft: &[C64], m: usize) -> Vec<f64> {
    let mut prod = vec![C64::zero(); m];
    for i in 0..m {
        prod[i] = a_fft[i].conj().mul(b_fft[i]);
    }
    fft_inplace(&mut prod, true);
    prod.into_iter().map(|c| c.re).collect()
}

/// Average-rank Spearman transform → [-1,1]. Missing → 0 (I-18). No jitter (W-14).
pub fn rank_transform(x: &[f64], mask: &[bool]) -> Vec<f64> {
    let n = x.len();
    let mut out = vec![0.0; n];
    let mut valid_idx: Vec<usize> = (0..n).filter(|&i| mask[i]).collect();
    let nv = valid_idx.len();
    if nv < 2 {
        return out;
    }
    valid_idx.sort_by(|&i, &j| {
        x[i].partial_cmp(&x[j])
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| i.cmp(&j))
    });
    let mut ranks = vec![0.0_f64; nv];
    for (r, &idx) in valid_idx.iter().enumerate() {
        // placeholder; rewrite with average ties
        let _ = idx;
        ranks[r] = r as f64;
    }
    // Stable argsort ranks with average ties on equal values
    let mut i = 0;
    while i < nv {
        let mut j = i;
        while j + 1 < nv && x[valid_idx[j + 1]] == x[valid_idx[i]] {
            j += 1;
        }
        let avg = (i + j) as f64 / 2.0;
        for k in i..=j {
            ranks[k] = avg;
        }
        i = j + 1;
    }
    let denom = (nv - 1) as f64;
    for (k, &idx) in valid_idx.iter().enumerate() {
        out[idx] = (ranks[k] / denom) * 2.0 - 1.0;
    }
    out
}

fn rho_at(
    s_xy: &[f64],
    s_x: &[f64],
    s_y: &[f64],
    s_xx: &[f64],
    s_yy: &[f64],
    n_arr: &[f64],
    tau: i32,
    m: usize,
) -> Option<(f64, i32)> {
    let idx = if tau >= 0 {
        tau as usize
    } else {
        (m as i32 + tau) as usize
    };
    let n_round = n_arr[idx].round();
    if n_round < N_MIN {
        return None;
    }
    let var_x = (n_round * s_xx[idx] - s_x[idx] * s_x[idx]).max(0.0);
    let var_y = (n_round * s_yy[idx] - s_y[idx] * s_y[idx]).max(0.0);
    if var_x <= 1e-9 || var_y <= 1e-9 {
        return None;
    }
    let num = n_round * s_xy[idx] - s_x[idx] * s_y[idx];
    let rho = (num / (var_x * var_y).sqrt()).clamp(-1.0, 1.0);
    Some((rho, n_round as i32))
}

fn quantile_abs(sorted_abs: &[f64], q: f64) -> f64 {
    if sorted_abs.is_empty() {
        return 0.0;
    }
    let pos = q * (sorted_abs.len() as f64 - 1.0);
    let lo = pos.floor() as usize;
    let hi = pos.ceil() as usize;
    if lo == hi {
        sorted_abs[lo]
    } else {
        let w = pos - lo as f64;
        sorted_abs[lo] * (1.0 - w) + sorted_abs[hi] * w
    }
}

/// `values` / `mask`: row-major (n_rows * n_lanes).
pub fn coupling_matrix(
    values: &[f64],
    mask: &[bool],
    n_rows: usize,
    n_lanes: usize,
    max_lag: i32,
) -> Result<CouplingMatrix, CouplingError> {
    if values.len() != n_rows * n_lanes || mask.len() != values.len() {
        return Err(CouplingError::ShapeMismatch);
    }
    let minimum = (2 * NULL_LAG_HI + 1) as usize;
    if n_rows <= 2 * NULL_LAG_HI as usize {
        return Err(CouplingError::SeriesTooShort {
            n_rows,
            minimum,
        });
    }
    let m = next_pow2(2 * n_rows); // W-7
    let mut y_fft = Vec::with_capacity(n_lanes);
    let mut mh_fft = Vec::with_capacity(n_lanes);
    let mut q_fft = Vec::with_capacity(n_lanes);
    for k in 0..n_lanes {
        let mut col = Vec::with_capacity(n_rows);
        let mut col_mask = Vec::with_capacity(n_rows);
        for t in 0..n_rows {
            col.push(values[t * n_lanes + k]);
            col_mask.push(mask[t * n_lanes + k]);
        }
        let y = rank_transform(&col, &col_mask);
        let mm: Vec<f64> = col_mask.iter().map(|b| if *b { 1.0 } else { 0.0 }).collect();
        let qq: Vec<f64> = y.iter().map(|a| a * a).collect();
        y_fft.push(rfft_pad(&y, m));
        mh_fft.push(rfft_pad(&mm, m));
        q_fft.push(rfft_pad(&qq, m));
    }

    let mut pairs = Vec::new();
    for i in 0..n_lanes {
        for j in (i + 1)..n_lanes {
            let s_xy = circular_xcorr(&y_fft[i], &y_fft[j], m);
            let s_x = circular_xcorr(&y_fft[i], &mh_fft[j], m);
            let s_y = circular_xcorr(&mh_fft[i], &y_fft[j], m);
            let s_xx = circular_xcorr(&q_fft[i], &mh_fft[j], m);
            let s_yy = circular_xcorr(&mh_fft[i], &q_fft[j], m);
            let n_arr = circular_xcorr(&mh_fft[i], &mh_fft[j], m);

            let mut null_abs = Vec::new();
            for tau in -NULL_LAG_HI..=-NULL_LAG_LO {
                if let Some((rho, _)) = rho_at(&s_xy, &s_x, &s_y, &s_xx, &s_yy, &n_arr, tau, m) {
                    null_abs.push(rho.abs());
                }
            }
            for tau in NULL_LAG_LO..=NULL_LAG_HI {
                if let Some((rho, _)) = rho_at(&s_xy, &s_x, &s_y, &s_xx, &s_yy, &n_arr, tau, m) {
                    null_abs.push(rho.abs());
                }
            }
            let (null_q99, null_ok) = if null_abs.len() < MIN_NULL_SAMPLES {
                (None, false)
            } else {
                null_abs.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
                (Some(quantile_abs(&null_abs, 0.99)), true)
            };

            let mut best_full: Option<(f64, i32, i32)> = None;
            for tau in -max_lag..=max_lag {
                if let Some((rho, n_eff)) = rho_at(&s_xy, &s_x, &s_y, &s_xx, &s_yy, &n_arr, tau, m)
                {
                    let better = match best_full {
                        None => true,
                        Some((best_rho, best_lag, _)) => {
                            let a = rho.abs();
                            let b = best_rho.abs();
                            a > b
                                || (a == b && tau.abs() < best_lag.abs())
                                || (a == b && tau.abs() == best_lag.abs() && tau < best_lag)
                        }
                    };
                    if better {
                        best_full = Some((rho, tau, n_eff));
                    }
                }
            }

            if let Some((rho, lag, n_eff)) = best_full {
                let (sig, reason) = if !null_ok {
                    (false, Some("null_insufficient".into()))
                } else {
                    let q = null_q99.unwrap_or(0.0);
                    let sig = rho.abs() > q && rho.abs() >= RHO_MIN;
                    (sig, None)
                };
                pairs.push(CouplingPair {
                    src: i,
                    dst: j,
                    lag,
                    rho: (rho * 1_000_000.0).round() / 1_000_000.0,
                    n_eff,
                    null_q99,
                    sig,
                    reason,
                });
            }
        }
    }

    pairs.sort_by(|a, b| {
        b.rho
            .abs()
            .partial_cmp(&a.rho.abs())
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.src.cmp(&b.src))
            .then_with(|| a.dst.cmp(&b.dst))
    });
    pairs.truncate(MAX_PAIRS_REPORTED);

    Ok(CouplingMatrix {
        pairs,
        n_rows,
        max_lag,
        n_lanes,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rank_transform_ties_average() {
        let x = [1.0, 2.0, 2.0, 4.0];
        let m = [true, true, true, true];
        let r = rank_transform(&x, &m);
        assert!((r[1] - r[2]).abs() < 1e-12);
    }
}
