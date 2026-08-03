//! Phase 13 — within-person spend × cognition event study + Benjamini–Hochberg FDR.
//!
//! # Academic grounding
//!
//! Behavioral economics / CBT: cognitive resource depletion and distortions
//! co-occur with impulse spending. We estimate **within-person contrasts** after
//! residualizing calendar confounders (weekday, month-position bin), then control
//! multiplicity with **Benjamini–Hochberg FDR** (1995). Fully deterministic (F-14):
//! no RNG, no egress, no external stats crates.

use std::collections::{BTreeMap, BTreeSet};

use serde::Serialize;

use crate::analytics::bias_profile::BURNS_CATEGORIES;
use crate::db::PurchaseRow;

/// Minimum distinct JST calendar days before any hypothesis is tested.
pub const MIN_SUPPORT_DAYS: usize = 14;
/// Minimum treated / control observations per hypothesis.
pub const MIN_GROUP_N: usize = 5;
/// FDR level q (BH).
pub const FDR_Q: f64 = 0.10;
/// Twin R(t) low-resource threshold (aligned with calendar “低”).
pub const LOW_R_THRESHOLD: f64 = 0.34;
/// Cap rows scanned for analysis (vault may hold more).
pub const ANALYSIS_ROW_CAP: usize = 2_000;

const SCHEMA: &str = "spend_cognition_fdr.v1";

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct SpendCognitionRelation {
    pub id: String,
    pub treatment: String,
    pub outcome: String,
    pub contrast: f64,
    pub n_treat: usize,
    pub n_control: usize,
    pub p_raw: f64,
    pub rejected_bh: bool,
    pub bh_rank: usize,
    pub bh_critical: f64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct SpendCognitionReport {
    pub schema: String,
    pub observation_count: usize,
    pub distinct_days: usize,
    pub support_ok: bool,
    pub hypothesis_count: usize,
    pub tested_count: usize,
    pub fdr_q: f64,
    pub relations: Vec<SpendCognitionRelation>,
}

#[derive(Clone)]
struct Obs {
    day_key: i64,
    amount: f64,
    late_night: f64,
    impulse: f64,
    low_r: bool,
    distortions: BTreeSet<String>,
    stratum: u16, // dow * 3 + mop_bin
}

/// Run the full pipeline on purchase fossils (newest-first or any order).
pub fn analyze_spend_cognition(rows: &[PurchaseRow]) -> SpendCognitionReport {
    let mut obs: Vec<Obs> = rows
        .iter()
        .take(ANALYSIS_ROW_CAP)
        .filter_map(purchase_to_obs)
        .collect();
    // Stable order for residual means (by day then amount).
    obs.sort_by(|a, b| {
        a.day_key
            .cmp(&b.day_key)
            .then(a.amount.partial_cmp(&b.amount).unwrap_or(std::cmp::Ordering::Equal))
    });

    let mut days: BTreeSet<i64> = BTreeSet::new();
    for o in &obs {
        days.insert(o.day_key);
    }
    let distinct_days = days.len();
    let support_ok = distinct_days >= MIN_SUPPORT_DAYS && obs.len() >= MIN_GROUP_N * 2;

    if !support_ok || obs.is_empty() {
        return SpendCognitionReport {
            schema: SCHEMA.into(),
            observation_count: obs.len(),
            distinct_days,
            support_ok: false,
            hypothesis_count: 0,
            tested_count: 0,
            fdr_q: FDR_Q,
            relations: Vec::new(),
        };
    }

    let amount_y: Vec<f64> = obs.iter().map(|o| (o.amount + 1.0).ln()).collect();
    let late_y: Vec<f64> = obs.iter().map(|o| o.late_night).collect();
    let impulse_y: Vec<f64> = obs.iter().map(|o| o.impulse).collect();
    let strata: Vec<u16> = obs.iter().map(|o| o.stratum).collect();

    let amount_res = residualize(&amount_y, &strata);
    let late_res = residualize(&late_y, &strata);
    let impulse_res = residualize(&impulse_y, &strata);

    let mut treatments: Vec<(String, Vec<bool>)> = Vec::new();
    treatments.push((
        "low_r".into(),
        obs.iter().map(|o| o.low_r).collect(),
    ));
    for cat in BURNS_CATEGORIES {
        let mask: Vec<bool> = obs.iter().map(|o| o.distortions.contains(cat)).collect();
        if mask.iter().any(|&t| t) {
            treatments.push((cat.to_string(), mask));
        }
    }

    let outcomes: [(&str, &[f64]); 3] = [
        ("late_night", &late_res),
        ("impulse_spend", &impulse_res),
        ("ln_amount", &amount_res),
    ];

    let mut raw: Vec<SpendCognitionRelation> = Vec::new();
    for (t_name, t_mask) in &treatments {
        for (o_name, o_vals) in outcomes {
            let id = format!("{t_name}__{o_name}");
            match within_person_contrast(o_vals, t_mask) {
                Some((contrast, n_t, n_c, p_raw)) => {
                    raw.push(SpendCognitionRelation {
                        id,
                        treatment: t_name.clone(),
                        outcome: o_name.to_string(),
                        contrast,
                        n_treat: n_t,
                        n_control: n_c,
                        p_raw,
                        rejected_bh: false,
                        bh_rank: 0,
                        bh_critical: 0.0,
                    });
                }
                None => {
                    // Skipped by min-support gate — not counted as a tested hypothesis.
                }
            }
        }
    }

    let tested_count = raw.len();
    let hypothesis_count = treatments.len() * outcomes.len();
    apply_benjamini_hochberg(&mut raw, FDR_Q);

    // Deterministic order: rejected first, then ascending p_raw, then id.
    raw.sort_by(|a, b| {
        b.rejected_bh
            .cmp(&a.rejected_bh)
            .then(
                a.p_raw
                    .partial_cmp(&b.p_raw)
                    .unwrap_or(std::cmp::Ordering::Equal),
            )
            .then(a.id.cmp(&b.id))
    });

    SpendCognitionReport {
        schema: SCHEMA.into(),
        observation_count: obs.len(),
        distinct_days,
        support_ok: true,
        hypothesis_count,
        tested_count,
        fdr_q: FDR_Q,
        relations: raw,
    }
}

fn purchase_to_obs(row: &PurchaseRow) -> Option<Obs> {
    if !row.r_at_decision.is_finite() || row.total_amount < 0 {
        return None;
    }
    let (day_key, dow, hour, mop_bin) = jst_calendar_parts(row.occurred_at);
    let late_night = if hour >= 22 || hour < 5 { 1.0 } else { 0.0 };
    let impulse = if row.verified == 0 { 1.0 } else { 0.0 };
    let distortions = parse_distortions(&row.active_distortions_json);
    Some(Obs {
        day_key,
        amount: row.total_amount as f64,
        late_night,
        impulse,
        low_r: row.r_at_decision <= LOW_R_THRESHOLD,
        distortions,
        stratum: (dow as u16) * 3 + mop_bin as u16,
    })
}

fn parse_distortions(raw: &str) -> BTreeSet<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() || trimmed == "[]" {
        return BTreeSet::new();
    }
    match serde_json::from_str::<Vec<String>>(trimmed) {
        Ok(items) => items
            .into_iter()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect(),
        Err(_) => BTreeSet::new(),
    }
}

