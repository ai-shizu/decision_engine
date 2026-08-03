//! Market kernel — SDE evolution (SPEC §5, BXS-I-12).
//!
//! tick = 1 week, quarter = 13 ticks, Euler–Maruyama in log space. All
//! randomness flows from three frozen substreams of `DOM_MARKET`; the normal
//! draw order per tick is a contract (z_commodity, z_equity_raw, z_rate,
//! z_demand, z_jump — always all five, jump used only when it fires) so the
//! stream consumption is position-predictable for replay (BXS-I-01).
//!
//! Authoritative published values are integers (minor units / centi-points /
//! micro / bp). The continuous f64 kernel state uses only the restricted
//! operation set (SPEC §3.3) and is exposed bit-wise for replay anchoring.
//!
//! Wall W-a: `MarketTickView` is the ONLY thing the outside world sees;
//! genesis true parameters never serialize into it (leak guard in
//! interop_tests pins this).

use serde::{Deserialize, Serialize};

use super::det_math::{det_exp, quantize_scaled, DetMathError};
use super::genesis::CampaignGenesis;
use super::genesis::MarketParams;
use super::normal::NormalSource;
use super::ring::{FixedRing, RingConfigError, RingPush};
use super::rng::{PhiloxStream, SeedDomain};

pub const MARKET_HISTORY_TICKS: usize = 512;

/// Frozen substream allocation inside `DOM_MARKET` (BXS-I-13).
const SUB_NORMALS: u64 = 0;
const SUB_REGIME: u64 = 1;
const SUB_JUMP: u64 = 2;

/// Initial equity index level: ln(10_000.00 points in centi = 1e6 centi)…
/// kept simply as ln(10_000) for the point scale; decimal literal is exact
/// and identical on every platform.
const EQUITY_LOG_INITIAL: f64 = 9.210_340_371_976_184;

/// Kernel-state clamps: keep quantization inside safe integer range and the
/// game economy inside sane magnitudes (deterministic comparisons only).
const LOG_COMMODITY_RANGE: (f64, f64) = (2.0, 14.0);
const LOG_EQUITY_RANGE: (f64, f64) = (2.0, 16.0);
const DEMAND_Y_RANGE: (f64, f64) = (-3.0, 3.0);
const RATE_BP_RANGE: (f64, f64) = (0.0, 2_000.0);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[repr(u8)]
pub enum Regime {
    Calm = 0,
    Stress = 1,
}

/// The player-visible market snapshot (wall W-a boundary object).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MarketTickView {
    pub tick: u32,
    pub regime: Regime,
    pub commodity_price_minor: i64,
    pub equity_index_centi: i64,
    pub demand_index_micro: i64,
    pub rate_bp: i32,
    pub jump_occurred: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MarketError {
    Math(DetMathError),
    Ring(RingConfigError),
    Overflow,
}

impl From<DetMathError> for MarketError {
    fn from(e: DetMathError) -> Self {
        MarketError::Math(e)
    }
}

#[inline]
fn micro_f(v: i64) -> f64 {
    // Exact int→f64 (|v| « 2^53), one rounded multiply — deterministic.
    (v as f64) * 1.0e-6
}

#[derive(Debug, Clone)]
pub struct MarketKernel {
    params: MarketParams,
    normals: NormalSource,
    regime_stream: PhiloxStream,
    jump_stream: PhiloxStream,
    tick: u32,
    regime: Regime,
    log_commodity: f64,
    log_equity: f64,
    demand_y: f64,
    rate_bp_state: f64,
    history: FixedRing<MarketTickView>,
}

impl MarketKernel {
    pub fn new(genesis: &CampaignGenesis) -> Result<Self, MarketError> {
        let key = genesis.campaign_key;
        let history = FixedRing::new(MARKET_HISTORY_TICKS).map_err(MarketError::Ring)?;
        Ok(Self {
            params: genesis.params,
            normals: NormalSource::new(PhiloxStream::new(key, SeedDomain::Market, SUB_NORMALS)),
            regime_stream: PhiloxStream::new(key, SeedDomain::Market, SUB_REGIME),
            jump_stream: PhiloxStream::new(key, SeedDomain::Market, SUB_JUMP),
            tick: 0,
            regime: Regime::Calm,
            log_commodity: micro_f(genesis.params.commodity.theta_log_micro),
            log_equity: EQUITY_LOG_INITIAL,
            demand_y: 0.0,
            rate_bp_state: genesis.params.rate.b_bp as f64,
            history,
        })
    }

    /// Triangular 52-week season in [1−amp, 1+amp] (deterministic integer →
    /// f64 shape; the Director may refine event seasonality in Phase 3).
    fn season_factor(&self) -> f64 {
        let week = self.tick % 52;
        let folded = week.min(52 - week);
        let tri = f64::from(folded) / 26.0; // [0, 1]
        1.0 + micro_f(self.params.demand.season_amp_micro) * (2.0 * tri - 1.0)
    }

