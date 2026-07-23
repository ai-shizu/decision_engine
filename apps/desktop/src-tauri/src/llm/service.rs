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

use std::num::NonZeroU32;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicU8, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::thread;
use std::time::Duration;

use serde::Serialize;
use tauri::ipc::Channel;
use tokio::sync::oneshot;

use llama_cpp_2::context::params::LlamaContextParams;
use llama_cpp_2::llama_backend::LlamaBackend;
use llama_cpp_2::llama_batch::LlamaBatch;
use llama_cpp_2::model::params::LlamaModelParams;
use llama_cpp_2::model::{AddBos, LlamaChatMessage, LlamaModel};
use llama_cpp_2::sampling::LlamaSampler;
use llama_cpp_2::token::LlamaToken;
use llama_cpp_2::TokenToStringError;

use super::params::{GenerationParams, LoadParams, MAX_N_CTX, MIN_N_CTX};
use super::prompt::{
    build_prompt, LlmTaskId, TASK_COGNITIVE_DISTORTION_V1, TASK_INTERVIEW_EVALUATION_V1,
    TASK_KAKEIBO_V1, TASK_METACOGNITIVE_DEBRIEF_V1, TASK_RECEIPT_OCR_V1,
};
use super::schema::{
    CognitiveDistortionReportV1, KakeiboEntryV1, ReceiptOcrV1, COGNITIVE_DISTORTION_V1_GBNF,
    KAKEIBO_V1_GBNF, RECEIPT_OCR_V1_GBNF,
};
use super::token_batch::TokenStreamBatcher;
use crate::coliseum::{
    InterviewEvaluationV1, MetacognitiveDebriefV1, INTERVIEW_EVALUATION_V1_GBNF,
    METACOGNITIVE_DEBRIEF_V1_GBNF,
};
use crate::monitor::{DegradationLevel, MemPhase, MemoryMonitor};

/// Idle wake cadence for the worker's command loop. Bounds how long the worker
/// may sit blocked before it observes a purge request and frees the model when
/// no command arrives to wake it. Short enough to be prompt, long enough to be
/// a negligible idle cost.
const PURGE_POLL_INTERVAL: Duration = Duration::from_millis(250);
/// Bounded ingress prevents a renderer or buggy caller from queueing an
/// unbounded number of multi-GB model operations. Cancellation bypasses this
/// queue through the lock-free governor.
const LLM_COMMAND_QUEUE_CAPACITY: usize = 8;

/// `LlmHandle::load/is_loaded/generate` bounds (2026-07-24 iOS-device
/// investigation: `send_rag_chat`'s Vault calls already bound on a 5s
/// `recv_timeout` — worker.rs — but these three used unbounded `recv()`/
/// `.await`. A wedged worker thread (single dedicated thread; all
/// `LlmCommand`s serialize through one queue) left every caller pending
/// forever with no error, matching the observed CONSULT/interview freeze:
/// device logs showed a clean embed-context construct+destroy, then total
/// silence with no further llama.cpp output. These do not fix why the worker
/// might wedge — they turn a silent hang into a surfaced, retryable error.
///
/// The worker keeps running past a caller's timeout (it only learns the
/// receiver was dropped when it tries to reply); once whatever blocked it
/// clears, the queue drains and the next call succeeds normally.
const LLM_READY_PROBE_TIMEOUT: Duration = Duration::from_secs(15);
/// mmap-load of a >1GB GGUF + Metal context construction. Observed well under
/// 1s warm on this hardware; generous for a cold/thermally-throttled load.
const LLM_LOAD_TIMEOUT: Duration = Duration::from_secs(60);
/// Generous per user instruction ("モバイル推論時間を考慮して長めに"): covers
/// a full ~384-900 token response even under poor (throttled/contended)
/// mobile throughput. Flat wall-clock bound on the whole call, not an
/// idle/per-token timeout — token progress still streams over the `tokens`
/// Channel throughout; this only bounds the terminal completion signal.
const LLM_GENERATE_TIMEOUT: Duration = Duration::from_secs(180);

/// Stable role boundary for free-form generation. Callers assemble rich RAG /
/// interview context inside the user message; this system message supplies the
/// model-level role that an instruction-tuned GGUF expects.
const CHAT_SYSTEM_PROMPT: &str = "あなたはCoraxisのオンデバイス推論エンジンです。ユーザーの指示に正確かつ簡潔に、日本語で回答してください。";

/// Incrementally reconstruct UTF-8 from llama token-piece bytes.
///
/// Qwen's byte-level BPE may split one Japanese scalar across multiple tokens.
/// `llama-cpp-2` 0.1.151's convenience `token_to_piece` can return an empty
/// string when its fixed output buffer is too small (`OutputFull`, zero input
/// consumed), so generation uses `token_to_piece_bytes` and owns the boundary
/// buffer explicitly.
#[derive(Default)]
struct Utf8TokenDecoder {
    pending: Vec<u8>,
}

impl Utf8TokenDecoder {
    fn push(&mut self, bytes: &[u8]) -> String {
        self.pending.extend_from_slice(bytes);
        let mut output = String::new();

        loop {
            let (valid_up_to, error_len) = match std::str::from_utf8(&self.pending) {
                Ok(valid) => {
                    output.push_str(valid);
                    self.pending.clear();
                    break;
                }
                Err(error) => (error.valid_up_to(), error.error_len()),
            };

            if valid_up_to > 0 {
                // `valid_up_to` is supplied by `from_utf8` for this exact slice.
                let valid = std::str::from_utf8(&self.pending[..valid_up_to])
                    .expect("validated UTF-8 prefix");
                output.push_str(valid);
                self.pending.drain(..valid_up_to);
            }

            match error_len {
                Some(invalid_len) => {
                    output.push('\u{FFFD}');
                    self.pending.drain(..invalid_len);
                }
                // A valid scalar is split at the current token boundary. Keep
                // the suffix until the next piece arrives.
                None => break,
            }
        }

        output
    }

