//! Hook: debounce company-name → ambient E0b/local enrichment (no input disable).

import { useEffect, useReducer, useRef, useState } from "react";

import {
  enrichCompanyFacts,
  patchFromEnriched,
  type CompanyFactsEnrichResult,
} from "./companyFactsEnrich";
import { todayIso } from "./dateUtils";
import {
  getKnowledgeResearchPolicy,
  knowledgeResearch,
} from "./engine";
import { fetchEdinetCompanyFacts, searchKnowledge } from "./pocketBrain";
import type { CompanyFacts } from "./pocketBrain/types";
import {
  INITIAL_RESEARCH_UI_STATE,
  isResearching,
  reduceResearchUi,
} from "./researchUiReducer";

const DEBOUNCE_MS = 700;

function liveDeps() {
  return {
    getPolicy: getKnowledgeResearchPolicy,
    knowledgeResearch,
    searchKnowledge: (query: string, limit?: number) =>
      searchKnowledge(query, limit),
    fetchEdinet: (args: { edinetCode: string; edinetDate: string }) =>
      fetchEdinetCompanyFacts(args),
    todayIso,
  };
}

export function useCompanyFactsEnrichment(
  facts: CompanyFacts,
  onPatch: (patch: Partial<CompanyFacts>) => void,
  enabled = true,
): {
  researching: boolean;
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
  factsRef.current = facts;
  const onPatchRef = useRef(onPatch);
  onPatchRef.current = onPatch;

  async function runEnrich(current: CompanyFacts): Promise<CompanyFactsEnrichResult> {
    return enrichCompanyFacts(current, liveDeps());
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
        })
        .catch(() => {
          if (seq !== seqRef.current) return;
          dispatchResearchUi({ kind: "FAIL", seq });
        });
    }, DEBOUNCE_MS);

    return () => {
      window.clearTimeout(timer);
    };
    // Re-run only when identity keys change — not when summary fills (loop guard).
  }, [enabled, facts.companyName, facts.edinetCode]);

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
      if (seq === seqRef.current) {
        dispatchResearchUi({ kind: "FAIL", seq });
      }
      throw err;
    }
  }

  return {
    researching: isResearching(researchUi),
    provenanceLabel,
    enrichNow,
  };
}
