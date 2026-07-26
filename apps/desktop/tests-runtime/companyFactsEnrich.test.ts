import {
  buildCompanyResearchQuery,
  buildWikipediaResearchQuery,
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

test("E-03 summarize hits preserves complete text", () => {
  const longSentence = `${"あ".repeat(900)}。`;
  const text = summarizeKnowledgeHits([
    { text_content: longSentence },
    { text_content: "末尾の段落です。" },
  ]);
  assertOk(text.startsWith(longSentence), "first hit is not hard-truncated");
  assertOk(text.endsWith("末尾の段落です。"), "last hit is preserved");
});

test("E-04 patchFromEnriched only changed keys", () => {
  const before = emptyCompanyFacts({ companyName: "A" });
  const after = emptyCompanyFacts({
    companyName: "A",
    businessSummary: "概要",
    source: "vault_company",
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

test("E-06 enrich: policy on runs research then edinet-by-name", async () => {
  let researchCalled = false;
  let edinetCalled = false;
  const result = await enrichCompanyFacts(
    emptyCompanyFacts({
      companyName: "公開企業",
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
      fetchEdinetByName: async (args) => {
        edinetCalled = true;
        assertOk(args.companyName === "公開企業", "name passed");
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
  assertOk(result.facts.edinetCode === "E02144", "auto code");
  assertOk(result.attemptedNet === true, "net attempted");
  assertOk(result.provenanceLabel !== null, "provenance");
});

test("E-07 enrich: searchKnowledge called with company namespace", async () => {
  let seenNs: string | undefined;
  await enrichCompanyFacts(emptyCompanyFacts({ companyName: "Acme" }), {
    getPolicy: async () => ({ enabled: false }),
    knowledgeResearch: async () => {
      throw new Error("should not call");
    },
    searchKnowledge: async (_q, _limit, namespace) => {
      seenNs = namespace;
      return { hits: [{ text_content: "企業概要" }] };
    },
    todayIso: () => "2026-07-25",
  });
  assertOk(seenNs === "company", `namespace=${seenNs}`);
});

test("E-08 enrich: company lane empty leaves businessSummary blank", async () => {
  const result = await enrichCompanyFacts(
    emptyCompanyFacts({ companyName: "マッキンゼー" }),
    {
      getPolicy: async () => ({ enabled: false }),
      knowledgeResearch: async () => {
        throw new Error("should not call");
      },
      searchKnowledge: async () => ({ hits: [] }),
      todayIso: () => "2026-07-25",
    },
  );
  assertOk(result.facts.businessSummary === "", "summary stays empty");
  assertOk(
    result.facts.source !== "vault_company",
    "no vault_company source without hits",
  );
});

test("E-09 buildWikipediaResearchQuery trims without adding qualifiers", () => {
  const q = buildWikipediaResearchQuery(" サンプル株式会社 ");
  assertOk(q === "サンプル株式会社", "trim only");
  assertOk(!q.includes("事業概要"), "no qualifier");
  assertOk(q.includes("株式会社"), "corporate suffix preserved");
});

test("E-10 wikipedia and vault lanes receive different queries", async () => {
  let researchQuery: string | undefined;
  const seenSearchQueries: string[] = [];
  let edinetName: string | undefined;
  await enrichCompanyFacts(
    emptyCompanyFacts({ companyName: "公開企業" }),
    {
      getPolicy: async () => ({ enabled: true }),
      knowledgeResearch: async (q) => {
        researchQuery = q;
        return {
          schema: "knowledge_research_receipt.v1",
          research_id: "a".repeat(64),
          results_persisted: 1,
        };
      },
      searchKnowledge: async (q) => {
        seenSearchQueries.push(q);
        return { hits: [] };
      },
      fetchEdinetByName: async (args) => {
        edinetName = args.companyName;
        return emptyCompanyFacts({
          companyName: "公開企業",
          edinetCode: "E02144",
          source: "edinet_list",
        });
      },
      todayIso: () => "2026-07-18",
    },
  );
  assertOk(researchQuery !== undefined && !researchQuery.includes("事業概要"), "wiki lane no qualifier");
  assertOk(
    seenSearchQueries.length > 0 && seenSearchQueries.every((q) => q.includes("事業概要")),
    "vault lane keeps qualifier",
  );
  assertOk(edinetName === "公開企業", "edinet name unmodified");
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
