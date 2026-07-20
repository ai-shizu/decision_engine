//! M14–M15 analytics: Gap / Tensor / Psychometrics (deterministic, offline).
//!
//! Ports the non-negotiable contracts from `core/gap_analysis.py`,
//! `core/tensor_profile.py`, romance pulse, Dynamic Ordinal Rasch, and PROBE.
//! LLM is never an authority for scores — discovery is code-only; [`prompt`]
//! only builds languageization text.

pub mod commands;
pub mod commands_psychometrics;
pub mod gap;
pub mod input;
pub mod nonlinear;
pub mod probe;
pub mod prompt;
pub mod rasch;
pub mod romance_pulse;
pub mod taxonomy;
pub mod tensor;
