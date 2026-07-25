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
use tauri::{AppHandle, Manager, State};

use super::associative_recall::{default_tau_days, fuse_and_rerank, RRF_K};
use super::chunk::{chunk_markdown, MAX_CHUNKS};
use super::embed_knowledge::{embed_for_knowledge, lexical_hash_embed, try_dense_passage_embed};
use super::line_import::{decode_line_export_bytes, format_line_import};
use super::prompt::{build_rag_prompt, RagContextRef};
use crate::db::{
    KnowledgeChunkRow, KnowledgeSearchHit as DbHit, VaultErrorCode, VaultHandle, VaultStatus,
};
use crate::llm::hashed_embed::hashed_ngram_embed_384;
use crate::llm::commands_consult::ensure_model_loaded;
use crate::llm::model_path::resolve_loadable_model_path;
use crate::llm::params::{GenerationParams, LoadParams};
use crate::llm::prompt_budget::fit_and_verify_prompt;
use crate::llm::service::{error_done_event, TokenEvent};
use crate::llm::LlmHandle;
use crate::rag::namespace::{parse_namespace_arg, KnowledgeNamespace};

/// Structured error code emitted over `Channel<TokenEvent>` when the vault is not
/// unlocked. The frontend maps this controlled code to a sterile sys-log message
/// (Finding 13: never surface raw IPC/embedding text).
const ERR_VAULT_LOCKED: &str = "VAULT_LOCKED";
const ERR_MODEL_NOT_LOADED: &str = "MODEL_NOT_LOADED";

const MAX_SOURCE_ID_BYTES: usize = 512;
const MAX_INGEST_TEXT_BYTES: usize = 512 * 1024;
/// LINE exports via AppData path staging. Soft Jetsam bound — full text is
/// split across multiple `source_id` parts rather than silently truncated.
const MAX_LINE_INGEST_BYTES: usize = 16 * 1024 * 1024;
/// Max chunk batches for one LINE file (each batch ≤ [`MAX_CHUNKS`]).
const MAX_LINE_PARTS: usize = 48;
/// Practical upper bound when enumerating all LINE chunks before batching.
const MAX_LINE_CHUNK_ENUM: usize = MAX_CHUNKS.saturating_mul(MAX_LINE_PARTS);
const DEFAULT_SEARCH_LIMIT: u32 = 5;
const MAX_SEARCH_LIMIT: u32 = 50;
const DEFAULT_RAG_N_CTX: u32 = 2048;
const DEFAULT_RAG_MAX_TOKENS: u32 = 256;

