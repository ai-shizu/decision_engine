//! Tauri commands for local RAG ingest, search, and RAG chat (M10/M11).
//!
//! Heavy work runs on `spawn_blocking` so the async runtime / UI thread is not
//! blocked by the embed loop. Embeddings go through `LlmHandle` (LLM worker);
//! SQL goes through `VaultHandle` (vault worker) — never from the main thread.
//! Generation streams over the existing M6 `Channel<TokenEvent>` pipeline.
//!
//! Phase 7 search path: lexical hashed KNN ⊕ optional dense KNN → RRF (k=60) →
//! Ebbinghaus time decay (UTC). See `associative_recall`.

use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use tauri::ipc::Channel;
use tauri::State;

use super::associative_recall::{default_tau_days, fuse_and_rerank, RRF_K};
use super::chunk::{chunk_markdown, MAX_CHUNKS};
use super::embed_knowledge::{embed_for_knowledge, lexical_hash_embed, try_dense_passage_embed};
use super::prompt::{build_rag_prompt, RagContextRef};
use crate::db::{
    KnowledgeChunkRow, KnowledgeSearchHit as DbHit, VaultErrorCode, VaultHandle, VaultStatus,
};
use crate::llm::hashed_embed::hashed_ngram_embed_384;
use crate::llm::params::GenerationParams;
use crate::llm::service::{error_done_event, TokenEvent};
use crate::llm::LlmHandle;

/// Structured error code emitted over `Channel<TokenEvent>` when the vault is not
/// unlocked. The frontend maps this controlled code to a sterile sys-log message
/// (Finding 13: never surface raw IPC/embedding text).
const ERR_VAULT_LOCKED: &str = "VAULT_LOCKED";

