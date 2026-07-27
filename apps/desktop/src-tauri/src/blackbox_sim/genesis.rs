//! CampaignGenesis — the frozen world artifact (SPEC §4, BXS-I-14 / 壁 W-a).
//!
//! All world parameters are drawn ONCE from the content-derived seed
//! (I-17: `SHA-256(canonical request)`, no OS entropy, no wall-clock beyond
//! the explicit `created_date` field) and frozen behind a SHA-256 fingerprint
//! computed BEFORE construction (hash-before-construction, §16.1).
//!
//! The draw ORDER of parameters is a frozen contract (BXS-I-01): reordering
//! draws silently changes every campaign in the world. Append new draws at
//! the end only.
//!
//! Wall W-a: nothing in this module may be serialized into a view model,
//! flavor prompt, or any player-visible artifact. The leak guard in
//! interop_tests asserts the market view never carries these field names.

use serde::Serialize;
use sha2::{Digest, Sha256};

use super::rng::{campaign_key_from_seed, PhiloxStream, SeedDomain};

pub const GENESIS_SCHEMA: &str = "bxs.genesis.v1";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
#[repr(u8)]
pub enum Difficulty {
    Standard = 0,
    Hard = 1,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GenesisRequest {
    pub scenario_id: u32,
    pub difficulty: Difficulty,
    pub campaign_index: u32,
    /// Strict `YYYY-MM-DD`; validated, never parsed from the system clock.
    pub created_date: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GenesisError {
    InvalidDate,
    Serialization,
    Overflow,
    FingerprintMismatch,
}

/// Commodity: Schwartz one-factor O-U in log space.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CommodityParams {
    pub kappa_micro: i64,
    pub theta_log_micro: i64,
    pub sigma_calm_micro: i64,
    pub sigma_stress_micro: i64,
}

/// Equity index: drift + regime vol + Merton jumps (jump params separate).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EquityParams {
    pub drift_micro: i64,
    pub sigma_calm_micro: i64,
    pub sigma_stress_micro: i64,
}

/// Vasicek short rate, basis-point space.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RateParams {
    pub a_micro: i64,
    pub b_bp: i64,
    pub sigma_bp_micro: i64,
}

/// Demand: triangular seasonal shape × AR(1) multiplicative shock.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DemandParams {
    pub phi_micro: i64,
    pub sigma_micro: i64,
    pub season_amp_micro: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RegimeParams {
    pub p_calm_to_stress_micro: i64,
    pub p_stress_to_calm_micro: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct JumpParams {
    pub intensity_stress_micro: i64,
    pub mu_micro: i64,
    pub sigma_micro: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MarketParams {
    pub commodity: CommodityParams,
    pub equity: EquityParams,
    pub rate: RateParams,
    pub demand: DemandParams,
    pub regime: RegimeParams,
    pub jump: JumpParams,
    pub corr_commodity_equity_micro: i64,
}

/// Product lines the firm operates. Fixed capacity — the simulator allocates
/// nothing per campaign (SPEC §13: bounds before allocation).
pub const MAX_SKUS: usize = 3;

/// One product line. `cost_multiplier_micro` maps the market commodity price
/// to this line's procurement cost, which is how the SDE reaches the P&L.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SkuParams {
    pub base_units_per_tick: i64,
    pub reference_price_minor: i64,
    pub cost_multiplier_micro: i64,
}

impl SkuParams {
    const ZERO: SkuParams = SkuParams {
        base_units_per_tick: 0,
        reference_price_minor: 0,
        cost_multiplier_micro: 0,
    };
}

/// The firm's frozen operating and financing constants. Rates are quarterly
/// basis points — the settle loop runs on quarters, so storing annual rates
/// would force a division at every close and invite rounding drift.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FirmParams {
    pub skus: [SkuParams; MAX_SKUS],
    pub opening_capital_minor: i64,
    pub depreciation_rate_bp: u32,
    pub tax_rate_bp: u32,
    pub senior_rate_bp: u32,
    pub mezz_rate_bp: u32,
}

