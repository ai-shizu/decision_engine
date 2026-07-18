//! [A] Core LLM Service module (docs/architecture_blueprint.md §3.6).
//!
//! Compiled only under the `pocket-brain` feature (gated in `lib.rs`), so the
//! default desktop build never sees it and stays byte-identical.
//!
//! Phase 1 status: worker + monitor logic implemented; commands defined and
//! delegating to that logic, but NOT yet registered in the `invoke_handler` and
//! State not managed (frontend not connected). Registration lands with the
//! frontend-integration step.

pub mod commands_llm;
pub mod model_path;
pub mod params;
pub mod service;

pub use params::{GenerationParams, LoadParams};
pub use service::{LlmHandle, TokenEvent};
