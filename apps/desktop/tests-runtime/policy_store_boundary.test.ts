import { parseKnowledgePolicy } from "../src/lib/parseKnowledgePolicy";
import {
  applyPolicyEnabled,
  policySetRequest,
} from "../src/lib/policyStore";

const DEFAULT_KNOWLEDGE_POLICY = {
  schema: "knowledge_policy.v1" as const,
  enabled: false,
};

type TestFn = () => void;
const tests: { name: string; fn: TestFn }[] = [];

function test(name: string, fn: TestFn): void {
  tests.push({ name, fn });
}

function assertOk(condition: boolean, message: string): void {
  if (!condition) throw new Error(message);
}

function expectReject(fn: () => unknown): void {
  let rejected = false;
  try {
    fn();
  } catch {
    rejected = true;
  }
  assertOk(rejected, "expected strict parser rejection");
}

test("P-01 policy parser accepts enabled true/false", () => {
  const on = parseKnowledgePolicy({ schema: "knowledge_policy.v1", enabled: true });
  assertOk(on.enabled === true, "enabled true");
  const off = parseKnowledgePolicy({ schema: "knowledge_policy.v1", enabled: false });
  assertOk(off.enabled === false, "enabled false");
});

test("P-02 policy parser rejects unknown keys and non-boolean", () => {
  expectReject(() => parseKnowledgePolicy({ schema: "knowledge_policy.v1", enabled: true, query: "x" }));
  expectReject(() => parseKnowledgePolicy({ schema: "knowledge_policy.v1", enabled: "true" }));
  expectReject(() => parseKnowledgePolicy({ schema: "knowledge_policy.v1" }));
});

test("P-03 policyStore pure helpers default off", () => {
  assertOk(DEFAULT_KNOWLEDGE_POLICY.enabled === false, "default off");
  const next = applyPolicyEnabled(DEFAULT_KNOWLEDGE_POLICY, true);
  assertOk(next.enabled === true && next.schema === "knowledge_policy.v1", "apply on");
  assertOk(policySetRequest(true).enabled === true, "set request");
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
