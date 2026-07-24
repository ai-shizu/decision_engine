//! 知識チャンクの名前空間分離（2026-07-25）。
//!
//! 型の正本は `crate::db::knowledge_namespace`（vault worker が参照するため）。
//! 本モジュールは RAG / FE IPC 向けの公開入口。

pub use crate::db::knowledge_namespace::KnowledgeNamespace;

/// IPC 文字列 → 名前空間。未指定は `All`（既存呼び出し互換）。不正は Err。
pub fn parse_namespace_arg(raw: Option<&str>) -> Result<KnowledgeNamespace, String> {
    match raw {
        None => Ok(KnowledgeNamespace::All),
        Some("all") => Ok(KnowledgeNamespace::All),
        Some("personal") => Ok(KnowledgeNamespace::Personal),
        Some("company") => Ok(KnowledgeNamespace::Company),
        Some(_) => Err("invalid namespace".into()),
    }
}
