//! [A] Core LLM Service module (docs/architecture_blueprint.md §3.6).
//!
//! Compiled only under the `pocket-brain` feature (gated in `lib.rs`), so the
//! default desktop build never sees it and stays byte-identical.
//!
//! Commands (`llm_*`, `memory_monitor_*`, `llm_events`) are registered from
//! `lib.rs` under `#[cfg(feature = "pocket-brain")]`. M7 adds the lock-free
//! `LlmMemoryGovernor` + out-of-band `LlmLifecycleEvent::MemoryPurged` channel.
//! M12 adds interview/ES prompt assembly (`prompt_sim`) and gated sim commands.
//! M17 adds mentor consult context + multi-stage interview machine.

pub mod commands_llm;
pub mod embed;
pub mod model_path;
pub mod params;
pub mod prompt;
pub mod prompt_sim;
pub mod schema;
pub mod service;

#[cfg(all(feature = "secure-vault", target_vendor = "apple"))]
pub mod commands_consult;
#[cfg(all(feature = "secure-vault", target_vendor = "apple"))]
pub mod commands_sim;
#[cfg(all(feature = "secure-vault", target_vendor = "apple"))]
pub mod consult_context;
#[cfg(all(feature = "secure-vault", target_vendor = "apple"))]
pub mod interview_machine;

// Re-exported for `lib.rs`'s `llm::LlmHandle::spawn(...)` wiring. Other types
// (e.g. `LlmMemoryGovernor`, `LlmLifecycleEvent`) are referenced through their
// `service` submodule directly, so they are not re-exported here.
pub use service::LlmHandle;
