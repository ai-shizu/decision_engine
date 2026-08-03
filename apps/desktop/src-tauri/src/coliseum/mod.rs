//! Phase 14 — Inner Coliseum (interviewer / oni mode).
//!
//! I-22 asymmetric boundary + Phase 14.2 structural ZPD gate / circuit breaker
//! + Phase 14.3 two-layer evaluation + Phase 14.4 immutable session artifacts.

pub mod commands;
pub mod evaluation;
pub mod interviewer_context;
pub mod mentor_zpd;
pub mod render_guard;
pub mod session_artifact;
pub mod tactics;

pub use evaluation::{
    InterviewEvaluationV1, MetacognitiveDebriefV1, INTERVIEW_EVALUATION_V1_GBNF,
    METACOGNITIVE_DEBRIEF_V1_GBNF,
};
pub use interviewer_context::{
    compile_interviewer_tactics, CognitiveFossilSnapshot, OniModePrompt,
};
pub use mentor_zpd::{
    resolve_oni_activation, COLISEUM_GENERATION_SEED, COLISEUM_GENERATION_TEMP,
};
pub use render_guard::RenderGuardError;
pub use session_artifact::InterviewSessionArtifact;

/// Oni-mode: compile fossils to sealed prompt. Never takes a vault handle type.
pub fn build_oni_pressure_prompt(
    snapshot: CognitiveFossilSnapshot,
    public_brief: &str,
) -> Result<String, RenderGuardError> {
    let tactics = compile_interviewer_tactics(snapshot);
    OniModePrompt::render_sealed(&tactics, public_brief)
}

/// Replay oni pressure from a frozen artifact directive set (no live vault).
pub fn build_oni_pressure_prompt_from_artifact(
    artifact: &InterviewSessionArtifact,
    public_brief: &str,
) -> Result<String, RenderGuardError> {
    OniModePrompt::render_sealed(&artifact.frozen_directives, public_brief)
}
