//! Tauri commands for local RAG ingest + search (M10).
//!
//! Heavy work runs on `spawn_blocking` so the async runtime / UI thread is not
//! blocked by the embed loop. Embeddings go through `LlmHandle` (LLM worker);
//! SQL goes through `VaultHandle` (vault worker) — never from the main thread.

use std::time::{SystemTime, UNIX_EPOCH};

use serde::Serialize;
use tauri::State;

use super::chunk::{chunk_markdown, MAX_CHUNKS};
use crate::db::{KnowledgeChunkRow, VaultErrorCode, VaultHandle};
use crate::llm::embed::{require_knowledge_embedding_dims, EMBED_DEFAULT_N_CTX};
use crate::llm::LlmHandle;

const MAX_SOURCE_ID_BYTES: usize = 512;
const MAX_INGEST_TEXT_BYTES: usize = 512 * 1024;
const DEFAULT_SEARCH_LIMIT: u32 = 5;
const MAX_SEARCH_LIMIT: u32 = 50;

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
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct SearchKnowledgeResult {
    pub hits: Vec<SearchKnowledgeHit>,
}

fn map_vault_err(code: VaultErrorCode) -> String {
    // Stable, non-sensitive codes only (no SQL / paths).
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
            // Prefix title into embed input so section headings influence retrieval.
            let embed_input = if chunk.title == source_id {
                chunk.text.clone()
            } else {
                format!("{}\n\n{}", chunk.title, chunk.text)
            };
            let embedding = llm.embed(embed_input, EMBED_DEFAULT_N_CTX)?;
            require_knowledge_embedding_dims(&embedding)?;
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

/// Embed `query` and run sqlite-vec KNN over `knowledge_chunks`.
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
        let embedding = llm.embed(query, EMBED_DEFAULT_N_CTX)?;
        require_knowledge_embedding_dims(&embedding)?;
        let hits = vault
            .knowledge_search(embedding, limit)
            .map_err(map_vault_err)?;
        Ok(SearchKnowledgeResult {
            hits: hits
                .into_iter()
                .map(|hit| SearchKnowledgeHit {
                    id: hit.id,
                    text_content: hit.text_content,
                    distance: hit.distance,
                })
                .collect(),
        })
    })
    .await
    .map_err(|_| "search task join failed".to_string())?
}
