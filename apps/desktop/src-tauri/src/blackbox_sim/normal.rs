//! Deterministic standard normals — Marsaglia polar method (SPEC §3.3).
//!
//! Box–Muller needs cosine (libm — forbidden, BXS-W-02); the polar method
//! needs only `sqrt` (IEEE-exact) and the in-crate `det_ln`. The rejection
//! loop consumes the underlying Philox stream deterministically, so identical
//! streams yield identical normal sequences. The loop is BOUNDED: exceeding
//! `MAX_REJECTION_ROUNDS` (probability ≈ 0.215^4096 — physically never) is a
//! typed error, not a hang (Zero Panic / bounded loops).

use super::det_math::{det_ln, DetMathError};
use super::rng::PhiloxStream;

pub const MAX_REJECTION_ROUNDS: u32 = 4_096;

#[derive(Debug, Clone)]
pub struct NormalSource {
    stream: PhiloxStream,
    cache: Option<f64>,
}

impl NormalSource {
    #[must_use]
    pub fn new(stream: PhiloxStream) -> Self {
        Self {
            stream,
            cache: None,
        }
    }

    /// Stream position plus the pending half of the polar pair. The cache must
    /// be included: two sources at the same stream offset, one holding a
    /// cached mate and one not, produce different next values.
    #[must_use]
    pub fn position(&self) -> (u64, u8, Option<u64>) {
        let (block, buf_next) = self.stream.position();
        (block, buf_next, self.cache.map(f64::to_bits))
    }

    /// One standard normal draw. Consumes 2 uniforms per accepted polar
    /// round; every second call is served from the cached pair member —
    /// the draw ORDER is part of the determinism contract (BXS-I-01).
    pub fn next_standard_normal(&mut self) -> Result<f64, DetMathError> {
        if let Some(z) = self.cache.take() {
            return Ok(z);
        }
        let mut rounds = 0_u32;
        loop {
            rounds = rounds.saturating_add(1);
            if rounds > MAX_REJECTION_ROUNDS {
                return Err(DetMathError::RejectionOverrun);
            }
            let u = 2.0 * self.stream.next_unit_f64() - 1.0;
            let v = 2.0 * self.stream.next_unit_f64() - 1.0;
            let s = u * u + v * v;
            if s > 0.0 && s < 1.0 {
                let f = (-2.0 * det_ln(s)? / s).sqrt();
                self.cache = Some(v * f);
                return Ok(u * f);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::blackbox_sim::rng::{campaign_key_from_seed, SeedDomain};

    fn ok<T, E: std::fmt::Debug>(r: Result<T, E>) -> T {
        match r {
            Ok(v) => v,
            Err(e) => unreachable!("normal test setup failed: {e:?}"),
        }
    }

    fn source(tag: u8) -> NormalSource {
        let key = campaign_key_from_seed([tag; 8]);
        NormalSource::new(PhiloxStream::new(key, SeedDomain::Market, 0))
    }

    #[test]
    fn identical_streams_yield_identical_normals() {
        let mut a = source(3);
        let mut b = source(3);
        for _ in 0..64 {
            let za = ok(a.next_standard_normal());
            let zb = ok(b.next_standard_normal());
            assert_eq!(za.to_bits(), zb.to_bits(), "normals must be bit-identical");
        }
    }

    #[test]
    fn moments_are_plausible() {
        // Sanity only (not a statistical proof): mean ≈ 0, var ≈ 1 over 4096
        // deterministic draws — a wrong transform (e.g. missing /s) fails this.
        let mut src = source(5);
        let n = 4_096;
        let mut sum = 0.0_f64;
        let mut sum_sq = 0.0_f64;
        for _ in 0..n {
            let z = ok(src.next_standard_normal());
            assert!(z.is_finite());
            assert!(z.abs() < 8.0, "|z| ≥ 8 is implausible for 4096 draws");
            sum += z;
            sum_sq += z * z;
        }
        let mean = sum / f64::from(n);
        let var = sum_sq / f64::from(n) - mean * mean;
        assert!(mean.abs() < 0.1, "mean {mean} too far from 0");
        assert!((var - 1.0).abs() < 0.1, "variance {var} too far from 1");
    }
}
