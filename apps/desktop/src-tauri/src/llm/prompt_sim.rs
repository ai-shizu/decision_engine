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

fn append_company_block(out: &mut String, facts: &CompanyFacts) {
    out.push_str("\n## 企業ファクト（EDINET）\n");
    out.push_str(&render_company_facts_block(facts));
}

/// Build the full interview-generation prompt.
pub fn build_interview_prompt(
    user_message: &str,
    facts: &CompanyFacts,
    experience_hits: &[ExperienceRef<'_>],
) -> String {
    let mut out = String::with_capacity(user_message.len() + 1024);
    out.push_str(INTERVIEWER_PERSONA);
    append_company_block(&mut out, facts);
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

#[cfg(test)]
mod tests {
    use super::*;

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
        let prompt = build_interview_prompt("自己紹介してください", &sample_facts(), &hits);
        assert!(prompt.contains("面接官"));
        assert!(prompt.contains("企業ファクト"));
        assert!(prompt.contains("テスト株式会社"));
        assert!(prompt.contains("留学でチーム"));
        assert!(prompt.contains("自己紹介"));
    }

    #[test]
    fn es_review_contains_draft() {
        let prompt = build_es_review_prompt("私は挑戦を大切にします。", &sample_facts(), &[]);
        assert!(prompt.contains("書類選考"));
        assert!(prompt.contains("挑戦を大切"));
        assert!(prompt.contains("経験チャンクなし"));
    }
}
