import {
  deriveProvenance,
  provenanceChipText,
  type Provenance,
} from "../src/lib/researchUiReducer";
import type { KnowledgeResearchReceipt } from "../src/lib/parseEngineResponse";

type TestFn = () => void;
const tests: { name: string; fn: TestFn }[] = [];

function test(name: string, fn: TestFn): void {
  tests.push({ name, fn });
}

function assertOk(condition: boolean, message: string): void {
  if (!condition) throw new Error(message);
}

const RECEIPT: KnowledgeResearchReceipt = {
  schema: "knowledge_research_receipt.v1",
  research_id: "a".repeat(64),
  results_persisted: 2,
};

test("V-01 results_persisted > 0 yields fixed provenance", () => {
  const p = deriveProvenance(RECEIPT);
  assertOk(p !== null, "provenance");
  const prov = p as Provenance;
  assertOk(prov.source === "wikipedia" && prov.label === "Wikipediaより参照", "fields");
  assertOk(provenanceChipText(prov) === "🔗 Wikipediaより参照", "chip");
});

test("V-02 zero results yields null", () => {
  const p = deriveProvenance({
    ...RECEIPT,
    results_persisted: 0,
  });
  assertOk(p === null, "null on zero");
});

test("V-03 null receipt yields null", () => {
  assertOk(deriveProvenance(null) === null, "null receipt");
});

test("V-04 provenance exposes no ids urls or raw text", () => {
  const p = deriveProvenance(RECEIPT) as Provenance;
  const keys = Object.keys(p);
  assertOk(keys.length === 2 && keys.includes("source") && keys.includes("label"), "exact keys");
  const serialized = JSON.stringify(p);
  assertOk(!serialized.includes(RECEIPT.research_id), "no research_id leak");
  assertOk(!serialized.includes("http"), "no url");
});

let failed = 0;
for (const { name, fn } of tests) {
  try {
    fn();
    console.log(`PASS ${name}`);
  } catch (err) {
    failed += 1;
    console.error(`FAIL ${name}`, err);
  }
}
console.log(`RESULT failed=${failed} total=${tests.length}`);
if (failed > 0) {
  throw new Error(`${failed} tests failed`);
}
