//! Shared "heuristic fit → real tokenizer verify → iterate" loop for LLM prompts.
//!
//! Lifted from `send_rag_chat` (2026-07-24 device bug: char heuristic undercounted
//! CJK/rare tokens → `generate()` died with `prompt exceeds context budget`).
//! Interview / ES / RAG must all call this before `llm.generate()`.

use super::context_budget::{fit_prompt_to_budget, fit_prompt_to_budget_with_markers};
use super::params::MIN_N_CTX;
use super::service::{LlmHandle, LlmMemoryGovernor};

const MAX_FIT_ATTEMPTS: u32 = 5;
const RAG_USER_MARKER: &str = "## ユーザーの質問";

fn fit_with_markers(prompt: &str, available_tokens: usize, markers: &[&str]) -> String {
    // Keep `fit_prompt_to_budget` live outside #[cfg(test)] (historical RAG/consult entry).
    if markers.len() == 1 && markers[0] == RAG_USER_MARKER {
        fit_prompt_to_budget(prompt, available_tokens)
    } else {
        fit_prompt_to_budget_with_markers(prompt, available_tokens, markers)
    }
}

/// Fit `prompt` into the same input budget `generate()` will enforce, verifying
/// with the resident model's real tokenizer (`count_tokens`).
///
/// Returns `(fitted_prompt, verified_tokens)` where `verified_tokens` is the last
/// successful `count_tokens` reading that fit the budget (`None` if unconverged
/// or the tokenizer probe failed).
///
/// `markers` identify the never-truncated user/candidate turn (see
/// [`super::context_budget::USER_TURN_MARKERS`]).
pub fn fit_and_verify_prompt(
    llm: &LlmHandle,
    governor: &LlmMemoryGovernor,
    prompt: String,
    n_ctx: u32,
    max_tokens: u32,
    markers: &[&str],
    tag: &str,
) -> (String, Option<usize>) {
    // n_ctx=0（既定値センチネル）等が渡っても clamp(min>max) で panic しないよう下限保証。
    let n_ctx = n_ctx.max(MIN_N_CTX);
    let context_factor = governor.degradation().context_factor();
    let scaled_ctx = ((n_ctx as f64) * f64::from(context_factor)).round() as u32;
    let scaled_ctx = scaled_ctx.clamp(MIN_N_CTX, n_ctx);
    let available_tokens = scaled_ctx.saturating_sub(max_tokens) as usize;

    let mut budget_target = available_tokens;
    let mut prompt = fit_with_markers(&prompt, budget_target, markers);
    let mut verified: Option<usize> = None;
    for attempt in 0..MAX_FIT_ATTEMPTS {
        match llm.count_tokens(prompt.clone()) {
            Ok(real_tokens) if real_tokens <= available_tokens => {
                verified = Some(real_tokens);
                break;
            }
            Ok(real_tokens) => {
                log::error!(
                    "{tag}: fitted prompt still exceeds real budget (real={real_tokens} > budget={available_tokens}, attempt={attempt}); tightening heuristic target"
                );
                if attempt + 1 == MAX_FIT_ATTEMPTS {
                    log::error!(
                        "{tag}: giving up after {MAX_FIT_ATTEMPTS} fit attempts; falling back to an empty context so generate() can still run"
                    );
                    // Fail closed to just the (never-truncated) user turn.
                    prompt = fit_with_markers(&prompt, 0, markers);
                    verified = None;
                    break;
                }
                let overshoot_ratio = available_tokens as f64 / real_tokens as f64;
                budget_target =
                    ((budget_target as f64) * overshoot_ratio * 0.9).floor() as usize;
                prompt = fit_with_markers(&prompt, budget_target, markers);
            }
            Err(e) => {
                log::error!("{tag}: count_tokens verification failed, trusting heuristic fit: {e}");
                verified = None;
                break;
            }
        }
    }
    (prompt, verified)
}