const MAX_SOURCE_ID_BYTES: usize = 512;
const MAX_INGEST_TEXT_BYTES: usize = 512 * 1024;
const DEFAULT_SEARCH_LIMIT: u32 = 5;
const MAX_SEARCH_LIMIT: u32 = 50;
const DEFAULT_RAG_N_CTX: u32 = 2048;
const DEFAULT_RAG_MAX_TOKENS: u32 = 256;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct IngestKnowledgeResult {
    pub source_id: String,
    pub chunk_count: usize,
    pub inserted: usize,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct SearchKnowledgeHit {
    pub id: String,
    pub text_content: String,
    pub distance: f64,
    /// Final associative recall score (RRF × Ebbinghaus retention).
    pub recall_score: f64,
    /// Chunk creation time (Unix UTC seconds).
    pub created_at: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct SearchKnowledgeResult {
    pub hits: Vec<SearchKnowledgeHit>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RagChatParams {
    pub n_ctx: Option<u32>,
    pub max_tokens: Option<u32>,
    pub temp: Option<f32>,
    pub top_k: Option<i32>,
    pub top_p: Option<f32>,
    pub seed: Option<u32>,
    /// KNN `k` for context retrieval.
    pub context_limit: Option<u32>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct SendRagChatResult {
    pub context_ids: Vec<String>,
    pub context_count: usize,
}

fn map_vault_err(code: VaultErrorCode) -> String {
    format!("{code:?}").to_ascii_lowercase()
}

fn now_unix_secs() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

fn validate_source_id(source_id: &str) -> Result<(), String> {
    let trimmed = source_id.trim();
    if trimmed.is_empty() || trimmed.len() > MAX_SOURCE_ID_BYTES {
        return Err("invalid source_id".into());
    }
    if trimmed.contains('%') || trimmed.contains('_') || trimmed.contains("::") {
        return Err("invalid source_id".into());
    }
    Ok(())
}

fn candidate_pool(limit: u32) -> u32 {
    limit.saturating_mul(3).clamp(limit, MAX_SEARCH_LIMIT)
}

fn merge_hit_meta(meta: &mut HashMap<String, (String, f64, i64)>, hits: &[DbHit]) {
    for hit in hits {
        match meta.get_mut(&hit.id) {
            Some((_text, dist, created)) => {
                if hit.distance < *dist {
                    *dist = hit.distance;
                }
                if *created == 0 && hit.created_at != 0 {
                    *created = hit.created_at;
                }
            }
            None => {
                meta.insert(
                    hit.id.clone(),
                    (hit.text_content.clone(), hit.distance, hit.created_at),
                );
            }
        }
    }
}

/// Lexical ranking over a candidate pool via hashed-ngram cosine (L2 ⇒ dot).
///
/// Independent of the vault embedding space: when the index is dense (DPR), this
/// still supplies a true lexical channel for RRF; when the index is hashed, it
/// diversifies order within the KNN pool without a second wrong-space query.
fn lexical_rank_ids(query: &str, hits: &[DbHit]) -> Vec<String> {
    let q = hashed_ngram_embed_384(query);
    let mut scored: Vec<(f64, &str)> = hits
        .iter()
        .map(|h| {
            let v = hashed_ngram_embed_384(&h.text_content);
            let mut dot = 0.0f64;
            for (a, b) in q.iter().zip(v.iter()) {
                dot += f64::from(*a) * f64::from(*b);
            }
            (dot, h.id.as_str())
        })
        .collect();
    scored.sort_by(|a, b| {
        b.0.partial_cmp(&a.0)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.1.cmp(b.1))
    });
    scored.into_iter().map(|(_, id)| id.to_string()).collect()
}

/// Hybrid associative recall: index-space KNN ⊕ lexical hash rank → RRF → Ebbinghaus.
///
/// Primary channel:
/// - **DPR** ([`try_dense_passage_embed`]) when a true 384-d model is loaded
///   (Karpukhin et al., 2020) — same space as dense ingest.
/// - else **hashed** ([`lexical_hash_embed`]) — same space as hashed ingest.
///
/// Lexical RRF partner always re-ranks the candidate pool by hashed-ngram cosine
/// over `text_content`, so fusion stays valid when vault vectors are dense
/// (never issues a second KNN in the wrong embedding space).
pub(crate) fn search_sync(
    vault: &VaultHandle,
    llm: &LlmHandle,
    query: &str,
    limit: u32,
) -> Result<Vec<DbHit>, String> {
    let pool = candidate_pool(limit);
    let primary_emb = match try_dense_passage_embed(llm, query) {
        Some(dense) => dense,
        None => lexical_hash_embed(query)?,
    };
    let primary_hits = vault
        .knowledge_search(primary_emb, pool)
        .map_err(map_vault_err)?;

    let primary_ids: Vec<String> = primary_hits.iter().map(|h| h.id.clone()).collect();
    let lexical_ids = lexical_rank_ids(query, &primary_hits);

    let mut meta: HashMap<String, (String, f64, i64)> = HashMap::new();
    merge_hit_meta(&mut meta, &primary_hits);

    let now = now_unix_secs();
    let ranked = fuse_and_rerank(
        &primary_ids,
        &lexical_ids,
        &meta,
        now,
        default_tau_days(),
        RRF_K,
        limit as usize,
    );

    Ok(ranked
        .into_iter()
        .map(|h| DbHit {
            id: h.id,
            text_content: h.text_content,
            distance: h.distance,
            created_at: h.created_at,
            recall_score: h.recall_score,
        })
        .collect())
}

fn to_ipc_hits(hits: Vec<DbHit>) -> Vec<SearchKnowledgeHit> {
    hits.into_iter()
        .map(|hit| SearchKnowledgeHit {
            id: hit.id,
            text_content: hit.text_content,
            distance: hit.distance,
            recall_score: hit.recall_score,
            created_at: hit.created_at,
        })
        .collect()
}

/// Ingest markdown/plain text: chunk → embed each → transactional vault replace.
#[tauri::command]
pub async fn ingest_knowledge(
    vault: State<'_, VaultHandle>,
    llm: State<'_, LlmHandle>,
    text: String,
    source_id: String,
) -> Result<IngestKnowledgeResult, String> {
    validate_source_id(&source_id)?;
    if text.is_empty() || text.len() > MAX_INGEST_TEXT_BYTES {
        return Err("invalid text".into());
    }

    let vault = vault.inner().clone();
    let llm = llm.inner().clone();
    let source_id = source_id.trim().to_string();

    tauri::async_runtime::spawn_blocking(move || {
        let chunks = chunk_markdown(&text, &source_id);
        if chunks.is_empty() {
            return Err("no chunks produced".into());
        }
        if chunks.len() > MAX_CHUNKS {
            return Err("too many chunks".into());
        }

        let created_at = now_unix_secs();
        let mut rows = Vec::with_capacity(chunks.len());
        for chunk in &chunks {
            let embed_input = if chunk.title == source_id {
                chunk.text.clone()
            } else {
                format!("{}\n\n{}", chunk.title, chunk.text)
            };
            let embedding = embed_for_knowledge(&llm, &embed_input)?;
            rows.push(KnowledgeChunkRow {
                id: chunk.id.clone(),
                text_content: chunk.text.clone(),
                embedding,
                created_at,
            });
        }

        let inserted = vault
            .knowledge_replace(source_id.clone(), rows)
            .map_err(map_vault_err)?;

        Ok(IngestKnowledgeResult {
            source_id,
            chunk_count: chunks.len(),
            inserted,
        })
    })
    .await
    .map_err(|_| "ingest task join failed".to_string())?
}

/// Hybrid recall over `knowledge_chunks` (RRF + Ebbinghaus).
#[tauri::command]
pub async fn search_knowledge(
    vault: State<'_, VaultHandle>,
    llm: State<'_, LlmHandle>,
    query: String,
    limit: Option<u32>,
) -> Result<SearchKnowledgeResult, String> {
    let limit = limit.unwrap_or(DEFAULT_SEARCH_LIMIT);
    if !(1..=MAX_SEARCH_LIMIT).contains(&limit) {
        return Err("invalid limit".into());
    }
    let query = query.trim().to_string();
    if query.is_empty() || query.len() > MAX_INGEST_TEXT_BYTES {
        return Err("invalid query".into());
    }

    let vault = vault.inner().clone();
    let llm = llm.inner().clone();

    tauri::async_runtime::spawn_blocking(move || {
        let hits = search_sync(&vault, &llm, &query, limit)?;
        Ok(SearchKnowledgeResult {
            hits: to_ipc_hits(hits),
        })
    })
    .await
    .map_err(|_| "search task join failed".to_string())?
}

/// Retrieve context for `message`, inject into a RAG prompt, then stream chat
/// tokens over `on_token` (same Channel pipeline as `llm_generate`).
#[tauri::command]
pub async fn send_rag_chat(
    vault: State<'_, VaultHandle>,
    llm: State<'_, LlmHandle>,
    message: String,
    params: Option<RagChatParams>,
    on_token: Channel<TokenEvent>,
) -> Result<SendRagChatResult, String> {
    let message = message.trim().to_string();
    if message.is_empty() || message.len() > MAX_INGEST_TEXT_BYTES {
        return Err("invalid message".into());
    }

    let opts = params.unwrap_or(RagChatParams {
        n_ctx: None,
        max_tokens: None,
        temp: None,
        top_k: None,
        top_p: None,
        seed: None,
        context_limit: None,
    });
    let context_limit = opts.context_limit.unwrap_or(DEFAULT_SEARCH_LIMIT);
    if !(1..=MAX_SEARCH_LIMIT).contains(&context_limit) {
        return Err("invalid context_limit".into());
    }

    // Fail-fast (Silent-Hang guard): a locked / unavailable vault cannot serve RAG
    // retrieval. `status()` is a cached read (no worker round-trip), so this never
    // blocks. Emit an explicit error event over the token channel and return at
    // once — never a context-free silent answer, never a perpetual "…" spinner.
    if !matches!(vault.status(), VaultStatus::Unlocked) {
        let _ = on_token.send(error_done_event(0, ERR_VAULT_LOCKED.to_string()));
        return Ok(SendRagChatResult {
            context_count: 0,
            context_ids: Vec::new(),
        });
    }

    let vault = vault.inner().clone();
    let llm_for_search = llm.inner().clone();
    let message_for_search = message.clone();

    let (prompt, context_ids) = tauri::async_runtime::spawn_blocking(move || {
        // Automatic retrieval: soft-fail KNN so Gap/Oracle/Tensor still inject.
        let hits = search_sync(&vault, &llm_for_search, &message_for_search, context_limit)
            .unwrap_or_default();
            let refs: Vec<RagContextRef<'_>> = hits
                .iter()
                .map(|hit| {
                    let relevance = if hit.recall_score.is_finite() && hit.recall_score > 0.0 {
                        hit.recall_score.clamp(0.0, 1.0)
                    } else if hit.distance.is_finite() {
                        (1.0 / (1.0 + hit.distance)).clamp(0.0, 1.0)
                    } else {
                        0.5
                    };
                    RagContextRef {
                        id: hit.id.as_str(),
                        text: hit.text_content.as_str(),
                        relevance,
                        created_at: hit.created_at,
                    }
                })
                .collect();
            let rag_prompt = build_rag_prompt(&message_for_search, &refs);
        // M17/M20-J: fail-safe Gap/Oracle/Tensor injection (missing → soft notes).
        let prompt = match crate::llm::consult_context::load_mentor_context(&vault) {
            Ok(mentor) => {
                crate::llm::consult_context::append_mentor_sections(&rag_prompt, &mentor)
            }
            Err(_) => rag_prompt,
        };
        let ids: Vec<String> = hits.into_iter().map(|hit| hit.id).collect();
        Ok::<_, String>((prompt, ids))
    })
    .await
    .map_err(|_| "rag retrieve task join failed".to_string())??;

    let gen = GenerationParams {
        prompt,
        n_ctx: opts.n_ctx.unwrap_or(DEFAULT_RAG_N_CTX),
        max_tokens: opts.max_tokens.unwrap_or(DEFAULT_RAG_MAX_TOKENS),
        temp: opts.temp.unwrap_or(0.7),
        top_k: opts.top_k.unwrap_or(40),
        top_p: opts.top_p.unwrap_or(0.95),
        seed: opts.seed.unwrap_or(0),
    };

    // Streams on the LLM worker; cancel/purge still go through LlmMemoryGovernor.
    llm.generate(gen, None, on_token)?;

    Ok(SendRagChatResult {
        context_count: context_ids.len(),
        context_ids,
    })
}
