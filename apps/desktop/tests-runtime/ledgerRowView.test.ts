import {
  formatLedgerYen,
  ledgerRiskLabel,
  ledgerRiskLevel,
  ledgerRowClassName,
  ledgerTypeCode,
} from "../src/lib/ledgerRowView";

type TestFn = () => void;
const tests: { name: string; fn: TestFn }[] = [];

function test(name: string, fn: TestFn): void {
  tests.push({ name, fn });
}

function assertOk(condition: boolean, message: string): void {
  if (!condition) throw new Error(message);
}

test("LD-01 yen formatting tabular", () => {
  assertOk(formatLedgerYen(1200) === "1,200", "1200");
  assertOk(formatLedgerYen(0) === "0", "0");
  assertOk(formatLedgerYen(Number.NaN) === "—", "nan");
});

test("LD-02 type codes", () => {
  assertOk(ledgerTypeCode("expense") === "EXP", "exp");
  assertOk(ledgerTypeCode("income") === "INC", "inc");
});

test("LD-03 risk none for ordinary category", () => {
  assertOk(ledgerRiskLevel("食費") === "none", "food");
  assertOk(ledgerRiskLabel("none") === null, "label");
  assertOk(ledgerRowClassName("none") === "ledger-row", "class");
});

test("LD-04 warn for unplanned markers", () => {
  assertOk(ledgerRiskLevel("非計画・コンビニ") === "warn", "jp");
  assertOk(ledgerRiskLevel("impulse buy") === "warn", "en");
  assertOk(ledgerRiskLabel("warn") === "UNPLANNED", "label");
  assertOk(ledgerRowClassName("warn").includes("ledger-row--warn"), "class");
});

test("LD-05 danger for distortion markers wins over warn", () => {
  assertOk(ledgerRiskLevel("破局視・非計画") === "danger", "both");
  assertOk(ledgerRiskLabel("danger") === "DISTORTION", "label");
  assertOk(ledgerRowClassName("danger").includes("ledger-row--danger"), "class");
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