    /// Advance one tick. Draw order is FROZEN (see module docs).
    pub fn step(&mut self) -> Result<MarketTickView, MarketError> {
        let p = self.params;
        // 1. Regime transition.
        let u_regime = self.regime_stream.next_unit_f64();
        self.regime = match self.regime {
            Regime::Calm if u_regime < micro_f(p.regime.p_calm_to_stress_micro) => Regime::Stress,
            Regime::Stress if u_regime < micro_f(p.regime.p_stress_to_calm_micro) => Regime::Calm,
            unchanged => unchanged,
        };
        let (sigma_c, sigma_e, jump_lambda) = match self.regime {
            Regime::Calm => (
                micro_f(p.commodity.sigma_calm_micro),
                micro_f(p.equity.sigma_calm_micro),
                0.0,
            ),
            Regime::Stress => (
                micro_f(p.commodity.sigma_stress_micro),
                micro_f(p.equity.sigma_stress_micro),
                micro_f(p.jump.intensity_stress_micro),
            ),
        };
        // 2. Normals — frozen order.
        let z_c = self.normals.next_standard_normal()?;
        let z_e_raw = self.normals.next_standard_normal()?;
        let z_r = self.normals.next_standard_normal()?;
        let z_d = self.normals.next_standard_normal()?;
        let z_j = self.normals.next_standard_normal()?;
        // 3. Correlation (fixed Cholesky 2×2).
        let rho = micro_f(p.corr_commodity_equity_micro);
        let z_e = rho * z_c + (1.0 - rho * rho).sqrt() * z_e_raw;
        // 4. Commodity O-U in log space.
        let theta = micro_f(p.commodity.theta_log_micro);
        self.log_commodity = (self.log_commodity
            + micro_f(p.commodity.kappa_micro) * (theta - self.log_commodity)
            + sigma_c * z_c)
            .clamp(LOG_COMMODITY_RANGE.0, LOG_COMMODITY_RANGE.1);
        // 5. Jump draw happens EVERY tick (uniform stream consumption);
        //    it only fires under stress.
        let u_jump = self.jump_stream.next_unit_f64();
        let jump_occurred = matches!(self.regime, Regime::Stress) && u_jump < jump_lambda;
        // 6. Equity index.
        let mut d_log_equity = micro_f(p.equity.drift_micro) + sigma_e * z_e;
        if jump_occurred {
            d_log_equity += micro_f(p.jump.mu_micro) + micro_f(p.jump.sigma_micro) * z_j;
        }
        self.log_equity =
            (self.log_equity + d_log_equity).clamp(LOG_EQUITY_RANGE.0, LOG_EQUITY_RANGE.1);
        // 7. Demand AR(1).
        self.demand_y = (micro_f(p.demand.phi_micro) * self.demand_y
            + micro_f(p.demand.sigma_micro) * z_d)
            .clamp(DEMAND_Y_RANGE.0, DEMAND_Y_RANGE.1);
        // 8. Vasicek rate in bp space.
        let r = self.rate_bp_state;
        self.rate_bp_state = (r
            + micro_f(p.rate.a_micro) * (p.rate.b_bp as f64 - r)
            + micro_f(p.rate.sigma_bp_micro) * z_r)
            .clamp(RATE_BP_RANGE.0, RATE_BP_RANGE.1);
        // 9. Quantize the authoritative view (integers only past this line).
        let commodity_price_minor = quantize_scaled(det_exp(self.log_commodity)?, 1.0)?;
        let equity_index_centi = quantize_scaled(det_exp(self.log_equity)?, 100.0)?;
        let demand_index_micro =
            quantize_scaled(self.season_factor() * det_exp(self.demand_y)?, 1.0e6)?;
        let rate_bp_i64 = quantize_scaled(self.rate_bp_state, 1.0)?;
        let rate_bp = i32::try_from(rate_bp_i64).map_err(|_| MarketError::Overflow)?;
        let view = MarketTickView {
            tick: self.tick,
            regime: self.regime,
            commodity_price_minor,
            equity_index_centi,
            demand_index_micro,
            rate_bp,
            jump_occurred,
        };
        // Derived data: evicting ring is correct here (BXS-W-03 — records go
        // through telemetry, never through this ring).
        let _evicted: RingPush<MarketTickView> = self.history.evicting_push(view);
        self.tick = self.tick.checked_add(1).ok_or(MarketError::Overflow)?;
        Ok(view)
    }

    #[must_use]
    pub fn tick(&self) -> u32 {
        self.tick
    }

    #[must_use]
    pub fn latest(&self) -> Option<&MarketTickView> {
        self.history.latest()
    }

