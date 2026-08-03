//! Tauri commands for CBT cognitive-bias fingerprint (Phase 8).
//!
//! Recording accepts a grammar-shaped report (from `llm_generate` with
//! `task_id=cognitive_distortion_v1`). Profiling is deterministic aggregation
//! over vault `distortion_tags` — no RNG, no LLM.
//!
//! # Academic grounding
//!
//! **CBT** — Beck (1976) & Burns (1980): ten cognitive distortions fingerprint
//! the user's habitual thought patterns. Labels are never invented here; only
//! vault rows whose `category` matches the Burns allowlist are counted.

use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use tauri::State;

use crate::analytics::bias_profile::{
    aggregate_bias_profile, CognitiveBiasProfile, BURNS_CATEGORIES,
};
use crate::db::{DistortionTagRow, VaultErrorCode, VaultHandle};

const MAX_TEXT_BYTES: usize = 64 * 1024;
const MAX_TAGS_LIMIT: u32 = 10_000;
const DEFAULT_TAGS_LIMIT: u32 = 5_000;

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DistortionDetectionIn {
    pub category: String,
    pub snippet: String,
    pub confidence_score: f64,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CognitiveDistortionReportIn {
    pub detected_distortions: Vec<DistortionDetectionIn>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RecordCognitiveDistortionsParams {
    pub report: CognitiveDistortionReportIn,
    pub source_kind: String,
    pub source_id: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct RecordCognitiveDistortionsResult {
    pub run_id: String,
    pub inserted: usize,
}

fn map_vault(err: VaultErrorCode) -> String {
    format!("{err:?}").to_ascii_lowercase()
}

fn now_unix() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

fn validate_source(kind: &str, id: &str) -> Result<(), String> {
    let kind = kind.trim();
    let id = id.trim();
    if kind.is_empty() || kind.len() > 64 {
        return Err("invalid source_kind".into());
    }
    if id.is_empty() || id.len() > 512 {
        return Err("invalid source_id".into());
    }
    Ok(())
}

fn is_burns_category(s: &str) -> bool {
    BURNS_CATEGORIES.iter().any(|c| *c == s)
}

/// Persist a CBT extraction report into `distortion_tags` (one row per detection).
#[tauri::command]
pub async fn record_cognitive_distortions(
    vault: State<'_, VaultHandle>,
    params: RecordCognitiveDistortionsParams,
) -> Result<RecordCognitiveDistortionsResult, String> {
    validate_source(&params.source_kind, &params.source_id)?;
    if params.report.detected_distortions.len() > 32 {
        return Err("too many detections".into());
    }

    let mut cleaned: Vec<DistortionDetectionIn> = Vec::new();
    for mut d in params.report.detected_distortions {
        d.category = d.category.trim().to_string();
        d.snippet = d.snippet.trim().to_string();
        if d.snippet.is_empty() || d.snippet == "unknown" {
            continue;
        }
        if !is_burns_category(&d.category) {
            return Err(format!("invalid category: {}", d.category));
        }
        if d.snippet.len() > MAX_TEXT_BYTES {
            return Err("snippet too large".into());
        }
        if !d.confidence_score.is_finite() {
            d.confidence_score = 0.0;
        }
        d.confidence_score = d.confidence_score.clamp(0.0, 1.0);
        cleaned.push(d);
    }

    let vault = vault.inner().clone();
    let source_kind = params.source_kind.trim().to_string();
    let source_id = params.source_id.trim().to_string();
    let created_at = now_unix();
    let run_id = format!("cbt-{created_at}");

    tauri::async_runtime::spawn_blocking(move || {
        let mut rows = Vec::with_capacity(cleaned.len());
        for (i, d) in cleaned.iter().enumerate() {
            rows.push(DistortionTagRow {
                id: format!("{run_id}-{i}"),
                created_at,
                category: d.category.clone(),
                snippet: d.snippet.clone(),
                confidence_score: d.confidence_score,
                source_kind: source_kind.clone(),
                source_id: source_id.clone(),
                run_id: run_id.clone(),
            });
        }
        let inserted = if rows.is_empty() {
            0
        } else {
            vault.distortion_tags_insert(rows).map_err(map_vault)?
        };
        Ok(RecordCognitiveDistortionsResult { run_id, inserted })
    })
    .await
    .map_err(|_| "record_cognitive_distortions join failed".to_string())?
}

/// Deterministic category fingerprint over accumulated distortion tags.
#[tauri::command]
pub async fn get_cognitive_bias_profile(
    vault: State<'_, VaultHandle>,
    limit: Option<u32>,
) -> Result<CognitiveBiasProfile, String> {
    let limit = limit.unwrap_or(DEFAULT_TAGS_LIMIT).clamp(1, MAX_TAGS_LIMIT);
    let vault = vault.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let rows = vault.distortion_tags_list(limit).map_err(map_vault)?;
        Ok(aggregate_bias_profile(&rows, now_unix()))
    })
    .await
    .map_err(|_| "get_cognitive_bias_profile join failed".to_string())?
}
