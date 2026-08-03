//! Interview / ES simulation prompt assembly (M12).
//!
//! Three layers, always in this order (static persona → company facts → RAG →
//! user turn). Gap / profiler insights are intentionally NOT injected — matches
//! Python interview_sim / es_review isolation (AI_SKILLS §7.1).

use crate::knowledge::edinet_client::{render_company_facts_block, CompanyFacts};

/// Soft cap on injected experience characters (Jetsam / n_ctx headroom).
pub const EXPERIENCE_CHAR_BUDGET: usize = 6_000;

/// Minimal view of a retrieved personal-knowledge chunk.
#[derive(Debug, Clone, Copy)]
pub struct ExperienceRef<'a> {
    pub id: &'a str,
    pub text: &'a str,
}

const INTERVIEWER_PERSONA: &str = "\
あなたは外資系 / テック企業の厳格な面接官である。\
企業ファクト（有価証券報告書由来）と本セッションの候補者発話だけを根拠に質問せよ。\
日常 Vault / Gap / Tensor を参照するな。ファクトに無い数字や戦略を補完して助け舟を出さない。\
曖昧な自己PRには具体例と定量を要求せよ。一度に1つの問いだけを投げる。\
人格攻撃は禁止。攻撃対象は論理と事実のみ。";

const ES_REVIEWER_PERSONA: &str = "\
あなたは外資系 / テック企業の書類選考責任者である。\
提出 ES の論理的強度のみを評価する。書き手の日常プロファイルは考慮しない。\
企業ファクトとの整合・定量根拠・再現性の欠如を容赦なく指摘し、該当箇所の引用と書き直し例を示せ。\
誉め言葉で薄めない。";

const COMPANY_ANALYST_SYSTEM: &str = "\
あなたは就職・転職の面接対策に特化した企業アナリストである。
以下の企業データ（有価証券報告書・公開百科事典由来）だけを根拠に分析せよ。
データに無い数字・戦略・将来予測を creative に補完することを固く禁じる。
根拠が薄い項目は「データ不足」と明記せよ。推測で埋めるな。

出力は必ず次の4セクションを \"## \" 見出しで、この順に出力すること。

## 面接で深掘りされやすい事業リスク
## 直近の企業戦略の変化
## 予想される質問
## 逆質問の候補

各セクションは3〜5個の箇条書き（\"- \" 始まり）。1項目は80文字以内。
「予想される質問」は面接官が候補者に投げる問い、「逆質問の候補」は候補者が面接官に投げる問いである。混同するな。";

const SESSION_MEMORY_SYSTEM: &str = "\
あなたは、この人物を長期にわたって伴走するメンターの記憶装置である。
以下の対話ログを読み、「今後のメンタリングにおいて最も価値をもたらすインサイト」を抽出せよ。

抽出する観点は、あなた自身が対話から自由に見出せ。
何を重要とみなすかの判断そのものが、あなたに委ねられている。
（思考の癖、論理展開の強度と弱点、無意識に置いている前提、言語化されていない価値観、
自信や不安の変化、キャリアの軸、といったものは着眼点の一例に過ぎない。
この列挙に縛られるな。ここに無い観点を見出したなら、それを優先して書け。）

制約:
- 対話ログから読み取れることだけを書け。人物像を creative に脚色するな。根拠の薄い断定を書くな。
- 出来事の要約ではなく、この人物を理解するための洞察を書け。
- 次に会話する自分自身が読んで即座に使える具体性で書け。抽象的な美辞麗句は書くな。
- 良し悪しの評価や励ましは不要。観察と解釈のみを書け。

出力形式（唯一の構造的制約）:
- 各インサイトを \"## \" 見出しで始め、その下に本文を書く。
- 見出しの言葉・粒度・個数は、あなたが内容に合わせて自由に決めよ。定型の項目名を使う必要はない。
- 該当する洞察が無ければ、何も出力しなくてよい。無理に埋めるな。";

