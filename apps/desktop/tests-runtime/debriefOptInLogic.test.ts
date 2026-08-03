import {
  applyLayer2Consent,
  applyLayer2Reveal,
  canRevealLayer2,
  formatTurnBadge,
} from "../src/lib/debriefOptInLogic";

type TestFn = () => void;
const tests: { name: string; fn: TestFn }[] = [];

function test(name: string, fn: TestFn): void {
  tests.push({ name, fn });
}

function assertOk(condition: boolean, message: string): void {
  if (!condition) throw new Error(message);
}

test("D2-01 consent arms reveal", () => {
  assertOk(applyLayer2Consent("sealed", true) === "consented", "consent");
  assertOk(canRevealLayer2("consented") === true, "can reveal");
  assertOk(canRevealLayer2("sealed") === false, "sealed blocked");
});

test("D2-02 reveal is one-way", () => {
  assertOk(applyLayer2Reveal("consented") === "revealed", "reveal");
  assertOk(applyLayer2Consent("revealed", false) === "revealed", "no reseal");
  assertOk(applyLayer2Reveal("sealed") === "sealed", "no skip");
});

test("D2-03 turn badge format", () => {
  assertOk(formatTurnBadge("t-6") === "[T-06]", "pad");
  assertOk(formatTurnBadge("t-12") === "[T-12]", "12");
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
