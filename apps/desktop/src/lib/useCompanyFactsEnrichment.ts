//! Hook: debounce company-name → ambient E0b/local enrichment (no input disable).

import { useEffect, useReducer, useRef, useState } from "react";

import {
  enrichCompanyFacts,
  patchFromEnriched,
  type EnrichmentSnapshot,
  type CompanyFactsEnrichResult,
} from "./companyFactsEnrich";
import { todayIso } from "./dateUtils";
import {
  getKnowledgeResearchPolicy,
  knowledgeResearch,
} from "./engine";
import { enrichCompanyFactsFromEdinet, ingestCompanyKnowledge, searchKnowledge } from "./pocketBrain";
import type { CompanyFacts, SubjectKey } from "./pocketBrain/types";
import {
  INITIAL_RESEARCH_UI_STATE,
  isResearching,
  reduceResearchUi,
} from "./researchUiReducer";
import filerNameNormalizationGolden from "./filerNameNormalizationGolden.json";

const DEBOUNCE_MS = 700;

export function normalizeSubjectName(value: string): SubjectKey {
  let normalized = value.normalize("NFKC").trim();
  for (const suffix of [
    "株式会社", "有限会社", "合同会社", "合名会社", "合資会社",
    "(株)", "（株）", "(有)", "（有）", "㈱", "㈲",
    "Inc.", "Inc", "Corp.", "Corp", "Ltd.", "Ltd", "LLC",
    "Co., Ltd.", "Co.,Ltd.", "Co. Ltd.",
  ]) {
    normalized = normalized.split(suffix).join("");
  }
  normalized = normalized
    .replace(/[ \u3000・･.\/／\-ー—_]/g, "")
    .toLowerCase();
  return `name:${normalized}`;
}

export function filerNameNormalizationGoldenMatches(): boolean {
  return filerNameNormalizationGolden.every(
    ({ input, normalized }) => normalizeSubjectName(input) === `name:${normalized}`,
  );
}

function liveDeps(
  snapshot: EnrichmentSnapshot | undefined,
  current: CompanyFacts,
  switching: boolean,
) {
  const targetSubject = normalizeSubjectName(current.companyName);
  return {
    getPolicy: getKnowledgeResearchPolicy,
    knowledgeResearch,
    searchKnowledge: (query: string, limit?: number) =>
      searchKnowledge(query, limit, "company"),
    fetchEdinetByName: (args: { companyName: string; edinetDate: string }) =>
      enrichCompanyFactsFromEdinet({
        schemaVersion: 3,
        subjectKey: switching ? targetSubject : (snapshot?.subjectKey ?? targetSubject),
        subjectRevision: snapshot?.subjectRevision ?? 0,
        subjectTransition:
          switching && snapshot
            ? { kind: "switch", from: snapshot.subjectKey, to: targetSubject }
            : null,
        factCells: snapshot?.factCells ?? null,
        companyFacts: current,
        edinetCode: null,
        edinetDate: args.edinetDate,
        filingText: null,
      }),
    todayIso,
  };
}