    fn finish(&mut self) -> String {
        String::from_utf8_lossy(&std::mem::take(&mut self.pending)).into_owned()
    }
}

fn required_piece_capacity(reported: i32) -> Option<usize> {
    reported
        .checked_neg()
        .and_then(|size| usize::try_from(size).ok())
        .filter(|size| *size > 0)
}

/// Decode the raw vocabulary bytes, retrying with llama.cpp's reported exact
/// size when the common eight-byte fast-path is insufficient.
fn token_piece_bytes(model: &LlamaModel, token: LlamaToken) -> Result<Vec<u8>, TokenToStringError> {
    match model.token_to_piece_bytes(token, 8, false, None) {
        Err(TokenToStringError::InsufficientBufferSpace(reported)) => {
            let Some(required) = required_piece_capacity(reported) else {
                return Err(TokenToStringError::InsufficientBufferSpace(reported));
            };
            model.token_to_piece_bytes(token, required, false, None)
        }
        result => result,
    }
}

/// Lock-free memory governor shared between the Tauri `State` handle, the LLM
/// worker thread, and the iOS lifecycle observer.
///
/// The iOS memory-warning / background callback runs on the main thread and may
/// ONLY touch this via [`request_purge`](Self::request_purge): three atomic
/// stores, no lock, no blocking. The heavy work — dropping the multi-GB model —
/// happens later on the worker thread, never in the callback.
pub struct LlmMemoryGovernor {
    /// Monotonic generation boundary. A cancellation advances the epoch so the
    /// currently running job stops without poisoning a later queued job.
    cancel_epoch: AtomicU64,
    /// Advances only for a purge. Load commands capture this value so work
    /// queued before a Jetsam/background transition cannot rehydrate the model
    /// after the purge has been consumed.
    purge_epoch: AtomicU64,
    /// Instructs the worker to drop the model/context and return memory to iOS.
    purge_requested: AtomicBool,
    /// Progressive degradation ladder (Nominal→Critical). Updated lock-free.
    degradation: AtomicU8,
}

impl LlmMemoryGovernor {
    fn new() -> Self {
        Self {
            cancel_epoch: AtomicU64::new(0),
            purge_epoch: AtomicU64::new(0),
            purge_requested: AtomicBool::new(false),
            degradation: AtomicU8::new(DegradationLevel::Nominal.as_u8()),
        }
    }

    /// Cancel the in-flight generation only (user "stop"). Lock-free.
    pub fn request_cancel(&self) {
        self.cancel_epoch.fetch_add(1, Ordering::SeqCst);
    }

    /// Request a full memory purge: cancel the in-flight generation AND drop the
    /// model. Safe to call from the iOS main-thread callback or the Jetsam
    /// sampler's `over_threshold` rising edge — three atomic updates, no lock, no
    /// blocking (satisfies the no-heavy-work-in-callback rule). The heavy model
    /// `Drop` happens later on the worker via [`take_purge`](Self::take_purge).
    pub fn request_purge(&self) {
        self.cancel_epoch.fetch_add(1, Ordering::SeqCst);
        self.purge_epoch.fetch_add(1, Ordering::SeqCst);
        self.purge_requested.store(true, Ordering::SeqCst);
    }

    /// Sync ladder rung from the thermal / pressure monitor (lock-free).
    pub fn set_degradation(&self, level: DegradationLevel) {
        self.degradation.store(level.as_u8(), Ordering::SeqCst);
    }

    pub fn degradation(&self) -> DegradationLevel {
        DegradationLevel::from_u8(self.degradation.load(Ordering::SeqCst))
    }

    fn cancel_epoch(&self) -> u64 {
        self.cancel_epoch.load(Ordering::SeqCst)
    }

    fn purge_epoch(&self) -> u64 {
        self.purge_epoch.load(Ordering::SeqCst)
    }

    fn purge_pending(&self) -> bool {
        self.purge_requested.load(Ordering::SeqCst)
    }

    fn load_is_stale(&self, submitted_purge_epoch: u64) -> bool {
        self.purge_pending() || self.purge_epoch() != submitted_purge_epoch
    }

