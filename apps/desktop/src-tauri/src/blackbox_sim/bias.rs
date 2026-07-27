//! Bias axes, N/A semantics, and the calibration type-gate (SPEC §10–§12).
//!
//! Every estimate is micro-quantized (×1e6, floor-half-up — the §4.15
//! quantization). `value_micro = None` is the ONLY representation of "not
//! measured": fabricating 0 for missing data is lying (LAW-20 / BXS-I-09).
//!
//! The 6D projection surface is type-gated: `authoritative_projection`
//! requires a `CalibrationCertificate`, which has NO production constructor
//! until Phase 6 lands the verified known-answer path AND the Commander
//! adjudicates (SPEC §11 double gate). Until then the only reachable output
//! is `uncalibrated_projection()` — all N/A, mirroring
//! `analytics::tensor::authoritative_profile`'s FSA-05 posture (that module
//! is intentionally NOT imported — wall W-b keeps this crate-corner free of
//! profile dependencies).

use serde::{Deserialize, Serialize};

use super::money::{floor_half_up, MoneyError, MICRO};

/// Lane numbers frozen forever (BXS-I-13, PKBTEN01 discipline):
/// never renumber, never reuse a retired lane. Additions append only.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[repr(u8)]
pub enum BiasAxis {
    LossAversion = 0,
    DispositionEffect = 1,
    Anchoring = 2,
    Overconfidence = 3,
    EscalationCommitment = 4,
    PressureDegradation = 5,
}

pub const N_AXES: usize = 6;

impl BiasAxis {
    pub const ALL: [BiasAxis; N_AXES] = [
        BiasAxis::LossAversion,
        BiasAxis::DispositionEffect,
        BiasAxis::Anchoring,
        BiasAxis::Overconfidence,
        BiasAxis::EscalationCommitment,
        BiasAxis::PressureDegradation,
    ];

    #[must_use]
    pub const fn lane(self) -> u8 {
        self as u8
    }

    #[inline]
    const fn index(self) -> usize {
        self as usize
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BiasError {
    SufficiencyOutOfRange { got_micro: i64 },
    MeasuredWithoutObservations,
    Quantization(MoneyError),
}

/// One micro-quantized estimate with its own evidence accounting.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BiasEstimate {
    /// None = not measured (the honest N/A). Never coerce to 0.
    pub value_micro: Option<i64>,
    pub n_obs: u32,
    /// Data sufficiency in [0, 1e6] — the §6.2-4 data_sufficiency discipline
    /// carried into this instrument.
    pub sufficiency_micro: i64,
}

impl BiasEstimate {
    pub const NOT_MEASURED: BiasEstimate = BiasEstimate {
        value_micro: None,
        n_obs: 0,
        sufficiency_micro: 0,
    };

