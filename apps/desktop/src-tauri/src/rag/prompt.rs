//! RAG prompt assembly (M11 / Phase 9 hierarchical budget).
//!
//! Retrieved chunks compete for a **token** budget via
//! [`crate::llm::context_budget`] (Baddeley chunking + MemGPT-style demotion +
//! Ebbinghaus salience). Low-salience overflow is compressed to metadata stubs
//! rather than silent tail truncation.

use std::time::{SystemTime, UNIX_EPOCH};

use crate::llm::context_budget::{
    fit_context_budget, BudgetChunk, DEFAULT_RAG_TOKEN_BUDGET,
};

/// Soft cap on injected context in estimated tokens (Phase 9).
pub const RAG_CONTEXT_TOKEN_BUDGET: usize = DEFAULT_RAG_TOKEN_BUDGET;

const RAG_SYSTEM_PREAMBLE: &str = "\
あなたはユーザーの個人知識ベースと行動記録を突き合わせる厳しい壁打ち相手である。\
以下の「参考情報」および注入されたギャップ／Tensor／Oracleを優先して回答せよ。\
同意やお世辞だけで終わらせるな。自己申告と記録が矛盾していれば「本当にそうか？」と問い、\
過去メモとの食い違いを具体的に指摘せよ。参考情報に無い事柄は推測で断定せず、不足を述べよ。\
回答は簡潔に、日本語で書いてください。";

/// Minimal view of a retrieved chunk for prompt injection.
#[derive(Debug, Clone, Copy)]
pub struct RagContextRef<'a> {
    pub id: &'a str,
    pub text: &'a str,
    /// Retrieval relevance in \[0, 1\] (e.g. normalized recall score).
    pub relevance: f64,
    /// Chunk creation time (Unix UTC). `0` ⇒ no temporal decay.
    pub created_at: i64,
}

fn now_unix() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// Build the full generation prompt from the user message and KNN hits.
pub fn build_rag_prompt(message: &str, hits: &[RagContextRef<'_>]) -> String {
    build_rag_prompt_with_budget(message, hits, RAG_CONTEXT_TOKEN_BUDGET, now_unix())
}

/// Testable entry: inject `now` and token budget.
pub fn build_rag_prompt_with_budget(
    message: &str,
    hits: &[RagContextRef<'_>],
    token_budget: usize,
    now_unix: i64,
) -> String {
    let mut out = String::with_capacity(message.len() + 512);
    out.push_str(RAG_SYSTEM_PREAMBLE);
    out.push_str("\n\n## 参考情報\n");

    if hits.is_empty() {
        out.push_str("（該当する知識チャンクは見つかりませんでした）\n");
    } else {
        let chunks: Vec<BudgetChunk> = hits
            .iter()
            .map(|h| BudgetChunk {
                id: h.id.to_string(),
                text: h.text.trim().to_string(),
                relevance: h.relevance,
                created_at_unix: h.created_at,
                token_estimate: None,
            })
            .collect();
        let fitted = fit_context_budget(&chunks, token_budget, now_unix);
        out.push_str(&format!(
            "（予算≈{token_budget}tok / 保持≈{}tok / 圧縮スタブ≈{}tok）\n",
            fitted.tokens_kept, fitted.tokens_compressed_stubs
        ));
        for (index, hit) in fitted.kept.iter().enumerate() {
            out.push_str(&format!(
                "[{}] (id={})\n{}\n\n",
                index + 1,
                hit.id,
                hit.text
            ));
        }
        if !fitted.compressed.is_empty() {
            out.push_str("### 圧縮コンテキスト（階層要約 / 低サリエンス）\n");
            for stub in &fitted.compressed {
                out.push_str(&stub.stub_text);
                out.push('\n');
            }
            out.push('\n');
        }
    }

    out.push_str("## ユーザーの質問\n");
    out.push_str(message.trim());
    out.push('\n');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn includes_message_and_empty_context_note() {
        let prompt = build_rag_prompt("こんにちは", &[]);
        assert!(prompt.contains("ユーザーの質問"));
        assert!(prompt.contains("こんにちは"));
        assert!(prompt.contains("見つかりませんでした"));
    }

    #[test]
    fn includes_hit_text() {
        let hits = [RagContextRef {
            id: "c1",
            text: "日記の内容",
            relevance: 1.0,
            created_at: 0,
        }];
        let prompt = build_rag_prompt("質問", &hits);
        assert!(prompt.contains("日記の内容"));
        assert!(prompt.contains("c1"));
    }

    #[test]
    fn overflow_emits_compressed_tier() {
        let long = "詳細な記憶テキスト。".repeat(100);
        let hits = [
            RagContextRef {
                id: "keep",
                text: "短い重要メモ",
                relevance: 1.0,
                created_at: 1_700_000_000,
            },
            RagContextRef {
                id: "drop",
                text: long.as_str(),
                relevance: 0.1,
                created_at: 1_700_000_000 - 90 * 86_400,
            },
        ];
        let prompt = build_rag_prompt_with_budget("Q", &hits, 40, 1_700_000_000);
        assert!(prompt.contains("短い重要メモ") || prompt.contains("keep"));
        assert!(
            prompt.contains("圧縮コンテキスト") || prompt.contains("[compressed id=drop]")
        );
    }
}
