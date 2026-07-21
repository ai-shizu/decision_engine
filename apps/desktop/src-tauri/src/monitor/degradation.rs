//! Progressive degradation ladder for thermal / memory pressure (Phase 4).
//!
//! Pure combine logic — no OS calls. Severity is monotonic: the highest signal wins.

use serde::Serialize;

/// Ladder rungs shared with MemSample / LlmLifecycleEvent / governor atomics.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
#[repr(u8)]
pub enum DegradationLevel {
    Nominal = 0,
    Fair = 1,
    Serious = 2,
    Critical = 3,
}

impl DegradationLevel {
    pub fn as_u8(self) -> u8 {
        self as u8
    }

    pub fn from_u8(v: u8) -> Self {
        match v {
            0 => Self::Nominal,
            1 => Self::Fair,
            2 => Self::Serious,
            _ => Self::Critical,
        }
    }

    /// Suppress RAG embed re-warm / non-essential background LLM work.
    pub fn suppress_background(self) -> bool {
        self >= Self::Fair
    }

    /// Shrink generation context and surface a FE warning.
    pub fn throttle_generation(self) -> bool {
        self >= Self::Serious
    }

    /// Context window multiplier applied under Serious/Critical.
    pub fn context_factor(self) -> f32 {
        match self {
            Self::Nominal | Self::Fair => 1.0,
            Self::Serious => 0.5,
            Self::Critical => 0.25,
        }
    }
}

/// Footprint ratio bands relative to the jetsam threshold.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FootprintBand {
    Comfort,
    Elevated,
    High,
    Over,
}

impl FootprintBand {
    pub fn from_ratio(ratio: f64) -> Self {
        if ratio >= 1.0 {
            Self::Over
        } else if ratio >= 0.85 {
            Self::High
        } else if ratio >= 0.70 {
            Self::Elevated
        } else {
            Self::Comfort
        }
    }

    pub fn as_level(self) -> DegradationLevel {
        match self {
            Self::Comfort => DegradationLevel::Nominal,
            Self::Elevated => DegradationLevel::Fair,
            Self::High => DegradationLevel::Serious,
            Self::Over => DegradationLevel::Critical,
        }
    }
}

/// OS memory-pressure class (Apple dispatch flags mapped; others use None).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PressureClass {
    Normal,
    Warn,
    Critical,
}

impl PressureClass {
    pub fn as_level(self) -> DegradationLevel {
        match self {
            Self::Normal => DegradationLevel::Nominal,
            Self::Warn => DegradationLevel::Fair,
            Self::Critical => DegradationLevel::Critical,
        }
    }
}

/// Combine thermal, pressure, and footprint into a single ladder rung (max severity).
pub fn combine_degradation(
    thermal: DegradationLevel,
    pressure: PressureClass,
    footprint_ratio: f64,
) -> DegradationLevel {
    let footprint = FootprintBand::from_ratio(footprint_ratio).as_level();
    let pressure = pressure.as_level();
    thermal.max(pressure).max(footprint)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn combine_takes_max_severity() {
        assert_eq!(
            combine_degradation(
                DegradationLevel::Nominal,
                PressureClass::Normal,
                0.5
            ),
            DegradationLevel::Nominal
        );
        assert_eq!(
            combine_degradation(DegradationLevel::Fair, PressureClass::Normal, 0.5),
            DegradationLevel::Fair
        );
        assert_eq!(
            combine_degradation(
                DegradationLevel::Nominal,
                PressureClass::Warn,
                0.5
            ),
            DegradationLevel::Fair
        );
        assert_eq!(
            combine_degradation(
                DegradationLevel::Nominal,
                PressureClass::Normal,
                0.9
            ),
            DegradationLevel::Serious
        );
        assert_eq!(
            combine_degradation(
                DegradationLevel::Serious,
                PressureClass::Critical,
                0.5
            ),
            DegradationLevel::Critical
        );
        assert_eq!(
            combine_degradation(
                DegradationLevel::Fair,
                PressureClass::Normal,
                1.05
            ),
            DegradationLevel::Critical
        );
    }

    #[test]
    fn context_factor_and_flags() {
        assert!(!DegradationLevel::Nominal.suppress_background());
        assert!(DegradationLevel::Fair.suppress_background());
        assert!(DegradationLevel::Serious.throttle_generation());
        assert!((DegradationLevel::Serious.context_factor() - 0.5).abs() < f32::EPSILON);
    }
}
