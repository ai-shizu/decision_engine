//! RAG prompt assembly (M11).
//!
//! Retrieved chunks are injected as an explicit "参考情報" block ahead of the
//! user question. The model is instructed to prefer that context and to say
//! when evidence is missing — never to invent citations.

/// Soft cap on injected context characters (Jetsam / n_ctx headroom).
pub const RAG_CONTEXT_CHAR_BUDGET: usize = 6_000;

const RAG_SYSTEM_PREAMBLE: &str = "\
あなたはユーザーの個人知識ベースを参照するアシスタントです。\
以下の「参考情報」に書かれた内容を優先して回答してください。\
参考情報に無い事柄は推測で断定せず、不足している旨を述べてください。\
回答は簡潔に、日本語で書いてください。";

/// Minimal view of a retrieved chunk for prompt injection.
#[derive(Debug, Clone, Copy)]
pub struct RagContextRef<'a> {
    pub id: &'a str,
    pub text: &'a str,
}

/// Build the full generation prompt from the user message and KNN hits.
///
/// Layout:
/// ```text
/// {system preamble}
///
/// ## 参考情報
/// [1] (id=…)
/// {chunk text}
/// …
///
/// ## ユーザーの質問
/// {message}
/// ```
pub fn build_rag_prompt(message: &str, hits: &[RagContextRef<'_>]) -> String {
    let mut out = String::with_capacity(message.len() + 512);
    out.push_str(RAG_SYSTEM_PREAMBLE);
    out.push_str("\n\n## 参考情報\n");

    if hits.is_empty() {
        out.push_str("（該当する知識チャンクは見つかりませんでした）\n");
    } else {
        let mut used = 0usize;
        for (index, hit) in hits.iter().enumerate() {
            let block = format!(
                "[{}] (id={})\n{}\n\n",
                index + 1,
                hit.id,
                hit.text.trim()
            );
            if used + block.len() > RAG_CONTEXT_CHAR_BUDGET && used > 0 {
                break;
            }
            out.push_str(&block);
            used = used.saturating_add(block.len());
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
            id: "doc::0000",
            text: "秘密のメモ",
        }];
        let prompt = build_rag_prompt("何が書いてある？", &hits);
        assert!(prompt.contains("秘密のメモ"));
        assert!(prompt.contains("doc::0000"));
    }
}
