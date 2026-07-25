//! Ambient company-fact enrichment for Interview / ES review (E0b-safe).
//!
//! Net paths require Settings NetworkPolicy On ∧ egress-live (same as Consult).
//! Python knowledge_fetcher remains E0a-locked — do not call it from here.
//! Never bind external_research_id into interview_sim / discussion prompts.
//! EDINET codes are resolved from company name (no manual code UI).

import type { CompanyFacts } from "./pocketBrain/types";
import type { KnowledgeResearchReceipt } from "./parseEngineResponse";
import { deriveProvenance, provenanceChipText } from "./researchUiReducer";

// Cap company-lane RAG summary so interview prompts (input budget ≈1792) are
// not exhausted by businessSummary alone before the budget fitter runs.
const MAX_SUMMARY_CHARS = 800;

export function buildCompanyResearchQuery(companyName: string): string {
  return `${companyName.trim()} 事業概要`;
}

/**
 * Wikipedia レーン専用クエリ。記事ヒット率を優先し、修飾語を付けない素の企業名。
 * Vault のベクトル検索とは目的が異なるため `buildCompanyResearchQuery` と分離する
 * （`事業概要` は全文検索ではノイズ語としてスコアを歪める）。
 * 法人格サフィックスの除去は意図的に行わない（Rust 側は list=search の全文検索であり、
 * 除去ヒューリスティックは前置・後置の揺れで誤動作するため）。
 */
export function buildWikipediaResearchQuery(companyName: string): string {
  return companyName.trim();
}

/** Fill empty base fields from incoming; never wipe user-typed non-empty values. */
export function mergeCompanyFactsPreferFilled(
  base: CompanyFacts,
  incoming: Partial<CompanyFacts>,
): CompanyFacts {
  const pick = (cur: string, next: string | undefined): string => {
    const c = cur.trim();
    if (c.length > 0) return cur;
    return (next ?? "").trim().length > 0 ? (next as string) : cur;
  };

  const nextSource =
    (incoming.source ?? "").trim().length > 0 && base.businessSummary.trim().length === 0
      ? (incoming.source as string)
      : base.source;

  return {
    companyName: pick(base.companyName, incoming.companyName),
    edinetCode: pick(base.edinetCode, incoming.edinetCode),
    docId: pick(base.docId, incoming.docId),
    businessSummary: pick(base.businessSummary, incoming.businessSummary),
    businessRisks: pick(base.businessRisks, incoming.businessRisks),
    performanceSummary: pick(base.performanceSummary, incoming.performanceSummary),
    source: nextSource.trim() || base.source || "injected",
  };
}

export function patchFromEnriched(
  before: CompanyFacts,
  after: CompanyFacts,
): Partial<CompanyFacts> {
  const patch: Partial<CompanyFacts> = {};
  if (after.companyName !== before.companyName) patch.companyName = after.companyName;
  if (after.edinetCode !== before.edinetCode) patch.edinetCode = after.edinetCode;
  if (after.docId !== before.docId) patch.docId = after.docId;
  if (after.businessSummary !== before.businessSummary) {
    patch.businessSummary = after.businessSummary;
  }
  if (after.businessRisks !== before.businessRisks) {
    patch.businessRisks = after.businessRisks;
  }
  if (after.performanceSummary !== before.performanceSummary) {
    patch.performanceSummary = after.performanceSummary;
  }
  if (after.source !== before.source) patch.source = after.source;
  return patch;
}

export function summarizeKnowledgeHits(
  hits: ReadonlyArray<{ text_content: string }>,
): string {
  const parts: string[] = [];
  let total = 0;
  for (const hit of hits) {
    const t = hit.text_content.trim();
    if (!t) continue;
    if (total + t.length > MAX_SUMMARY_CHARS) {
      const room = MAX_SUMMARY_CHARS - total;
      if (room > 80) parts.push(t.slice(0, room));
      break;
    }
    parts.push(t);
    total += t.length + 1;
  }
  return parts.join("\n\n");
}