fn append_rag_block(out: &mut String, hits: &[ExperienceRef<'_>]) {
    out.push_str("\n## 候補者の過去経験（明示指定時のみ）\n");
    if hits.is_empty() {
        out.push_str("（経験チャンクなし — 企業ファクトと提出文面のみで評価せよ）\n");
        return;
    }
    let mut used = 0usize;
    for (index, hit) in hits.iter().enumerate() {
        let block = format!(
            "[{}] (id={})\n{}\n\n",
            index + 1,
            hit.id,
            hit.text.trim()
        );
        if used + block.len() > EXPERIENCE_CHAR_BUDGET && used > 0 {
            break;
        }
        out.push_str(&block);
        used = used.saturating_add(block.len());
    }
}

fn append_company_data_block(out: &mut String, chunks: &[ExperienceRef<'_>]) {
    out.push_str("\n## 企業データ\n");
    if chunks.is_empty() {
        out.push_str("（企業チャンクなし）\n");
        return;
    }
    let mut used = 0usize;
    for (index, hit) in chunks.iter().enumerate() {
        let block = format!(
            "[{}] (id={})\n{}\n\n",
            index + 1,
            hit.id,
            hit.text.trim()
        );
        if used + block.len() > EXPERIENCE_CHAR_BUDGET && used > 0 {
            break;
        }
        out.push_str(&block);
        used = used.saturating_add(block.len());
    }
}

fn append_company_block(out: &mut String, facts: &CompanyFacts) {
    out.push_str("\n## 企業ファクト（EDINET）\n");
    out.push_str(&render_company_facts_block(facts));
}

fn append_es_base_block(out: &mut String, es_text: Option<&str>) {
    let Some(body) = es_text.map(str::trim).filter(|s| !s.is_empty()) else {
        return;
    };
    out.push_str("\n## 候補者の提出 ES（面接のベース）\n");
    out.push_str(body);
    out.push('\n');
}

/// Build the full interview-generation prompt.
pub fn build_interview_prompt(
    user_message: &str,
    facts: &CompanyFacts,
    experience_hits: &[ExperienceRef<'_>],
    es_text: Option<&str>,
) -> String {
    let mut out = String::with_capacity(user_message.len() + 1024);
    out.push_str(INTERVIEWER_PERSONA);
    append_company_block(&mut out, facts);
    append_es_base_block(&mut out, es_text);
    append_rag_block(&mut out, experience_hits);
    out.push_str("\n## 候補者の発話\n");
    out.push_str(user_message.trim());
    out.push('\n');
    out
}

/// Build the ES review prompt (draft + company facts + optional experience).
pub fn build_es_review_prompt(
    es_draft: &str,
    facts: &CompanyFacts,
    experience_hits: &[ExperienceRef<'_>],
) -> String {
    let mut out = String::with_capacity(es_draft.len() + 1024);
    out.push_str(ES_REVIEWER_PERSONA);
    append_company_block(&mut out, facts);
    append_rag_block(&mut out, experience_hits);
    out.push_str("\n## 提出 ES 原稿\n");
    out.push_str(es_draft.trim());
    out.push('\n');
    out
}

/// Company-namespace dashboard analysis (non-persistent; delayed evaluation).
pub fn build_company_analysis_prompt(
    company_name: &str,
    chunks: &[ExperienceRef<'_>],
) -> String {
    let mut out = String::with_capacity(2048);
    out.push_str(COMPANY_ANALYST_SYSTEM);
    out.push_str("\n\n## 分析対象\n");
    out.push_str(company_name.trim());
    out.push('\n');
    append_company_data_block(&mut out, chunks);
    out
}

/// Mentor session-memory extraction (Personal namespace ingest).
pub fn build_session_memory_prompt(session_kind: &str, transcript: &str) -> String {
    let mut out = String::with_capacity(transcript.len() + 1024);
    out.push_str(SESSION_MEMORY_SYSTEM);
    out.push_str("\n\n## セッション種別\n");
    out.push_str(session_kind.trim());
    out.push_str("\n\n## 対話ログ\n");
    out.push_str(transcript.trim());
    out.push('\n');
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::llm::context_budget::fit_prompt_to_budget_with_markers;

    fn sample_facts() -> CompanyFacts {
        CompanyFacts {
            company_name: "テスト株式会社".into(),
            edinet_code: "E00001".into(),
            doc_id: "S100TEST".into(),
            business_summary: "SaaS".into(),
            business_risks: "競合激化".into(),
            performance_summary: "増収".into(),
            source: "injected".into(),
        }
    }

    #[test]
    fn interview_contains_three_layers() {
        let hits = [ExperienceRef {
            id: "memo::0000",
            text: "留学でチームを率いた",
        }];
        let prompt = build_interview_prompt("自己紹介してください", &sample_facts(), &hits, None);
        assert!(prompt.contains("面接官"));
        assert!(prompt.contains("企業ファクト"));
        assert!(prompt.contains("テスト株式会社"));
        assert!(prompt.contains("留学でチーム"));
        assert!(prompt.contains("自己紹介"));
        assert!(!prompt.contains("提出 ES"));
    }

    #[test]
    fn interview_includes_es_base_when_present() {
        let prompt = build_interview_prompt(
            "志望動機を述べてください",
            &sample_facts(),
            &[],
            Some("ガクチカ: 研究で定量検証した。"),
        );
        assert!(prompt.contains("提出 ES"));
        assert!(prompt.contains("ガクチカ"));
        assert!(prompt.contains("志望動機"));
    }

    #[test]
    fn es_review_contains_draft() {
        let prompt = build_es_review_prompt("私は挑戦を大切にします。", &sample_facts(), &[]);
        assert!(prompt.contains("書類選考"));
        assert!(prompt.contains("挑戦を大切"));
        assert!(prompt.contains("経験チャンクなし"));
    }

    #[test]
    fn company_analysis_has_four_sections_and_target_marker() {
        let chunks = [ExperienceRef {
            id: "wiki-acme::0000",
            text: "事業はクラウドです。",
        }];
        let prompt = build_company_analysis_prompt("Acme株式会社", &chunks);
        assert!(prompt.contains("## 面接で深掘りされやすい事業リスク"));
        assert!(prompt.contains("## 直近の企業戦略の変化"));
        assert!(prompt.contains("## 予想される質問"));
        assert!(prompt.contains("## 逆質問の候補"));
        assert!(prompt.contains("## 分析対象"));
        assert!(prompt.contains("Acme株式会社"));
        assert!(prompt.contains("wiki-acme::0000"));
    }

    #[test]
    fn session_memory_has_log_marker_without_fixed_schema() {
        let prompt =
            build_session_memory_prompt("interview", "候補者: こんにちは\n面接官: 自己紹介を");
        assert!(prompt.contains("## 対話ログ"));
        assert!(prompt.contains("候補者: こんにちは"));
        // 第3条: 抽出項目の定型スキーマ語をプロンプトに固定しない。
        assert!(!prompt.contains("以下の項目を抽出"));
        assert!(!prompt.contains("次の項目を書け"));
        assert!(!prompt.contains("必須項目"));
    }

    #[test]
    fn company_analysis_target_marker_survives_budget_fit() {
        let big = "企業データの長文。".repeat(800);
        let chunks = [ExperienceRef {
            id: "wiki-x::0000",
            text: &big,
        }];
        let prompt = build_company_analysis_prompt("テスト株式会社", &chunks);
        let out = fit_prompt_to_budget_with_markers(&prompt, 120, &["## 分析対象"]);
        let idx = out.find("## 分析対象").expect("marker must survive");
        assert!(
            out[idx..].contains("テスト株式会社"),
            "protected tail must keep company name"
        );
    }

    #[test]
    fn session_memory_log_marker_survives_budget_fit() {
        let head_pad = "前置き。".repeat(400);
        let log = "候補者の重要な発言がここにあります。";
        let prompt = format!(
            "{head_pad}{}",
            build_session_memory_prompt("consult", log)
        );
        let out = fit_prompt_to_budget_with_markers(&prompt, 80, &["## 対話ログ"]);
        let idx = out.find("## 対話ログ").expect("log marker must survive");
        assert!(out[idx..].contains(log));
    }
}
