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
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::thread;
use std::time::Duration;

use serde::Serialize;
use tauri::ipc::Channel;

use llama_cpp_2::context::params::LlamaContextParams;
use llama_cpp_2::llama_backend::LlamaBackend;
use llama_cpp_2::llama_batch::LlamaBatch;
use llama_cpp_2::model::params::LlamaModelParams;
use llama_cpp_2::model::{AddBos, LlamaChatMessage, LlamaModel, Special};
use llama_cpp_2::sampling::LlamaSampler;

use super::params::{GenerationParams, LoadParams};
use super::prompt::{
    build_prompt, LlmTaskId, TASK_COGNITIVE_DISTORTION_V1, TASK_KAKEIBO_V1,
};
use super::schema::{
    CognitiveDistortionReportV1, KakeiboEntryV1, COGNITIVE_DISTORTION_V1_GBNF, KAKEIBO_V1_GBNF,
};
use super::token_batch::TokenStreamBatcher;
use crate::monitor::{DegradationLevel, MemPhase, MemoryMonitor};

/// Idle wake cadence for the worker's command loop. Bounds how long the worker
/// may sit blocked before it observes a purge request and frees the model when
/// no command arrives to wake it. Short enough to be prompt, long enough to be
/// a negligible idle cost.
const PURGE_POLL_INTERVAL: Duration = Duration::from_millis(250);

/// Lock-free memory governor shared between the Tauri `State` handle, the LLM
/// worker thread, and the iOS lifecycle observer.
///
/// The iOS memory-warning / background callback runs on the main thread and may
/// ONLY touch this via [`request_purge`](Self::request_purge): two atomic
/// stores, no lock, no blocking. The heavy work — dropping the multi-GB model —
/// happens later on the worker thread, never in the callback.
pub struct LlmMemoryGovernor {
    /// Breaks the in-flight decode loop within one token (checked per token).
    cancel: AtomicBool,
    /// Instructs the worker to drop the model/context and return memory to iOS.
    purge_requested: AtomicBool,
    /// Progressive degradation ladder (Nominal→Critical). Updated lock-free.
    degradation: AtomicU8,
}

impl LlmMemoryGovernor {
    fn new() -> Self {
        Self {
            cancel: AtomicBool::new(false),
            purge_requested: AtomicBool::new(false),
            degradation: AtomicU8::new(DegradationLevel::Nominal.as_u8()),
        }
    }

    /// Cancel the in-flight generation only (user "stop"). Lock-free.
    pub fn request_cancel(&self) {
        self.cancel.store(true, Ordering::SeqCst);
    }

    /// Request a full memory purge: cancel the in-flight generation AND drop the
    /// model. Safe to call from the iOS main-thread callback or the Jetsam
    /// sampler's `over_threshold` rising edge — two atomic stores, no lock, no
    /// blocking (satisfies the no-heavy-work-in-callback rule). The heavy model
    /// `Drop` happens later on the worker via [`take_purge`](Self::take_purge).
    pub fn request_purge(&self) {
        self.cancel.store(true, Ordering::SeqCst);
        self.purge_requested.store(true, Ordering::SeqCst);
    }

    /// Sync ladder rung from the thermal / pressure monitor (lock-free).
    pub fn set_degradation(&self, level: DegradationLevel) {
        self.degradation.store(level.as_u8(), Ordering::SeqCst);
    }

    pub fn degradation(&self) -> DegradationLevel {
        DegradationLevel::from_u8(self.degradation.load(Ordering::SeqCst))
    }

    fn is_cancelled(&self) -> bool {
        self.cancel.load(Ordering::SeqCst)
    }

    fn reset_cancel(&self) {
        self.cancel.store(false, Ordering::SeqCst);
    }

    /// Consume a pending purge request (true at most once per request).
    fn take_purge(&self) -> bool {
        self.purge_requested.swap(false, Ordering::SeqCst)
    }
}

