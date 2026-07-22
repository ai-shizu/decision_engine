//! M12 interview / ES simulation Tauri commands.
//!
//! Retrieves personal RAG context (vault KNN), merges EDINET company facts
//! (injected offline or dual-gated live list fetch), builds prompts via
//! `prompt_sim`, then streams through the existing M6 `LlmHandle::generate`
//! Channel pipeline (M7 cancel/purge unchanged).

use std::time::Duration;

use serde::{Deserialize, Serialize};
use tauri::ipc::Channel;
use tauri::State;

use crate::db::{VaultErrorCode, VaultHandle};
use crate::knowledge::edinet_client::{
    sanitize_company_facts, subscription_key_from_env, CompanyFacts, EdinetError,
};
use crate::knowledge::{
    refuse_if_egress_unavailable, refuse_if_policy_off, NetworkPolicyStore,
};
use crate::llm::params::GenerationParams;
use crate::llm::prompt_sim::{build_es_review_prompt, build_interview_prompt, ExperienceRef};
use crate::llm::service::TokenEvent;
use crate::llm::LlmHandle;
use crate::rag::commands_rag::search_sync;

const MAX_TEXT_BYTES: usize = 64 * 1024;
const DEFAULT_CONTEXT_LIMIT: u32 = 5;
const MAX_CONTEXT_LIMIT: u32 = 20;
const DEFAULT_N_CTX: u32 = 2048;
const DEFAULT_MAX_TOKENS: u32 = 256;
const EDINET_FETCH_DEADLINE: Duration = Duration::from_secs(15);
/// How many calendar days before anchor to scan when resolving by filer name.
const EDINET_NAME_LOOKBACK_DAYS: u32 = 21;

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SimGenParams {
    pub n_ctx: Option<u32>,
    pub max_tokens: Option<u32>,
    pub temp: Option<f32>,
    pub top_k: Option<i32>,
    pub top_p: Option<f32>,
    pub seed: Option<u32>,
    pub context_limit: Option<u32>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StartInterviewParams {
    pub message: String,
    /// Offline / pre-fetched facts (preferred when egress is Off).
    pub company_facts: Option<CompanyFacts>,
    /// When set with Live∧egress-live, fetch documents.json for this code.
    pub edinet_code: Option<String>,
    /// Filing date YYYY-MM-DD for live list fetch.
    pub edinet_date: Option<String>,
    /// Optional UTF-8 yuho excerpt to merge risk/performance sections.
    pub filing_text: Option<String>,
    /// Optional ES body used as interview base (empty = zero-base).
    pub es_text: Option<String>,
    pub gen: Option<SimGenParams>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReviewEsParams {
    pub es_draft: String,
    pub company_facts: Option<CompanyFacts>,
    pub edinet_code: Option<String>,
    pub edinet_date: Option<String>,
    pub filing_text: Option<String>,
    /// Query used for RAG experience retrieval (defaults to es_draft prefix).
    pub experience_query: Option<String>,
    pub gen: Option<SimGenParams>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FetchEdinetFactsParams {
    /// Preferred: resolve EDINET code from filer name (UI never asks for code).
    pub company_name: Option<String>,
    /// Optional explicit code (legacy / internal). Prefer company_name.
    pub edinet_code: Option<String>,
    /// Filing date YYYY-MM-DD (anchor for list scan).
    pub edinet_date: Option<String>,
    pub filing_text: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct SimSessionResult {
    pub context_ids: Vec<String>,
    pub context_count: usize,
    pub company_name: String,
    pub facts_source: String,
}

fn map_vault_err(code: VaultErrorCode) -> String {
    format!("{code:?}").to_ascii_lowercase()
}

fn map_edinet_err(err: EdinetError) -> String {
    err.to_string()
}

fn resolve_gen(gen: Option<&SimGenParams>) -> (u32, GenerationParams) {
    let g = gen;
    let context_limit = g
        .and_then(|p| p.context_limit)
        .unwrap_or(DEFAULT_CONTEXT_LIMIT)
        .clamp(1, MAX_CONTEXT_LIMIT);
    let params = GenerationParams {
        prompt: String::new(),
        n_ctx: g.and_then(|p| p.n_ctx).unwrap_or(DEFAULT_N_CTX),
        max_tokens: g.and_then(|p| p.max_tokens).unwrap_or(DEFAULT_MAX_TOKENS),
        temp: g.and_then(|p| p.temp).unwrap_or(0.7),
        top_k: g.and_then(|p| p.top_k).unwrap_or(40),
        top_p: g.and_then(|p| p.top_p).unwrap_or(0.95),
        seed: g.and_then(|p| p.seed).unwrap_or(0),
    };
    (context_limit, params)
}

/// Multistage / oni interview: temperature is hallucination floor only; seed fixed (F-14).
/// FE `temp` cannot raise difficulty — intensity is tactic-driven when `oni_active`.
/// `turn_seed` comes from the frozen artifact's `per_turn_seeds` when present.
fn resolve_coliseum_gen(
    gen: Option<&SimGenParams>,
    turn_seed: Option<u32>,
) -> (u32, GenerationParams) {
    use crate::coliseum::{COLISEUM_GENERATION_SEED, COLISEUM_GENERATION_TEMP};
    let g = gen;
    let context_limit = g
        .and_then(|p| p.context_limit)
        .unwrap_or(DEFAULT_CONTEXT_LIMIT)
        .clamp(1, MAX_CONTEXT_LIMIT);
    let params = GenerationParams {
        prompt: String::new(),
        n_ctx: g.and_then(|p| p.n_ctx).unwrap_or(DEFAULT_N_CTX),
        max_tokens: g.and_then(|p| p.max_tokens).unwrap_or(DEFAULT_MAX_TOKENS),
        temp: COLISEUM_GENERATION_TEMP,
        top_k: g.and_then(|p| p.top_k).unwrap_or(40),
        top_p: g.and_then(|p| p.top_p).unwrap_or(0.95),
        seed: turn_seed.unwrap_or(COLISEUM_GENERATION_SEED),
    };
    (context_limit, params)
}

fn artifact_turn_seed(session: &InterviewSession) -> Option<u32> {
    session
        .session_artifact
        .as_ref()
        .map(|a| a.seed_for_turn(session.total_turns as usize))
}

async fn resolve_company_facts(
    store: &NetworkPolicyStore,
    injected: Option<CompanyFacts>,
    edinet_code: Option<String>,
    edinet_date: Option<String>,
    filing_text: Option<String>,
) -> Result<CompanyFacts, String> {
    let injected_sanitized = match injected {
        Some(facts) => Some(sanitize_company_facts(&facts).map_err(map_edinet_err)?),
        None => None,
    };

    let mut code = edinet_code.unwrap_or_default();
    if code.trim().is_empty() {
        if let Some(base) = injected_sanitized.as_ref() {
            code = base.edinet_code.clone();
        }
    }
    let date = edinet_date.unwrap_or_default();
    let lookup_name = injected_sanitized
        .as_ref()
        .map(|f| f.company_name.trim().to_string())
        .unwrap_or_default();

    let can_fetch_by_code = !code.trim().is_empty() && !date.trim().is_empty();
    let can_fetch_by_name = !lookup_name.is_empty() && !date.trim().is_empty();

    if let Some(base) = injected_sanitized.as_ref() {
        let summary_ok = !base.business_summary.trim().is_empty();
        let code_ok = !base.edinet_code.trim().is_empty();
        if summary_ok && code_ok {
            return Ok(base.clone());
        }
        if summary_ok && !can_fetch_by_code && !can_fetch_by_name {
            return Ok(base.clone());
        }
        if !can_fetch_by_code && !can_fetch_by_name {
            if !base.company_name.trim().is_empty() {
                return Ok(base.clone());
            }
            return Err("company_facts_or_edinet_code_required".into());
        }
    } else if !can_fetch_by_code && !can_fetch_by_name {
        return Err("company_facts_or_edinet_code_required".into());
    }

    // Policy/egress off → soft-fail to injected company name (interview must not die).
    if refuse_if_policy_off(store.get()).is_err() || refuse_if_egress_unavailable().is_err() {
        if let Some(base) = injected_sanitized {
            if !base.company_name.trim().is_empty() {
                return Ok(base);
            }
        }
        return Err("EGRESS_LIVE_NOT_READY".into());
    }
    let key = match subscription_key_from_env() {
        Ok(k) => k,
        Err(err) => {
            if let Some(base) = injected_sanitized {
                if !base.company_name.trim().is_empty() {
                    return Ok(base);
                }
            }
            return Err(map_edinet_err(err));
        }
    };

    #[cfg(feature = "egress-live")]
    {
        use crate::knowledge::edinet_client::{
            fetch_company_facts_by_code, fetch_company_facts_by_name,
        };
        use crate::knowledge::net_gateway::ReqwestTransport;
        let transport = ReqwestTransport::new().map_err(|e| e.to_string())?;

        let fetched = if can_fetch_by_code {
            fetch_company_facts_by_code(
                &transport,
                date.trim(),
                code.trim(),
                &key,
                filing_text.as_deref(),
                EDINET_FETCH_DEADLINE,
            )
            .await
        } else {
            fetch_company_facts_by_name(
                &transport,
                date.trim(),
                &lookup_name,
                &key,
                filing_text.as_deref(),
                EDINET_FETCH_DEADLINE,
                EDINET_NAME_LOOKBACK_DAYS,
            )
            .await
        };

        match fetched {
            Ok(fetched) => {
                if let Some(base) = injected_sanitized {
                    Ok(merge_company_facts_prefer_filled(&base, &fetched))
                } else {
                    Ok(fetched)
                }
            }
            Err(err) => {
                if let Some(base) = injected_sanitized {
                    if !base.company_name.trim().is_empty() {
                        return Ok(base);
                    }
                }
                Err(map_edinet_err(err))
            }
        }
    }

    #[cfg(not(feature = "egress-live"))]
    {
        let _ = (
            filing_text,
            key,
            EDINET_FETCH_DEADLINE,
            EDINET_NAME_LOOKBACK_DAYS,
            can_fetch_by_code,
            can_fetch_by_name,
            lookup_name,
            code,
            date,
        );
        if let Some(base) = injected_sanitized {
            if !base.company_name.trim().is_empty() {
                return Ok(base);
            }
        }
        Err("EGRESS_LIVE_NOT_READY".into())
    }
}

/// Fill empty fields on `base` from `incoming` (interview start merge).
/// Only compiled with `egress-live` — the sole call site is the live EDINET fetch path.
#[cfg(feature = "egress-live")]
fn merge_company_facts_prefer_filled(base: &CompanyFacts, incoming: &CompanyFacts) -> CompanyFacts {
    let pick = |cur: &str, next: &str| -> String {
        if !cur.trim().is_empty() {
            cur.to_string()
        } else if !next.trim().is_empty() {
            next.to_string()
        } else {
            cur.to_string()
        }
    };
    let source = if base.business_summary.trim().is_empty() && !incoming.source.trim().is_empty()
    {
        incoming.source.clone()
    } else {
        base.source.clone()
    };
    CompanyFacts {
        company_name: pick(&base.company_name, &incoming.company_name),
        edinet_code: pick(&base.edinet_code, &incoming.edinet_code),
        doc_id: pick(&base.doc_id, &incoming.doc_id),
        business_summary: pick(&base.business_summary, &incoming.business_summary),
        business_risks: pick(&base.business_risks, &incoming.business_risks),
        performance_summary: pick(&base.performance_summary, &incoming.performance_summary),
        source,
    }
}

/// Dual-gated EDINET list → [`CompanyFacts`] (UI prep / offline inject preferred).
#[tauri::command]
pub async fn fetch_edinet_company_facts(
    store: State<'_, NetworkPolicyStore>,
    params: FetchEdinetFactsParams,
) -> Result<CompanyFacts, String> {
    let injected = params.company_name.as_ref().and_then(|n| {
        let name = n.trim();
        if name.is_empty() {
            None
        } else {
            Some(CompanyFacts {
                company_name: name.to_string(),
                ..CompanyFacts::default()
            })
        }
    });
    resolve_company_facts(
        store.inner(),
        injected,
        params.edinet_code,
        params.edinet_date,
        params.filing_text,
    )
    .await
}

/// Interview turn: RAG experience + company facts → streaming interviewer.
#[tauri::command]
pub async fn start_interview_session(
    vault: State<'_, VaultHandle>,
    llm: State<'_, LlmHandle>,
    store: State<'_, NetworkPolicyStore>,
    params: StartInterviewParams,
    on_token: Channel<TokenEvent>,
) -> Result<SimSessionResult, String> {
    let message = params.message.trim().to_string();
    if message.is_empty() || message.len() > MAX_TEXT_BYTES {
        return Err("invalid message".into());
    }

    let facts = resolve_company_facts(
        store.inner(),
        params.company_facts,
        params.edinet_code,
        params.edinet_date,
        params.filing_text,
    )
    .await?;

    let es_text = params
        .es_text
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string());

    let (context_limit, mut gen) = resolve_gen(params.gen.as_ref());
    let _ = (context_limit, vault.inner()); // no Vault auto-RAG in interview production (M20-J)
    let message_for_prompt = message.clone();
    let facts_for_prompt = facts.clone();
    let es_for_prompt = es_text;

    let (prompt, context_ids) = tauri::async_runtime::spawn_blocking(move || {
        // Foundation-style: company facts + optional ES base + utterance (no vault KNN).
        let prompt = build_interview_prompt(
            &message_for_prompt,
            &facts_for_prompt,
            &[],
            es_for_prompt.as_deref(),
        );
        Ok::<_, String>((prompt, Vec::<String>::new()))
    })
    .await
    .map_err(|_| "interview retrieve task join failed".to_string())??;

    gen.prompt = prompt;
    llm.generate(gen, None, on_token).await?;

    Ok(SimSessionResult {
        context_count: context_ids.len(),
        context_ids,
        company_name: facts.company_name,
        facts_source: facts.source,
    })
}

/// ES draft review: RAG experience + company facts → streaming reviewer.
#[tauri::command]
pub async fn review_es_draft(
    vault: State<'_, VaultHandle>,
    llm: State<'_, LlmHandle>,
    store: State<'_, NetworkPolicyStore>,
    params: ReviewEsParams,
    on_token: Channel<TokenEvent>,
) -> Result<SimSessionResult, String> {
    let draft = params.es_draft.trim().to_string();
    if draft.is_empty() || draft.len() > MAX_TEXT_BYTES {
        return Err("invalid es_draft".into());
    }

    let facts = resolve_company_facts(
        store.inner(),
        params.company_facts,
        params.edinet_code,
        params.edinet_date,
        params.filing_text,
    )
    .await?;

    let (context_limit, mut gen) = resolve_gen(params.gen.as_ref());
    let query = params
        .experience_query
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or(draft.as_str())
        .to_string();

    let vault = vault.inner().clone();
    let llm_search = llm.inner().clone();
    let draft_for_prompt = draft.clone();
    let facts_for_prompt = facts.clone();

    let (prompt, context_ids) = tauri::async_runtime::spawn_blocking(move || {
        let hits = search_sync(&vault, &llm_search, &query, context_limit).unwrap_or_default();
        let refs: Vec<ExperienceRef<'_>> = hits
            .iter()
            .map(|hit| ExperienceRef {
                id: hit.id.as_str(),
                text: hit.text_content.as_str(),
            })
            .collect();
        let prompt = build_es_review_prompt(&draft_for_prompt, &facts_for_prompt, &refs);
        let ids: Vec<String> = hits.into_iter().map(|hit| hit.id).collect();
        Ok::<_, String>((prompt, ids))
    })
    .await
    .map_err(|_| "es review retrieve task join failed".to_string())??;

    gen.prompt = prompt;
    llm.generate(gen, None, on_token).await?;

    Ok(SimSessionResult {
        context_count: context_ids.len(),
        context_ids,
        company_name: facts.company_name,
        facts_source: facts.source,
    })
}

// ─── M17 multi-stage interview machine ───────────────────────────────────────

use std::time::{SystemTime, UNIX_EPOCH};

use crate::db::InterviewSessionRow;
use crate::knowledge::edinet_client::render_company_facts_block;
use crate::llm::consult_context::load_mentor_context;
use crate::coliseum::session_artifact::{
    amount_band, is_late_night_jst, purchase_text_summary, DistortionEvidenceSnap,
    EvidenceSnapshot, InterviewSessionArtifact, PurchaseEvidenceSnap,
};
use crate::coliseum::CognitiveFossilSnapshot;
use crate::llm::interview_machine::{
    apply_candidate_answer, artifact_evidence_prompt_block, build_stage_prompt_prefix,
    oni_pressure_prompt_from_artifact, oni_pressure_prompt_from_fossils, record_interviewer_utterance,
    AdvanceOutcome, InterviewSession, InterviewStage,
};
use crate::llm::model_path::MODEL_FILENAME;
use sha2::{Digest, Sha256};

fn now_unix() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

fn model_hash_label() -> String {
    hex::encode(Sha256::digest(MODEL_FILENAME.as_bytes()))
}

fn freeze_evidence_from_vault(vault: &VaultHandle, frozen_at_unix: i64) -> EvidenceSnapshot {
    let distortions = vault
        .distortion_tags_list(32)
        .unwrap_or_default()
        .into_iter()
        .map(|row| DistortionEvidenceSnap {
            source_id: row.id,
            category: row.category,
            text_summary: row.snippet,
            confidence_score: row.confidence_score,
        })
        .collect();
    let purchases = vault
        .purchase_list_recent(32)
        .unwrap_or_default()
        .into_iter()
        .map(|row| {
            let late = is_late_night_jst(row.occurred_at);
            let band = amount_band(row.total_amount).to_string();
            PurchaseEvidenceSnap {
                source_id: row.id,
                text_summary: purchase_text_summary(&row.merchant_norm, row.total_amount, late),
                amount_band: band,
                late_night: late,
            }
        })
        .collect();
    EvidenceSnapshot {
        distortions,
        purchases,
        frozen_at_unix,
    }
}

fn fossil_from_evidence(evidence: &EvidenceSnapshot, r_unit: f64) -> CognitiveFossilSnapshot {
    CognitiveFossilSnapshot {
        r_at_decision: Some(r_unit.clamp(0.0, 1.0)),
        distortion_categories: evidence
            .distortions
            .iter()
            .map(|d| d.category.clone())
            .collect(),
        purchase_amounts: evidence
            .purchases
            .iter()
            .filter_map(|p| match p.amount_band.as_str() {
                "micro" => Some(500),
                "low" => Some(2_000),
                "mid" => Some(10_000),
                "high" => Some(40_000),
                "extreme" => Some(200_000),
                _ => None,
            })
            .collect(),
        late_night_purchase_count: evidence.purchases.iter().filter(|p| p.late_night).count() as u32,
        impulse_purchase_count: 0,
        blacklist_seed_terms: Vec::new(),
    }
}

fn persist_session(vault: &VaultHandle, session: &InterviewSession) -> Result<(), String> {
    let payload_json =
        serde_json::to_string(session).map_err(|_| "interview session serialize failed".to_string())?;
    let (artifact_json, artifact_fingerprint) = match &session.session_artifact {
        Some(art) => {
            let json = serde_json::to_string(art)
                .map_err(|_| "interview artifact serialize failed".to_string())?;
            (json, art.fingerprint.clone())
        }
        None => (String::new(), String::new()),
    };
    vault
        .interview_session_put(InterviewSessionRow {
            id: session.id.clone(),
            updated_at: now_unix(),
            stage: session.stage.as_str().into(),
            status: session.status.clone(),
            payload_json,
            artifact_json,
            artifact_fingerprint,
        })
        .map_err(map_vault_err)
}

fn load_session(vault: &VaultHandle, id: &str) -> Result<InterviewSession, String> {
    let row = vault
        .interview_session_get(id.to_string())
        .map_err(map_vault_err)?
        .ok_or_else(|| "interview_session_not_found".to_string())?;
    let mut session: InterviewSession = serde_json::from_str(&row.payload_json)
        .map_err(|_| "interview session parse failed".to_string())?;
    // Prefer dedicated columns when payload predates artifact embedding.
    if session.session_artifact.is_none() && !row.artifact_json.is_empty() {
        let mut art: InterviewSessionArtifact = serde_json::from_str(&row.artifact_json)
            .map_err(|_| "interview artifact parse failed".to_string())?;
        if !row.artifact_fingerprint.is_empty() && art.fingerprint != row.artifact_fingerprint {
            return Err("session_artifact_fingerprint_mismatch".into());
        }
        art.rehydrate_directives();
        // Fingerprint is over canonical body; rehydrate must be idempotent.
        if !art.verify_fingerprint() {
            return Err("session_artifact_fingerprint_mismatch".into());
        }
        session.attach_artifact(art);
    }
    Ok(session)
}

fn build_machine_prompt(
    vault: &VaultHandle,
    llm: &LlmHandle,
    session: &InterviewSession,
    facts: &CompanyFacts,
    user_message: &str,
    context_limit: u32,
) -> Result<(String, Vec<String>), String> {
    let is_debrief = session.stage == InterviewStage::Debrief;

    // M20-J scope: Vault auto-RAG + Gap/Tensor ONLY in Debrief.
    // Foundation / Pressure: company facts + session transcript only (no vault KNN).
    let hits = if is_debrief {
        search_sync(vault, llm, user_message, context_limit).unwrap_or_default()
    } else {
        Vec::new()
    };
    let refs: Vec<ExperienceRef<'_>> = hits
        .iter()
        .map(|hit| ExperienceRef {
            id: hit.id.as_str(),
            text: hit.text_content.as_str(),
        })
        .collect();

    let mut prompt = build_stage_prompt_prefix(session, is_debrief);
    prompt.push_str("\n## 企業ファクト（EDINET）\n");
    prompt.push_str(&render_company_facts_block(facts));

    if is_debrief {
        prompt.push_str("\n## 参考情報（Vault 自動探索・講評専用）\n");
        if refs.is_empty() {
            prompt.push_str("（該当する知識チャンクは見つかりませんでした）\n");
        } else {
            for (i, hit) in refs.iter().enumerate() {
                prompt.push_str(&format!(
                    "[{}] (id={})\n{}\n\n",
                    i + 1,
                    hit.id,
                    hit.text.trim()
                ));
            }
        }
        // I-22 / M20-J: Gap / Tensor / Oracle only in debrief.
        match load_mentor_context(vault) {
            Ok(mentor) => {
                prompt.push_str("\n## 主観×客観ギャップ（講評専用）\n");
                prompt.push_str(&mentor.gap_block);
                prompt.push_str("\n## Tensorプロファイル（講評専用）\n");
                prompt.push_str(&mentor.tensor_block);
                prompt.push_str("\n## Oracle予測（講評専用）\n");
                prompt.push_str(&mentor.oracle_block);
            }
            Err(_) => {
                prompt.push_str(
                    "\n（Gap/Tensor/Oracle 取得不可 — 講評は企業ファクトと会話ログのみ）\n",
                );
            }
        }
    } else {
        prompt.push_str(
            "\n## 参照範囲（面接本番）\n\
（Vault / Gap / Tensor の自動探索は無効。企業ファクトと本セッションの対話のみを根拠にせよ。）\n",
        );
        // Phase 14.4: Pressure uses frozen artifact directives (replayable; no live vault).
        if session.stage == InterviewStage::Pressure && session.oni_active {
            let sealed = if let Some(art) = session.session_artifact.as_ref() {
                oni_pressure_prompt_from_artifact(art, "")
            } else {
                let empty = CognitiveFossilSnapshot {
                    r_at_decision: None,
                    distortion_categories: Vec::new(),
                    purchase_amounts: Vec::new(),
                    late_night_purchase_count: 0,
                    impulse_purchase_count: 0,
                    blacklist_seed_terms: Vec::new(),
                };
                oni_pressure_prompt_from_fossils(empty, "")
            };
            match sealed {
                Ok(text) if !text.is_empty() => {
                    prompt.push_str("\n## 鬼モード抽象戦術（I-22 凍結ディレクティブ）\n");
                    prompt.push_str(&text);
                    prompt.push('\n');
                }
                _ => {}
            }
        }
    }

    if is_debrief {
        if let Some(art) = session.session_artifact.as_ref() {
            prompt.push('\n');
            prompt.push_str(&artifact_evidence_prompt_block(art));
        }
    }

    prompt.push_str("\n## 候補者の発話\n");
    prompt.push_str(user_message.trim());
    prompt.push('\n');

    let ids: Vec<String> = hits.into_iter().map(|h| h.id).collect();
    Ok((prompt, ids))
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StartMultistageInterviewParams {
    pub opening_message: Option<String>,
    pub company_facts: Option<CompanyFacts>,
    pub edinet_code: Option<String>,
    pub edinet_date: Option<String>,
    pub filing_text: Option<String>,
    pub gen: Option<SimGenParams>,
    /// Request 鬼モード (Pressure + abstract tactics). Subject to ZPD hard gate.
    pub oni_mode: Option<bool>,
    /// Quantized Twin R(t) on 0..=100. Required when oni_mode; else ignored.
    pub r_t: Option<u8>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct MultistageInterviewResult {
    pub session_id: String,
    pub stage: String,
    pub status: String,
    pub turn_in_stage: u32,
    pub total_turns: u32,
    pub context_ids: Vec<String>,
    pub company_name: String,
    pub outcome: String,
    pub oni_active: bool,
}

/// Start Foundation stage; streams first interviewer question.
#[tauri::command]
pub async fn start_multistage_interview(
    vault: State<'_, VaultHandle>,
    llm: State<'_, LlmHandle>,
    store: State<'_, NetworkPolicyStore>,
    params: StartMultistageInterviewParams,
    on_token: Channel<TokenEvent>,
) -> Result<MultistageInterviewResult, String> {
    let facts = resolve_company_facts(
        store.inner(),
        params.company_facts,
        params.edinet_code,
        params.edinet_date,
        params.filing_text,
    )
    .await?;
    let facts_json =
        serde_json::to_string(&facts).map_err(|_| "facts serialize failed".to_string())?;
    let session_id = format!("iv-{}", now_unix());
    let mut session =
        InterviewSession::new(session_id.clone(), facts.company_name.clone(), facts_json);
    let oni_requested = params.oni_mode.unwrap_or(false);
    let r_t = params
        .r_t
        .unwrap_or_else(|| crate::coliseum::mentor_zpd::r_t_from_unit_interval(1.0));
    session.oni_active = if !oni_requested {
        false
    } else if crate::coliseum::mentor_zpd::evaluate_oni_mode_eligibility(r_t).is_err() {
        // Structural hard-downgrade: depleted R never enters oni Pressure.
        false
    } else {
        crate::coliseum::resolve_oni_activation(true, r_t).unwrap_or(false)
    };

    // Phase 14.4: freeze start-of-session artifact (evidence bodies + directives).
    let frozen_at = now_unix();
    let evidence = freeze_evidence_from_vault(vault.inner(), frozen_at);
    let r_unit = f64::from(r_t) / 100.0;
    let fossil = fossil_from_evidence(&evidence, r_unit);
    let artifact = InterviewSessionArtifact::freeze(
        session_id.clone(),
        model_hash_label(),
        r_unit,
        fossil,
        evidence,
    );
    session.attach_artifact(artifact);

    let opening = params
        .opening_message
        .unwrap_or_else(|| "自己紹介と、志望動機を簡潔に述べてください。".into());

    let (context_limit, mut gen) =
        resolve_coliseum_gen(params.gen.as_ref(), artifact_turn_seed(&session));
    let vault_c = vault.inner().clone();
    let llm_s = llm.inner().clone();
    let facts_c = facts.clone();
    let session_snapshot = session.clone();
    let opening_c = opening.clone();

    let (prompt, context_ids) = tauri::async_runtime::spawn_blocking(move || {
        build_machine_prompt(
            &vault_c,
            &llm_s,
            &session_snapshot,
            &facts_c,
            &opening_c,
            context_limit,
        )
    })
    .await
    .map_err(|_| "multistage start join failed".to_string())??;

    record_interviewer_utterance(&mut session, &opening);
    persist_session(vault.inner(), &session)?;

    gen.prompt = prompt;
    llm.generate(gen, None, on_token).await?;

    Ok(MultistageInterviewResult {
        session_id,
        stage: session.stage.as_str().into(),
        status: session.status,
        turn_in_stage: session.turn_in_stage,
        total_turns: session.total_turns,
        context_ids,
        company_name: facts.company_name,
        outcome: "started".into(),
        oni_active: session.oni_active,
    })
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AdvanceInterviewParams {
    pub session_id: String,
    pub candidate_answer: String,
    pub gen: Option<SimGenParams>,
}

/// Apply candidate answer, advance FSM, stream next interviewer turn (or debrief).
#[tauri::command]
pub async fn advance_interview_stage(
    vault: State<'_, VaultHandle>,
    llm: State<'_, LlmHandle>,
    params: AdvanceInterviewParams,
    on_token: Channel<TokenEvent>,
) -> Result<MultistageInterviewResult, String> {
    let answer = params.candidate_answer.trim().to_string();
    if answer.is_empty() || answer.len() > MAX_TEXT_BYTES {
        return Err("invalid candidate_answer".into());
    }
    let session_id = params.session_id.trim().to_string();
    if session_id.is_empty() {
        return Err("invalid session_id".into());
    }

    let vault_h = vault.inner().clone();
    let mut session = load_session(&vault_h, &session_id)?;
    let outcome = apply_candidate_answer(&mut session, &answer)?;
    let outcome_label = match &outcome {
        AdvanceOutcome::ContinueQuestion => "continue",
        AdvanceOutcome::EnteredStage(s) => match s {
            InterviewStage::Pressure => "entered_pressure",
            InterviewStage::Debrief => "entered_debrief",
            _ => "entered_stage",
        },
        AdvanceOutcome::CircuitBreakToDebrief => "circuit_break_debrief",
        AdvanceOutcome::Closed => "closed",
    };

    if matches!(outcome, AdvanceOutcome::Closed) {
        persist_session(&vault_h, &session)?;
        return Ok(MultistageInterviewResult {
            session_id,
            stage: session.stage.as_str().into(),
            status: session.status,
            turn_in_stage: session.turn_in_stage,
            total_turns: session.total_turns,
            context_ids: vec![],
            company_name: session.company_name,
            outcome: outcome_label.into(),
            oni_active: session.oni_active,
        });
    }

    let facts: CompanyFacts = serde_json::from_str(&session.facts_json)
        .map_err(|_| "stored facts parse failed".to_string())?;
    let (context_limit, mut gen) =
        resolve_coliseum_gen(params.gen.as_ref(), artifact_turn_seed(&session));
    let llm_s = llm.inner().clone();
    let session_snap = session.clone();
    let answer_c = answer.clone();
    let facts_c = facts.clone();
    let vault_c = vault_h.clone();

    let (prompt, context_ids) = tauri::async_runtime::spawn_blocking(move || {
        build_machine_prompt(
            &vault_c,
            &llm_s,
            &session_snap,
            &facts_c,
            &answer_c,
            context_limit,
        )
    })
    .await
    .map_err(|_| "multistage advance join failed".to_string())??;

    persist_session(&vault_h, &session)?;
    gen.prompt = prompt;
    llm.generate(gen, None, on_token).await?;

    Ok(MultistageInterviewResult {
        session_id,
        stage: session.stage.as_str().into(),
        status: session.status,
        turn_in_stage: session.turn_in_stage,
        total_turns: session.total_turns,
        context_ids,
        company_name: session.company_name,
        outcome: outcome_label.into(),
        oni_active: session.oni_active,
    })
}

#[tauri::command]
pub async fn get_interview_session(
    vault: State<'_, VaultHandle>,
    session_id: String,
) -> Result<InterviewSession, String> {
    let id = session_id.trim().to_string();
    if id.is_empty() {
        return Err("invalid session_id".into());
    }
    let vault = vault.inner().clone();
    tauri::async_runtime::spawn_blocking(move || load_session(&vault, &id))
        .await
        .map_err(|_| "get_interview_session join failed".to_string())?
}
