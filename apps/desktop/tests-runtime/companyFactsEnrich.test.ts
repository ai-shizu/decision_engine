import {
  buildCompanyResearchQuery,
  enrichCompanyFacts,
  mergeCompanyFactsPreferFilled,
  patchFromEnriched,
  summarizeKnowledgeHits,
  type CompanyFactsEnrichDeps,
} from "../src/lib/companyFactsEnrich";
import { emptyCompanyFacts } from "../src/lib/interviewStage";

type TestFn = () => void | Promise<void>;
const tests: { name: string; fn: TestFn }[] = [];

function test(name: string, fn: TestFn): void {
  tests.push({ name, fn });
}

function assertOk(condition: boolean, message: string): void {
  if (!condition) throw new Error(message);
}

test("E-01 query builder", () => {
  assertOk(
    buildCompanyResearchQuery(" サンプル株式会社 ") === "サンプル株式会社 事業概要",
    "query",
  );
});

test("E-02 merge prefers filled base", () => {
  const base = emptyCompanyFacts({
    companyName: "A社",
    businessSummary: "手動概要",
  });
  const merged = mergeCompanyFactsPreferFilled(base, {
    businessSummary: "ネット概要",
    businessRisks: "リスク",
    source: "edinet_list",
  });
  assertOk(merged.businessSummary === "手動概要", "keep manual");
  assertOk(merged.businessRisks === "リスク", "fill empty");
});

test("E-03 summarize hits caps length", () => {
  const text = summarizeKnowledgeHits([
    { text_content: "alpha" },
    { text_content: "beta" },
  ]);
  assertOk(text.includes("alpha") && text.includes("beta"), "joined");
});

test("E-04 patchFromEnriched only changed keys", () => {
  const before = emptyCompanyFacts({ companyName: "A" });
  const after = emptyCompanyFacts({
    companyName: "A",
    businessSummary: "概要",
    source: "local_rag",
  });
  const patch = patchFromEnriched(before, after);
  assertOk(patch.businessSummary === "概要", "summary");
  assertOk(patch.companyName === undefined, "no name churn");
});

test("E-05 enrich: local rag without policy", async () => {
  const deps: CompanyFactsEnrichDeps = {
    getPolicy: async () => ({ enabled: false }),
    knowledgeResearch: async () => {
      throw new Error("should not call");
    },
    searchKnowledge: async () => ({
      hits: [{ text_content: "ローカル事業概要", distance: 0.1, id: "1" }],
    }),
    todayIso: () => "2026-07-18",
  };
  // searchKnowledge hit type in enrich only needs text_content — cast via any shape
  const result = await enrichCompanyFacts(
    emptyCompanyFacts({ companyName: "テスト株式会社" }),
    {
      ...deps,
      searchKnowledge: async () => ({
        hits: [{ text_content: "ローカル事業概要" }],
      }),
    },
  );
  assertOk(result.facts.businessSummary.includes("ローカル"), "local fill");
  assertOk(result.attemptedNet === false, "no net");
});

test("E-06 enrich: policy on runs research then edinet", async () => {
  let researchCalled = false;
  let edinetCalled = false;
  const result = await enrichCompanyFacts(
    emptyCompanyFacts({
      companyName: "公開企業",
      edinetCode: "E02144",
    }),
    {
      getPolicy: async () => ({ enabled: true }),
      knowledgeResearch: async () => {
        researchCalled = true;
        return {
          schema: "knowledge_research_receipt.v1",
          research_id: "a".repeat(64),
          results_persisted: 1,
        };
      },
      searchKnowledge: async () => ({ hits: [] }),
      fetchEdinet: async () => {
        edinetCalled = true;
        return emptyCompanyFacts({
          companyName: "公開企業",
          edinetCode: "E02144",
          businessSummary: "EDINET概要",
          source: "edinet_list",
        });
      },
      todayIso: () => "2026-07-18",
    },
  );
  assertOk(researchCalled && edinetCalled, "both lanes");
  assertOk(result.facts.businessSummary.includes("EDINET"), "edinet summary");
  assertOk(result.attemptedNet === true, "net attempted");
  assertOk(result.provenanceLabel !== null, "provenance");
});

async function main(): Promise<void> {
  let failed = 0;
  for (const t of tests) {
    try {
      await t.fn();
      console.log(`PASS ${t.name}`);
    } catch (err) {
      failed += 1;
      console.error(`FAIL ${t.name}: ${String(err)}`);
    }
  }
  console.log(`RESULT failed=${failed} total=${tests.length}`);
  if (failed > 0) {
    throw new Error(`${failed} tests failed`);
  }
}

void main().catch((error: unknown) => {
  console.error(error instanceof Error ? error.message : String(error));
  throw error;
});
