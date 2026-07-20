//! M10 on-device RAG pipeline (chunk → embed → vault vec0).
//!
//! Feature-gated behind both `pocket-brain` and `secure-vault` (see `lib.rs`).

pub mod chunk;
pub mod commands_daily;
pub mod commands_rag;
pub mod prompt;