    fn is_cancelled_since(&self, start_epoch: u64) -> bool {
        self.purge_requested.load(Ordering::SeqCst)
            || self.cancel_epoch.load(Ordering::SeqCst) != start_epoch
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
    /// Set only on the final success event of receipt OCR extraction.
    pub validated_receipt: Option<ReceiptOcrV1>,
    /// Checksum gate result for receipt extract (`Σ amount + tax == total`).
    pub receipt_verified: Option<bool>,
    /// Layer-1 interview scorecard (transcript-only; Phase 14.3).
    pub validated_interview_evaluation: Option<InterviewEvaluationV1>,
    /// Layer-2 opt-in metacognitive debrief (Phase 14.3; never feeds pass/fail).
    pub validated_metacognitive_debrief: Option<MetacognitiveDebriefV1>,
}

/// Resolved generation branch. Pure helper — unit-tested without a model.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GenerationMode {
    Chat,
    KakeiboV1,
    CognitiveDistortionV1,
    ReceiptOcrV1,
    InterviewEvaluationV1,
    MetacognitiveDebriefV1,
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
                LlmTaskId::ReceiptOcrV1 => GenerationMode::ReceiptOcrV1,
                LlmTaskId::InterviewEvaluationV1 => GenerationMode::InterviewEvaluationV1,
                LlmTaskId::MetacognitiveDebriefV1 => GenerationMode::MetacognitiveDebriefV1,
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

pub fn finalize_receipt_extraction(buf: &str, cancelled: bool) -> Result<ReceiptOcrV1, String> {
    if cancelled {
        return Err("extraction cancelled".to_string());
    }
    ReceiptOcrV1::from_json_str(buf).map_err(|e| format!("receipt extraction parse: {e}"))
}

/// Parse Layer-1 scorecard JSON. Transcript provenance checks happen outside
/// the worker (callers pass `[TranscriptTurnRef]` into `validate_interview_evaluation`).
pub fn finalize_interview_evaluation(
    buf: &str,
    cancelled: bool,
) -> Result<InterviewEvaluationV1, String> {
    if cancelled {
        return Err("extraction cancelled".to_string());
    }
    InterviewEvaluationV1::from_json_str(buf)
        .map_err(|e| format!("interview evaluation parse: {e}"))
}

/// Parse Layer-2 debrief JSON. Mirror / turn validation is caller-side.
pub fn finalize_metacognitive_debrief(
    buf: &str,
    cancelled: bool,
) -> Result<MetacognitiveDebriefV1, String> {
    if cancelled {
        return Err("extraction cancelled".to_string());
    }
    MetacognitiveDebriefV1::from_json_str(buf)
        .map_err(|e| format!("metacognitive debrief parse: {e}"))
}

fn streaming_token(seq: u32, text: String) -> TokenEvent {
    TokenEvent {
        seq,
        text,
        done: false,
        error: None,
        validated: None,
        validated_distortions: None,
        validated_receipt: None,
        receipt_verified: None,
        validated_interview_evaluation: None,
        validated_metacognitive_debrief: None,
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
        validated_receipt: None,
        receipt_verified: None,
        validated_interview_evaluation: None,
        validated_metacognitive_debrief: None,
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
        validated_receipt: None,
        receipt_verified: None,
        validated_interview_evaluation: None,
        validated_metacognitive_debrief: None,
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
        validated_receipt: None,
        receipt_verified: None,
        validated_interview_evaluation: None,
        validated_metacognitive_debrief: None,
    }
}

fn receipt_done_event(seq: u32, report: ReceiptOcrV1) -> TokenEvent {
    let verified = report.checksum_ok();
    TokenEvent {
        seq,
        text: String::new(),
        done: true,
        error: None,
        validated: None,
        validated_distortions: None,
        validated_receipt: Some(report),
        receipt_verified: Some(verified),
        validated_interview_evaluation: None,
        validated_metacognitive_debrief: None,
    }
}

fn interview_eval_done_event(seq: u32, report: InterviewEvaluationV1) -> TokenEvent {
    TokenEvent {
        seq,
        text: String::new(),
        done: true,
        error: None,
        validated: None,
        validated_distortions: None,
        validated_receipt: None,
        receipt_verified: None,
        validated_interview_evaluation: Some(report),
        validated_metacognitive_debrief: None,
    }
}

fn metacognitive_debrief_done_event(seq: u32, report: MetacognitiveDebriefV1) -> TokenEvent {
    TokenEvent {
        seq,
        text: String::new(),
        done: true,
        error: None,
        validated: None,
        validated_distortions: None,
        validated_receipt: None,
        receipt_verified: None,
        validated_interview_evaluation: None,
        validated_metacognitive_debrief: Some(report),
    }
}

pub(crate) fn error_done_event(seq: u32, error: String) -> TokenEvent {
    TokenEvent {
        seq,
        text: String::new(),
        done: true,
        error: Some(error),
        validated: None,
        validated_distortions: None,
        validated_receipt: None,
        receipt_verified: None,
        validated_interview_evaluation: None,
        validated_metacognitive_debrief: None,
    }
}

/// Report a failed generation over the authoritative token channel, then
/// release the async caller. The completion stays `Ok` whenever a terminal
/// event reached the channel; only a broken terminal channel rejects invoke.
/// Successful generation already emitted its terminal event inside [`generate`].
fn complete_generation(
    result: Result<(), String>,
    tokens: &Channel<TokenEvent>,
    completion: oneshot::Sender<Result<(), String>>,
) {
    let result = match result {
        Ok(()) => Ok(()),
        Err(error) => tokens
            .send(error_done_event(0, error))
            .map_err(|send_error| format!("token channel send: {send_error}")),
    };
    let _ = completion.send(result);
}

/// Command sent to the worker thread. Every reply lane is also bounded to one
/// value, so a vanished caller cannot accumulate unbounded acknowledgements.
enum LlmCommand {
    Load {
        model_path: PathBuf,
        params: LoadParams,
        purge_epoch: u64,
        reply: mpsc::SyncSender<Result<(), String>>,
    },
    Generate {
        params: GenerationParams,
        task_id: Option<String>,
        tokens: Channel<TokenEvent>,
        completion: oneshot::Sender<Result<(), String>>,
    },
    /// One-shot embedding on the worker (separate short-lived context).
    Embed {
        text: String,
        n_ctx: u32,
        reply: mpsc::SyncSender<Result<Vec<f32>, String>>,
    },
    /// Phase 10: Jetsam / foreground restore — is the GGUF still resident?
    IsLoaded {
        reply: mpsc::SyncSender<bool>,
    },
    RegisterEvents {
        channel: Channel<LlmLifecycleEvent>,
    },
}

/// Send + Sync handle placed in Tauri `State`.
#[derive(Clone)]
pub struct LlmHandle {
    tx: Arc<Mutex<mpsc::SyncSender<LlmCommand>>>,
    governor: Arc<LlmMemoryGovernor>,
    startup_error: Option<Arc<str>>,
}

impl LlmHandle {
    /// Spawn the worker thread (initializes `LlamaBackend`, enters command loop).
    /// `monitor` is shared so the worker can mark memory phases (ModelLoaded /
    /// CtxCreated / Inference / Idle) as it progresses.
    pub fn spawn(monitor: Arc<MemoryMonitor>) -> Result<Self, String> {
        let (tx, rx) = mpsc::sync_channel::<LlmCommand>(LLM_COMMAND_QUEUE_CAPACITY);
        let (startup_tx, startup_rx) = mpsc::sync_channel(1);
        let governor = Arc::new(LlmMemoryGovernor::new());
        let governor_worker = Arc::clone(&governor);
        thread::Builder::new()
            .name("pocket-brain-llm".into())
            .spawn(move || worker_loop(rx, governor_worker, monitor, startup_tx))
            .map_err(|error| format!("llm worker spawn failed: {error}"))?;
        startup_rx
            .recv()
            .map_err(|_| "llm worker gone during startup".to_string())??;
        Ok(Self {
            tx: Arc::new(Mutex::new(tx)),
            governor,
            startup_error: None,
        })
    }

