import {
  CIRCUIT_WARN_PCT,
  circuitStatusFromDistress,
  clampDistress,
  formatCircuitTelemetry,
} from "../src/lib/circuitBreakerLogic";

type TestFn = () => void;
const tests: { name: string; fn: TestFn }[] = [];

function test(name: string, fn: TestFn): void {
  tests.push({ name, fn });
}

function assertOk(condition: boolean, message: string): void {
  if (!condition) throw new Error(message);
}

test("CB-01 nominal below warn", () => {
  assertOk(circuitStatusFromDistress(0) === "NOMINAL", "0");
  assertOk(circuitStatusFromDistress(CIRCUIT_WARN_PCT - 1) === "NOMINAL", "77");
});

test("CB-02 warning at 78 inclusive", () => {
  assertOk(circuitStatusFromDistress(78) === "WARNING", "78");
  assertOk(circuitStatusFromDistress(99) === "WARNING", "99");
});

test("CB-03 tripped at 100", () => {
  assertOk(circuitStatusFromDistress(100) === "TRIPPED", "100");
  assertOk(clampDistress(140) === 100, "clamp");
});

test("CB-04 telemetry format", () => {
  const s = formatCircuitTelemetry({
    lenDeltaPct: -4,
    withdrawal: 0,
    silenceSec: 0.9,
  });
  assertOk(s.includes("len Δ -4%"), "len");
  assertOk(s.includes("withdrawal 0"), "wd");
  assertOk(s.includes("silence 0.9s"), "silence");
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