/// JST day key (days since epoch), weekday Mon=0..Sun=6, hour 0..23, mop bin 0..2.
fn jst_calendar_parts(unix: i64) -> (i64, u8, u8, u8) {
    let jst = unix.saturating_add(9 * 3_600);
    let day_key = jst.div_euclid(86_400);
    let tod = jst.rem_euclid(86_400) as u32;
    let hour = (tod / 3_600) as u8;
    // 1970-01-01 was Thursday; JST day_key 0 → Thu. Mon=0 → (day_key + 3) % 7
    let dow = ((day_key + 3).rem_euclid(7)) as u8;
    let (_y, _m, d) = civil_from_days(day_key);
    let mop_bin = if d <= 10 {
        0
    } else if d <= 20 {
        1
    } else {
        2
    };
    (day_key, dow, hour, mop_bin)
}

fn civil_from_days(z: i64) -> (i32, u32, u32) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    (y as i32, m as u32, d as u32)
}

/// Subtract stratum means (within-person FE for weekday × month-position).
fn residualize(values: &[f64], strata: &[u16]) -> Vec<f64> {
    debug_assert_eq!(values.len(), strata.len());
    let mut sums: BTreeMap<u16, (f64, usize)> = BTreeMap::new();
    for (&s, &v) in strata.iter().zip(values.iter()) {
        let e = sums.entry(s).or_insert((0.0, 0));
        e.0 += v;
        e.1 += 1;
    }
    let mut means: BTreeMap<u16, f64> = BTreeMap::new();
    for (s, (sum, n)) in sums {
        if n > 0 {
            means.insert(s, sum / n as f64);
        }
    }
    values
        .iter()
        .zip(strata.iter())
        .map(|(&v, &s)| v - means.get(&s).copied().unwrap_or(0.0))
        .collect()
}

