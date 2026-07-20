//! M17 mentor consult command — RAG + forced Gap/Oracle injection.

use serde::{Deserialize, Serialize};
use tauri::ipc::Channel;
use tauri::State;

use crate::db::VaultHandle;
use crate::llm::consult_context::{build_consult_with_oracle_prompt, load_mentor_context};
use crate::llm::params::GenerationParams;
use crate::llm::service::TokenEvent;
use crate::llm::LlmHandle;
use crate::rag::commands_rag::{search_sync, RagChatParams};
use crate::rag::prompt::{build_rag_prompt, RagContextRef};

const MAX_MESSAGE_BYTES: usize = 64 * 1024;
const DEFAULT_N_CTX: u32 = 2048;
const DEFAULT_MAX_TOKENS: u32 = 384;
const DEFAULT_CONTEXT_LIMIT: u32 = 5;

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConsultWithOracleParams {
    pub message: String,
    pub gen: Option<RagChatParams>,
    /// When true (default), retrieve RAG chunks before mentor sections.
    pub include_rag: Option<bool>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct ConsultWithOracleResult {
    pub context_ids: Vec<String>,
    pub context_count: usize,
    pub gap_available: bool,
    pub oracle_available: bool,
    pub gap_run_id: Option<String>,
    pub oracle_run_id: Option<String>,
}

/// Mentor consult: vault Gap+Oracle (fail-safe) + optional RAG → streaming LLM.
#[tauri::command]
pub async fn consult_with_oracle_context(
    vault: State<'_, VaultHandle>,
    llm: State<'_, LlmHandle>,
    params: ConsultWithOracleParams,
    on_token: Channel<TokenEvent>,
) -> Result<ConsultWithOracleResult, String> {
    let message = params.message.trim().to_string();
    if message.is_empty() || message.len() > MAX_MESSAGE_BYTES {
        return Err("invalid message".into());
    }
    let include_rag = params.include_rag.unwrap_or(true);
    let opts = params.gen.unwrap_or(RagChatParams {
        n_ctx: None,
        max_tokens: None,
        temp: None,
        top_k: None,
        top_p: None,
        seed: None,
        context_limit: None,
    });
    let context_limit = opts.context_limit.unwrap_or(DEFAULT_CONTEXT_LIMIT).clamp(1, 50);

    let vault = vault.inner().clone();
    let llm_search = llm.inner().clone();
    let message_for_search = message.clone();

    let (prompt, meta) = tauri::async_runtime::spawn_blocking(move || {
        // Fail-safe mentor load: soft sections on missing data; hard error only on vault IPC.
        let mentor = load_mentor_context(&vault).unwrap_or_default();
        let mut context_ids = Vec::new();
        let prompt = if include_rag {
            let hits = search_sync(&vault, &llm_search, &message_for_search, context_limit)
                .unwrap_or_default();
            let refs: Vec<RagContextRef<'_>> = hits
                .iter()
                .map(|hit| RagContextRef {
                    id: hit.id.as_str(),
                    text: hit.text_content.as_str(),
                })
                .collect();
            let rag = build_rag_prompt(&message_for_search, &refs);
            // Strip preamble duplication: rebuild with mentor-first layout.
            let rag_only = {
                // Extract ## 参考情報 … before ユーザーの質問
                if let (Some(a), Some(b)) = (rag.find("## 参考情報"), rag.find("## ユーザーの質問"))
                {
                    rag[a..b].to_string()
                } else {
                    String::new()
                }
            };
            context_ids = hits.into_iter().map(|h| h.id).collect();
            build_consult_with_oracle_prompt(&message_for_search, &mentor, &rag_only)
        } else {
            build_consult_with_oracle_prompt(&message_for_search, &mentor, "")
        };
        Ok::<_, String>((
            prompt,
            (
                context_ids,
                mentor.gap_available,
                mentor.oracle_available,
                mentor.gap_run_id,
                mentor.oracle_run_id,
            ),
        ))
    })
    .await
    .map_err(|_| "consult retrieve task join failed".to_string())??;

    let (context_ids, gap_available, oracle_available, gap_run_id, oracle_run_id) = meta;

    let gen = GenerationParams {
        prompt,
        n_ctx: opts.n_ctx.unwrap_or(DEFAULT_N_CTX),
        max_tokens: opts.max_tokens.unwrap_or(DEFAULT_MAX_TOKENS),
        temp: opts.temp.unwrap_or(0.6),
        top_k: opts.top_k.unwrap_or(40),
        top_p: opts.top_p.unwrap_or(0.95),
        seed: opts.seed.unwrap_or(0),
    };
    llm.generate(gen, None, on_token)?;

    Ok(ConsultWithOracleResult {
        context_count: context_ids.len(),
        context_ids,
        gap_available,
        oracle_available,
        gap_run_id,
        oracle_run_id,
    })
}
