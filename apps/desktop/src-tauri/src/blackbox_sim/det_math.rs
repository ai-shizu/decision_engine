//! Deterministic transcendental functions (SPEC §3.3, BXS-I-12 / BXS-W-02).
//!
//! libm's exp/log families are NOT bit-stable across platforms; these
//! replacements use only IEEE-exact operations — `+ − × ÷`, `sqrt`, `floor`,
//! and bit manipulation — so results are bit-identical on every target the
//! app builds for. Accuracy is ~1e-15 relative, far beyond game needs; the
//! point is reproducibility, not ulp-perfection.
//!
//! Domain policy is fail-closed: out-of-domain input is a typed error, never
//! a NaN, never a saturation, never a panic (第六律/第七律).
//!
//! Every float literal below is hand-pinned and load-bearing, so clippy's
//! constant advice must NOT be followed here (trap BXS-W-08). `LN2_HI` is
//! deliberately a TRUNCATED high part of ln 2, not ln 2 — the Cody-Waite
//! split only cancels error because the high part has trailing zero bits.
//! Replacing it with `f64::consts::LN_2`, or letting `excessive_precision`
//! shorten it, destroys the cancellation and silently degrades every
//! exponential in the simulator. The reference values in the tests are
//! decimal literals on purpose (§16.6: an expectation computed by libm would
//! make the test a mirror of the thing it is supposed to check).
#![allow(clippy::approx_constant, clippy::excessive_precision)]

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DetMathError {
    OutOfDomain,
    RejectionOverrun,
}

/// ln(2) split into high/low parts (musl constants) so `x − k·ln2` stays
/// accurate; the split itself is exact IEEE arithmetic.
const LN2_HI: f64 = 6.931_471_803_691_238_164_90e-01;
const LN2_LO: f64 = 1.908_214_929_270_587_700_02e-10;
const INV_LN2: f64 = 1.442_695_040_888_963_387_00e+00;

/// |x| bound keeping the 2^k scale inside normal f64 range (SPEC §3.3).
pub const DET_EXP_MAX_ABS: f64 = 708.0;

/// Deterministic e^x over [−708, 708].
///
/// Range reduction x = k·ln2 + r (|r| ≤ ln2/2), Taylor series for e^r
/// (13 terms; tail < 1e-17 on the reduced range), exact 2^k scaling via
/// exponent-bit construction.
pub fn det_exp(x: f64) -> Result<f64, DetMathError> {
    if !x.is_finite() || !(-DET_EXP_MAX_ABS..=DET_EXP_MAX_ABS).contains(&x) {
        return Err(DetMathError::OutOfDomain);
    }
    let kf = (x * INV_LN2 + 0.5).floor();
    let r = (x - kf * LN2_HI) - kf * LN2_LO;
    // Taylor e^r = Σ r^n / n!, evaluated iteratively (all ops exact IEEE;
    // term update order is part of the determinism contract — do not reorder).
    let mut sum = 1.0_f64;
    let mut term = 1.0_f64;
    let mut n = 1.0_f64;
    while n <= 13.0 {
        term = term * r / n;
        sum += term;
        n += 1.0;
    }
    // 2^k with k ∈ [−1021, 1021] (guaranteed by the domain bound): normal
    // range only, built exactly from exponent bits.
    let k = kf as i64;
    let bits = ((k + 1023) as u64) << 52;
    Ok(sum * f64::from_bits(bits))
}

/// Deterministic natural log for positive, normal (non-subnormal) inputs.
///
/// Decompose x = m·2^e (m ∈ [√2/2, √2) after centering), then
/// ln(m) = 2·atanh(s) with s = (m−1)/(m+1) via odd series to s¹⁹.
pub fn det_ln(x: f64) -> Result<f64, DetMathError> {
    if !x.is_finite() || x <= 0.0 {
        return Err(DetMathError::OutOfDomain);
    }
    let bits = x.to_bits();
    if bits < (1_u64 << 52) {
        // Subnormal: outside the simulator's value domain; fail closed.
        return Err(DetMathError::OutOfDomain);
    }
    let mut e = (((bits >> 52) & 0x7FF) as i64) - 1023;
    let mut m = f64::from_bits((bits & 0x000F_FFFF_FFFF_FFFF) | (1023_u64 << 52));
    const SQRT2: f64 = 1.414_213_562_373_095_1;
    if m > SQRT2 {
        m *= 0.5;
        e += 1;
    }
    let s = (m - 1.0) / (m + 1.0);
    let t = s * s;
    // 2·(s + s³/3 + … + s¹⁹/19); |s| ≤ 0.1716 so the tail is < 1e-16.
    let mut sum = 0.0_f64;
    let mut power = s;
    let mut odd = 1.0_f64;
    while odd <= 19.0 {
        sum += power / odd * 2.0;
        power *= t;
        odd += 2.0;
    }
    let ef = e as f64;
    Ok(ef * LN2_HI + (sum + ef * LN2_LO))
}

