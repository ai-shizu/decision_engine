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

use crate::db::{KnowledgeSearchHit as DbHit, VaultErrorCode, VaultHandle};
use crate::knowledge::edinet_client::{
    sanitize_company_facts, subscription_key_from_env, CompanyFacts, EdinetError,
};
use crate::knowledge::{
    refuse_if_egress_unavailable, refuse_if_policy_off, NetworkPolicyStore, OrchestratorError,
};
use crate::llm::embed::{require_knowledge_embedding_dims, EMBED_DEFAULT_N_CTX};
use crate::llm::params::GenerationParams;
use crate::llm::prompt_sim::{build_es_review_prompt, build_interview_prompt, ExperienceRef};
use crate::llm::service::TokenEvent;
use crate::llm::LlmHandle;

const MAX_TEXT_BYTES: usize = 64 * 1024;
const DEFAULT_CONTEXT_LIMIT: u32 = 5;
const MAX_CONTEXT_LIMIT: u32 = 20;
const DEFAULT_N_CTX: u32 = 2048;
const DEFAULT_MAX_TOKENS: u32 = 256;
const EDINET_FETCH_DEADLINE: Duration = Duration::from_secs(15);

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
    pub edinet_code: String,
    pub edinet_date: String,
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

fn map_orch_err(err: OrchestratorError) -> String {
    err.to_string()
}

fn map_edinet_err(err: EdinetError) -> String {
    err.to_string()
}

fn search_sync(
    vault: &VaultHandle,
    llm: &LlmHandle,
    query: &str,
    limit: u32,
) -> Result<Vec<DbHit>, String> {
    let embedding = llm.embed(query.to_string(), EMBED_DEFAULT_N_CTX)?;
    require_knowledge_embedding_dims(&embedding)?;
    vault
        .knowledge_search(embedding, limit)
        .map_err(map_vault_err)
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

async fn resolve_company_facts(
    store: &NetworkPolicyStore,
    injected: Option<CompanyFacts>,
    edinet_code: Option<String>,
    edinet_date: Option<String>,
    filing_text: Option<String>,
) -> Result<CompanyFacts, String> {
    if let Some(facts) = injected {
        return sanitize_company_facts(&facts).map_err(map_edinet_err);
    }

    let code = edinet_code.unwrap_or_default();
    let date = edinet_date.unwrap_or_default();
    if code.trim().is_empty() || date.trim().is_empty() {
        return Err("company_facts_or_edinet_code_required".into());
    }

    refuse_if_policy_off(store.get()).map_err(map_orch_err)?;
    refuse_if_egress_unavailable().map_err(map_orch_err)?;
    let key = subscription_key_from_env().map_err(map_edinet_err)?;

    #[cfg(feature = "egress-live")]
    {
        use crate::knowledge::edinet_client::fetch_company_facts_by_code;
        use crate::knowledge::net_gateway::ReqwestTransport;
        let transport = ReqwestTransport::new().map_err(|e| e.to_string())?;
        let facts = fetch_company_facts_by_code(
            &transport,
            date.trim(),
            code.trim(),
            &key,
            filing_text.as_deref(),
            EDINET_FETCH_DEADLINE,
        )
        .await
        .map_err(map_edinet_err)?;
        return Ok(facts);
    }

    #[cfg(not(feature = "egress-live"))]
    {
        let _ = (filing_text, key, EDINET_FETCH_DEADLINE);
        Err("EGRESS_LIVE_NOT_READY".into())
    }
}

/// Dual-gated EDINET list → [`CompanyFacts`] (UI prep / offline inject preferred).
#[tauri::command]
pub async fn fetch_edinet_company_facts(
    store: State<'_, NetworkPolicyStore>,
    params: FetchEdinetFactsParams,
) -> Result<CompanyFacts, String> {
    resolve_company_facts(
        store.inner(),
        None,
        Some(params.edinet_code),
        Some(params.edinet_date),
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

    let (context_limit, mut gen) = resolve_gen(params.gen.as_ref());
    let vault = vault.inner().clone();
    let llm_search = llm.inner().clone();
    let message_for_search = message.clone();
    let facts_for_prompt = facts.clone();

    let (prompt, context_ids) = tauri::async_runtime::spawn_blocking(move || {
        let hits = search_sync(&vault, &llm_search, &message_for_search, context_limit)?;
        let refs: Vec<ExperienceRef<'_>> = hits
            .iter()
            .map(|hit| ExperienceRef {
                id: hit.id.as_str(),
                text: hit.text_content.as_str(),
            })
            .collect();
        let prompt = build_interview_prompt(&message_for_search, &facts_for_prompt, &refs);
        let ids: Vec<String> = hits.into_iter().map(|hit| hit.id).collect();
        Ok::<_, String>((prompt, ids))
    })
    .await
    .map_err(|_| "interview retrieve task join failed".to_string())??;

    gen.prompt = prompt;
    llm.generate(gen, None, on_token)?;

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
        let hits = search_sync(&vault, &llm_search, &query, context_limit)?;
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
    llm.generate(gen, None, on_token)?;

    Ok(SimSessionResult {
        context_count: context_ids.len(),
        context_ids,
        company_name: facts.company_name,
        facts_source: facts.source,
    })
}
