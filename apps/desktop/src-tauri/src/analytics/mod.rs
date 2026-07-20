//! M14–M16 analytics: Gap / Tensor / Psychometrics / Twin / Oracle.
//!
//! Deterministic only — LLM never updates scores or interventions.
//! Echo sterile payload: [`oracle`]. State equation: [`digital_twin`].

pub mod commands;
pub mod commands_oracle;
pub mod commands_psychometrics;
pub mod coupling;
pub mod digital_twin;
pub mod gap;
pub mod input;
pub mod nonlinear;
pub mod oracle;
pub mod probe;
pub mod prompt;
pub mod rasch;
pub mod romance_pulse;
pub mod taxonomy;
pub mod tensor;
