//! Phase 14.2 — structural ZPD gate for 鬼モード entry (hard safety valve).
//!
//! Intensity is **not** controlled by LLM temperature. Depleted `R(t)` blocks or
//! downgrades oni mode in Rust before any prompt is built (F-14 / structural safety).

use std::fmt;

/// Quantized Twin R(t) on `[0, 100]` (round(R×100)). Matches `llm::mentor_zpd::R_DEPLETED` (0.40).
pub const ONI_R_T_THRESHOLD: u8 = 40;

/// Fixed generation temperature for coliseum / interview streams (hallucination floor).
/// Difficulty must never be expressed by raising this value — use `InterviewerTactic` instead.
pub const COLISEUM_GENERATION_TEMP: f32 = 0.1;

/// Fixed seed for reproducible oni / interview generation (F-14).
pub const COLISEUM_GENERATION_SEED: u32 = 14;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ZpdDepletionError {
    /// Cognitive resource at or below the structural floor — oni entry denied.
    CognitiveResourceDepleted { r_t: u8, threshold: u8 },
}

impl fmt::Display for ZpdDepletionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::CognitiveResourceDepleted { r_t, threshold } => write!(
                f,
                "oni_mode_blocked_zpd_depleted:r_t={r_t}:threshold={threshold}"
            ),
        }
    }
}

impl std::error::Error for ZpdDepletionError {}

/// Map unit-interval R(t) → quantized `r_t` (deterministic round).
pub fn r_t_from_unit_interval(r: f64) -> u8 {
    if !r.is_finite() {
        return 0;
    }
    let q = (r.clamp(0.0, 1.0) * 100.0).round();
    q.clamp(0.0, 100.0) as u8
}

/// Hard gate: oni mode is eligible iff quantized R exceeds the depletion floor.
pub fn evaluate_oni_mode_eligibility(r_t: u8) -> Result<(), ZpdDepletionError> {
    if r_t <= ONI_R_T_THRESHOLD {
        Err(ZpdDepletionError::CognitiveResourceDepleted {
            r_t,
            threshold: ONI_R_T_THRESHOLD,
        })
    } else {
        Ok(())
    }
}

/// Resolve whether oni stays active: `Ok(true)` active, `Ok(false)` forced downgrade.
pub fn resolve_oni_activation(oni_requested: bool, r_t: u8) -> Result<bool, ZpdDepletionError> {
    if !oni_requested {
        return Ok(false);
    }
    evaluate_oni_mode_eligibility(r_t)?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blocks_at_or_below_threshold() {
        assert!(evaluate_oni_mode_eligibility(0).is_err());
        assert!(evaluate_oni_mode_eligibility(ONI_R_T_THRESHOLD).is_err());
        assert!(evaluate_oni_mode_eligibility(ONI_R_T_THRESHOLD + 1).is_ok());
    }

    #[test]
    fn unit_interval_quantization() {
        assert_eq!(r_t_from_unit_interval(0.40), 40);
        assert_eq!(r_t_from_unit_interval(0.401), 40);
        assert_eq!(r_t_from_unit_interval(0.405), 41);
    }

    #[test]
    fn downgrade_when_requested_but_depleted() {
        assert_eq!(resolve_oni_activation(false, 10), Ok(false));
        assert!(resolve_oni_activation(true, 10).is_err());
        assert_eq!(resolve_oni_activation(true, 55), Ok(true));
    }

    #[test]
    fn coliseum_temp_is_hallucination_floor_not_difficulty() {
        assert!(COLISEUM_GENERATION_TEMP <= 0.1 + f32::EPSILON);
        assert_eq!(COLISEUM_GENERATION_SEED, 14);
    }
}
