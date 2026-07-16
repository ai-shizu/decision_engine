import {
  INITIAL_RESEARCH_UI_STATE,
  isResearching,
  reduceResearchUi,
} from "../src/lib/researchUiReducer";

type TestFn = () => void;
const tests: { name: string; fn: TestFn }[] = [];

function test(name: string, fn: TestFn): void {
  tests.push({ name, fn });
}

function assertOk(condition: boolean, message: string): void {
  if (!condition) throw new Error(message);
}

test("R-01 START moves to researching with seq", () => {
  const next = reduceResearchUi(INITIAL_RESEARCH_UI_STATE, { kind: "START", seq: 1 });
  assertOk(next.phase === "researching" && next.seq === 1, "researching");
  assertOk(isResearching(next), "isResearching");
});

test("R-02 DONE matching seq returns idle", () => {
  const researching = { phase: "researching" as const, seq: 2 };
  const next = reduceResearchUi(researching, { kind: "DONE", seq: 2 });
  assertOk(next.phase === "idle" && next.seq === 2, "idle");
});

test("R-03 stale DONE is ignored", () => {
  const researching = { phase: "researching" as const, seq: 3 };
  const next = reduceResearchUi(researching, { kind: "DONE", seq: 2 });
  assertOk(next.phase === "researching" && next.seq === 3, "stale ignored");
});

test("R-04 consecutive START advances seq monotonically", () => {
  const s1 = reduceResearchUi(INITIAL_RESEARCH_UI_STATE, { kind: "START", seq: 1 });
  const s2 = reduceResearchUi(s1, { kind: "START", seq: 2 });
  assertOk(s2.seq === 2 && s2.phase === "researching", "seq advanced");
  const stale = reduceResearchUi(s2, { kind: "START", seq: 1 });
  assertOk(stale.seq === 2, "older start ignored");
});

test("R-05 FAIL matching seq returns idle", () => {
  const researching = { phase: "researching" as const, seq: 4 };
  const next = reduceResearchUi(researching, { kind: "FAIL", seq: 4 });
  assertOk(next.phase === "idle", "fail to idle");
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