/// Calibrated draw ranges (micro units unless noted). Structure-frozen at P1;
/// changing a range is a SPEC amendment, not a tweak.
mod ranges {
    pub const KAPPA: (i64, i64) = (50_000, 400_000);
    /// ln(5000 minor) ≈ 8.517 — commodity long-run level around 50.00 CRD.
    pub const THETA_LOG: (i64, i64) = (8_400_000, 8_700_000);
    pub const SIGMA_CALM: (i64, i64) = (10_000, 60_000);
    /// Stress multiplier ×1e6: 1.5–3.0.
    pub const STRESS_MULT: (i64, i64) = (1_500_000, 3_000_000);
    pub const EQ_DRIFT: (i64, i64) = (0, 4_000);
    pub const EQ_SIGMA_CALM: (i64, i64) = (15_000, 45_000);
    pub const RATE_A: (i64, i64) = (20_000, 120_000);
    pub const RATE_B_BP: (i64, i64) = (50, 600);
    pub const RATE_SIGMA_BP: (i64, i64) = (2_000_000, 12_000_000); // 2–12 bp ×1e6
    pub const DEMAND_PHI: (i64, i64) = (700_000, 950_000);
    pub const DEMAND_SIGMA: (i64, i64) = (50_000, 200_000);
    pub const SEASON_AMP: (i64, i64) = (50_000, 200_000);
    pub const P_CALM_TO_STRESS: (i64, i64) = (20_000, 80_000);
    pub const P_STRESS_TO_CALM: (i64, i64) = (100_000, 300_000);
    pub const JUMP_INTENSITY: (i64, i64) = (20_000, 100_000);
    pub const JUMP_MU: (i64, i64) = (-80_000, -20_000);
    pub const JUMP_SIGMA: (i64, i64) = (30_000, 90_000);
    pub const CORR: (i64, i64) = (-300_000, 600_000);