    pub fn measured(
        value_micro: i64,
        n_obs: u32,
        sufficiency_micro: i64,
    ) -> Result<Self, BiasError> {
        if n_obs == 0 {
            return Err(BiasError::MeasuredWithoutObservations);
        }
        if !(0..=MICRO).contains(&sufficiency_micro) {
            return Err(BiasError::SufficiencyOutOfRange {
                got_micro: sufficiency_micro,
            });
        }
        Ok(Self {
            value_micro: Some(value_micro),
            n_obs,
            sufficiency_micro,
        })
    }
}

/// The shared confidence gate every estimator must pass through (SPEC §10):
/// below `min_n` observations the answer is NOT_MEASURED — never a fabricated
/// zero. At or above the gate, sufficiency = min(1, n / (2·min_n)) in micro
/// units (floor-half-up).
pub fn gate_estimate(value_micro: i64, n_obs: u32, min_n: u32) -> Result<BiasEstimate, BiasError> {
    if min_n == 0 {
        // A zero gate would let a single anecdote brand a human (LAW-21).
        return Err(BiasError::MeasuredWithoutObservations);
    }
    if n_obs < min_n {
        return Ok(BiasEstimate::NOT_MEASURED);
    }
    let num = i128::from(n_obs)
        .checked_mul(i128::from(MICRO))
        .ok_or(BiasError::Quantization(MoneyError::Overflow))?;
    let den = i128::from(min_n)
        .checked_mul(2)
        .ok_or(BiasError::Quantization(MoneyError::Overflow))?;
    let raw = floor_half_up(num, den).map_err(BiasError::Quantization)?;
    let sufficiency = raw.min(i128::from(MICRO));
    let sufficiency_micro =
        i64::try_from(sufficiency).map_err(|_| BiasError::Quantization(MoneyError::Overflow))?;
    BiasEstimate::measured(value_micro, n_obs, sufficiency_micro)
}

pub const SCHEMA_BLACKBOX_PROFILE_V1: &str = "blackbox_profile.v1";
pub const SCHEMA_BLACKBOX_PROJECTION_6D_V1: &str = "blackbox_projection.6d.v1";
pub const INSTRUMENT_ID: &str = "blackbox_sim";
pub const CALIBRATION_UNCALIBRATED: &str = "uncalibrated-instrument";

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BlackboxProfile {
    pub schema: &'static str,
    /// Truncated fingerprint of the CampaignGenesis this profile derives from.
    pub campaign_digest: [u8; 8],
    pub axes: [BiasEstimate; N_AXES],
}

impl BlackboxProfile {
    #[must_use]
    pub fn empty(campaign_digest: [u8; 8]) -> Self {
        Self {
            schema: SCHEMA_BLACKBOX_PROFILE_V1,
            campaign_digest,
            axes: [BiasEstimate::NOT_MEASURED; N_AXES],
        }
    }

    #[must_use]
    pub fn axis(&self, axis: BiasAxis) -> BiasEstimate {
        self.axes
            .get(axis.index())
            .copied()
            .unwrap_or(BiasEstimate::NOT_MEASURED)
    }

    pub fn set_axis(&mut self, axis: BiasAxis, estimate: BiasEstimate) {
        if let Some(slot) = self.axes.get_mut(axis.index()) {
            *slot = estimate;
        }
    }
}

/// Proof token that the known-answer calibration suite passed (SPEC §12).
/// Deliberately impossible to construct in production code in Phase 0–5:
/// the only constructor is `#[cfg(test)]`. Phase 6 introduces the verified
/// production path AFTER calibration is GREEN and the Commander adjudicates.
#[derive(Debug)]
pub struct CalibrationCertificate {
    _sealed: (),
}

impl CalibrationCertificate {
    #[cfg(test)]
    pub(crate) fn test_only() -> Self {
        Self { _sealed: () }
    }
}

/// Projection onto the interview 6D dimension order (SPEC §11). This is a
/// SEPARATE instrument surface — provenance-tagged, never merged silently
/// into `tensor_profile.6d.v1` (construct purity, §4.59).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Projection6D {
    pub schema: &'static str,
    pub instrument: &'static str,
    /// Canonical interview-dimension order (problem_structuring,
    /// quantitative_rigor, hypothesis_evidence, synthesis_judgment,
    /// communication, collaboration_adaptability). Entries with no
    /// deterministic observer stay None forever from this instrument.
    pub scores_micro: [Option<i64>; 6],
    pub calibration: &'static str,
}

/// The only projection reachable without a certificate: all N/A.
#[must_use]
pub fn uncalibrated_projection() -> Projection6D {
    Projection6D {
        schema: SCHEMA_BLACKBOX_PROJECTION_6D_V1,
        instrument: INSTRUMENT_ID,
        scores_micro: [None; 6],
        calibration: CALIBRATION_UNCALIBRATED,
    }
}

