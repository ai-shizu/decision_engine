//! [A] Core LLM Service — worker thread (docs/architecture_blueprint.md §3.6).
//!
//! Design invariant (verified in the M0 spike): `LlamaContext` is neither `Send`
//! nor `Sync` and borrows its `LlamaModel`. It therefore can never live in Tauri
//! `State`. Backend / model / context are confined to a single worker thread;
//! `LlmHandle` (in State) is only a Send+Sync command sender.
//!
//! M5 Phase 2: generation branches on an explicit `task_id` (not embedded in
//! `GenerationParams`). Chat keeps the prior sampler chain; `kakeibo_v1` uses
//! grammar + greedy and validates the full buffer before emitting `validated`.

// `token_to_str` + `Special` are deprecated upstream in favour of `token_to_piece`,
// but that replacement requires an `encoding_rs::Decoder` (a new dependency not in
// the blueprint). The current path is functional; migrating it is deferred to a
// later phase, so we explicitly allow the deprecation here rather than pull the dep.
#![allow(deprecated)]

use std::num::NonZeroU32;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::thread;

use serde::Serialize;
use tauri::ipc::Channel;

use llama_cpp_2::context::params::LlamaContextParams;
use llama_cpp_2::llama_backend::LlamaBackend;
use llama_cpp_2::llama_batch::LlamaBatch;
use llama_cpp_2::model::params::LlamaModelParams;
use llama_cpp_2::model::{AddBos, LlamaChatMessage, LlamaModel, Special};
use llama_cpp_2::sampling::LlamaSampler;

use super::params::{GenerationParams, LoadParams};
use super::prompt::{build_prompt, TASK_KAKEIBO_V1};
use super::schema::{KakeiboEntryV1, KAKEIBO_V1_GBNF};
use crate::monitor::{MemPhase, MemoryMonitor};

/// One streamed token pushed to the frontend over `tauri::ipc::Channel`.
#[derive(Clone, Serialize)]
pub struct TokenEvent {
    pub seq: u32,
    pub text: String,
    pub done: bool,
    pub error: Option<String>,
    /// Set only on the final success event of a kakeibo extraction.
    pub validated: Option<KakeiboEntryV1>,
}

/// Resolved generation branch. Pure helper — unit-tested without a model.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GenerationMode {
    Chat,
    KakeiboV1,
}

/// Map `task_id` to a generation mode. Unknown ids fail closed (no chat fallback).
pub fn resolve_generation_mode(task_id: Option<&str>) -> Result<GenerationMode, String> {
    match task_id {
        None => Ok(GenerationMode::Chat),
        Some(TASK_KAKEIBO_V1) => Ok(GenerationMode::KakeiboV1),
        Some(other) => Err(format!("unknown extraction task_id: {other}")),
    }
}

/// Finalize an extraction buffer after the sample loop.
/// Cancelled runs and malformed JSON never produce a validated entry.
pub fn finalize_extraction(buf: &str, cancelled: bool) -> Result<KakeiboEntryV1, String> {
    if cancelled {
        return Err("extraction cancelled".to_string());
    }
    KakeiboEntryV1::from_json_str(buf).map_err(|e| format!("extraction parse: {e}"))
}

fn streaming_token(seq: u32, text: String) -> TokenEvent {
    TokenEvent {
        seq,
        text,
        done: false,
        error: None,
        validated: None,
    }
}

fn chat_done_event(seq: u32) -> TokenEvent {
    TokenEvent {
        seq,
        text: String::new(),
        done: true,
        error: None,
        validated: None,
    }
}

fn extract_done_event(seq: u32, entry: KakeiboEntryV1) -> TokenEvent {
    TokenEvent {
        seq,
        text: String::new(),
        done: true,
        error: None,
        validated: Some(entry),
    }
}

fn error_done_event(seq: u32, error: String) -> TokenEvent {
    TokenEvent {
        seq,
        text: String::new(),
        done: true,
        error: Some(error),
        validated: None,
    }
}

/// Command sent to the worker thread. `mpsc::Sender` is `!Sync`, so `LlmHandle`
/// wraps it in a `Mutex` to satisfy Tauri `State: Send + Sync`.
enum LlmCommand {
    Load {
        model_path: PathBuf,
        params: LoadParams,
        reply: mpsc::Sender<Result<(), String>>,
    },
    Generate {
        params: GenerationParams,
        task_id: Option<String>,
        tokens: Channel<TokenEvent>,
    },
    Shutdown,
}