    /// Preserve a managed Tauri state even when startup failed, so every IPC
    /// command receives a deterministic Worker Gone error instead of a missing
    /// state/panic. Used only by application bootstrap after `spawn` returned Err.
    pub fn unavailable(error: String) -> Self {
        let (tx, rx) = mpsc::sync_channel(1);
        drop(rx);
        Self {
            tx: Arc::new(Mutex::new(tx)),
            governor: Arc::new(LlmMemoryGovernor::new()),
            startup_error: Some(Arc::<str>::from(error)),
        }
    }

    fn enqueue(&self, command: LlmCommand) -> Result<(), String> {
        if let Some(error) = self.startup_error.as_deref() {
            return Err(format!("llm worker unavailable: {error}"));
        }
        let tx = self
            .tx
            .lock()
            .map_err(|_| "llm tx poisoned".to_string())?;
        match tx.try_send(command) {
            Ok(()) => Ok(()),
            Err(mpsc::TrySendError::Full(_)) => Err("llm worker queue full".into()),
            Err(mpsc::TrySendError::Disconnected(_)) => Err("llm worker gone".into()),
        }
    }

    /// Clone of the lock-free governor, to hand to the iOS lifecycle observer.
    pub fn governor(&self) -> Arc<LlmMemoryGovernor> {
        Arc::clone(&self.governor)
    }

    /// Register the frontend lifecycle event sink (single sink; a later call
    /// replaces the earlier one). Mirrors M6's `vault_events`.
    pub fn register_events(&self, channel: Channel<LlmLifecycleEvent>) -> Result<(), String> {
        self.enqueue(LlmCommand::RegisterEvents { channel })
    }

    /// Blocking: mmap-load the GGUF on the worker thread and await the result.
    /// Bounded by [`LLM_LOAD_TIMEOUT`] — see its doc comment.
    pub fn load(&self, model_path: PathBuf, params: LoadParams) -> Result<(), String> {
        let (reply, ack) = mpsc::sync_channel(1);
        let purge_epoch = self.governor.purge_epoch();
        self.enqueue(LlmCommand::Load {
            model_path,
            params,
            purge_epoch,
            reply,
        })?;
        match ack.recv_timeout(LLM_LOAD_TIMEOUT) {
            Ok(result) => result,
            Err(mpsc::RecvTimeoutError::Timeout) => {
                Err("llm load timed out (worker unresponsive)".to_string())
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                Err("llm worker dropped reply".to_string())
            }
        }
    }

    /// Start a generation, streaming tokens over `tokens`, and asynchronously
    /// wait until the worker has sent its terminal event.
    /// `task_id` is the sole authority for extraction routing (not copied into params).
    /// Bounded by [`LLM_GENERATE_TIMEOUT`] — see its doc comment.
    pub async fn generate(
        &self,
        params: GenerationParams,
        task_id: Option<String>,
        tokens: Channel<TokenEvent>,
    ) -> Result<(), String> {
        params.validate()?;
        let (completion, ack) = oneshot::channel();
        self.enqueue(LlmCommand::Generate {
            params,
            task_id,
            tokens,
            completion,
        })?;
        match tokio::time::timeout(LLM_GENERATE_TIMEOUT, ack).await {
            Ok(Ok(result)) => result,
            Ok(Err(_)) => Err("llm worker dropped generation completion".to_string()),
            Err(_) => Err("llm generation timed out (worker unresponsive)".to_string()),
        }
    }

