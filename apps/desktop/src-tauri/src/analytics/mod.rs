//! M14 analytics: Gap Analysis + Tensor Profile (deterministic, offline).
//!
//! Ports the non-negotiable contracts from `core/gap_analysis.py` and
//! `core/tensor_profile.py`. LLM is never an authority for scores — discovery
//! is code-only; [`prompt`] only builds languageization text.

pub mod commands;
pub mod gap;
pub mod input;
pub mod nonlinear;
pub mod prompt;
pub mod taxonomy;
pub mod tensor;
