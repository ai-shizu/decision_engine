//! M17 mentor consult command — RAG + forced Gap/Oracle injection.
//!
//! Phase 6: Twin `R(t)` / `p_lapse` → deterministic ZPD mentor intensity
//! (Vygotsky 1978 / Yerkes–Dodson 1908 / Bjork 1994) before prompt + temp apply.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use tauri::ipc::Channel;
use tauri::{AppHandle, State};

use crate::db::VaultHandle;
use crate::llm::consult_context::{
    build_consult_with_oracle_prompt, format_profile_block, load_mentor_context,
};
use crate::llm::mentor_zpd::{load_mentor_zpd_signal, MentorZpdSignal};
use crate::llm::model_path::resolve_loadable_model_path;
use crate::llm::params::{GenerationParams, LoadParams};
use crate::llm::prompt_budget::fit_and_verify_prompt;
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
    /// SETTINGS fixed-attributes (birthday/gender/height/weight/address/occupation/…)
    /// as currently held by the frontend (localStorage on mobile / engine cache on
    /// desktop — FE is the single source since this Vault has no settings table).
    /// Optional/empty ⇒ profile section omitted from the prompt.
    pub profile: Option<BTreeMap<String, String>>,
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
    /// Phase 6 ZPD ladder level (`depleted` / `neutral` / `high_resource`).
    pub mentor_zpd_level: String,
    pub mentor_zpd_temperature: f32,
    pub mentor_zpd_twin_available: bool,
}

/// Probe the worker and load the on-device GGUF if it is not resident, so the
/// caller can `generate()` safely. Same load policy as `send_rag_chat`'s inline
/// guard (full GPU offload + mmap). Returns `Err` only when no loadable model
/// exists or the load itself fails — the caller surfaces that to the UI.
///
/// `pub(crate)`: also used by `llm::commands_sim` (面接/ES review) — those
/// commands called `llm.generate()` completely blind (no load check at all,
/// not even the guard-less version consult used to have), the same class of
/// bug this module's own guard was added to fix.
pub(crate) async fn ensure_model_loaded(app: &AppHandle, llm: &LlmHandle) -> Result<(), String> {
    let llm_probe = llm.clone();
    let probe_result = tauri::async_runtime::spawn_blocking(move || llm_probe.is_loaded())
        .await
        .map_err(|e| {
            log::error!("consult: llm ready probe task panicked/join failed: {e}");
            "llm ready probe join failed".to_string()
        })?;
    // A probe error (e.g. timeout — service.rs already logs the specifics)
    // is treated as "not confirmed loaded" rather than aborting, so consult
    // still attempts a fresh load; log it so the timeout is visible here too,
    // not just as an unexplained extra load attempt.
    let loaded = probe_result.unwrap_or_else(|e| {
        log::error!("consult: llm ready probe returned error, assuming not loaded: {e}");
        false
    });
    if loaded {
        return Ok(());
    }
    let path = resolve_loadable_model_path(app).map_err(|e| {
        log::error!("consult: resolve_loadable_model_path failed: {e}");
        e
    })?;
    let load_params = LoadParams {
        n_gpu_layers: 999,
        use_mmap: true,
    };
    let llm_load = llm.clone();
    tauri::async_runtime::spawn_blocking(move || llm_load.load(path, load_params))
        .await
        .map_err(|e| {
            log::error!("consult: llm load task panicked/join failed: {e}");
            "llm load join failed".to_string()
        })?
}