/// Mean residual treated − control + Welch two-sided p. `None` if support gate fails.
fn within_person_contrast(
    y_res: &[f64],
    treat: &[bool],
) -> Option<(f64, usize, usize, f64)> {
    debug_assert_eq!(y_res.len(), treat.len());
    let mut a: Vec<f64> = Vec::new();
    let mut b: Vec<f64> = Vec::new();
    for (&y, &t) in y_res.iter().zip(treat.iter()) {
        if !y.is_finite() {
            continue;
        }
        if t {
            a.push(y);
        } else {
            b.push(y);
        }
    }
    if a.len() < MIN_GROUP_N || b.len() < MIN_GROUP_N {
        return None;
    }
    let mean_a = mean(&a);
    let mean_b = mean(&b);
    let contrast = mean_a - mean_b;
    let p = welch_two_sided_p(&a, &b).unwrap_or(1.0);
    Some((contrast, a.len(), b.len(), p.clamp(0.0, 1.0)))
}

fn mean(xs: &[f64]) -> f64 {
    if xs.is_empty() {
        return 0.0;
    }
    xs.iter().sum::<f64>() / xs.len() as f64
}

fn sample_var(xs: &[f64], mean: f64) -> f64 {
    if xs.len() < 2 {
        return 0.0;
    }
    let mut ss = 0.0;
    for &x in xs {
        let d = x - mean;
        ss += d * d;
    }
    ss / (xs.len() - 1) as f64
}

/// Welch t-test → two-sided p via normal approximation of t (df large) / Student-t
/// with Hartung df and Abramowitz–Stegun normal CDF (deterministic).
fn welch_two_sided_p(a: &[f64], b: &[f64]) -> Option<f64> {
    let na = a.len() as f64;
    let nb = b.len() as f64;
    if na < 2.0 || nb < 2.0 {
        return None;
    }
    let ma = mean(a);
    let mb = mean(b);
    let va = sample_var(a, ma);
    let vb = sample_var(b, mb);
    let se2 = va / na + vb / nb;
    if se2 <= 0.0 || !se2.is_finite() {
        // Identical residuals → no evidence of difference.
        return Some(1.0);
    }
    let t = (ma - mb) / se2.sqrt();
    // Welch–Satterthwaite df
    let num = se2 * se2;
    let den = (va / na).powi(2) / (na - 1.0) + (vb / nb).powi(2) / (nb - 1.0);
    let df = if den > 0.0 { num / den } else { na + nb - 2.0 };
    Some(student_t_two_sided_p(t.abs(), df))
}

fn student_t_two_sided_p(t_abs: f64, df: f64) -> f64 {
    if !t_abs.is_finite() || !df.is_finite() || df <= 0.0 {
        return 1.0;
    }
    // For df ≥ 30, normal approx is adequate; else use regularized incomplete beta
    // via continued fraction — still deterministic.
    if df >= 30.0 {
        return 2.0 * standard_normal_sf(t_abs);
    }
    // P(|T|>t) = I_{df/(df+t²)}(df/2, 1/2)  (upper incomplete beta relation)
    let x = df / (df + t_abs * t_abs);
    let a = df / 2.0;
    let b = 0.5;
    regularized_incomplete_beta(x, a, b).clamp(0.0, 1.0)
}

/// Survival function 1-Φ(x) for standard normal.
fn standard_normal_sf(x: f64) -> f64 {
    0.5 * erfc_approx(x / std::f64::consts::SQRT_2)
}

/// Complementary error function (Abramowitz & Stegun 7.1.26), |err| < 1.5e-7.
fn erfc_approx(x: f64) -> f64 {
    let z = x.abs();
    let t = 1.0 / (1.0 + 0.3275911 * z);
    let poly = t
        * (0.254829592
            + t * (-0.284496736
                + t * (1.421413741 + t * (-1.453152027 + t * 1.061405429))));
    let ans = poly * (-z * z).exp();
    if x >= 0.0 {
        ans
    } else {
        2.0 - ans
    }
}