export interface CompanyFactsEnrichDeps {
  getPolicy: () => Promise<{ enabled: boolean }>;
  knowledgeResearch: (query: string) => Promise<KnowledgeResearchReceipt>;
  searchKnowledge?: (
    query: string,
    limit?: number,
    namespace?: "personal" | "company" | "all",
  ) => Promise<{ hits: Array<{ text_content: string }> }>;
  /** Auto-resolve EDINET code from company name (no manual code). */
  fetchEdinetByName?: (args: {
    companyName: string;
    edinetDate: string;
  }) => Promise<CompanyFacts>;
  todayIso: () => string;
}

export interface CompanyFactsEnrichResult {
  facts: CompanyFacts;
  provenanceLabel: string | null;
  attemptedNet: boolean;
}

/**
 * Best-effort enrichment. Failures never throw to callers that wrap this —
 * individual steps soft-fail so offline inject still works.
 */
export async function enrichCompanyFacts(
  facts: CompanyFacts,
  deps: CompanyFactsEnrichDeps,
): Promise<CompanyFactsEnrichResult> {
  const name = facts.companyName.trim();
  if (!name) {
    return { facts, provenanceLabel: null, attemptedNet: false };
  }

  let next = { ...facts, companyName: name };
  let provenanceLabel: string | null = null;
  let attemptedNet = false;

  // Offline RAG: company namespace only — never pull LINE/daily personal chunks.
  if (next.businessSummary.trim().length === 0 && deps.searchKnowledge) {
    try {
      const local = await deps.searchKnowledge(
        buildCompanyResearchQuery(name),
        3,
        "company",
      );
      const summary = summarizeKnowledgeHits(local.hits);
      if (summary) {
        next = mergeCompanyFactsPreferFilled(next, {
          businessSummary: summary,
          source: "local_rag",
        });
      }
      // hits=0 → leave businessSummary empty (never invent a placeholder).
    } catch (err) {
      // eslint-disable-next-line no-console -- intentional diagnostic
      console.error("[companyFactsEnrich] local_rag lane failed:", err);
    }
  }

  let policyOn = false;
  try {
    policyOn = (await deps.getPolicy()).enabled;
  } catch {
    policyOn = false;
  }

  if (!policyOn) {
    return { facts: next, provenanceLabel: null, attemptedNet: false };
  }

  attemptedNet = true;
  const vaultQuery = buildCompanyResearchQuery(name);
  const wikiQuery = buildWikipediaResearchQuery(name);

  // E0b Wikipedia/research lane (fail-closed without egress-live).
  try {
    const receipt = await deps.knowledgeResearch(wikiQuery);
    const provenance = deriveProvenance(receipt);
    if (provenance) {
      provenanceLabel = provenanceChipText(provenance);
      if (deps.searchKnowledge && next.businessSummary.trim().length === 0) {
        try {
          const again = await deps.searchKnowledge(vaultQuery, 3, "company");
          const summary = summarizeKnowledgeHits(again.hits);
          if (summary) {
            next = mergeCompanyFactsPreferFilled(next, {
              businessSummary: summary,
              source: "wikipedia_research",
            });
          }
        } catch (err) {
          // eslint-disable-next-line no-console -- intentional diagnostic
          console.error("[companyFactsEnrich] wikipedia_research lane failed:", err);
        }
      }
    }
  } catch (err) {
    // eslint-disable-next-line no-console -- intentional diagnostic
    console.error("[companyFactsEnrich] knowledgeResearch lane failed:", err);
  }

  // EDINET: company-name auto lookup (fills edinetCode + facts behind the scenes).
  if (deps.fetchEdinetByName) {
    try {
      const edinet = await deps.fetchEdinetByName({
        companyName: name,
        edinetDate: deps.todayIso(),
      });
      next = mergeCompanyFactsPreferFilled(next, edinet);
      if (!provenanceLabel && edinet.source) {
        provenanceLabel = `🔗 EDINET (${edinet.source})`;
      }
    } catch (err) {
      // eslint-disable-next-line no-console -- intentional diagnostic
      console.error("[companyFactsEnrich] edinet lane failed:", err);
    }
  }

  return { facts: next, provenanceLabel, attemptedNet };
}
