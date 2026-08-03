//! M13 daily-context sync command: merge → chunk → embed → vault replace.
//!
//! Updates for one calendar day run inside `VaultHandle::knowledge_replace`
//! (single SQLCipher transaction: DELETE `daily-{date}::%` then INSERT).

use std::time::{SystemTime, UNIX_EPOCH};

use serde::Serialize;
use tauri::State;

use crate::db::{KnowledgeChunkRow, VaultErrorCode, VaultHandle};
use crate::knowledge::context_merger::{
    build_daily_context_markdown, daily_source_id, MergeError,
};
use crate::llm::LlmHandle;
use crate::rag::chunk::{chunk_markdown, MAX_CHUNKS};
use crate::rag::embed_knowledge::embed_for_knowledge;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct SyncDailyContextResult {
    pub date: String,
    pub source_id: String,
    pub chunk_count: usize,
    pub inserted: usize,
    pub markdown_bytes: usize,
}

fn map_vault_err(code: VaultErrorCode) -> String {
    format!("{code:?}").to_ascii_lowercase()
}

fn map_merge_err(err: MergeError) -> String {
    err.to_string()
}

fn now_unix_secs() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// Merge calendar JSON + daily log into Daily Context Markdown, then ingest
/// into `knowledge_chunks` under `source_id = daily-{YYYY-MM-DD}` (upsert).
#[tauri::command]
pub async fn sync_daily_context(
    vault: State<'_, VaultHandle>,
    llm: State<'_, LlmHandle>,
    date_str: String,
    events_json: String,
    daily_log: String,
) -> Result<SyncDailyContextResult, String> {
    let markdown = build_daily_context_markdown(&date_str, &events_json, &daily_log)
        .map_err(map_merge_err)?;
    let source_id = daily_source_id(&date_str).map_err(map_merge_err)?;
    let date = match source_id.strip_prefix("daily-") {
        Some(d) => d.to_string(),
        None => date_str.trim().to_string(),
    };
    let markdown_bytes = markdown.len();

    let vault = vault.inner().clone();
    let llm = llm.inner().clone();

    tauri::async_runtime::spawn_blocking(move || {
        let chunks = chunk_markdown(&markdown, &source_id);
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

        Ok(SyncDailyContextResult {
            date,
            source_id,
            chunk_count: chunks.len(),
            inserted,
            markdown_bytes,
        })
    })
    .await
    .map_err(|_| "sync_daily_context task join failed".to_string())?
}