    /// Phase 10: whether the worker still holds a loaded GGUF (Jetsam may have purged).
    /// Bounded by [`LLM_READY_PROBE_TIMEOUT`] — see its doc comment.
    pub fn is_loaded(&self) -> Result<bool, String> {
        let (reply, ack) = mpsc::sync_channel(1);
        self.enqueue(LlmCommand::IsLoaded { reply })?;
        match ack.recv_timeout(LLM_READY_PROBE_TIMEOUT) {
            Ok(result) => Ok(result),
            Err(mpsc::RecvTimeoutError::Timeout) => {
                Err("llm ready probe timed out (worker unresponsive)".to_string())
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                Err("llm worker dropped reply".to_string())
            }
        }
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
        if !(MIN_N_CTX..=MAX_N_CTX).contains(&n_ctx) {
            return Err(format!(
                "embedding n_ctx must be in {MIN_N_CTX}..={MAX_N_CTX}"
            ));
        }
        let (reply, ack) = mpsc::sync_channel(1);
        self.enqueue(LlmCommand::Embed { text, n_ctx, reply })?;
        ack.recv()
            .map_err(|_| "llm worker dropped reply".to_string())?
    }
}

fn worker_loop(
    rx: mpsc::Receiver<LlmCommand>,
    governor: Arc<LlmMemoryGovernor>,
    monitor: Arc<MemoryMonitor>,
    startup: mpsc::SyncSender<Result<(), String>>,
) {
    let backend = match LlamaBackend::init() {
        Ok(backend) => {
            let _ = startup.send(Ok(()));
            backend
        }
        Err(e) => {
            let error = format!("llm backend init failed: {e}");
            let _ = startup.send(Err(error));
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
        commit_pending_purge(&governor, &mut model, &monitor, events.as_ref());

        let cmd = match rx.recv_timeout(PURGE_POLL_INTERVAL) {
            Ok(cmd) => cmd,
            // Idle wake: re-check purge + emit Serious+ degradation once.
            Err(mpsc::RecvTimeoutError::Timeout) => {
                let level = governor.degradation();
                if level.throttle_generation() && last_degradation_emit < DegradationLevel::Serious
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

        // A purge can race between the loop-head check and a command waking
        // `recv_timeout`. Commit it before dispatch so a queued generation
        // cannot observe a model that the OS already ordered us to release.
        commit_pending_purge(&governor, &mut model, &monitor, events.as_ref());

        match cmd {
            LlmCommand::Load {
                model_path,
                params,
                purge_epoch,
                reply,
            } => {
                let result = if governor.load_is_stale(purge_epoch) {
                    Err("model load cancelled by memory purge".into())
                } else {
                    load_model(&backend, &model_path, &params).and_then(|loaded| {
                        if governor.load_is_stale(purge_epoch) {
                            // A purge arrived during the blocking mmap/load.
                            // Drop the temporary model instead of publishing it.
                            Err("model load cancelled by memory purge".into())
                        } else {
                            model = Some(Arc::new(loaded));
                            monitor.set_phase(MemPhase::ModelLoaded);
                            Ok(())
                        }
                    })
                };
                let _ = reply.send(result);
            }
            LlmCommand::Generate {
                params,
                task_id,
                tokens,
                completion,
            } => {
                let cancel_epoch = governor.cancel_epoch();
                let result = match model.as_ref() {
                    Some(model) => generate(
                        &backend,
                        model,
                        &params,
                        task_id.as_deref(),
                        &tokens,
                        &governor,
                        cancel_epoch,
                        &monitor,
                    ),
                    None => Err("MODEL_NOT_LOADED".into()),
                };
                monitor.set_phase(MemPhase::Idle);
                complete_generation(result, &tokens, completion);
            }
            LlmCommand::IsLoaded { reply } => {
                let _ = reply.send(model.is_some());
            }
            LlmCommand::Embed { text, n_ctx, reply } => {
                // Fair+: suppress expensive LLM embed re-warm; callers fall back
                // to hashed-ngram (embed_knowledge).
                if governor.degradation().suppress_background() {
                    let _ = reply.send(Err("degraded: background embed suppressed".into()));
                    continue;
                }
                let result = match model.as_ref() {
                    None => Err("MODEL_NOT_LOADED".into()),
                    Some(model) => {
                        let cancel_epoch = governor.cancel_epoch();
                        monitor.set_phase(MemPhase::CtxCreated);
                        let out = super::embed::embed_text(
                            &backend,
                            model.as_ref(),
                            &text,
                            n_ctx,
                            || governor.is_cancelled_since(cancel_epoch),
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
        }
    }
}

/// Consume an OS/Jetsam purge request on the worker thread. Returns whether a
/// request was consumed, independently of whether a model happened to be
/// resident.
fn commit_pending_purge(
    governor: &LlmMemoryGovernor,
    model: &mut Option<Arc<LlamaModel>>,
    monitor: &MemoryMonitor,
    events: Option<&Channel<LlmLifecycleEvent>>,
) -> bool {
    if !governor.take_purge() {
        return false;
    }
    let _ = model.take();
    monitor.set_phase(MemPhase::Baseline);
    // Emit even when no model is currently resident: a queued/in-flight load
    // may still need its frontend retry state invalidated by this boundary.
    if let Some(sink) = events {
        let _ = sink.send(LlmLifecycleEvent::MemoryPurged);
    }
    true
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

/// Resolve the tokenizer's baked-in BOS policy. `llama-cpp-2` exposes BOS as
/// an explicit enum rather than a model-default option, so mirror the GGUF
/// metadata and retain the historical `Always` fallback for legacy files.
fn add_bos_from_metadata(raw: Option<&str>) -> AddBos {
    match raw.map(str::trim).map(str::to_ascii_lowercase).as_deref() {
        Some("false" | "0") => AddBos::Never,
        _ => AddBos::Always,
    }
}

fn model_add_bos(model: &LlamaModel) -> AddBos {
    let raw = model.meta_val_str("tokenizer.ggml.add_bos_token").ok();
    add_bos_from_metadata(raw.as_deref())
}

/// The Simulator exposes a synthetic Metal device without the unified/shared
/// memory capabilities llama.cpp expects for reliable quantized inference.
/// Keep full Metal offload on physical iOS devices, but force Simulator builds
/// onto the host CPU so corrupted logits cannot reach the stream.
fn effective_n_gpu_layers(requested: u32, ios_simulator: bool) -> u32 {
    if ios_simulator {
        0
    } else {
        requested
    }
}

fn apply_context_device_policy(
    params: LlamaContextParams,
    ios_simulator: bool,
) -> LlamaContextParams {
    if ios_simulator {
        params.with_offload_kqv(false).with_op_offload(false)
    } else {
        params
    }
}

fn load_model(backend: &LlamaBackend, path: &Path, p: &LoadParams) -> Result<LlamaModel, String> {
    super::model_path::validate_gguf_file(path)?;
    let n_gpu_layers = effective_n_gpu_layers(
        p.n_gpu_layers,
        cfg!(all(target_os = "ios", target_abi = "sim")),
    );
    if n_gpu_layers != p.n_gpu_layers {
        eprintln!(
            "Coraxis LLM: iOS Simulator detected; forcing CPU inference (requested Metal layers: {})",
            p.n_gpu_layers
        );
    }
    let params = LlamaModelParams::default()
        .with_n_gpu_layers(n_gpu_layers)
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
    cancel_epoch: u64,
    monitor: &MemoryMonitor,
) -> Result<(), String> {
    g.validate()?;
    if governor.is_cancelled_since(cancel_epoch) {
        return Err("generation cancelled".into());
    }

    // Fail closed before allocating context / sampling for unknown tasks.
    let mode = resolve_generation_mode(task_id)?;

    let prompt = match mode {
        // Free-form RAG / consult / interview prompts are message content, not
        // pre-rendered model input. Always cross the model's baked-in role
        // boundary before tokenization (Qwen2.5-Instruct uses ChatML).
        GenerationMode::Chat => render_chat_prompt(model, CHAT_SYSTEM_PROMPT, &g.prompt)?,
        GenerationMode::KakeiboV1 => {
            let (system, user) = build_prompt(TASK_KAKEIBO_V1, &g.prompt)?;
            render_chat_prompt(model, &system, &user)?
        }
        GenerationMode::CognitiveDistortionV1 => {
            let (system, user) = build_prompt(TASK_COGNITIVE_DISTORTION_V1, &g.prompt)?;
            render_chat_prompt(model, &system, &user)?
        }
        GenerationMode::ReceiptOcrV1 => {
            let (system, user) = build_prompt(TASK_RECEIPT_OCR_V1, &g.prompt)?;
            render_chat_prompt(model, &system, &user)?
        }
        GenerationMode::InterviewEvaluationV1 => {
            let (system, user) = build_prompt(TASK_INTERVIEW_EVALUATION_V1, &g.prompt)?;
            render_chat_prompt(model, &system, &user)?
        }
        GenerationMode::MetacognitiveDebriefV1 => {
            let (system, user) = build_prompt(TASK_METACOGNITIVE_DEBRIEF_V1, &g.prompt)?;
            render_chat_prompt(model, &system, &user)?
        }
    };

    if governor.is_cancelled_since(cancel_epoch) {
        return Err("generation cancelled".into());
    }

    let requested_ctx = g.resolved_n_ctx();
    let trained_ctx = model.n_ctx_train();
    if trained_ctx > 0 && requested_ctx > trained_ctx {
        return Err(format!(
            "requested context {requested_ctx} exceeds model context {trained_ctx}"
        ));
    }
    let level = governor.degradation();
    let scaled_ctx = (((requested_ctx as f64) * f64::from(level.context_factor())).round() as u32)
        .clamp(MIN_N_CTX, requested_ctx);
    let input_token_budget = scaled_ctx
        .checked_sub(g.max_tokens)
        .ok_or_else(|| "output token budget exceeds degraded context".to_string())?
        as usize;

    let prompt_tokens = model
        .str_to_token(&prompt, model_add_bos(model))
        .map_err(|e| format!("tokenize: {e}"))?;
    if prompt_tokens.len() > input_token_budget {
        return Err(format!(
            "prompt exceeds context budget: {} > {input_token_budget}",
            prompt_tokens.len()
        ));
    }
    if governor.is_cancelled_since(cancel_epoch) {
        return Err("generation cancelled".into());
    }

    let ctx_params = apply_context_device_policy(
        LlamaContextParams::default().with_n_ctx(NonZeroU32::new(scaled_ctx)),
        cfg!(all(target_os = "ios", target_abi = "sim")),
    );
    let mut ctx = model
        .new_context(backend, ctx_params)
        .map_err(|e| format!("context create: {e}"))?;
    monitor.set_phase(MemPhase::CtxCreated);

    let mut batch = LlamaBatch::new(prompt_tokens.len().max(1), 1);
    let last = prompt_tokens.len().saturating_sub(1);
    for (i, token) in prompt_tokens.iter().enumerate() {
        batch
            .add(*token, i as i32, &[0], i == last)
            .map_err(|e| format!("batch add: {e}"))?;
    }
    if governor.is_cancelled_since(cancel_epoch) {
        return Err("generation cancelled".into());
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
        GenerationMode::ReceiptOcrV1 => {
            let grammar = LlamaSampler::grammar(model, RECEIPT_OCR_V1_GBNF, "root")
                .map_err(|e| format!("grammar init: {e}"))?;
            LlamaSampler::chain_simple([grammar, LlamaSampler::greedy()])
        }
        GenerationMode::InterviewEvaluationV1 => {
            let grammar = LlamaSampler::grammar(model, INTERVIEW_EVALUATION_V1_GBNF, "root")
                .map_err(|e| format!("grammar init: {e}"))?;
            LlamaSampler::chain_simple([grammar, LlamaSampler::greedy()])
        }
        GenerationMode::MetacognitiveDebriefV1 => {
            let grammar = LlamaSampler::grammar(model, METACOGNITIVE_DEBRIEF_V1_GBNF, "root")
                .map_err(|e| format!("grammar init: {e}"))?;
            LlamaSampler::chain_simple([grammar, LlamaSampler::greedy()])
        }
    };

    let mut extract_buf = String::new();
    let mut cancelled = false;
    let mut n_cur = batch.n_tokens();
    // A token piece can end halfway through a UTF-8 scalar. The decoder must
    // survive across the entire generation; decoding each token independently
    // discards Japanese byte fragments and corrupts streamed text.
    let mut text_decoder = Utf8TokenDecoder::default();
    let mut batcher = TokenStreamBatcher::new(|seq, text| {
        tokens
            .send(streaming_token(seq, text))
            .map_err(|e| format!("channel send: {e}"))
    });
    for seq in 0..g.max_tokens {
        if governor.is_cancelled_since(cancel_epoch) {
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
        let piece_bytes =
            token_piece_bytes(model, token).map_err(|e| format!("token decode: {e}"))?;
        let piece = text_decoder.push(&piece_bytes);
        batcher
            .push(seq, &piece)
            .map_err(|e| format!("channel send: {e}"))?;
        if matches!(
            mode,
            GenerationMode::KakeiboV1
                | GenerationMode::CognitiveDistortionV1
                | GenerationMode::ReceiptOcrV1
                | GenerationMode::InterviewEvaluationV1
                | GenerationMode::MetacognitiveDebriefV1
        ) {
            extract_buf.push_str(&piece);
        }
        // `LlamaSampler::sample` is sample-and-accept. Calling `accept` again
        // double-advances stateful grammar / repetition samplers.

        batch.clear();
        batch
            .add(token, n_cur, &[0], true)
            .map_err(|e| format!("batch add: {e}"))?;
        n_cur += 1;
        ctx.decode(&mut batch).map_err(|e| format!("decode: {e}"))?;
    }

    // Finalize a possible trailing partial scalar (for example when max_tokens
    // cuts between byte-fallback tokens) before the terminal event.
    let trailing = text_decoder.finish();
    if !trailing.is_empty() {
        batcher.push(g.max_tokens, &trailing)?;
        if !matches!(mode, GenerationMode::Chat) {
            extract_buf.push_str(&trailing);
        }
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
        GenerationMode::ReceiptOcrV1 => {
            let report = finalize_receipt_extraction(&extract_buf, cancelled)?;
            tokens
                .send(receipt_done_event(g.max_tokens, report))
                .map_err(|e| format!("channel send: {e}"))?;
            Ok(())
        }
        GenerationMode::InterviewEvaluationV1 => {
            let report = finalize_interview_evaluation(&extract_buf, cancelled)?;
            tokens
                .send(interview_eval_done_event(g.max_tokens, report))
                .map_err(|e| format!("channel send: {e}"))?;
            Ok(())
        }
        GenerationMode::MetacognitiveDebriefV1 => {
            let report = finalize_metacognitive_debrief(&extract_buf, cancelled)?;
            tokens
                .send(metacognitive_debrief_done_event(g.max_tokens, report))
                .map_err(|e| format!("channel send: {e}"))?;
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn disconnected_test_handle() -> (LlmHandle, mpsc::Receiver<LlmCommand>) {
        let (tx, rx) = mpsc::sync_channel(LLM_COMMAND_QUEUE_CAPACITY);
        (
            LlmHandle {
                tx: Arc::new(Mutex::new(tx)),
                governor: Arc::new(LlmMemoryGovernor::new()),
                startup_error: None,
            },
            rx,
        )
    }

    fn test_generation_params() -> GenerationParams {
        GenerationParams {
            prompt: "test".into(),
            n_ctx: 512,
            max_tokens: 1,
            temp: 0.0,
            top_k: 1,
            top_p: 1.0,
            seed: 0,
        }
    }

    #[test]
    fn dropping_clone_does_not_stop_shared_worker_channel() {
        let (handle, rx) = disconnected_test_handle();
        drop(handle.clone());

        assert!(matches!(rx.try_recv(), Err(mpsc::TryRecvError::Empty)));

        drop(handle);
        assert!(matches!(
            rx.try_recv(),
            Err(mpsc::TryRecvError::Disconnected)
        ));
    }

    #[test]
    fn bounded_queue_rejects_excess_work_without_blocking() {
        let (tx, _rx) = mpsc::sync_channel(1);
        let handle = LlmHandle {
            tx: Arc::new(Mutex::new(tx)),
            governor: Arc::new(LlmMemoryGovernor::new()),
            startup_error: None,
        };
        let first = Channel::new(|_| Ok(()));
        assert!(handle.register_events(first).is_ok());
        let second = Channel::new(|_| Ok(()));
        assert_eq!(
            handle.register_events(second),
            Err("llm worker queue full".into())
        );
    }

    #[test]
    fn unavailable_worker_propagates_startup_error() {
        let handle = LlmHandle::unavailable("backend unavailable".into());
        let channel = Channel::new(|_| Ok(()));
        assert_eq!(
            handle.register_events(channel),
            Err("llm worker unavailable: backend unavailable".into())
        );
    }

    #[tokio::test]
    async fn generate_waits_for_worker_completion_ack() {
        let (handle, rx) = disconnected_test_handle();
        let tokens = Channel::new(|_| Ok(()));
        let generation = handle.generate(test_generation_params(), None, tokens);
        tokio::pin!(generation);

        tokio::select! {
            result = &mut generation => panic!("generation resolved before worker ack: {result:?}"),
            _ = tokio::task::yield_now() => {}
        }

        let completion = match rx.try_recv() {
            Ok(LlmCommand::Generate { completion, .. }) => completion,
            Ok(_) => panic!("unexpected worker command"),
            Err(error) => panic!("generation command not queued: {error}"),
        };

        assert!(completion.send(Ok(())).is_ok());
        assert_eq!(generation.await, Ok(()));
    }

    #[test]
    fn error_terminal_is_sent_before_completion_ack() {
        let (completion, ack) = oneshot::channel();
        let ack = Arc::new(Mutex::new(ack));
        let ack_during_terminal = Arc::clone(&ack);
        let terminal_seen = Arc::new(AtomicBool::new(false));
        let terminal_seen_by_channel = Arc::clone(&terminal_seen);
        let tokens = Channel::new(move |body| {
            assert!(matches!(
                ack_during_terminal.lock().expect("ack lock").try_recv(),
                Err(oneshot::error::TryRecvError::Empty)
            ));
            match body {
                tauri::ipc::InvokeResponseBody::Json(json) => {
                    assert!(json.contains("\"done\":true"));
                    assert!(json.contains("\"error\":\"boom\""));
                }
                tauri::ipc::InvokeResponseBody::Raw(_) => panic!("unexpected raw token event"),
            }
            terminal_seen_by_channel.store(true, Ordering::SeqCst);
            Ok(())
        });

        complete_generation(Err("boom".into()), &tokens, completion);

        assert!(terminal_seen.load(Ordering::SeqCst));
        assert_eq!(ack.lock().expect("ack lock").try_recv(), Ok(Ok(())));
    }

    #[test]
    fn failed_terminal_delivery_rejects_completion_ack() {
        let (completion, mut ack) = oneshot::channel();
        let tokens = Channel::new(|_| Err(tauri::Error::FailedToReceiveMessage));

        complete_generation(Err("boom".into()), &tokens, completion);

        let error = ack
            .try_recv()
            .expect("completion ack")
            .expect_err("broken terminal channel must reject invoke");
        assert!(error.starts_with("token channel send:"));
    }

    #[test]
    fn governor_request_purge_advances_epoch_and_stays_sticky_until_consumed() {
        let g = LlmMemoryGovernor::new();
        let before = g.cancel_epoch();
        let purge_before = g.purge_epoch();
        assert!(!g.is_cancelled_since(before));
        assert!(!g.take_purge());

        g.request_purge();
        assert!(g.is_cancelled_since(before));
        assert_ne!(g.purge_epoch(), purge_before);
        let after = g.cancel_epoch();
        // Even a snapshot taken after the epoch advance must observe the
        // unconsumed purge request.
        assert!(g.is_cancelled_since(after));
        assert!(g.take_purge());
        assert!(!g.is_cancelled_since(after));
        assert!(!g.take_purge());
    }

    #[test]
    fn governor_cancel_only_invalidates_the_running_epoch() {
        let g = LlmMemoryGovernor::new();
        let running = g.cancel_epoch();
        let submitted_load = g.purge_epoch();
        g.request_cancel();
        assert!(g.is_cancelled_since(running));
        assert!(!g.take_purge());
        assert!(!g.load_is_stale(submitted_load));
        let next_job = g.cancel_epoch();
        assert!(!g.is_cancelled_since(next_job));
    }

    #[test]
    fn queued_load_from_before_purge_stays_stale_after_purge_is_consumed() {
        let g = LlmMemoryGovernor::new();
        let stale_load = g.purge_epoch();
        g.request_purge();
        assert!(g.load_is_stale(stale_load));
        assert!(g.take_purge());
        assert!(g.load_is_stale(stale_load));

        let foreground_load = g.purge_epoch();
        assert!(!g.load_is_stale(foreground_load));
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
    fn mode_receipt_ocr_v1_is_extract() {
        assert_eq!(
            resolve_generation_mode(Some(TASK_RECEIPT_OCR_V1)),
            Ok(GenerationMode::ReceiptOcrV1)
        );
    }

    #[test]
    fn mode_interview_evaluation_v1_is_extract() {
        assert_eq!(
            resolve_generation_mode(Some(TASK_INTERVIEW_EVALUATION_V1)),
            Ok(GenerationMode::InterviewEvaluationV1)
        );
    }

    #[test]
    fn mode_metacognitive_debrief_v1_is_extract() {
        assert_eq!(
            resolve_generation_mode(Some(TASK_METACOGNITIVE_DEBRIEF_V1)),
            Ok(GenerationMode::MetacognitiveDebriefV1)
        );
    }

    #[test]
    fn mode_unknown_is_err() {
        assert!(resolve_generation_mode(Some("nope")).is_err());
    }

    #[test]
    fn bos_policy_tracks_gguf_metadata() {
        assert_eq!(add_bos_from_metadata(Some("false")), AddBos::Never);
        assert_eq!(add_bos_from_metadata(Some(" 0 ")), AddBos::Never);
        assert_eq!(add_bos_from_metadata(Some("true")), AddBos::Always);
        assert_eq!(add_bos_from_metadata(None), AddBos::Always);
    }

    #[test]
    fn simulator_forces_cpu_without_changing_device_offload() {
        assert_eq!(effective_n_gpu_layers(999, true), 0);
        assert_eq!(effective_n_gpu_layers(999, false), 999);
        assert_eq!(effective_n_gpu_layers(0, false), 0);

        let simulator = apply_context_device_policy(LlamaContextParams::default(), true);
        assert!(!simulator.offload_kqv());
        assert!(!simulator.op_offload());

        let device = apply_context_device_policy(LlamaContextParams::default(), false);
        assert!(device.offload_kqv());
        assert!(device.op_offload());
    }

    #[test]
    fn utf8_decoder_preserves_scalar_split_across_token_pieces() {
        let mut decoder = Utf8TokenDecoder::default();
        let bytes = "思".as_bytes();

        assert!(decoder.push(&bytes[..1]).is_empty());
        assert_eq!(decoder.push(&bytes[1..]), "思");
        assert!(decoder.finish().is_empty());
    }

    #[test]
    fn utf8_decoder_replaces_only_irrecoverably_invalid_bytes() {
        let mut decoder = Utf8TokenDecoder::default();
        assert_eq!(decoder.push(b"A\xFFB"), "A\u{FFFD}B");
        assert!(decoder.finish().is_empty());
    }

    #[test]
    fn token_piece_retry_uses_llama_reported_capacity() {
        assert_eq!(required_piece_capacity(-9), Some(9));
        assert_eq!(required_piece_capacity(0), None);
        assert_eq!(required_piece_capacity(9), None);
        assert_eq!(required_piece_capacity(i32::MIN), None);
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
