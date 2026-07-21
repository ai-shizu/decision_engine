import {
  evaluateDevilGate,
  resolveModeSelection,
  rBandLabel,
} from "../src/lib/coliseumLobbyLogic";

type TestFn = () => void;
const tests: { name: string; fn: TestFn }[] = [];

function test(name: string, fn: TestFn): void {
  tests.push({ name, fn });
}

function assertOk(condition: boolean, message: string): void {
  if (!condition) throw new Error(message);
}

test("CL-01 devil ready when R>=0.40 and P_LAPSE<=0.55", () => {
  const g = evaluateDevilGate(0.4, 0.55);
  assertOk(g.ready === true, "ready");
  assertOk(g.locked === false, "not locked");
});

test("CL-02 depleted R locks devil (protection)", () => {
  const g = evaluateDevilGate(0.31, 0.2);
  assertOk(g.ready === false, "not ready");
  assertOk(g.locked === true, "locked");
  assertOk(g.gateFormula.includes("0.40"), "formula");
});

test("CL-03 high p_lapse locks devil", () => {
  const g = evaluateDevilGate(0.8, 0.56);
  assertOk(g.locked === true, "locked by p_lapse");
});

test("CL-04 mode falls back to standard when locked", () => {
  const g = evaluateDevilGate(0.2, 0.1);
  assertOk(resolveModeSelection("devil", g) === "standard", "fallback");
  assertOk(resolveModeSelection("standard", g) === "standard", "standard ok");
});

test("CL-05 band labels", () => {
  assertOk(rBandLabel(0.31) === "DEPLETED", "depleted");
  assertOk(rBandLabel(0.55) === "MID", "mid");
  assertOk(rBandLabel(0.7) === "HIGH", "high");
});

let failed = 0;
for (const t of tests) {
  try {
    t.fn();
    console.log(`PASS ${t.name}`);
  } catch (e) {
    failed += 1;
    console.error(`FAIL ${t.name}`, e);
  }
}
console.log(`RESULT failed=${failed} total=${tests.length}`);
if (failed > 0) {
  throw new Error(`${failed} tests failed`);
}