    pub fn history(&self) -> impl Iterator<Item = &MarketTickView> {
        self.history.iter()
    }

    /// Bit-exact continuous-state anchor for replay/determinism assertions
    /// (order: log_commodity, log_equity, demand_y, rate_bp_state).
    #[must_use]
    pub fn raw_state_bits(&self) -> [u64; 4] {
        [
            self.log_commodity.to_bits(),
            self.log_equity.to_bits(),
            self.demand_y.to_bits(),
            self.rate_bp_state.to_bits(),
        ]
    }

    #[must_use]
    pub fn regime(&self) -> Regime {
        self.regime
    }

    /// ORACLE — sealed inside L2 (wall W-a). The one-step-ahead expectation of
    /// the demand index under the TRUE parameters, which no player can compute
    /// because `demand_y`, `phi`, `sigma` and the season shape are all private.
    ///
    /// For the AR(1) log-demand `y' = phi*y + sigma*z`, the index is
    /// `season * exp(y')`, so `E[index'] = season' * exp(phi*y + sigma^2/2)`
    /// (log-normal mean). Call this AFTER `step`, when `self.tick` already
    /// names the next tick, so `season_factor` reads the next season.
    ///
    /// This is the only supported source of "true value" for the anchoring
    /// stimulus. Never serialise it into a view model.
    pub fn oracle_expected_next_demand_micro(&self) -> Result<i64, MarketError> {
        let sigma = micro_f(self.params.demand.sigma_micro);
        let mean_log = micro_f(self.params.demand.phi_micro) * self.demand_y + 0.5 * sigma * sigma;
        Ok(quantize_scaled(
            self.season_factor() * det_exp(mean_log.clamp(DEMAND_Y_RANGE.0, DEMAND_Y_RANGE.1))?,
            1.0e6,
        )?)
    }

    /// Everything that determines the NEXT tick and is not already in
    /// `raw_state_bits`: the position of all three entropy sources plus the
    /// normal source's cached pair member. Together these two accessors cover
    /// the kernel's entire evolution state, which is what makes the replay
    /// digest total rather than merely plausible.
    #[must_use]
    pub fn rng_position_bits(&self) -> [u64; 8] {
        let (n_block, n_buf, n_cache) = self.normals.position();
        let (r_block, r_buf) = self.regime_stream.position();
        let (j_block, j_buf) = self.jump_stream.position();
        [
            n_block,
            u64::from(n_buf),
            // Presence is its own word: folding it into the value would let
            // "no cached mate" alias against some legitimate bit pattern.
            u64::from(n_cache.is_some()),
            n_cache.unwrap_or(0),
            r_block,
            u64::from(r_buf),
            j_block,
            u64::from(j_buf),
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::blackbox_sim::genesis::{build_campaign_genesis, Difficulty, GenesisRequest};

    fn ok<T, E: std::fmt::Debug>(r: Result<T, E>) -> T {
        match r {
            Ok(v) => v,
            Err(e) => unreachable!("market test setup failed: {e:?}"),
        }
    }

    fn kernel() -> MarketKernel {
        let genesis = ok(build_campaign_genesis(GenesisRequest {
            scenario_id: 7,
            difficulty: Difficulty::Standard,
            campaign_index: 0,
            created_date: "2026-07-27".to_string(),
        }));
        ok(MarketKernel::new(&genesis))
    }

    #[test]
    fn two_kernels_are_bit_identical() {
        let mut a = kernel();
        let mut b = kernel();
        for _ in 0..104 {
            let va = ok(a.step());
            let vb = ok(b.step());
            assert_eq!(va, vb);
        }
        assert_eq!(a.raw_state_bits(), b.raw_state_bits());
    }

    #[test]
    fn published_values_stay_in_sane_integer_ranges() {
        let mut k = kernel();
        for _ in 0..208 {
            let v = ok(k.step());
            assert!(v.commodity_price_minor > 0);
            assert!(v.commodity_price_minor < 2_000_000, "price cap breached");
            assert!(v.equity_index_centi > 0);
            assert!((0..=2_000).contains(&v.rate_bp));
            assert!(v.demand_index_micro > 0);
        }
        assert_eq!(k.tick(), 208);
    }

    #[test]
    fn history_ring_is_bounded() {
        let mut k = kernel();
        for _ in 0..(MARKET_HISTORY_TICKS + 100) {
            let _ = ok(k.step());
        }
        assert_eq!(k.history().count(), MARKET_HISTORY_TICKS);
        let latest_tick = match k.latest() {
            Some(v) => v.tick,
            None => unreachable!("history cannot be empty after stepping"),
        };
        assert_eq!(latest_tick, (MARKET_HISTORY_TICKS + 100 - 1) as u32);
    }
}
