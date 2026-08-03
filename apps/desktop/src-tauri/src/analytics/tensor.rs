//! 6D Tensor Profile (`tensor_profile.6d.v1`).
//!
//! Authoritative production profile is always N/A — LLM evidence must not
//! update authority state (AI_SKILLS §5.10 / FSA-05). Aggregation helpers are
//! retained as pure functions for future code-derived rubrics.

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

pub const TENSOR_SCHEMA: &str = "tensor_profile.6d.v1";

pub const DIMENSION_IDS: &[&str] = &[
    "problem_structuring",
    "quantitative_rigor",
    "hypothesis_evidence",
    "synthesis_judgment",
    "communication",
    "collaboration_adaptability",
];

pub const CALCULUS_AXIS: &[&str] = &[
    "Structural_Decomposition",
    "Quantitative_Agility",
    "Logical_Rigor",
    "Domain_Adaptability",
    "Communication_Bandwidth",
    "Cognitive_Flexibility",
];

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct DimensionScore {
    pub dimension_id: String,
    pub calculus_axis: String,
    /// `null` means N/A (no deterministic observer yet).
    pub score: Option<f64>,
    pub confidence: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct TensorProfile {
    pub schema: String,
    pub model_hash: String,
    pub dimensions: Vec<DimensionScore>,
    pub evidence: Vec<Value>,
}

/// Authoritative profile: all scores N/A until a code-only rubric exists.
pub fn authoritative_profile() -> TensorProfile {
    let dimensions = DIMENSION_IDS
        .iter()
        .zip(CALCULUS_AXIS.iter())
        .map(|(id, axis)| DimensionScore {
            dimension_id: (*id).to_string(),
            calculus_axis: (*axis).to_string(),
            score: None,
            confidence: 0.0,
        })
        .collect();
    TensorProfile {
        schema: TENSOR_SCHEMA.into(),
        model_hash: "no-llm-authority".into(),
        dimensions,
        evidence: Vec::new(),
    }
}

pub fn tensor_to_json(profile: &TensorProfile) -> Value {
    json!(profile)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn authoritative_is_all_na() {
        let p = authoritative_profile();
        assert_eq!(p.dimensions.len(), 6);
        assert!(p.dimensions.iter().all(|d| d.score.is_none()));
        assert_eq!(p.model_hash, "no-llm-authority");
    }
}