/// Persistent lifecycle event pushed to the frontend over a registered
/// `tauri::ipc::Channel` (mirrors M6's `VaultLifecycleEvent`). Out-of-band from
/// any single generation, so it uses its own sink rather than the token channel.
#[derive(Clone, Serialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum LlmLifecycleEvent {
    /// The model was dropped to survive memory pressure; the UI must suspend and
    /// await an explicit reload.
    MemoryPurged,
    /// Thermal / memory ladder entered Serious or Critical — UI shows ambient warn.
    Degradation { level: DegradationLevel },
}

/// One streamed token pushed to the frontend over `tauri::ipc::Channel`.
#[derive(Clone, Serialize)]
pub struct TokenEvent {
    pub seq: u32,
    pub text: String,
    pub done: bool,
    pub error: Option<String>,
    /// Set only on the final success event of a kakeibo extraction.
    pub validated: Option<KakeiboEntryV1>,
    /// Set only on the final success event of CBT distortion extraction.
    pub validated_distortions: Option<CognitiveDistortionReportV1>,
}

/// Resolved generation branch. Pure helper — unit-tested without a model.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GenerationMode {
    Chat,
    KakeiboV1,
    CognitiveDistortionV1,
}

/// Map `task_id` to a generation mode. Unknown ids fail closed (no chat fallback).
pub fn resolve_generation_mode(task_id: Option<&str>) -> Result<GenerationMode, String> {
    match task_id {
        None => Ok(GenerationMode::Chat),
        Some(s) => {
            let id = LlmTaskId::parse(s)?;
            if id.as_str() != s {
                return Err(format!("unknown extraction task_id: {s}"));
            }
            Ok(match id {
                LlmTaskId::KakeiboV1 => GenerationMode::KakeiboV1,
                LlmTaskId::CognitiveDistortionV1 => GenerationMode::CognitiveDistortionV1,
            })
        }
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

pub fn finalize_distortion_extraction(
    buf: &str,
    cancelled: bool,
) -> Result<CognitiveDistortionReportV1, String> {
    if cancelled {
        return Err("extraction cancelled".to_string());
    }
    CognitiveDistortionReportV1::from_json_str(buf)
        .map_err(|e| format!("distortion extraction parse: {e}"))
}

fn streaming_token(seq: u32, text: String) -> TokenEvent {
    TokenEvent {
        seq,
        text,
        done: false,
        error: None,
        validated: None,
        validated_distortions: None,
    }
}

fn chat_done_event(seq: u32) -> TokenEvent {
    TokenEvent {
        seq,
        text: String::new(),
        done: true,
        error: None,
        validated: None,
        validated_distortions: None,
    }
}

fn extract_done_event(seq: u32, entry: KakeiboEntryV1) -> TokenEvent {
    TokenEvent {
        seq,
        text: String::new(),
        done: true,
        error: None,
        validated: Some(entry),
        validated_distortions: None,
    }
}

fn distortion_done_event(seq: u32, report: CognitiveDistortionReportV1) -> TokenEvent {
    TokenEvent {
        seq,
        text: String::new(),
        done: true,
        error: None,
        validated: None,
        validated_distortions: Some(report),
    }
}

fn error_done_event(seq: u32, error: String) -> TokenEvent {
    TokenEvent {
        seq,
        text: String::new(),
        done: true,
        error: Some(error),
        validated: None,
        validated_distortions: None,
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
    /// One-shot embedding on the worker (separate short-lived context).
    Embed {
        text: String,
        n_ctx: u32,
        reply: mpsc::Sender<Result<Vec<f32>, String>>,
    },
    RegisterEvents {
        channel: Channel<LlmLifecycleEvent>,
    },
    Shutdown,
}

/// Send + Sync handle placed in Tauri `State`.
#[derive(Clone)]
pub struct LlmHandle {
    tx: Arc<Mutex<mpsc::Sender<LlmCommand>>>,
    governor: Arc<LlmMemoryGovernor>,
}

impl LlmHandle {
    /// Spawn the worker thread (initializes `LlamaBackend`, enters command loop).
    /// `monitor` is shared so the worker can mark memory phases (ModelLoaded /
    /// CtxCreated / Inference / Idle) as it progresses.
    pub fn spawn(monitor: Arc<MemoryMonitor>) -> Self {
        let (tx, rx) = mpsc::channel::<LlmCommand>();
        let governor = Arc::new(LlmMemoryGovernor::new());
        let governor_worker = Arc::clone(&governor);
        thread::Builder::new()
            .name("pocket-brain-llm".into())
            .spawn(move || worker_loop(rx, governor_worker, monitor))
            .expect("spawn pocket-brain llm worker");
        Self {
            tx: Arc::new(Mutex::new(tx)),
            governor,
        }
    }

    /// Clone of the lock-free governor, to hand to the iOS lifecycle observer.
    pub fn governor(&self) -> Arc<LlmMemoryGovernor> {
        Arc::clone(&self.governor)
    }

    /// Register the frontend lifecycle event sink (single sink; a later call
    /// replaces the earlier one). Mirrors M6's `vault_events`.
    pub fn register_events(&self, channel: Channel<LlmLifecycleEvent>) -> Result<(), String> {
        self.tx
            .lock()
            .map_err(|_| "llm tx poisoned".to_string())?
            .send(LlmCommand::RegisterEvents { channel })
            .map_err(|_| "llm worker gone".to_string())
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
        self.governor.reset_cancel();
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

    /// Blocking: embed `text` and return little-endian f32 bytes (Phase 9 binary IPC).
    pub fn embed_binary(&self, text: String, n_ctx: u32) -> Result<Vec<u8>, String> {
        let v = self.embed(text, n_ctx)?;
        let bytes = super::embed::f32_slice_to_le_bytes(&v);
        // Round-trip guard keeps decode helper live (Zero Warnings) and catches packing bugs.
        let back = super::embed::le_bytes_to_f32_vec(&bytes)?;
        if back.len() != v.len() {
            return Err("embed binary round-trip length mismatch".into());
        }
        Ok(bytes)
    }

    /// Request cancellation of the in-flight generation (checked each token).
    pub fn cancel(&self) {
        self.governor.request_cancel();
    }

    /// Blocking: embed `text` on the worker with a short-lived embeddings context.
    /// Does not share the generation context; respects cancel/purge via governor.
    pub fn embed(&self, text: String, n_ctx: u32) -> Result<Vec<f32>, String> {
        self.governor.reset_cancel();
        let (reply, ack) = mpsc::channel();
        self.tx
            .lock()
            .map_err(|_| "llm tx poisoned".to_string())?
            .send(LlmCommand::Embed {
                text,
                n_ctx,
                reply,
            })
            .map_err(|_| "llm worker gone".to_string())?;
        ack.recv()
            .map_err(|_| "llm worker dropped reply".to_string())?
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
    governor: Arc<LlmMemoryGovernor>,
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
    let mut events: Option<Channel<LlmLifecycleEvent>> = None;
    let mut last_degradation_emit = DegradationLevel::Nominal;

    loop {
        // Commit any pending purge FIRST — including after a generation the
        // memory warning cancel-broke, and on the timeout wake when idle. The
        // heavy `Drop` of the multi-GB model happens here, on the worker thread,
        // never in the OS callback. Any in-flight `ctx`/`Arc` clone was already
        // dropped when `generate` returned, so `take()` releases the last ref.
        if governor.take_purge() && model.take().is_some() {
            monitor.set_phase(MemPhase::Baseline);
            if let Some(sink) = events.as_ref() {
                let _ = sink.send(LlmLifecycleEvent::MemoryPurged);
            }
        }

        let cmd = match rx.recv_timeout(PURGE_POLL_INTERVAL) {
            Ok(cmd) => cmd,
            // Idle wake: re-check purge + emit Serious+ degradation once.
            Err(mpsc::RecvTimeoutError::Timeout) => {
                let level = governor.degradation();
                if level.throttle_generation()
                    && last_degradation_emit < DegradationLevel::Serious
                {
                    if let Some(sink) = events.as_ref() {
                        let _ = sink.send(LlmLifecycleEvent::Degradation { level });
                    }
                }
                last_degradation_emit = if level.throttle_generation() {
                    level
                } else {
                    DegradationLevel::Nominal
                };
                continue;
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        };

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
                    &governor,
                    &monitor,
                ) {
                    let _ = tokens.send(error_done_event(0, e));
                }
                monitor.set_phase(MemPhase::Idle);
            }
            LlmCommand::Embed {
                text,
                n_ctx,
                reply,
            } => {
                // Fair+: suppress expensive LLM embed re-warm; callers fall back
                // to hashed-ngram (embed_knowledge).
                if governor.degradation().suppress_background() {
                    let _ = reply.send(Err("degraded: background embed suppressed".into()));
                    continue;
                }
                let result = match model.as_ref() {
                    None => Err("model not loaded".into()),
                    Some(model) => {
                        monitor.set_phase(MemPhase::CtxCreated);
                        let out = super::embed::embed_text(
                            &backend,
                            model.as_ref(),
                            &text,
                            n_ctx,
                            || governor.is_cancelled(),
                        );
                        monitor.set_phase(MemPhase::Idle);
                        out
                    }
                };
                let _ = reply.send(result);
            }
            LlmCommand::RegisterEvents { channel } => {
                events = Some(channel);
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
    governor: &LlmMemoryGovernor,
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
        GenerationMode::CognitiveDistortionV1 => {
            let (system, user) = build_prompt(TASK_COGNITIVE_DISTORTION_V1, &g.prompt)?;
            render_chat_prompt(model, &system, &user)?
        }
    };

    let level = governor.degradation();
    let scaled_ctx = ((g.n_ctx as f32) * level.context_factor())
        .round()
        .clamp(512.0, g.n_ctx as f32) as u32;
    let ctx_params = LlamaContextParams::default().with_n_ctx(NonZeroU32::new(scaled_ctx));
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
        GenerationMode::CognitiveDistortionV1 => {
            let grammar = LlamaSampler::grammar(model, COGNITIVE_DISTORTION_V1_GBNF, "root")
                .map_err(|e| format!("grammar init: {e}"))?;
            LlamaSampler::chain_simple([grammar, LlamaSampler::greedy()])
        }
    };

    let mut extract_buf = String::new();
    let mut cancelled = false;
    let mut n_cur = batch.n_tokens();
    let mut batcher = TokenStreamBatcher::new(|seq, text| {
        tokens
            .send(streaming_token(seq, text))
            .map_err(|e| format!("channel send: {e}"))
    });
    for seq in 0..g.max_tokens {
        if governor.is_cancelled() {
            cancelled = true;
            break;
        }
        if governor.degradation().throttle_generation() {
            // Serious+: soft rate-limit decode to shed thermal load.
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        let token = sampler.sample(&ctx, batch.n_tokens() - 1);
        if model.is_eog_token(token) {
            break;
        }
        let piece = model
            .token_to_str(token, Special::Plaintext)
            .map_err(|e| format!("token decode: {e}"))?;
        batcher
            .push(seq, &piece)
            .map_err(|e| format!("channel send: {e}"))?;
        if matches!(
            mode,
            GenerationMode::KakeiboV1 | GenerationMode::CognitiveDistortionV1
        ) {
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

    batcher.flush()?;

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
        GenerationMode::CognitiveDistortionV1 => {
            let report = finalize_distortion_extraction(&extract_buf, cancelled)?;
            tokens
                .send(distortion_done_event(g.max_tokens, report))
                .map_err(|e| format!("channel send: {e}"))?;
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn governor_request_purge_sets_cancel_and_purge() {
        let g = LlmMemoryGovernor::new();
        assert!(!g.is_cancelled());
        assert!(!g.take_purge());
        g.request_purge();
        assert!(g.is_cancelled());
        assert!(g.take_purge());
        // Consumed once.
        assert!(!g.take_purge());
    }

    #[test]
    fn governor_request_cancel_does_not_purge() {
        let g = LlmMemoryGovernor::new();
        g.request_cancel();
        assert!(g.is_cancelled());
        assert!(!g.take_purge());
    }

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
    fn mode_cognitive_distortion_v1_is_extract() {
        assert!(matches!(
            resolve_generation_mode(Some(TASK_COGNITIVE_DISTORTION_V1)),
            Ok(GenerationMode::CognitiveDistortionV1)
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