/// Quantize a non-negative f64 into integer units of `scale` with the §4.15
/// floor-half-up convention: `floor(v·scale + 1/2)`. `floor` and the cast are
/// exact/deterministic. The value domain is capped so the cast cannot saturate.
pub fn quantize_scaled(v: f64, scale: f64) -> Result<i64, DetMathError> {
    if !v.is_finite() || !scale.is_finite() || scale <= 0.0 {
        return Err(DetMathError::OutOfDomain);
    }
    let scaled = v * scale + 0.5;
    if !(-9.0e18..=9.0e18).contains(&scaled) {
        return Err(DetMathError::OutOfDomain);
    }
    Ok(scaled.floor() as i64)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ok<T, E: std::fmt::Debug>(r: Result<T, E>) -> T {
        match r {
            Ok(v) => v,
            Err(e) => unreachable!("det_math test setup failed: {e:?}"),
        }
    }

    fn assert_close(got: f64, want: f64, rel: f64) {
        let scale = want.abs().max(1e-300);
        let err = ((got - want) / scale).abs();
        assert!(
            err < rel,
            "got {got:e}, want {want:e}, rel err {err:e} ≥ {rel:e}"
        );
    }

    #[test]
    fn det_exp_matches_reference_within_tolerance() {
        // Reference values are decimal literals (not libm calls) so this test
        // itself contains no platform-dependent math.
        let cases: [(f64, f64); 7] = [
            (0.0, 1.0),
            (1.0, 2.718_281_828_459_045_2),
            (-1.0, 0.367_879_441_171_442_33),
            (0.5, 1.648_721_270_700_128_2),
            (-0.031_25, 0.969_233_234_476_344_3),
            (10.0, 22_026.465_794_806_718),
            (-20.0, 2.061_153_622_438_557_8e-9),
        ];
        for (x, want) in cases {
            assert_close(ok(det_exp(x)), want, 1e-14);
        }
    }

    #[test]
    fn det_ln_matches_reference_within_tolerance() {
        let cases: [(f64, f64); 6] = [
            (1.0, 0.0),
            (2.0, 0.693_147_180_559_945_3),
            (0.5, -0.693_147_180_559_945_3),
            (2.718_281_828_459_045_2, 1.0),
            (5_000.0, 8.517_193_191_416_238),
            (1.0e-6, -13.815_510_557_964_274),
        ];
        for (x, want) in cases {
            assert_close(ok(det_ln(x)), want, 1e-14);
        }
    }

    #[test]
    fn exp_ln_roundtrip() {
        let xs = [-30.0, -2.5, -0.1, 0.0, 0.1, 1.0, 8.517, 40.0, 300.0];
        for x in xs {
            let y = ok(det_exp(x));
            let back = ok(det_ln(y));
            if x == 0.0 {
                assert_eq!(back, 0.0, "ln(exp(0)) must be exactly zero");
            } else {
                assert_close(back, x, 1e-12);
            }
        }
    }

    #[test]
    fn domains_fail_closed() {
        assert!(matches!(det_exp(709.0), Err(DetMathError::OutOfDomain)));
        assert!(matches!(det_exp(f64::NAN), Err(DetMathError::OutOfDomain)));
        assert!(matches!(det_ln(0.0), Err(DetMathError::OutOfDomain)));
        assert!(matches!(det_ln(-1.0), Err(DetMathError::OutOfDomain)));
        assert!(matches!(det_ln(f64::NAN), Err(DetMathError::OutOfDomain)));
        assert!(matches!(det_ln(5.0e-324), Err(DetMathError::OutOfDomain)));
        assert!(matches!(
            quantize_scaled(1.0e30, 1.0e30),
            Err(DetMathError::OutOfDomain)
        ));
    }

    #[test]
    fn quantize_scaled_is_floor_half_up() {
        assert_eq!(ok(quantize_scaled(0.75, 100.0)), 75);
        assert_eq!(ok(quantize_scaled(0.005, 100.0)), 1); // exactly half → up
        assert_eq!(ok(quantize_scaled(0.004_9, 100.0)), 0);
        assert_eq!(ok(quantize_scaled(123.456_789, 1_000_000.0)), 123_456_789);
    }

    /// Bit-exactness anchor (BXS-I-12): the cross-platform golden bits. CI on
    /// other targets must reproduce them exactly; a mismatch means a
    /// non-deterministic operation crept in (BXS-W-02).
    ///
    /// These literals are MEASURED output of this implementation, not the
    /// correctly-rounded mathematical values: `det_exp` is deliberately 1–2
    /// ULP off libm (e.g. exp(1) here ends …768, libm ends …769) because it
    /// buys reproducibility instead of ulp-perfection. Accuracy is the job of
    /// `det_exp_matches_reference_within_tolerance`; this test's only job is
    /// drift detection, so NEVER "fix" a mismatch by re-measuring — a changed
    /// bit means the arithmetic changed and needs a SPEC-level decision.
    #[test]
    fn golden_bits_pinned() {
        let exp_cases: [(f64, u64); 4] = [
            (1.0, 0x4005_BF0A_8B14_5768),
            (-0.5, 0x3FE3_68B2_FC6F_960C),
            (0.062_5, 0x3FF1_082B_577D_34EE),
            (100.0, 0x48F3_494A_9B17_1BF3),
        ];
        for (x, want) in exp_cases {
            assert_eq!(ok(det_exp(x)).to_bits(), want, "det_exp({x}) bits drifted");
        }
        let ln_cases: [(f64, u64); 3] = [
            (2.0, 0x3FE6_2E42_FEFA_39EF),
            (0.5, 0xBFE6_2E42_FEFA_39EF),
            (5_000.0, 0x4021_08CD_8BC5_B177),
        ];
        for (x, want) in ln_cases {
            assert_eq!(ok(det_ln(x)).to_bits(), want, "det_ln({x}) bits drifted");
        }
    }
}
