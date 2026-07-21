//! M10 on-device RAG pipeline (chunk → embed → vault vec0).
//!
//! Phase 7 adds associative recall (`associative_recall`): lexical hash KNN ×
//! optional dense KNN via RRF (Cormack 2009) + Ebbinghaus time decay (1885).
//!
//! Feature-gated behind both `pocket-brain` and `secure-vault` (see `lib.rs`).

pub mod associative_recall;
pub mod chunk;
pub mod commands_daily;
pub mod commands_rag;
pub mod embed_knowledge;
pub mod prompt;