/// Send + Sync handle placed in Tauri `State`.
pub struct LlmHandle {
    tx: Mutex<mpsc::Sender<LlmCommand>>,
    cancel: Arc<AtomicBool>,
}

impl LlmHandle {
    /// Spawn the worker thread (initializes `LlamaBackend`, enters command loop).
    /// `monitor` is shared so the worker can mark memory phases (ModelLoaded /
    /// CtxCreated / Inference / Idle) as it progresses.
    pub fn spawn(monitor: Arc<MemoryMonitor>) -> Self {
        let (tx, rx) = mpsc::channel::<LlmCommand>();
        let cancel = Arc::new(AtomicBool::new(false));
        let cancel_worker = Arc::clone(&cancel);
        thread::Builder::new()
            .name("pocket-brain-llm".into())
            .spawn(move || worker_loop(rx, cancel_worker, monitor))
            .expect("spawn pocket-brain llm worker");
        Self {
            tx: Mutex::new(tx),
            cancel,
        }
    }

    /// Blocking: mmap-load the GGUF on the worker thread and await the result.
    pub fn load(&self, model_path: PathBuf, params: LoadParams) -> Result<(), String> {
        let (reply, ack) = mpsc::channel();
        self.tx
            .lock()
            .map_err(|_| "llm tx poisoned".to_string())?
            .send(LlmCommand::Load {
                model_path,
                params,
                reply,
            })
            .map_err(|_| "llm worker gone".to_string())?;
        ack.recv()
            .map_err(|_| "llm worker dropped reply".to_string())?
    }

    /// Fire-and-forget: start a generation, streaming tokens over `tokens`.
    /// `task_id` is the sole authority for extraction routing (not copied into params).
    pub fn generate(
        &self,
        params: GenerationParams,
        task_id: Option<String>,
        tokens: Channel<TokenEvent>,
    ) -> Result<(), String> {
        self.cancel.store(false, Ordering::SeqCst);
        self.tx
            .lock()
            .map_err(|_| "llm tx poisoned".to_string())?
            .send(LlmCommand::Generate {
                params,
                task_id,
                tokens,
            })
            .map_err(|_| "llm worker gone".to_string())
    }

    /// Request cancellation of the in-flight generation (checked each token).
    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::SeqCst);
    }
}

impl Drop for LlmHandle {
    fn drop(&mut self) {
        if let Ok(tx) = self.tx.lock() {
            let _ = tx.send(LlmCommand::Shutdown);
        }
    }
}

fn worker_loop(
    rx: mpsc::Receiver<LlmCommand>,
    cancel: Arc<AtomicBool>,
    monitor: Arc<MemoryMonitor>,
) {
    let backend = match LlamaBackend::init() {
        Ok(b) => b,
        Err(e) => {
            eprintln!("[pocket-brain] backend init failed: {e}");
            return;
        }
    };
    let mut model: Option<Arc<LlamaModel>> = None;

    while let Ok(cmd) = rx.recv() {
        match cmd {
            LlmCommand::Load {
                model_path,
                params,
                reply,
            } => {
                let result = load_model(&backend, &model_path, &params).map(|m| {
                    model = Some(Arc::new(m));
                    monitor.set_phase(MemPhase::ModelLoaded);
                });
                let _ = reply.send(result);
            }
            LlmCommand::Generate {
                params,
                task_id,
                tokens,
            } => {
                let model = match model.as_ref() {
                    Some(m) => Arc::clone(m),
                    None => {
                        let _ = tokens.send(error_done_event(0, "model not loaded".into()));
                        continue;
                    }
                };
                if let Err(e) = generate(
                    &backend,
                    &model,
                    &params,
                    task_id.as_deref(),
                    &tokens,
                    &cancel,
                    &monitor,
                ) {
                    let _ = tokens.send(error_done_event(0, e));
                }
                monitor.set_phase(MemPhase::Idle);
            }
            LlmCommand::Shutdown => break,
        }
    }
}