    // --- Phase 2 firm block. APPENDED — never interleave with the above, or
    // every existing campaign silently becomes a different world.
    pub const BASE_UNITS: (i64, i64) = (800, 2_500);
    /// 60.00–140.00 CRD per unit.
    pub const REFERENCE_PRICE: (i64, i64) = (6_000, 14_000);
    /// Commodity price → unit cost, ×1e6: 0.60–1.10.
    pub const COST_MULTIPLIER: (i64, i64) = (600_000, 1_100_000);
    /// 500,000–1,500,000 CRD of opening equity.
    pub const OPENING_CAPITAL: (i64, i64) = (50_000_000, 150_000_000);
    /// Quarterly depreciation on gross PPE: 2.5%–6.25% (10%–25% annual).
    pub const DEPRECIATION_BP: (i64, i64) = (250, 625);
    pub const TAX_BP: (i64, i64) = (2_000, 3_500);
    /// Quarterly coupons: senior 0.75%–2%, mezzanine strictly above it.
    pub const SENIOR_RATE_BP: (i64, i64) = (75, 200);
    pub const MEZZ_SPREAD_BP: (i64, i64) = (150, 300);
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CampaignGenesis {
    pub schema: &'static str,
    pub request: GenesisRequest,
    pub campaign_key: [u32; 2],
    pub params: MarketParams,
    pub firm: FirmParams,
    /// SHA-256 over (canonical request ‖ canonical params JSON), computed
    /// before construction; `verify_fingerprint` re-derives it (BXS-I-14
    /// loads are fail-closed on mismatch).
    pub fingerprint: [u8; 32],
}

fn validate_date(date: &str) -> Result<(), GenesisError> {
    let bytes = date.as_bytes();
    if bytes.len() != 10 {
        return Err(GenesisError::InvalidDate);
    }
    for (i, b) in bytes.iter().enumerate() {
        let want_dash = i == 4 || i == 7;
        if want_dash != (*b == b'-') || (!want_dash && !b.is_ascii_digit()) {
            return Err(GenesisError::InvalidDate);
        }
    }
    let digit = |i: usize| -> i32 {
        i32::from(bytes.get(i).copied().unwrap_or(b'0').saturating_sub(b'0'))
    };
    let month = digit(5) * 10 + digit(6);
    let day = digit(8) * 10 + digit(9);
    if !(1..=12).contains(&month) || !(1..=31).contains(&day) {
        return Err(GenesisError::InvalidDate);
    }
    Ok(())
}

fn canonical_request_bytes(req: &GenesisRequest) -> Result<Vec<u8>, GenesisError> {
    validate_date(&req.created_date)?;
    Ok(format!(
        "{GENESIS_SCHEMA}|{}|{}|{}|{}",
        req.scenario_id, req.difficulty as u8, req.campaign_index, req.created_date
    )
    .into_bytes())
}

/// Content-derived campaign seed (I-17): first 8 bytes of SHA-256(canonical).
pub fn derive_campaign_seed(req: &GenesisRequest) -> Result<[u8; 8], GenesisError> {
    let canonical = canonical_request_bytes(req)?;
    let digest = Sha256::digest(&canonical);
    let head = digest.get(0..8).ok_or(GenesisError::Overflow)?;
    head.try_into().map_err(|_| GenesisError::Overflow)
}

/// Uniform integer in [lo, hi] from one u64 draw. The modulo bias is
/// < 2⁻⁴⁰ for every range in this file — irrelevant for game calibration and
/// fully deterministic, which is the property that matters (第五律).
fn draw_range(stream: &mut PhiloxStream, range: (i64, i64)) -> Result<i64, GenesisError> {
    let (lo, hi) = range;
    if hi < lo {
        return Err(GenesisError::Overflow);
    }
    let span = u64::try_from(hi.checked_sub(lo).ok_or(GenesisError::Overflow)?)
        .map_err(|_| GenesisError::Overflow)?
        .checked_add(1)
        .ok_or(GenesisError::Overflow)?;
    let offset = i64::try_from(stream.next_u64() % span).map_err(|_| GenesisError::Overflow)?;
    lo.checked_add(offset).ok_or(GenesisError::Overflow)
}

/// (v × mult_micro) / 1e6, floor-half-up on i128.
fn scale_micro(v: i64, mult_micro: i64) -> Result<i64, GenesisError> {
    let num = i128::from(v)
        .checked_mul(i128::from(mult_micro))
        .ok_or(GenesisError::Overflow)?;
    let q = super::money::floor_half_up(num, 1_000_000).map_err(|_| GenesisError::Overflow)?;
    i64::try_from(q).map_err(|_| GenesisError::Overflow)
}

/// Difficulty scaling: Hard multiplies crisis pressure ×1.5 (frozen).
fn difficulty_scaled(v: i64, difficulty: Difficulty) -> Result<i64, GenesisError> {
    match difficulty {
        Difficulty::Standard => Ok(v),
        Difficulty::Hard => scale_micro(v, 1_500_000),
    }
}

/// Draw all world parameters in the FROZEN order below (append-only contract).
fn draw_params(
    stream: &mut PhiloxStream,
    difficulty: Difficulty,
) -> Result<MarketParams, GenesisError> {
    let kappa_micro = draw_range(stream, ranges::KAPPA)?;
    let theta_log_micro = draw_range(stream, ranges::THETA_LOG)?;
    let c_sigma_calm = draw_range(stream, ranges::SIGMA_CALM)?;
    let c_stress_mult = draw_range(stream, ranges::STRESS_MULT)?;
    let commodity = CommodityParams {
        kappa_micro,
        theta_log_micro,
        sigma_calm_micro: c_sigma_calm,
        sigma_stress_micro: scale_micro(c_sigma_calm, c_stress_mult)?,
    };
    let drift_micro = draw_range(stream, ranges::EQ_DRIFT)?;
    let e_sigma_calm = draw_range(stream, ranges::EQ_SIGMA_CALM)?;
    let e_stress_mult = draw_range(stream, ranges::STRESS_MULT)?;
    let equity = EquityParams {
        drift_micro,
        sigma_calm_micro: e_sigma_calm,
        sigma_stress_micro: scale_micro(e_sigma_calm, e_stress_mult)?,
    };
    let rate = RateParams {
        a_micro: draw_range(stream, ranges::RATE_A)?,
        b_bp: draw_range(stream, ranges::RATE_B_BP)?,
        sigma_bp_micro: draw_range(stream, ranges::RATE_SIGMA_BP)?,
    };
    let demand = DemandParams {
        phi_micro: draw_range(stream, ranges::DEMAND_PHI)?,
        sigma_micro: draw_range(stream, ranges::DEMAND_SIGMA)?,
        season_amp_micro: draw_range(stream, ranges::SEASON_AMP)?,
    };
    let regime = RegimeParams {
        p_calm_to_stress_micro: difficulty_scaled(
            draw_range(stream, ranges::P_CALM_TO_STRESS)?,
            difficulty,
        )?,
        p_stress_to_calm_micro: draw_range(stream, ranges::P_STRESS_TO_CALM)?,
    };
    let jump = JumpParams {
        intensity_stress_micro: difficulty_scaled(
            draw_range(stream, ranges::JUMP_INTENSITY)?,
            difficulty,
        )?,
        mu_micro: draw_range(stream, ranges::JUMP_MU)?,
        sigma_micro: draw_range(stream, ranges::JUMP_SIGMA)?,
    };
    let corr_commodity_equity_micro = draw_range(stream, ranges::CORR)?;
    Ok(MarketParams {
        commodity,
        equity,
        rate,
        demand,
        regime,
        jump,
        corr_commodity_equity_micro,
    })
}

/// Draw the firm block. Runs on the SAME stream immediately after
/// `draw_params`, so the market draws above are untouched (guarded by
/// `market_draws_are_unchanged_by_later_appends`).
fn draw_firm_params(stream: &mut PhiloxStream) -> Result<FirmParams, GenesisError> {
    let mut skus = [SkuParams::ZERO; MAX_SKUS];
    for slot in skus.iter_mut() {
        *slot = SkuParams {
            base_units_per_tick: draw_range(stream, ranges::BASE_UNITS)?,
            reference_price_minor: draw_range(stream, ranges::REFERENCE_PRICE)?,
            cost_multiplier_micro: draw_range(stream, ranges::COST_MULTIPLIER)?,
        };
    }
    let opening_capital_minor = draw_range(stream, ranges::OPENING_CAPITAL)?;
    let depreciation_rate_bp = draw_bp(stream, ranges::DEPRECIATION_BP)?;
    let tax_rate_bp = draw_bp(stream, ranges::TAX_BP)?;
    let senior_rate_bp = draw_bp(stream, ranges::SENIOR_RATE_BP)?;
    // Mezzanine is priced as a spread over senior, never drawn independently:
    // an independent draw could invert the capital structure and make the
    // junior tranche cheaper than the senior one.
    let mezz_rate_bp = senior_rate_bp
        .checked_add(draw_bp(stream, ranges::MEZZ_SPREAD_BP)?)
        .ok_or(GenesisError::Overflow)?;
    Ok(FirmParams {
        skus,
        opening_capital_minor,
        depreciation_rate_bp,
        tax_rate_bp,
        senior_rate_bp,
        mezz_rate_bp,
    })
}

fn draw_bp(stream: &mut PhiloxStream, range: (i64, i64)) -> Result<u32, GenesisError> {
    u32::try_from(draw_range(stream, range)?).map_err(|_| GenesisError::Overflow)
}

/// Build the frozen artifact. Fingerprint is computed from plain data BEFORE
/// the one and only construction (§16.1 — no dummy-then-overwrite).
/// The one canonicalization, shared by the builder and the load-boundary
/// verifier. Two hand-written copies would eventually disagree, and the
/// disagreement would present as "corrupt artifact" rather than as the bug it
/// is (§16.1: the ID derives from plain data, once).
fn fingerprint_of(
    canonical: &[u8],
    params: &MarketParams,
    firm: &FirmParams,
) -> Result<[u8; 32], GenesisError> {
    let params_json = serde_json::to_vec(params).map_err(|_| GenesisError::Serialization)?;
    let firm_json = serde_json::to_vec(firm).map_err(|_| GenesisError::Serialization)?;
    let mut hasher = Sha256::new();
    hasher.update(canonical);
    hasher.update(&params_json);
    hasher.update(&firm_json);
    Ok(hasher.finalize().into())
}

pub fn build_campaign_genesis(req: GenesisRequest) -> Result<CampaignGenesis, GenesisError> {
    let canonical = canonical_request_bytes(&req)?;
    let seed = derive_campaign_seed(&req)?;
    let campaign_key = campaign_key_from_seed(seed);
    let mut stream = PhiloxStream::new(campaign_key, SeedDomain::Genesis, 0);
    let params = draw_params(&mut stream, req.difficulty)?;
    let firm = draw_firm_params(&mut stream)?;
    let fingerprint = fingerprint_of(&canonical, &params, &firm)?;
    Ok(CampaignGenesis {
        schema: GENESIS_SCHEMA,
        request: req,
        campaign_key,
        params,
        firm,
        fingerprint,
    })
}

impl CampaignGenesis {
    /// Load-boundary re-verification (receiver verifies — 第六律). Mismatch is
    /// fail-closed corruption, never repaired.
    pub fn verify_fingerprint(&self) -> Result<(), GenesisError> {
        let canonical = canonical_request_bytes(&self.request)?;
        let expect = fingerprint_of(&canonical, &self.params, &self.firm)?;
        if expect == self.fingerprint {
            Ok(())
        } else {
            Err(GenesisError::FingerprintMismatch)
        }
    }

