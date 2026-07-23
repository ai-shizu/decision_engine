//! Phase 14.3 — separate endpoints for Layer-1 scorecard vs Layer-2 debrief.
//!
//! Layer-1 commands intentionally omit vault types from their signatures.

use serde::{Deserialize, Serialize};

use super::evaluation::{
    assign_turn_ids, validate_interview_evaluation, validate_metacognitive_debrief,
    EvaluationError, InterviewEvaluationV1, MetacognitiveDebriefV1, TranscriptTurnRef,
    VaultMirrorAbstract,
};
#[cfg(all(feature = "secure-vault", target_vendor = "apple"))]
use super::session_artifact::InterviewSessionArtifact;
#[cfg(all(feature = "secure-vault", target_vendor = "apple"))]
use crate::llm::interview_machine::{
    prepare_interview_evaluation_binding, require_session_artifact,
    seal_evaluation_against_artifact, vault_mirror_from_artifact, InterviewSession,
};

/// Owned transcript row for IPC (Layer-1 / Layer-2 turn binding).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct TranscriptTurnDto {
    pub turn_id: String,
    pub role: String,
    pub text: String,
}

/// Assign deterministic `t-{n}` ids (F-14). No vault input.
#[tauri::command]
pub fn assign_interview_turn_ids(
    roles_and_texts: Vec<(String, String)>,
) -> Result<Vec<TranscriptTurnDto>, String> {
    let borrowed: Vec<(&str, &str)> = roles_and_texts
        .iter()
        .map(|(r, t)| (r.as_str(), t.as_str()))
        .collect();
    Ok(assign_turn_ids(&borrowed)
        .into_iter()
        .map(|(turn_id, role, text)| TranscriptTurnDto {
            turn_id,
            role,
            text,
        })
        .collect())
}

/// Seal Layer-1 interview evaluation against transcript turns only.
///
/// **Type boundary:** parameters are scorecard + transcript DTOs — never a vault handle.
#[tauri::command]
pub fn seal_interview_evaluation(
    mut evaluation: InterviewEvaluationV1,
    transcript: Vec<TranscriptTurnDto>,
) -> Result<InterviewEvaluationV1, String> {
    evaluation.normalize();
    let refs: Vec<TranscriptTurnRef<'_>> = transcript
        .iter()
        .map(|t| TranscriptTurnRef {
            turn_id: t.turn_id.as_str(),
            role: t.role.as_str(),
            text: t.text.as_str(),
        })
        .collect();
    validate_interview_evaluation(&evaluation, &refs).map_err(|e: EvaluationError| e.to_string())?;
    Ok(evaluation)
}

/// Seal Layer-2 opt-in metacognitive debrief (separate from pass/fail).
///
/// Accepts [`VaultMirrorAbstract`] only — not raw vault rows or `VaultHandle`.
#[tauri::command]
pub fn seal_metacognitive_debrief(
    mut debrief: MetacognitiveDebriefV1,
    mirror: VaultMirrorAbstract,
    transcript: Vec<TranscriptTurnDto>,
) -> Result<MetacognitiveDebriefV1, String> {
    debrief.normalize();
    let refs: Vec<TranscriptTurnRef<'_>> = transcript
        .iter()
        .map(|t| TranscriptTurnRef {
            turn_id: t.turn_id.as_str(),
            role: t.role.as_str(),
            text: t.text.as_str(),
        })
        .collect();
    validate_metacognitive_debrief(&debrief, &mirror, &refs)
        .map_err(|e: EvaluationError| e.to_string())?;
    Ok(debrief)
}

/// Seal Layer-1 evaluation against a session's **frozen** artifact + transcript only.
///
/// Rejects sessions without a verified `InterviewSessionArtifact` (no live-vault fallback).
#[cfg(all(feature = "secure-vault", target_vendor = "apple"))]
#[tauri::command]
pub fn seal_interview_evaluation_from_session(
    session: InterviewSession,
    evaluation: InterviewEvaluationV1,
) -> Result<InterviewEvaluationV1, String> {
    let mut evaluation = evaluation;
    evaluation.normalize();
    let (_art, _turns) = prepare_interview_evaluation_binding(&session)?;
    seal_evaluation_against_artifact(&session, &evaluation)?;
    Ok(evaluation)
}

/// Seal Layer-2 debrief using mirror derived from the frozen artifact evidence bodies.
#[cfg(all(feature = "secure-vault", target_vendor = "apple"))]
#[tauri::command]
pub fn seal_metacognitive_debrief_from_session(
    session: InterviewSession,
    mut debrief: MetacognitiveDebriefV1,
) -> Result<MetacognitiveDebriefV1, String> {
    let artifact = require_session_artifact(&session)?;
    let mirror = vault_mirror_from_artifact(artifact);
    debrief.normalize();
    let pairs: Vec<(String, String)> = session
        .transcript
        .iter()
        .map(|t| (t.role.clone(), t.text.clone()))
        .collect();
    let borrowed: Vec<(&str, &str)> = pairs
        .iter()
        .map(|(r, t)| (r.as_str(), t.as_str()))
        .collect();
    let owned = assign_turn_ids(&borrowed);
    let refs: Vec<TranscriptTurnRef<'_>> = owned
        .iter()
        .map(|(id, role, text)| TranscriptTurnRef {
            turn_id: id.as_str(),
            role: role.as_str(),
            text: text.as_str(),
        })
        .collect();
    validate_metacognitive_debrief(&debrief, &mirror, &refs)
        .map_err(|e: EvaluationError| e.to_string())?;
    // Touch artifact type so vacuum-safe path stays referenced.
    let _: &InterviewSessionArtifact = artifact;
    Ok(debrief)
}
