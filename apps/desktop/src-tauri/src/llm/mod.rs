//! [A] Core LLM Service module (docs/architecture_blueprint.md §3.6).
//!
//! Compiled only under the `pocket-brain` feature (gated in `lib.rs`), so the
//! default desktop build never sees it and stays byte-identical.
//!
//! Commands (`llm_*`, `memory_monitor_*`, `llm_events`) are registered from
//! `lib.rs` under `#[cfg(feature = "pocket-brain")]`. M7 adds the lock-free
//! `LlmMemoryGovernor` + out-of-band `LlmLifecycleEvent::MemoryPurged` channel.

pub mod commands_llm;
pub mod model_path;
pub mod params;
pub mod prompt;
pub mod schema;
pub mod service;

// Re-exported for `lib.rs`'s `llm::LlmHandle::spawn(...)` wiring. Other types
// (e.g. `LlmMemoryGovernor`, `LlmLifecycleEvent`) are referenced through their
// `service` submodule directly, so they are not re-exported here.
pub use service::LlmHandle;
