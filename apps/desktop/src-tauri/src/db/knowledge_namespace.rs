//! 知識チャンクの名前空間分離（2026-07-25: 企業「事業概要」に LINE 会話ログが
//! 混入した実バグの再発防止）。knowledge_chunks は単一 vec0 テーブルであり、
//! パーソナル / 外部知識が同居する。分類は chunk id の source_id 接頭辞から
//! 決定論的に導出する（スキーマ変更不要）。
//!
//! 正本は db 側（vault worker が参照）。`rag::namespace` は再エクスポート。

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KnowledgeNamespace {
    /// LINE 履歴・日次コンテキスト・ユーザー手記など本人由来の情報。
    Personal,
    /// 企業・外部知識レーン（EDINET / Wikipedia 等）。
    Company,
    /// 名前空間を問わない（従来動作）。CONSULT / ES 経験検索など本人文脈が正当な用途のみ。
    All,
}

/// `"::"` の手前を返す（区切りが無ければ全体）。
pub fn source_id_of(chunk_id: &str) -> &str {
    match chunk_id.split_once("::") {
        Some((source, _)) => source,
        None => chunk_id,
    }
}

/// source_id 接頭辞で分類。未知は Personal（fail-closed — 企業レーンへ流さない）。
pub fn namespace_of(chunk_id: &str) -> KnowledgeNamespace {
    let source = source_id_of(chunk_id);
    if source.starts_with("company-")
        || source.starts_with("edinet-")
        || source.starts_with("wiki-")
        || source.starts_with("knowledge-")
    {
        KnowledgeNamespace::Company
    } else {
        // line- / daily- / 任意・空・区切り無し → Personal
        KnowledgeNamespace::Personal
    }
}

pub fn matches(ns: KnowledgeNamespace, chunk_id: &str) -> bool {
    match ns {
        KnowledgeNamespace::All => true,
        other => namespace_of(chunk_id) == other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn namespace_of_line_chunk() {
        assert_eq!(
            namespace_of("line-talk2024::0003"),
            KnowledgeNamespace::Personal
        );
    }

    #[test]
    fn namespace_of_daily_chunk() {
        assert_eq!(
            namespace_of("daily-2026-07-20::0000"),
            KnowledgeNamespace::Personal
        );
    }

    #[test]
    fn namespace_of_edinet_chunk() {
        assert_eq!(
            namespace_of("edinet-E00001::0001"),
            KnowledgeNamespace::Company
        );
    }

    #[test]
    fn namespace_of_unknown_is_personal_fail_closed() {
        assert_eq!(namespace_of("mymemo::0002"), KnowledgeNamespace::Personal);
    }

    #[test]
    fn namespace_of_no_separator() {
        assert_eq!(namespace_of("plain-id"), KnowledgeNamespace::Personal);
        assert_eq!(source_id_of("plain-id"), "plain-id");
    }

    #[test]
    fn namespace_of_empty() {
        assert_eq!(namespace_of(""), KnowledgeNamespace::Personal);
        assert_eq!(source_id_of(""), "");
    }

    #[test]
    fn matches_all_always_true() {
        assert!(matches(KnowledgeNamespace::All, "line-x::0000"));
        assert!(matches(KnowledgeNamespace::All, "edinet-E1::0000"));
    }
}