/// Mentor consult: vault Gap+Oracle (fail-safe) + optional RAG → streaming LLM.
#[tauri::command]
pub async fn consult_with_oracle_context(
    app: AppHandle,
    vault: State<'_, VaultHandle>,
    llm: State<'_, LlmHandle>,
    params: ConsultWithOracleParams,
    on_token: Channel<TokenEvent>,
) -> Result<ConsultWithOracleResult, String> {
    let message = params.message.trim().to_string();
    if message.is_empty() || message.len() > MAX_MESSAGE_BYTES {
        log::error!(
            "consult: rejected message (len={}, empty={})",
            message.len(),
            message.is_empty()
        );
        return Err("invalid message".into());
    }
    log::info!("consult: request received (message_len={})", message.len());

    // Ensure the GGUF is resident before generate. A cold CONSULT navigation or
    // a Jetsam eviction leaves the worker unloaded; unlike `send_rag_chat`, this
    // path used to call `generate()` blind → the model errored and the user saw
    // a sterile "応答を生成できませんでした". Mirror the RAG-chat auto-load so
    // CONSULT recovers on its own instead of failing.
    ensure_model_loaded(&app, &llm).await?;
    log::info!("consult: model confirmed loaded, proceeding to prompt build");

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
    let profile_block = format_profile_block(&params.profile.unwrap_or_default());

    let vault = vault.inner().clone();
    let llm_search = llm.inner().clone();
    let message_for_search = message.clone();
    let n_ctx = opts.n_ctx.unwrap_or(DEFAULT_N_CTX);
    let max_tokens = opts.max_tokens.unwrap_or(DEFAULT_MAX_TOKENS);
    let governor = llm.inner().governor();

    let (prompt, verified_tokens, meta) = tauri::async_runtime::spawn_blocking(move || {
        // Fail-safe mentor load: soft sections on missing data; hard error only on vault IPC.
        let mentor = load_mentor_context(&vault).unwrap_or_else(|e| {
            log::error!("consult: load_mentor_context failed, using empty sections: {e}");
            Default::default()
        });
        // Twin missing ⇒ Neutral soft-default; vault transport error ⇒ same (never hard-fail consult).
        let zpd = load_mentor_zpd_signal(&vault).unwrap_or_else(|e| {
            log::error!("consult: load_mentor_zpd_signal failed, using neutral default: {e}");
            MentorZpdSignal::neutral_default()
        });
        let mut context_ids = Vec::new();
        let prompt = if include_rag {
            let hits = search_sync(
                &vault,
                &llm_search,
                &message_for_search,
                context_limit,
                crate::rag::namespace::KnowledgeNamespace::All,
            )
                .unwrap_or_else(|e| {
                    log::error!("consult: search_sync failed, proceeding without RAG context: {e}");
                    Vec::new()
                });
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
            build_consult_with_oracle_prompt(
                &message_for_search,
                &mentor,
                &rag_only,
                &zpd,
                &profile_block,
            )
        } else {
            build_consult_with_oracle_prompt(&message_for_search, &mentor, "", &zpd, &profile_block)
        };
        let (prompt, verified_tokens) = fit_and_verify_prompt(
            &llm_search,
            governor.as_ref(),
            prompt,
            n_ctx,
            max_tokens,
            &["## ユーザーの相談"],
            "consult",
        );
        Ok::<_, String>((
            prompt,
            verified_tokens,
            (
                context_ids,
                mentor.gap_available,
                mentor.oracle_available,
                mentor.gap_run_id,
                mentor.oracle_run_id,
                zpd,
            ),
        ))
    })
    .await
    .map_err(|e| {
        log::error!("consult: prompt-build task panicked/join failed: {e}");
        "consult retrieve task join failed".to_string()
    })?
    .map_err(|e| {
        log::error!("consult: prompt-build task returned error: {e}");
        e
    })?;

    let (context_ids, gap_available, oracle_available, gap_run_id, oracle_run_id, zpd) = meta;
    log::info!(
        "consult: prompt built (len={}, context_count={}, gap_available={gap_available}, oracle_available={oracle_available})",
        prompt.len(),
        context_ids.len()
    );

    // ZPD temperature is the deterministic default; explicit FE `temp` overrides (debug/tests).
    let gen = GenerationParams {
        prompt,
        n_ctx,
        max_tokens,
        temp: opts.temp.unwrap_or(zpd.temperature),
        top_k: opts.top_k.unwrap_or(40),
        top_p: opts.top_p.unwrap_or(0.95),
        seed: opts.seed.unwrap_or(0),
    };
    log::info!(
        "consult: calling llm.generate (n_ctx={}, max_tokens={}, temp={}, prompt_tokens={:?})",
        gen.n_ctx,
        gen.max_tokens,
        gen.temp,
        verified_tokens
    );
    llm.generate(gen, None, on_token).await.map_err(|e| {
        log::error!("consult: llm.generate failed: {e}");
        e
    })?;
    log::info!("consult: llm.generate completed successfully");

    Ok(ConsultWithOracleResult {
        context_count: context_ids.len(),
        context_ids,
        gap_available,
        oracle_available,
        gap_run_id,
        oracle_run_id,
        mentor_zpd_level: zpd.level.as_str().to_string(),
        mentor_zpd_temperature: zpd.temperature,
        mentor_zpd_twin_available: zpd.twin_available,
    })
}