export function useCompanyFactsEnrichment(
  facts: CompanyFacts,
  onPatch: (patch: Partial<CompanyFacts>) => void,
  enabled = true,
): {
  researching: boolean;
  /** Alias of researching — blocks start/send only (inputs stay enabled). */
  preparing: boolean;
  provenanceLabel: string | null;
  enrichNow: () => Promise<CompanyFactsEnrichResult>;
} {
  const [researchUi, dispatchResearchUi] = useReducer(
    reduceResearchUi,
    INITIAL_RESEARCH_UI_STATE,
  );
  const [provenanceLabel, setProvenanceLabel] = useState<string | null>(null);
  const seqRef = useRef(0);
  const factsRef = useRef(facts);
  const enrichmentSnapshotRef = useRef<EnrichmentSnapshot | undefined>(undefined);
  const enrichmentCompanyRef = useRef<string | undefined>(undefined);
  factsRef.current = facts;
  const onPatchRef = useRef(onPatch);
  onPatchRef.current = onPatch;

  async function runEnrich(current: CompanyFacts): Promise<CompanyFactsEnrichResult> {
    const normalizedCompany = current.companyName.normalize("NFKC").trim();
    const switching =
      enrichmentCompanyRef.current !== undefined &&
      enrichmentCompanyRef.current !== normalizedCompany;
    const result = await enrichCompanyFacts(
      current,
      liveDeps(enrichmentSnapshotRef.current, current, switching),
    );
    if (result.enrichmentSnapshot) {
      enrichmentSnapshotRef.current = result.enrichmentSnapshot;
      enrichmentCompanyRef.current = normalizedCompany;
    }
    return result;
  }

  useEffect(() => {
    if (!enabled) return;
    const name = facts.companyName.trim();
    if (!name) {
      setProvenanceLabel(null);
      return;
    }

    const seq = seqRef.current + 1;
    seqRef.current = seq;
    const timer = window.setTimeout(() => {
      dispatchResearchUi({ kind: "START", seq });
      void runEnrich(factsRef.current)
        .then((result) => {
          if (seq !== seqRef.current) return;
          const patch = patchFromEnriched(factsRef.current, result.facts);
          if (Object.keys(patch).length > 0) {
            onPatchRef.current(patch);
          }
          setProvenanceLabel(result.provenanceLabel);
          dispatchResearchUi({ kind: "DONE", seq });
          // 実企業知識を Company 名前空間へ永続化（fire-and-forget・ソフトフェイル）。
          // EDINET コードは企業の同一性を強める任意キーであり、Wikipedia や手動
          // ファクトの保存条件にはしない。vault_company は既存 Vault 行からの
          // 復元なので再インジェストしない。
          const f = result.facts;
          const restoredFromCompanyVault = f.source.trim() === "vault_company";
          const hasCompanyKnowledge =
            f.businessSummary.trim().length > 0 ||
            f.businessRisks.trim().length > 0 ||
            f.performanceSummary.trim().length > 0;
          if (!restoredFromCompanyVault && hasCompanyKnowledge) {
            void ingestCompanyKnowledge(f).catch((err) => {
              // eslint-disable-next-line no-console -- intentional diagnostic
              console.error(
                "[useCompanyFactsEnrichment] company knowledge persist failed:",
                err,
              );
            });
          }
        })
        .catch((err) => {
          // eslint-disable-next-line no-console -- intentional diagnostic
          console.error("[useCompanyFactsEnrichment] debounce enrich failed:", err);
          if (seq !== seqRef.current) return;
          dispatchResearchUi({ kind: "FAIL", seq });
        });
    }, DEBOUNCE_MS);

    return () => {
      window.clearTimeout(timer);
    };
    // Re-run only when company name changes — EDINET code is auto-resolved.
  }, [enabled, facts.companyName]);

  async function enrichNow(): Promise<CompanyFactsEnrichResult> {
    const seq = seqRef.current + 1;
    seqRef.current = seq;
    dispatchResearchUi({ kind: "START", seq });
    try {
      const result = await runEnrich(factsRef.current);
      if (seq === seqRef.current) {
        const patch = patchFromEnriched(factsRef.current, result.facts);
        if (Object.keys(patch).length > 0) {
          onPatchRef.current(patch);
        }
        setProvenanceLabel(result.provenanceLabel);
        dispatchResearchUi({ kind: "DONE", seq });
      }
      return result;
    } catch (err) {
      // eslint-disable-next-line no-console -- intentional diagnostic
      console.error("[useCompanyFactsEnrichment] enrichNow failed:", err);
      if (seq === seqRef.current) {
        dispatchResearchUi({ kind: "FAIL", seq });
      }
      throw err;
    }
  }

  const researching = isResearching(researchUi);
  return {
    researching,
    preparing: researching,
    provenanceLabel,
    enrichNow,
  };
}