/// Apply the model's baked-in chat template to a `(system, user)` pair.
///
/// Uses the verified llama-cpp-2 0.1.151 APIs:
/// `LlamaModel::chat_template` → `LlamaChatMessage::new` → `LlamaModel::apply_chat_template`.
/// `add_ass = true` so the rendered prompt ends with the assistant open tag.
pub fn render_chat_prompt(model: &LlamaModel, system: &str, user: &str) -> Result<String, String> {
    let tmpl = model
        .chat_template(None)
        .map_err(|e| format!("chat_template: {e}"))?;
    let messages = vec![
        LlamaChatMessage::new("system".into(), system.to_string())
            .map_err(|e| format!("chat message system: {e}"))?,
        LlamaChatMessage::new("user".into(), user.to_string())
            .map_err(|e| format!("chat message user: {e}"))?,
    ];
    model
        .apply_chat_template(&tmpl, &messages, true)
        .map_err(|e| format!("apply_chat_template: {e}"))
}

fn load_model(backend: &LlamaBackend, path: &Path, p: &LoadParams) -> Result<LlamaModel, String> {
    if !path.exists() {
        return Err(format!("GGUF not found at {}", path.display()));
    }
    let params = LlamaModelParams::default()
        .with_n_gpu_layers(p.n_gpu_layers)
        .with_use_mmap(p.use_mmap);
    LlamaModel::load_from_file(backend, path, &params).map_err(|e| format!("model load: {e}"))
}