/// Controlled error codes for on-device LINE import. Frontend maps these to
/// sterile UI copy (Finding 13) — never put path/filename/body into the string.
fn line_err(code: &str) -> String {
    format!("LINE_IMPORT:{code}")
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct IngestKnowledgeResult {
    pub source_id: String,
    pub chunk_count: usize,
    pub inserted: usize,
    /// True only if the file exceeded [`MAX_LINE_PARTS`] × [`MAX_CHUNKS`].
    #[serde(default)]
    pub truncated: bool,
    /// Number of vault `source_id` parts written (LINE multi-part ingest).
    #[serde(default)]
    pub part_count: usize,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct KnowledgeSourceRow {
    pub source_id: String,
    pub chunk_count: usize,
    pub created_at: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct ListKnowledgeSourcesResult {
    pub sources: Vec<KnowledgeSourceRow>,
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

pub(crate) fn validate_source_id(source_id: &str) -> Result<(), String> {
    let trimmed = source_id.trim();
    if trimmed.is_empty() || trimmed.len() > MAX_SOURCE_ID_BYTES {
        return Err("invalid source_id".into());
    }
    if trimmed.contains('%') || trimmed.contains('_') || trimmed.contains("::") {
        return Err("invalid source_id".into());
    }
    Ok(())
}

/// Derive a `validate_source_id`-safe id from a LINE export filename so
/// re-importing the same file replaces its prior chunks (`knowledge_replace`
/// is keyed by source_id) instead of accumulating duplicates. `%`, `_`, `:`
/// are disallowed in source_ids (wildcard / internal-separator collision) so
/// they are folded to `-`; an empty/degenerate result falls back to a fixed
/// default rather than failing the import outright.
fn line_source_id(filename: &str) -> String {
    let stem = std::path::Path::new(filename)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("");
    let sanitized: String = stem
        .chars()
        .map(|c| if matches!(c, '%' | '_' | ':') { '-' } else { c })
        .collect();
    let sanitized = sanitized.trim_matches('-');
    let id = if sanitized.is_empty() {
        "line-import".to_string()
    } else {
        format!("line-{sanitized}")
    };
    if id.len() <= MAX_SOURCE_ID_BYTES {
        return id;
    }
    // Truncate to a UTF-8 char boundary at/under the byte cap.
    let mut end = MAX_SOURCE_ID_BYTES;
    while end > 0 && !id.is_char_boundary(end) {
        end -= 1;
    }
    id[..end].to_string()
}

/// 企業ファクト → Company 名前空間の source_id。
/// 接頭辞（`edinet-` / `company-`）は sanitize 後に付与し、`namespace_of` が必ず Company と
/// 判定することを保証する（接頭辞を trim で削らない）。
fn company_source_id(facts: &crate::knowledge::edinet_client::CompanyFacts) -> String {
    fn sanitize_body(s: &str) -> String {
        let mapped: String = s
            .chars()
            .map(|c| if matches!(c, '%' | '_' | ':') { '-' } else { c })
            .collect();
        mapped.trim_matches('-').to_string()
    }
    let code = sanitize_body(facts.edinet_code.trim());
    let (prefix, body) = if !code.is_empty() {
        ("edinet-", code)
    } else {
        let name = sanitize_body(facts.company_name.trim());
        (
            "company-",
            if name.is_empty() {
                "unknown".to_string()
            } else {
                name
            },
        )
    };
    let id = format!("{prefix}{body}");
    // MAX_SOURCE_ID_BYTES 以内へ UTF-8 境界で切り詰め（接頭辞は必ず残る）。
    if id.len() <= MAX_SOURCE_ID_BYTES {
        return id;
    }
    let mut end = MAX_SOURCE_ID_BYTES;
    while end > 0 && !id.is_char_boundary(end) {
        end -= 1;
    }
    id[..end].to_string()
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
    namespace: KnowledgeNamespace,
) -> Result<Vec<DbHit>, String> {
    let pool = candidate_pool(limit);
    let primary_emb = match try_dense_passage_embed(llm, query) {
        Some(dense) => dense,
        None => lexical_hash_embed(query)?,
    };
    let primary_hits = vault
        .knowledge_search(primary_emb, pool, namespace)
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

/// Chunk → embed each → transactional vault replace. Shared blocking core for
/// [`ingest_knowledge`] and [`ingest_line_history`] (must run off the async
/// runtime — embedding is CPU-bound).
fn ingest_text_blocking(
    vault: &VaultHandle,
    llm: &LlmHandle,
    text: &str,
    source_id: &str,
) -> Result<IngestKnowledgeResult, String> {
    let chunks = chunk_markdown(text, source_id);
    if chunks.is_empty() {
        return Err("no chunks produced".into());
    }
    // chunk_markdown already truncates at MAX_CHUNKS; flag so callers can warn.
    let truncated = chunks.len() >= MAX_CHUNKS;

    let created_at = now_unix_secs();
    let mut rows = Vec::with_capacity(chunks.len());
    for chunk in &chunks {
        let embed_input = if chunk.title == source_id {
            chunk.text.clone()
        } else {
            format!("{}\n\n{}", chunk.title, chunk.text)
        };
        let embedding = embed_for_knowledge(llm, &embed_input)?;
        rows.push(KnowledgeChunkRow {
            id: chunk.id.clone(),
            text_content: chunk.text.clone(),
            embedding,
            created_at,
        });
    }

    let inserted = vault
        .knowledge_replace(source_id.to_string(), rows)
        .map_err(map_vault_err)?;

    Ok(IngestKnowledgeResult {
        source_id: source_id.to_string(),
        chunk_count: chunks.len(),
        inserted,
        truncated,
        part_count: 1,
    })
}

/// Body-only content hash (SHA-256 hex). Sole incremental-diff key.
///
/// Known limitation (intentional): hash is `chunk.text` only. If a heading
/// changes while the body is identical, the prior vector is reused — embed
/// cost reduction over title fidelity.
#[cfg_attr(not(feature = "egress-live"), allow(dead_code))]
fn chunk_content_hash(text: &str) -> String {
    use sha2::{Digest, Sha256};
    hex::encode(Sha256::digest(text.as_bytes()))
}

/// Resolve embeddings for chunks, reusing `prior` hits keyed by [`chunk_content_hash`].
/// Returns `(embeddings aligned to chunks, number of embed() calls)`.
///
/// Injectable `embed` enables unit tests to count calls without a live `LlmHandle`.
#[cfg_attr(not(feature = "egress-live"), allow(dead_code))]
fn resolve_incremental_embeddings<E>(
    chunks: &[super::chunk::TextChunk],
    source_id: &str,
    prior: &HashMap<String, Vec<f32>>,
    mut embed: E,
) -> Result<(Vec<Vec<f32>>, usize), String>
where
    E: FnMut(&str) -> Result<Vec<f32>, String>,
{
    let mut embeddings = Vec::with_capacity(chunks.len());
    let mut embed_calls = 0usize;
    for chunk in chunks {
        let embed_input = if chunk.title == source_id {
            chunk.text.clone()
        } else {
            format!("{}\n\n{}", chunk.title, chunk.text)
        };
        let hash = chunk_content_hash(&chunk.text);
        if let Some(existing) = prior.get(&hash) {
            embeddings.push(existing.clone());
        } else {
            embed_calls = embed_calls.saturating_add(1);
            embeddings.push(embed(&embed_input)?);
        }
    }
    Ok((embeddings, embed_calls))
}

/// Incremental ingest (Path A): list existing source rows → reuse embeddings by
/// body hash → replace all rows. DB write is cheap; embed is the expensive part.
#[cfg_attr(not(feature = "egress-live"), allow(dead_code))]
pub(crate) fn ingest_text_incremental_blocking(
    vault: &VaultHandle,
    llm: &LlmHandle,
    text: &str,
    source_id: &str,
) -> Result<IngestKnowledgeResult, String> {
    let chunks = chunk_markdown(text, source_id);
    if chunks.is_empty() {
        return Err("no chunks produced".into());
    }
    let truncated = chunks.len() >= MAX_CHUNKS;

    let existing = vault
        .knowledge_list_source(source_id.to_string())
        .map_err(map_vault_err)?;
    let mut prior: HashMap<String, Vec<f32>> = HashMap::new();
    for row in existing {
        // First-seen hash wins; later duplicate bodies share the same vector.
        prior
            .entry(chunk_content_hash(&row.text_content))
            .or_insert(row.embedding);
    }

    let (embeddings, _embed_calls) =
        resolve_incremental_embeddings(&chunks, source_id, &prior, |input| {
            embed_for_knowledge(llm, input)
        })?;

    let created_at = now_unix_secs();
    let mut rows = Vec::with_capacity(chunks.len());
    for (chunk, embedding) in chunks.iter().zip(embeddings.into_iter()) {
        rows.push(KnowledgeChunkRow {
            id: chunk.id.clone(),
            text_content: chunk.text.clone(),
            embedding,
            created_at,
        });
    }

    let inserted = vault
        .knowledge_replace(source_id.to_string(), rows)
        .map_err(map_vault_err)?;

    Ok(IngestKnowledgeResult {
        source_id: source_id.to_string(),
        chunk_count: chunks.len(),
        inserted,
        truncated,
        part_count: 1,
    })
}

/// Wikipedia lane → Company namespace `source_id` (`wiki-` prefix, same sanitize
/// rules as [`company_source_id`]). Prefix is applied after sanitize so it cannot
/// be trimmed away; runtime `namespace_of` must still be checked by the caller.
#[cfg_attr(not(feature = "egress-live"), allow(dead_code))]
pub(crate) fn wiki_source_id(title: &str) -> String {
    let mapped: String = title
        .chars()
        .map(|c| if matches!(c, '%' | '_' | ':') { '-' } else { c })
        .collect();
    let body = mapped.trim_matches('-');
    let body = if body.is_empty() { "unknown" } else { body };
    let id = format!("wiki-{body}");
    if id.len() <= MAX_SOURCE_ID_BYTES {
        return id;
    }
    let mut end = MAX_SOURCE_ID_BYTES;
    while end > 0 && !id.is_char_boundary(end) {
        end -= 1;
    }
    id[..end].to_string()
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
        ingest_text_blocking(&vault, &llm, &text, &source_id)
    })
    .await
    .map_err(|_| "ingest task join failed".to_string())?
}

/// Persist EDINET / injected company facts into the Company knowledge namespace.
///
/// Writer for `searchKnowledge(..., "company")`. Uses `edinet-` / `company-`
/// source_id prefixes so `namespace_of` classifies chunks as Company.
#[tauri::command]
pub async fn ingest_company_knowledge(
    app: AppHandle,
    vault: State<'_, VaultHandle>,
    llm: State<'_, LlmHandle>,
    facts: crate::knowledge::edinet_client::CompanyFacts,
) -> Result<IngestKnowledgeResult, String> {
    use crate::knowledge::edinet_client::{render_company_facts_block, sanitize_company_facts};

    // 外部由来テキストは常に sanitize（render_guard 規約）。
    let facts = sanitize_company_facts(&facts).map_err(|e| format!("{e:?}").to_ascii_lowercase())?;

    // 実知識が無いものは Company 空間に入れない（"（未取得）" チャンクで汚さない）。
    let has_knowledge = !facts.business_summary.trim().is_empty()
        || !facts.business_risks.trim().is_empty()
        || !facts.performance_summary.trim().is_empty();
    if !has_knowledge {
        return Ok(IngestKnowledgeResult {
            source_id: String::new(),
            chunk_count: 0,
            inserted: 0,
            truncated: false,
            part_count: 0,
        });
    }

    let source_id = company_source_id(&facts);
    validate_source_id(&source_id)?; // 生成物が規約準拠であることの最終保証（防御的）
    let markdown = render_company_facts_block(&facts);
    if markdown.is_empty() || markdown.len() > MAX_INGEST_TEXT_BYTES {
        return Err("invalid company knowledge text".into());
    }

    // 埋め込みには GGUF 常駐が必要（他の generate/embed 経路と同じ前提）。
    ensure_model_loaded(&app, &llm).await?;

    let vault = vault.inner().clone();
    let llm = llm.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        ingest_text_blocking(&vault, &llm, &markdown, &source_id)
    })
    .await
    .map_err(|_| "company ingest task join failed".to_string())?
}

/// Wipe prior LINE vault rows for this export (legacy single source + part family).
fn wipe_line_family(vault: &VaultHandle, base_source_id: &str) {
    let _ = vault.knowledge_replace(base_source_id.to_string(), Vec::new());
    for i in 0..MAX_LINE_PARTS {
        let part_id = format!("{base_source_id}-p{i:02}");
        let _ = vault.knowledge_replace(part_id, Vec::new());
    }
}

/// Embed a batch of chunks under `part_source_id` and vault-replace.
fn ingest_chunk_batch(
    vault: &VaultHandle,
    llm: &LlmHandle,
    part_source_id: &str,
    batch: &[super::chunk::TextChunk],
) -> Result<usize, String> {
    let created_at = now_unix_secs();
    let mut rows = Vec::with_capacity(batch.len());
    for (i, chunk) in batch.iter().enumerate() {
        let embed_input = if chunk.title.contains(" § ") {
            format!("{}\n\n{}", chunk.title, chunk.text)
        } else {
            chunk.text.clone()
        };
        let embedding = embed_for_knowledge(llm, &embed_input)?;
        rows.push(KnowledgeChunkRow {
            id: format!("{part_source_id}::{i:04}"),
            text_content: chunk.text.clone(),
            embedding,
            created_at,
        });
    }
    vault
        .knowledge_replace(part_source_id.to_string(), rows)
        .map_err(map_vault_err)
}

/// Shared blocking core after bytes are in memory (IPC payload or AppData file).
/// Full LINE body is chunked without the 128 soft-cap, then written as
/// `source_id-p00` … parts of ≤[`MAX_CHUNKS`] each (no silent head-only truncate).
fn ingest_line_bytes_blocking(
    vault: &VaultHandle,
    llm: &LlmHandle,
    bytes: &[u8],
    filename: &str,
    source_id: &str,
) -> Result<IngestKnowledgeResult, String> {
    use super::chunk::chunk_markdown_capped;

    let byte_len = bytes.len();
    let decoded = decode_line_export_bytes(bytes);
    let decoded_chars = decoded.chars().count();
    let formatted = format_line_import(&decoded, filename);
    if formatted.trim().is_empty() {
        log::error!(
            "ingest_line_history: blank after decode (bytes={byte_len}, decoded_chars={decoded_chars})"
        );
        return Err(line_err("BLANK"));
    }

    let all = chunk_markdown_capped(&formatted, source_id, MAX_LINE_CHUNK_ENUM);
    if all.is_empty() {
        return Err(line_err("NO_CHUNKS"));
    }
    let truncated = all.len() >= MAX_LINE_CHUNK_ENUM;
    if truncated {
        log::warn!(
            "ingest_line_history: hit MAX_LINE_CHUNK_ENUM={MAX_LINE_CHUNK_ENUM} (bytes={byte_len})"
        );
    }

    wipe_line_family(vault, source_id);

    let mut total_inserted = 0usize;
    let mut part_count = 0usize;
    for (part_idx, batch) in all.chunks(MAX_CHUNKS).enumerate() {
        if part_idx >= MAX_LINE_PARTS {
            break;
        }
        let part_id = format!("{source_id}-p{part_idx:02}");
        if let Err(e) = validate_source_id(&part_id) {
            log::error!("ingest_line_history: bad part source_id ({e})");
            return Err(line_err("BAD_SOURCE_ID"));
        }
        match ingest_chunk_batch(vault, llm, &part_id, batch) {
            Ok(n) => {
                total_inserted = total_inserted.saturating_add(n);
                part_count += 1;
            }
            Err(e) => {
                let code = if e.contains("locked") {
                    "VAULT_LOCKED"
                } else if e.contains("embed") || e.contains("model") {
                    "EMBED"
                } else {
                    log::error!(
                        "ingest_line_history: part {part_idx} failed ({e}) bytes={byte_len}"
                    );
                    "PIPELINE"
                };
                return Err(line_err(code));
            }
        }
    }

    log::info!(
        "ingest_line_history: ok (base={source_id}, parts={part_count}, chunks={}, inserted={total_inserted}, bytes={byte_len})",
        all.len().min(MAX_LINE_PARTS.saturating_mul(MAX_CHUNKS))
    );

    Ok(IngestKnowledgeResult {
        source_id: if part_count == 1 {
            format!("{source_id}-p00")
        } else {
            source_id.to_string()
        },
        chunk_count: total_inserted,
        inserted: total_inserted,
        truncated,
        part_count,
    })
}

/// On-device LINE トーク履歴 (.txt) import (M20 データ連携 Part 1).
///
/// iOS has no Python sidecar (`engine.rs` boot is `#[cfg(not(mobile))]`), so
/// the desktop-only `import_line_batch` path never runs there. This command
/// decodes the raw file bytes itself (BOM-aware UTF-8/UTF-16/Shift-JIS —
/// never trusts the frontend to have decoded correctly, and never panics on
/// malformed input), then reuses the same chunk → embed → vault pipeline as
/// [`ingest_knowledge`] so the content becomes searchable by on-device
/// CONSULT/RAG immediately. `source_id` is derived from the filename so
/// re-importing the same export replaces rather than duplicates.
///
/// Prefer [`ingest_line_history_path`] for multi-hundred-KB exports — shipping
/// `Vec<u8>` as a JSON number array OOMs / Jetsams WKWebView on device.
#[tauri::command]
pub async fn ingest_line_history(
    vault: State<'_, VaultHandle>,
    llm: State<'_, LlmHandle>,
    bytes: Vec<u8>,
    filename: String,
) -> Result<IngestKnowledgeResult, String> {
    let byte_len = bytes.len();
    if bytes.is_empty() {
        log::error!("ingest_line_history: empty file (bytes=0)");
        return Err(line_err("EMPTY"));
    }
    if byte_len > MAX_LINE_INGEST_BYTES {
        log::error!(
            "ingest_line_history: file too large (bytes={byte_len}, max={MAX_LINE_INGEST_BYTES})"
        );
        return Err(line_err("TOO_LARGE"));
    }
    let source_id = line_source_id(&filename);
    if let Err(e) = validate_source_id(&source_id) {
        log::error!("ingest_line_history: bad source_id after sanitize ({e})");
        return Err(line_err("BAD_SOURCE_ID"));
    }

    let vault = vault.inner().clone();
    let llm = llm.inner().clone();

    tauri::async_runtime::spawn_blocking(move || {
        ingest_line_bytes_blocking(&vault, &llm, &bytes, &filename, &source_id)
    })
    .await
    .map_err(|e| {
        log::error!("ingest_line_history: join failed ({e})");
        line_err("JOIN")
    })?
}

/// Same pipeline as [`ingest_line_history`], but bytes are read from a staged
/// file under `$APPDATA/imports/…` (written by FE `plugin-fs`). Avoids
/// serializing multi-MB LINE exports as JSON `number[]` over IPC.
#[tauri::command]
pub async fn ingest_line_history_path(
    app: AppHandle,
    vault: State<'_, VaultHandle>,
    llm: State<'_, LlmHandle>,
    relative_path: String,
    filename: String,
) -> Result<IngestKnowledgeResult, String> {
    let rel = relative_path.trim().replace('\\', "/");
    if rel.is_empty()
        || rel.contains("..")
        || rel.starts_with('/')
        || !rel.starts_with("imports/")
    {
        log::error!("ingest_line_history_path: rejected relative_path");
        return Err(line_err("BAD_PATH"));
    }
    let source_id = line_source_id(&filename);
    if let Err(e) = validate_source_id(&source_id) {
        log::error!("ingest_line_history_path: bad source_id ({e})");
        return Err(line_err("BAD_SOURCE_ID"));
    }

    let app_data = app
        .path()
        .app_data_dir()
        .map_err(|e| {
            log::error!("ingest_line_history_path: app_data_dir failed: {e}");
            line_err("BAD_PATH")
        })?;
    let full = app_data.join(&rel);
    // Ensure resolved path stays inside app_data (no symlink escape).
    let app_canon = app_data.canonicalize().unwrap_or(app_data.clone());
    let full_canon = full.canonicalize().map_err(|e| {
        log::error!("ingest_line_history_path: canonicalize failed: {e}");
        line_err("READ")
    })?;
    if !full_canon.starts_with(&app_canon) {
        log::error!("ingest_line_history_path: path escaped app_data");
        return Err(line_err("BAD_PATH"));
    }

    let vault = vault.inner().clone();
    let llm = llm.inner().clone();
    let filename_owned = filename;

    tauri::async_runtime::spawn_blocking(move || {
        let meta = std::fs::metadata(&full_canon).map_err(|e| {
            log::error!("ingest_line_history_path: metadata failed: {e}");
            line_err("READ")
        })?;
        let byte_len = meta.len() as usize;
        if byte_len == 0 {
            return Err(line_err("EMPTY"));
        }
        if byte_len > MAX_LINE_INGEST_BYTES {
            log::error!(
                "ingest_line_history_path: file too large (bytes={byte_len}, max={MAX_LINE_INGEST_BYTES})"
            );
            return Err(line_err("TOO_LARGE"));
        }
        let bytes = std::fs::read(&full_canon).map_err(|e| {
            log::error!("ingest_line_history_path: read failed: {e}");
            line_err("READ")
        })?;
        ingest_line_bytes_blocking(&vault, &llm, &bytes, &filename_owned, &source_id)
    })
    .await
    .map_err(|e| {
        log::error!("ingest_line_history_path: join failed ({e})");
        line_err("JOIN")
    })?
}

/// Hybrid recall over `knowledge_chunks` (RRF + Ebbinghaus).
#[tauri::command]
pub async fn search_knowledge(
    vault: State<'_, VaultHandle>,
    llm: State<'_, LlmHandle>,
    query: String,
    limit: Option<u32>,
    namespace: Option<String>,
) -> Result<SearchKnowledgeResult, String> {
    let limit = limit.unwrap_or(DEFAULT_SEARCH_LIMIT);
    if !(1..=MAX_SEARCH_LIMIT).contains(&limit) {
        return Err("invalid limit".into());
    }
    let ns = parse_namespace_arg(namespace.as_deref())?;
    let query = query.trim().to_string();
    if query.is_empty() || query.len() > MAX_INGEST_TEXT_BYTES {
        return Err("invalid query".into());
    }

    let vault = vault.inner().clone();
    let llm = llm.inner().clone();

    tauri::async_runtime::spawn_blocking(move || {
        let hits = search_sync(&vault, &llm, &query, limit, ns)?;
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
    app: AppHandle,
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

    if !matches!(vault.status(), VaultStatus::Unlocked) {
        let _ = on_token.send(error_done_event(0, ERR_VAULT_LOCKED.to_string()));
        return Ok(SendRagChatResult {
            context_count: 0,
            context_ids: Vec::new(),
        });
    }

    // Jetsam / cold start: ensure GGUF is resident before generate (else sterile
    // MODEL_NOT_LOADED — never a misleading "format mismatch" catch-all alone).
    let llm_probe = llm.inner().clone();
    let loaded = tauri::async_runtime::spawn_blocking(move || llm_probe.is_loaded())
        .await
        .map_err(|_| "llm ready probe join failed".to_string())?
        .unwrap_or(false);
    if !loaded {
        let path = match resolve_loadable_model_path(&app) {
            Ok(p) => p,
            Err(e) => {
                log::error!("send_rag_chat: model path resolve failed: {e}");
                let _ = on_token.send(error_done_event(0, ERR_MODEL_NOT_LOADED.to_string()));
                return Ok(SendRagChatResult {
                    context_count: 0,
                    context_ids: Vec::new(),
                });
            }
        };
        let load_params = LoadParams {
            n_gpu_layers: 999,
            use_mmap: true,
        };
        let llm_load = llm.inner().clone();
        if let Err(e) = tauri::async_runtime::spawn_blocking(move || llm_load.load(path, load_params))
            .await
            .map_err(|_| "llm load join failed".to_string())?
        {
            log::error!("send_rag_chat: auto-load failed: {e}");
            let _ = on_token.send(error_done_event(0, ERR_MODEL_NOT_LOADED.to_string()));
            return Ok(SendRagChatResult {
                context_count: 0,
                context_ids: Vec::new(),
            });
        }
    }

    let n_ctx = opts.n_ctx.unwrap_or(DEFAULT_RAG_N_CTX);
    let max_tokens_for_gen = opts.max_tokens.unwrap_or(DEFAULT_RAG_MAX_TOKENS);

    let vault = vault.inner().clone();
    let llm_for_search = llm.inner().clone();
    let governor = llm.inner().governor();
    let message_for_search = message.clone();

    let (prompt, context_ids) = tauri::async_runtime::spawn_blocking(move || {
        let hits = search_sync(
            &vault,
            &llm_for_search,
            &message_for_search,
            context_limit,
            KnowledgeNamespace::All,
        )
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
        let prompt = match crate::llm::consult_context::load_mentor_context(&vault) {
            Ok(mentor) => {
                crate::llm::consult_context::append_mentor_sections(&rag_prompt, &mentor)
            }
            Err(_) => rag_prompt,
        };

        // Final cross-section guard + real tokenizer verify (see prompt_budget).
        let (prompt, _prompt_tokens) = fit_and_verify_prompt(
            &llm_for_search,
            governor.as_ref(),
            prompt,
            n_ctx,
            max_tokens_for_gen,
            &["## ユーザーの質問"],
            "send_rag_chat",
        );

        let ids: Vec<String> = hits.into_iter().map(|hit| hit.id).collect();
        Ok::<_, String>((prompt, ids))
    })
    .await
    .map_err(|_| "rag retrieve task join failed".to_string())??;

    let gen = GenerationParams {
        prompt,
        n_ctx,
        max_tokens: max_tokens_for_gen,
        temp: opts.temp.unwrap_or(0.7),
        top_k: opts.top_k.unwrap_or(40),
        top_p: opts.top_p.unwrap_or(0.95),
        seed: opts.seed.unwrap_or(0),
    };

    llm.generate(gen, None, on_token).await?;

    Ok(SendRagChatResult {
        context_count: context_ids.len(),
        context_ids,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn line_source_id_sanitizes_disallowed_chars() {
        let id = line_source_id("2024_01_15%talk::export.txt");
        assert!(validate_source_id(&id).is_ok());
        assert!(!id.contains('_'));
        assert!(!id.contains('%'));
        assert!(!id.contains("::"));
        assert!(id.starts_with("line-"));
    }

    #[test]
    fn line_source_id_falls_back_when_stem_empty_after_sanitizing() {
        // A stem made entirely of disallowed chars sanitizes to "" (after
        // trim_matches('-')) — must fall back rather than validate-fail.
        let id = line_source_id("___.txt");
        assert_eq!(id, "line-import");
        assert!(validate_source_id(&id).is_ok());
    }

    #[test]
    fn line_source_id_is_stable_for_reimport_of_same_file() {
        assert_eq!(line_source_id("family_chat.txt"), line_source_id("family_chat.txt"));
    }

    #[test]
    fn line_source_id_respects_byte_cap_at_char_boundary() {
        let long_stem = "あ".repeat(MAX_SOURCE_ID_BYTES); // 3 bytes/char in utf-8
        let id = line_source_id(&format!("{long_stem}.txt"));
        assert!(id.len() <= MAX_SOURCE_ID_BYTES);
        assert!(validate_source_id(&id).is_ok());
        // Must not have been truncated mid-codepoint (would panic on slice).
        assert!(id.chars().all(|c| c == 'あ' || c == '-' || c == 'l' || c == 'i' || c == 'n' || c == 'e'));
    }

    #[test]
    fn company_source_id_is_company_namespace() {
        use crate::db::knowledge_namespace::{namespace_of, KnowledgeNamespace};
        use crate::knowledge::edinet_client::CompanyFacts;

        let f = CompanyFacts {
            edinet_code: "E00001".into(),
            company_name: "マッキンゼー".into(),
            ..Default::default()
        };
        let sid = company_source_id(&f);
        assert!(sid.starts_with("edinet-"));
        assert_eq!(
            namespace_of(&format!("{sid}::0000")),
            KnowledgeNamespace::Company
        );
        assert!(validate_source_id(&sid).is_ok());

        // コードなし → company- 接頭辞、記号は sanitize、判定は Company。
        let f2 = CompanyFacts {
            edinet_code: "".into(),
            company_name: "A_B:C%D".into(),
            ..Default::default()
        };
        let sid2 = company_source_id(&f2);
        assert!(sid2.starts_with("company-"));
        assert!(!sid2.contains('_') && !sid2.contains('%') && !sid2.contains(':'));
        assert_eq!(
            namespace_of(&format!("{sid2}::0000")),
            KnowledgeNamespace::Company
        );
        assert!(validate_source_id(&sid2).is_ok());
    }

    #[test]
    fn chunk_content_hash_stable_and_distinct() {
        let a = chunk_content_hash("同一本文");
        let b = chunk_content_hash("同一本文");
        let c = chunk_content_hash("別本文");
        assert_eq!(a, b);
        assert_ne!(a, c);
        assert_eq!(a.len(), 64);
    }

    #[test]
    fn incremental_second_pass_embeds_zero_times() {
        let source_id = "wiki-inc-test";
        let text = "## 概要\n本文Aです。\n\n## 沿革\n本文Bです。";
        let chunks = chunk_markdown(text, source_id);
        assert!(chunks.len() >= 2);

        let mut calls = 0usize;
        let (emb1, n1) = resolve_incremental_embeddings(
            &chunks,
            source_id,
            &HashMap::new(),
            |_| {
                calls += 1;
                Ok(vec![0.1f32; 384])
            },
        )
        .unwrap();
        assert_eq!(n1, chunks.len());
        assert_eq!(calls, chunks.len());

        let mut prior = HashMap::new();
        for (chunk, emb) in chunks.iter().zip(emb1.iter()) {
            prior.insert(chunk_content_hash(&chunk.text), emb.clone());
        }

        calls = 0;
        let (_emb2, n2) = resolve_incremental_embeddings(
            &chunks,
            source_id,
            &prior,
            |_| {
                calls += 1;
                Ok(vec![0.9f32; 384])
            },
        )
        .unwrap();
        assert_eq!(n2, 0, "second pass must reuse all embeddings");
        assert_eq!(calls, 0);
    }

    #[test]
    fn incremental_position_shift_reuses_existing_bodies() {
        let source_id = "wiki-shift-test";
        let text1 = "## 概要\n不変本文です。\n\n## 沿革\n別の不変本文。";
        let text2 = "## 新節\n新規だけ埋め込む。\n\n## 概要\n不変本文です。\n\n## 沿革\n別の不変本文。";
        let chunks1 = chunk_markdown(text1, source_id);
        let chunks2 = chunk_markdown(text2, source_id);
        assert!(chunks2.len() > chunks1.len());

        let mut prior = HashMap::new();
        let (emb1, _) = resolve_incremental_embeddings(&chunks1, source_id, &prior, |_| {
            Ok(vec![0.25f32; 384])
        })
        .unwrap();
        for (chunk, emb) in chunks1.iter().zip(emb1.iter()) {
            prior.insert(chunk_content_hash(&chunk.text), emb.clone());
        }

        let mut calls = 0usize;
        let (_emb2, n2) = resolve_incremental_embeddings(&chunks2, source_id, &prior, |_| {
            calls += 1;
            Ok(vec![0.5f32; 384])
        })
        .unwrap();
        // Only the newly inserted section body should embed.
        assert_eq!(n2, 1, "only the new section should embed; got {n2}");
        assert_eq!(calls, 1);
    }

    #[test]
    fn wiki_source_id_namespace_and_sanitize_cases() {
        use crate::db::knowledge_namespace::{namespace_of, KnowledgeNamespace};

        let cases = [
            ("Toyota", true),
            ("A%B_C:D", true),
            ("", true), // falls back to wiki-unknown
            ("マッキンゼー", true),
            (&"あ".repeat(400), true),
        ];
        for (title, _) in cases {
            let sid = wiki_source_id(title);
            assert!(
                sid.starts_with("wiki-"),
                "prefix must survive sanitize: {sid}"
            );
            assert!(validate_source_id(&sid).is_ok(), "sid={sid}");
            assert_eq!(
                namespace_of(&format!("{sid}::0000")),
                KnowledgeNamespace::Company
            );
            assert!(!sid.contains('%') && !sid.contains('_') && !sid.contains(':'));
            assert!(sid.len() <= MAX_SOURCE_ID_BYTES);
        }
    }
}