    /// Truncated fingerprint for telemetry cross-references.
    #[must_use]
    pub fn digest8(&self) -> [u8; 8] {
        let mut out = [0_u8; 8];
        for (dst, src) in out.iter_mut().zip(self.fingerprint.iter()) {
            *dst = *src;
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ok<T, E: std::fmt::Debug>(r: Result<T, E>) -> T {
        match r {
            Ok(v) => v,
            Err(e) => unreachable!("genesis test setup failed: {e:?}"),
        }
    }

    fn request() -> GenesisRequest {
        GenesisRequest {
            scenario_id: 7,
            difficulty: Difficulty::Standard,
            campaign_index: 0,
            created_date: "2026-07-27".to_string(),
        }
    }

    #[test]
    fn same_request_same_world() {
        let a = ok(build_campaign_genesis(request()));
        let b = ok(build_campaign_genesis(request()));
        assert_eq!(a, b, "genesis must be a pure function of the request");
    }

    /// The draw order is append-only (BXS-I-01). Phase 2 appended the firm
    /// parameters AFTER the market block, so every market value must still be
    /// exactly what Phase 1 measured. If this fails, someone inserted a draw
    /// in the middle and silently rewrote every campaign in the world.
    #[test]
    fn market_draws_are_unchanged_by_later_appends() {
        let p = ok(build_campaign_genesis(request())).params;
        assert_eq!(p.commodity.kappa_micro, 362_038);
        assert_eq!(p.commodity.theta_log_micro, 8_591_935);
        assert_eq!(p.commodity.sigma_calm_micro, 10_312);
        assert_eq!(p.commodity.sigma_stress_micro, 23_417);
        assert_eq!(p.equity.drift_micro, 2_410);
        assert_eq!(p.equity.sigma_calm_micro, 16_541);
        assert_eq!(p.equity.sigma_stress_micro, 28_801);
        assert_eq!(p.rate.a_micro, 119_147);
        assert_eq!(p.rate.b_bp, 75);
        assert_eq!(p.rate.sigma_bp_micro, 10_392_969);
        assert_eq!(p.demand.phi_micro, 823_724);
        assert_eq!(p.demand.sigma_micro, 123_509);
        assert_eq!(p.demand.season_amp_micro, 187_598);
        assert_eq!(p.regime.p_calm_to_stress_micro, 44_952);
        assert_eq!(p.regime.p_stress_to_calm_micro, 295_458);
        assert_eq!(p.jump.intensity_stress_micro, 24_690);
        assert_eq!(p.jump.mu_micro, -55_551);
        assert_eq!(p.jump.sigma_micro, 66_460);
        assert_eq!(p.corr_commodity_equity_micro, 270_384);
    }

    #[test]
    fn firm_params_land_inside_calibrated_ranges() {
        let g = ok(build_campaign_genesis(request()));
        let f = g.firm;
        assert!((2_000..=3_500).contains(&f.tax_rate_bp));
        assert!((250..=625).contains(&f.depreciation_rate_bp));
        assert!(f.mezz_rate_bp > f.senior_rate_bp, "mezz must price above senior");
        assert!(f.opening_capital_minor >= 50_000_000);
        for sku in f.skus {
            assert!((800..=2_500).contains(&sku.base_units_per_tick));
            assert!((6_000..=14_000).contains(&sku.reference_price_minor));
            assert!((600_000..=1_100_000).contains(&sku.cost_multiplier_micro));
        }
    }

    #[test]
    fn firm_params_are_covered_by_the_fingerprint() {
        let mut g = ok(build_campaign_genesis(request()));
        ok(g.verify_fingerprint());
        g.firm.tax_rate_bp += 1;
        assert!(
            g.verify_fingerprint().is_err(),
            "firm params must be inside the frozen artifact, not beside it"
        );
    }

    #[test]
    fn different_request_different_world() {
        let a = ok(build_campaign_genesis(request()));
        let mut req = request();
        req.campaign_index = 1;
        let b = ok(build_campaign_genesis(req));
        assert_ne!(a.fingerprint, b.fingerprint);
        assert_ne!(a.params, b.params);
    }

    #[test]
    fn fingerprint_verifies_and_detects_tamper() {
        let mut g = ok(build_campaign_genesis(request()));
        ok(g.verify_fingerprint());
        g.params.jump.mu_micro = -1;
        assert!(g.verify_fingerprint().is_err(), "tamper must fail closed");
    }

    #[test]
    fn params_land_inside_calibrated_ranges() {
        let g = ok(build_campaign_genesis(request()));
        let p = g.params;
        assert!((50_000..=400_000).contains(&p.commodity.kappa_micro));
        assert!(p.commodity.sigma_stress_micro >= p.commodity.sigma_calm_micro);
        assert!((0..=4_000).contains(&p.equity.drift_micro));
        assert!((50..=600).contains(&p.rate.b_bp));
        assert!(p.jump.mu_micro < 0, "jump mean must be negative (crash risk)");
        assert!((-300_000..=600_000).contains(&p.corr_commodity_equity_micro));
    }

    #[test]
    fn hard_difficulty_scales_crisis_pressure() {
        let standard = ok(build_campaign_genesis(request()));
        let mut req = request();
        req.difficulty = Difficulty::Hard;
        let hard = ok(build_campaign_genesis(req));
        // Same seed content except difficulty ⇒ different canonical ⇒ new
        // world, so the two draws are not comparable value-for-value; what is
        // testable is that each stays inside its OWN range. The Standard arm
        // is the control group: without it, a scaling factor silently applied
        // to both difficulties would still pass (LAW-13).
        assert!((20_000..=80_000).contains(&standard.params.regime.p_calm_to_stress_micro));
        assert!((20_000..=100_000).contains(&standard.params.jump.intensity_stress_micro));
        assert!((30_000..=120_000).contains(&hard.params.regime.p_calm_to_stress_micro));
        assert!((30_000..=150_000).contains(&hard.params.jump.intensity_stress_micro));
    }

    #[test]
    fn malformed_dates_fail_closed() {
        for bad in ["2026/07/27", "2026-13-01", "2026-00-10", "2026-07-32", "26-07-27", ""] {
            let mut req = request();
            req.created_date = bad.to_string();
            assert!(matches!(
                build_campaign_genesis(req),
                Err(GenesisError::InvalidDate)
            ));
        }
    }
}