/// Regularized incomplete beta I_x(a,b) via continued fraction (Lentz), deterministic.
fn regularized_incomplete_beta(x: f64, a: f64, b: f64) -> f64 {
    if x <= 0.0 {
        return 0.0;
    }
    if x >= 1.0 {
        return 1.0;
    }
    let ln_beta = ln_gamma(a) + ln_gamma(b) - ln_gamma(a + b);
    let front = (a * x.ln() + b * (1.0 - x).ln() - ln_beta).exp() / a;
    if x < (a + 1.0) / (a + b + 2.0) {
        front * betacf(x, a, b)
    } else {
        // symmetry: I_x(a,b) = 1 - I_{1-x}(b,a)
        let front2 = (b * (1.0 - x).ln() + a * x.ln() - ln_beta).exp() / b;
        1.0 - front2 * betacf(1.0 - x, b, a)
    }
}

fn betacf(x: f64, a: f64, b: f64) -> f64 {
    const MAX_ITER: usize = 200;
    const EPS: f64 = 3e-12;
    let qab = a + b;
    let qap = a + 1.0;
    let qam = a - 1.0;
    let mut c = 1.0;
    let mut d = 1.0 - qab * x / qap;
    if d.abs() < 1e-30 {
        d = 1e-30;
    }
    d = 1.0 / d;
    let mut h = d;
    for m in 1..=MAX_ITER {
        let m_f = m as f64;
        let m2 = 2.0 * m_f;
        let mut aa = m_f * (b - m_f) * x / ((qam + m2) * (a + m2));
        d = 1.0 + aa * d;
        if d.abs() < 1e-30 {
            d = 1e-30;
        }
        c = 1.0 + aa / c;
        if c.abs() < 1e-30 {
            c = 1e-30;
        }
        d = 1.0 / d;
        h *= d * c;
        aa = -(a + m_f) * (qab + m_f) * x / ((a + m2) * (qap + m2));
        d = 1.0 + aa * d;
        if d.abs() < 1e-30 {
            d = 1e-30;
        }
        c = 1.0 + aa / c;
        if c.abs() < 1e-30 {
            c = 1e-30;
        }
        d = 1.0 / d;
        let del = d * c;
        h *= del;
        if (del - 1.0).abs() < EPS {
            break;
        }
    }
    h
}

/// Lanczos approximation for ln Γ(z), z > 0.
fn ln_gamma(z: f64) -> f64 {
    const G: f64 = 7.0;
    const COEFF: [f64; 9] = [
        0.999_999_999_999_809_9,
        676.520_368_121_885_1,
        -1_259.139_216_722_402_8,
        771.323_428_777_653_1,
        -176.615_029_162_140_59,
        12.507_343_278_686_905,
        -0.138_571_095_265_720_12,
        9.984_369_654_078_915e-6,
        1.505_632_735_149_311_6e-7,
    ];
    if z < 0.5 {
        // reflection not needed for our a,b ≥ 0.5
        return ln_gamma(z + 1.0) - z.ln();
    }
    let z = z - 1.0;
    let mut x = COEFF[0];
    for (i, c) in COEFF.iter().enumerate().skip(1) {
        x += c / (z + i as f64);
    }
    let t = z + G + 0.5;
    (2.0 * std::f64::consts::PI).sqrt().ln() + (z + 0.5) * t.ln() - t + x.ln()
}

/// Benjamini–Hochberg (1995): reject H_(i) for i ≤ k* where
/// k* = max { i : p_(i) ≤ (i/m) q }. Deterministic stable sort by (p, id).
pub fn apply_benjamini_hochberg(relations: &mut [SpendCognitionRelation], q: f64) {
    let m = relations.len();
    if m == 0 {
        return;
    }
    let mut order: Vec<usize> = (0..m).collect();
    order.sort_by(|&i, &j| {
        relations[i]
            .p_raw
            .partial_cmp(&relations[j].p_raw)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(relations[i].id.cmp(&relations[j].id))
    });
    let mut max_k: Option<usize> = None;
    for (rank0, &idx) in order.iter().enumerate() {
        let i = rank0 + 1;
        let crit = (i as f64 / m as f64) * q;
        relations[idx].bh_rank = i;
        relations[idx].bh_critical = crit;
        if relations[idx].p_raw <= crit {
            max_k = Some(i);
        }
    }
    let k_star = max_k.unwrap_or(0);
    for (rank0, &idx) in order.iter().enumerate() {
        let i = rank0 + 1;
        relations[idx].rejected_bh = i <= k_star;
    }
}