/// Certificate-gated projection. Phase 6 defines the projection weights AFTER
/// calibration data exists (LAW-19: freeze structure, not speculative
/// constants) — until then even the gated path yields the all-N/A surface.
#[must_use]
pub fn authoritative_projection(
    _profile: &BlackboxProfile,
    _certificate: &CalibrationCertificate,
) -> Projection6D {
    uncalibrated_projection()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lane_numbers_frozen() {
        assert_eq!(BiasAxis::LossAversion.lane(), 0);
        assert_eq!(BiasAxis::DispositionEffect.lane(), 1);
        assert_eq!(BiasAxis::Anchoring.lane(), 2);
        assert_eq!(BiasAxis::Overconfidence.lane(), 3);
        assert_eq!(BiasAxis::EscalationCommitment.lane(), 4);
        assert_eq!(BiasAxis::PressureDegradation.lane(), 5);
        assert_eq!(BiasAxis::ALL.len(), N_AXES);
    }

    #[test]
    fn insufficient_is_none_not_zero() {
        let e = match gate_estimate(123_456, 5, 12) {
            Ok(v) => v,
            Err(err) => unreachable!("gate failed: {err:?}"),
        };
        assert_eq!(e, BiasEstimate::NOT_MEASURED);
        assert_eq!(e.value_micro, None, "missing data must stay N/A (LAW-20)");
    }

    #[test]
    fn gate_at_min_measures_with_half_sufficiency() {
        let e = match gate_estimate(-250_000, 12, 12) {
            Ok(v) => v,
            Err(err) => unreachable!("gate failed: {err:?}"),
        };
        assert_eq!(e.value_micro, Some(-250_000));
        assert_eq!(e.n_obs, 12);
        assert_eq!(e.sufficiency_micro, 500_000); // 12/(2·12) = 0.5
        let full = match gate_estimate(1, 24, 12) {
            Ok(v) => v,
            Err(err) => unreachable!("gate failed: {err:?}"),
        };
        assert_eq!(full.sufficiency_micro, MICRO); // capped at 1.0
    }

    #[test]
    fn zero_min_gate_is_rejected() {
        assert!(matches!(
            gate_estimate(0, 100, 0),
            Err(BiasError::MeasuredWithoutObservations)
        ));
    }

    #[test]
    fn measured_validates_ranges() {
        assert!(matches!(
            BiasEstimate::measured(0, 0, 0),
            Err(BiasError::MeasuredWithoutObservations)
        ));
        assert!(matches!(
            BiasEstimate::measured(0, 1, MICRO + 1),
            Err(BiasError::SufficiencyOutOfRange { .. })
        ));
        assert!(matches!(
            BiasEstimate::measured(0, 1, -1),
            Err(BiasError::SufficiencyOutOfRange { .. })
        ));
    }

    #[test]
    fn empty_profile_is_all_not_measured() {
        let p = BlackboxProfile::empty([9; 8]);
        assert_eq!(p.schema, SCHEMA_BLACKBOX_PROFILE_V1);
        for axis in BiasAxis::ALL {
            assert_eq!(p.axis(axis), BiasEstimate::NOT_MEASURED);
        }
    }

    #[test]
    fn uncalibrated_projection_is_all_na() {
        let proj = uncalibrated_projection();
        assert_eq!(proj.schema, SCHEMA_BLACKBOX_PROJECTION_6D_V1);
        assert_eq!(proj.instrument, INSTRUMENT_ID);
        assert_eq!(proj.calibration, CALIBRATION_UNCALIBRATED);
        assert!(proj.scores_micro.iter().all(Option::is_none));
    }

    #[test]
    fn certificate_gated_projection_stays_na_in_phase0() {
        // Even holding a (test-only) certificate, Phase 0 has no projection
        // weights: the surface must remain all-N/A until Phase 6 (SPEC §11).
        let cert = CalibrationCertificate::test_only();
        let mut profile = BlackboxProfile::empty([1; 8]);
        let measured = match gate_estimate(300_000, 20, 10) {
            Ok(v) => v,
            Err(err) => unreachable!("gate failed: {err:?}"),
        };
        profile.set_axis(BiasAxis::LossAversion, measured);
        let proj = authoritative_projection(&profile, &cert);
        assert!(proj.scores_micro.iter().all(Option::is_none));
    }
}