fn generate(
    backend: &LlamaBackend,
    model: &LlamaModel,
    g: &GenerationParams,
    task_id: Option<&str>,
    tokens: &Channel<TokenEvent>,
    cancel: &AtomicBool,
    monitor: &MemoryMonitor,
) -> Result<(), String> {
    // Fail closed before allocating context / sampling for unknown tasks.
    let mode = resolve_generation_mode(task_id)?;

    let prompt = match mode {
        GenerationMode::Chat => g.prompt.clone(),
        GenerationMode::KakeiboV1 => {
            let (system, user) = build_prompt(TASK_KAKEIBO_V1, &g.prompt)?;
            render_chat_prompt(model, &system, &user)?
        }
    };

    let ctx_params = LlamaContextParams::default().with_n_ctx(NonZeroU32::new(g.n_ctx));
    let mut ctx = model
        .new_context(backend, ctx_params)
        .map_err(|e| format!("context create: {e}"))?;
    monitor.set_phase(MemPhase::CtxCreated);

    let prompt_tokens = model
        .str_to_token(&prompt, AddBos::Always)
        .map_err(|e| format!("tokenize: {e}"))?;
    let mut batch = LlamaBatch::new(prompt_tokens.len().max(1), 1);
    let last = prompt_tokens.len().saturating_sub(1);
    for (i, token) in prompt_tokens.iter().enumerate() {
        batch
            .add(*token, i as i32, &[0], i == last)
            .map_err(|e| format!("batch add: {e}"))?;
    }
    ctx.decode(&mut batch).map_err(|e| format!("decode: {e}"))?;
    monitor.set_phase(MemPhase::Inference);

    let mut sampler = match mode {
        GenerationMode::Chat => {
            if g.temp <= 0.0 {
                LlamaSampler::greedy()
            } else {
                LlamaSampler::chain_simple([
                    LlamaSampler::top_k(g.top_k),
                    LlamaSampler::top_p(g.top_p, 1),
                    LlamaSampler::temp(g.temp),
                    LlamaSampler::dist(g.seed),
                ])
            }
        }
        GenerationMode::KakeiboV1 => {
            let grammar = LlamaSampler::grammar(model, KAKEIBO_V1_GBNF, "root")
                .map_err(|e| format!("grammar init: {e}"))?;
            LlamaSampler::chain_simple([grammar, LlamaSampler::greedy()])
        }
    };

    let mut extract_buf = String::new();
    let mut cancelled = false;
    let mut n_cur = batch.n_tokens();
    for seq in 0..g.max_tokens {
        if cancel.load(Ordering::SeqCst) {
            cancelled = true;
            break;
        }
        let token = sampler.sample(&ctx, batch.n_tokens() - 1);
        if model.is_eog_token(token) {
            break;
        }
        let piece = model
            .token_to_str(token, Special::Plaintext)
            .map_err(|e| format!("token decode: {e}"))?;
        tokens
            .send(streaming_token(seq, piece.clone()))
            .map_err(|e| format!("channel send: {e}"))?;
        if matches!(mode, GenerationMode::KakeiboV1) {
            extract_buf.push_str(&piece);
        }
        sampler.accept(token);

        batch.clear();
        batch
            .add(token, n_cur, &[0], true)
            .map_err(|e| format!("batch add: {e}"))?;
        n_cur += 1;
        ctx.decode(&mut batch).map_err(|e| format!("decode: {e}"))?;
    }

    match mode {
        GenerationMode::Chat => {
            // Preserve prior chat cancel semantics: break then emit success done.
            tokens
                .send(chat_done_event(g.max_tokens))
                .map_err(|e| format!("channel send: {e}"))?;
            Ok(())
        }
        GenerationMode::KakeiboV1 => {
            let entry = finalize_extraction(&extract_buf, cancelled)?;
            tokens
                .send(extract_done_event(g.max_tokens, entry))
                .map_err(|e| format!("channel send: {e}"))?;
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mode_none_is_chat() {
        assert!(matches!(
            resolve_generation_mode(None),
            Ok(GenerationMode::Chat)
        ));
    }

    #[test]
    fn mode_kakeibo_v1_is_extract() {
        assert!(matches!(
            resolve_generation_mode(Some(TASK_KAKEIBO_V1)),
            Ok(GenerationMode::KakeiboV1)
        ));
    }

    #[test]
    fn mode_unknown_is_err() {
        assert!(resolve_generation_mode(Some("nope")).is_err());
    }

    #[test]
    fn finalize_valid_json_ok() {
        let raw = r#"{"date":"2026-07-18","amount":1500,"category":"食費","payee":"スーパー","memo":"弁当"}"#;
        let result = finalize_extraction(raw, false);
        assert!(result.is_ok());
        if let Ok(e) = result {
            assert_eq!(e.date, "2026-07-18");
            assert_eq!(e.amount, Some(1500));
        }
    }

    #[test]
    fn finalize_amount_string_normalizes() {
        let raw = r#"{"date":"2026-07-18","amount":"1,000","category":"食費","payee":"unknown","memo":"unknown"}"#;
        let result = finalize_extraction(raw, false);
        assert!(result.is_ok());
        if let Ok(e) = result {
            assert_eq!(e.amount, Some(1000));
        }
        let raw_fw = r#"{"date":"2026-07-18","amount":"１０００","category":"食費","payee":"unknown","memo":"unknown"}"#;
        let result_fw = finalize_extraction(raw_fw, false);
        assert!(result_fw.is_ok());
        if let Ok(e) = result_fw {
            assert_eq!(e.amount, Some(1000));
        }
    }

    #[test]
    fn finalize_extra_key_fails() {
        let raw = r#"{"date":"2026-01-01","amount":1,"category":"a","payee":"b","memo":"c","extra":true}"#;
        assert!(finalize_extraction(raw, false).is_err());
    }

    #[test]
    fn finalize_malformed_json_fails() {
        assert!(finalize_extraction("{not json", false).is_err());
    }

    #[test]
    fn finalize_cancelled_never_validates() {
        let raw = r#"{"date":"2026-07-18","amount":1,"category":"a","payee":"b","memo":"c"}"#;
        let result = finalize_extraction(raw, true);
        assert!(result.is_err());
        if let Err(msg) = result {
            assert!(msg.contains("cancelled"));
        }
    }

    #[test]
    fn chat_done_has_no_validated() {
        let ev = chat_done_event(9);
        assert!(ev.done);
        assert!(ev.error.is_none());
        assert!(ev.validated.is_none());
    }

    #[test]
    fn extract_done_carries_validated() {
        let parsed = KakeiboEntryV1::from_json_str(
            r#"{"date":"2026-07-18","amount":null,"category":"unknown","payee":"unknown","memo":"unknown"}"#,
        );
        assert!(parsed.is_ok());
        if let Ok(entry) = parsed {
            let ev = extract_done_event(9, entry);
            assert!(ev.done);
            assert!(ev.error.is_none());
            assert!(ev.validated.is_some());
        }
    }

    #[test]
    fn error_done_has_no_validated() {
        let ev = error_done_event(0, "boom".into());
        assert!(ev.done);
        assert!(ev.error.is_some());
        assert!(ev.validated.is_none());
    }
}
