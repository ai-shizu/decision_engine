//! [A] Core LLM Service — worker thread (docs/architecture_blueprint.md §3.6).
//!
//! Design invariant (verified in the M0 spike): `LlamaContext` is neither `Send`
//! nor `Sync` and borrows its `LlamaModel`. It therefore can never live in Tauri
//! `State`. Backend / model / context are confined to a single worker thread;
//! `LlmHandle` (in State) is only a Send+Sync command sender.
//!
//! Phase 1: the worker + generation loop are implemented against verified
//! llama-cpp-2 0.1.151 APIs, but the commands that drive them are not yet
//! registered in the `invoke_handler` (frontend not connected).

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
use llama_cpp_2::model::{AddBos, LlamaModel, Special};
use llama_cpp_2::sampling::LlamaSampler;

use super::params::{GenerationParams, LoadParams};
use crate::monitor::{MemPhase, MemoryMonitor};

/// One streamed token pushed to the frontend over `tauri::ipc::Channel`.
#[derive(Clone, Serialize)]
pub struct TokenEvent {
    pub seq: u32,
    pub text: String,
    pub done: bool,
    pub error: Option<String>,
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
        ack.recv().map_err(|_| "llm worker dropped reply".to_string())?
    }

    /// Fire-and-forget: start a generation, streaming tokens over `tokens`.
    pub fn generate(&self, params: GenerationParams, tokens: Channel<TokenEvent>) -> Result<(), String> {
        self.cancel.store(false, Ordering::SeqCst);
        self.tx
            .lock()
            .map_err(|_| "llm tx poisoned".to_string())?
            .send(LlmCommand::Generate { params, tokens })
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

fn worker_loop(rx: mpsc::Receiver<LlmCommand>, cancel: Arc<AtomicBool>, monitor: Arc<MemoryMonitor>) {
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
            LlmCommand::Generate { params, tokens } => {
                let model = match model.as_ref() {
                    Some(m) => Arc::clone(m),
                    None => {
                        let _ = tokens.send(TokenEvent {
                            seq: 0,
                            text: String::new(),
                            done: true,
                            error: Some("model not loaded".into()),
                        });
                        continue;
                    }
                };
                if let Err(e) = generate(&backend, &model, &params, &tokens, &cancel, &monitor) {
                    let _ = tokens.send(TokenEvent {
                        seq: 0,
                        text: String::new(),
                        done: true,
                        error: Some(e),
                    });
                }
                monitor.set_phase(MemPhase::Idle);
            }
            LlmCommand::Shutdown => break,
        }
    }
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
    tokens: &Channel<TokenEvent>,
    cancel: &AtomicBool,
    monitor: &MemoryMonitor,
) -> Result<(), String> {
    let ctx_params = LlamaContextParams::default().with_n_ctx(NonZeroU32::new(g.n_ctx));
    let mut ctx = model
        .new_context(backend, ctx_params)
        .map_err(|e| format!("context create: {e}"))?;
    monitor.set_phase(MemPhase::CtxCreated);

    // Prompt ingest.
    let prompt_tokens = model
        .str_to_token(&g.prompt, AddBos::Always)
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

    let mut sampler = if g.temp <= 0.0 {
        LlamaSampler::greedy()
    } else {
        LlamaSampler::chain_simple([
            LlamaSampler::top_k(g.top_k),
            LlamaSampler::top_p(g.top_p, 1),
            LlamaSampler::temp(g.temp),
            LlamaSampler::dist(g.seed),
        ])
    };

    let mut n_cur = batch.n_tokens();
    for seq in 0..g.max_tokens {
        if cancel.load(Ordering::SeqCst) {
            break;
        }
        // Logits for the last decoded token sit at index n_tokens-1.
        let token = sampler.sample(&ctx, batch.n_tokens() - 1);
        if model.is_eog_token(token) {
            break;
        }
        let piece = model.token_to_str(token, Special::Plaintext).unwrap_or_default();
        tokens
            .send(TokenEvent {
                seq,
                text: piece,
                done: false,
                error: None,
            })
            .map_err(|e| format!("channel send: {e}"))?;
        sampler.accept(token);

        batch.clear();
        batch
            .add(token, n_cur, &[0], true)
            .map_err(|e| format!("batch add: {e}"))?;
        n_cur += 1;
        ctx.decode(&mut batch).map_err(|e| format!("decode: {e}"))?;
    }

    tokens
        .send(TokenEvent {
            seq: g.max_tokens,
            text: String::new(),
            done: true,
            error: None,
        })
        .map_err(|e| format!("channel send: {e}"))?;
    Ok(())
}
