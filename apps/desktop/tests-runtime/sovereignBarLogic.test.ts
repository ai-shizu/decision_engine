import {
  isSovereignArmed,
  resolveSovereignTap,
  SOVEREIGN_ARM_IDLE,
  SOVEREIGN_ARM_WINDOW_MS,
} from "../src/lib/sovereignBarLogic";

type TestFn = () => void;
const tests: { name: string; fn: TestFn }[] = [];

function test(name: string, fn: TestFn): void {
  tests.push({ name, fn });
}

function assertOk(condition: boolean, message: string): void {
  if (!condition) throw new Error(message);
}

test("SB-01 first tap arms without confirm", () => {
  const r = resolveSovereignTap(SOVEREIGN_ARM_IDLE, 1_000);
  assertOk(r.confirmed === false, "not confirmed");
  assertOk(r.next.armed === true, "armed");
  assertOk(r.next.armedUntilMs === 1_000 + SOVEREIGN_ARM_WINDOW_MS, "window");
});

test("SB-02 second tap within window confirms and clears", () => {
  const armed = resolveSovereignTap(SOVEREIGN_ARM_IDLE, 1_000).next;
  const r = resolveSovereignTap(armed, 1_000 + 500);
  assertOk(r.confirmed === true, "confirmed");
  assertOk(r.next.armed === false, "cleared");
});

test("SB-03 expired arm does not confirm; re-arms", () => {
  const armed = resolveSovereignTap(SOVEREIGN_ARM_IDLE, 1_000).next;
  const late = 1_000 + SOVEREIGN_ARM_WINDOW_MS + 1;
  assertOk(isSovereignArmed(armed, late) === false, "expired");
  const r = resolveSovereignTap(armed, late);
  assertOk(r.confirmed === false, "no confirm on expiry");
  assertOk(r.next.armed === true, "re-armed");
  assertOk(r.next.armedUntilMs === late + SOVEREIGN_ARM_WINDOW_MS, "new window");
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