/// FDR survivors only (for commitment proposals).
pub fn robust_relations(report: &SpendCognitionReport) -> Vec<&SpendCognitionRelation> {
    report
        .relations
        .iter()
        .filter(|r| r.rejected_bh && r.contrast > 0.0)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pur(
        id: &str,
        unix: i64,
        amount: i64,
        r: f64,
        verified: i64,
        distortions: &str,
    ) -> PurchaseRow {
        PurchaseRow {
            id: id.into(),
            occurred_at: unix,
            merchant_norm: "x".into(),
            total_amount: amount,
            tax: 0,
            verified,
            r_at_decision: r,
            active_distortions_json: distortions.into(),
        }
    }

    #[test]
    fn bh_rejects_smallest_p_under_threshold() {
        let mut rels = vec![
            SpendCognitionRelation {
                id: "a".into(),
                treatment: "t".into(),
                outcome: "o".into(),
                contrast: 1.0,
                n_treat: 10,
                n_control: 10,
                p_raw: 0.001,
                rejected_bh: false,
                bh_rank: 0,
                bh_critical: 0.0,
            },
            SpendCognitionRelation {
                id: "b".into(),
                treatment: "t".into(),
                outcome: "o".into(),
                contrast: 1.0,
                n_treat: 10,
                n_control: 10,
                p_raw: 0.04,
                rejected_bh: false,
                bh_rank: 0,
                bh_critical: 0.0,
            },
            SpendCognitionRelation {
                id: "c".into(),
                treatment: "t".into(),
                outcome: "o".into(),
                contrast: 1.0,
                n_treat: 10,
                n_control: 10,
                p_raw: 0.5,
                rejected_bh: false,
                bh_rank: 0,
                bh_critical: 0.0,
            },
        ];
        apply_benjamini_hochberg(&mut rels, 0.10);
        assert!(rels.iter().find(|r| r.id == "a").unwrap().rejected_bh);
        assert!(rels.iter().find(|r| r.id == "b").unwrap().rejected_bh);
        assert!(!rels.iter().find(|r| r.id == "c").unwrap().rejected_bh);
    }

    #[test]
    fn min_support_gate_skips_sparse_history() {
        let rows = vec![pur("1", 1_700_000_000, 100, 0.2, 1, "[]")];
        let report = analyze_spend_cognition(&rows);
        assert!(!report.support_ok);
        assert!(report.relations.is_empty());
    }

    /// JST local civil time → unix (day_key 20000 + offset, hour in JST).
    fn jst_unix(day_offset: i64, hour: i64) -> i64 {
        let day_key = 20_000 + day_offset;
        day_key * 86_400 - 9 * 3_600 + hour * 3_600
    }

    #[test]
    fn late_night_low_r_signal_detectable() {
        // ≥14 distinct JST days: even = low-R late-night impulse; odd = high-R daytime.
        let mut rows = Vec::new();
        for day in 0..20 {
            if day % 2 == 0 {
                rows.push(pur(
                    &format!("e{day}"),
                    jst_unix(day, 23),
                    5000,
                    0.2,
                    0,
                    r#"["magnification_minimization"]"#,
                ));
            } else {
                rows.push(pur(
                    &format!("o{day}"),
                    jst_unix(day, 12),
                    800,
                    0.8,
                    1,
                    "[]",
                ));
            }
        }
        let report = analyze_spend_cognition(&rows);
        assert!(report.support_ok, "days={} obs={}", report.distinct_days, report.observation_count);
        assert!(report.tested_count > 0);
        assert!(report.relations.iter().any(|r| r.treatment == "low_r"));
    }

    #[test]
    fn residualize_removes_stratum_mean() {
        let y = vec![1.0, 3.0, 10.0, 12.0];
        let s = vec![0, 0, 1, 1];
        let r = residualize(&y, &s);
        assert!((r[0] + 1.0).abs() < 1e-9); // 1 - 2
        assert!((r[1] - 1.0).abs() < 1e-9); // 3 - 2
        assert!((r[2] + 1.0).abs() < 1e-9);
        assert!((r[3] - 1.0).abs() < 1e-9);
    }

    #[test]
    fn erfc_known_values() {
        assert!((erfc_approx(0.0) - 1.0).abs() < 1e-6);
        assert!(erfc_approx(10.0) < 1e-6);
    }
}
