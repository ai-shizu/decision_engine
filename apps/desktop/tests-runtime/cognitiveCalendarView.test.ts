import {
  buildCognitiveMonthGrid,
  cognitiveDayAriaLabel,
  dayHasRecord,
  expenseBarPct,
  rHeatCss,
  rLevelJa,
} from "../src/lib/cognitiveCalendarView";
import type { CognitiveDayView } from "../src/lib/pocketBrain/types";

type TestFn = () => void;
const tests: { name: string; fn: TestFn }[] = [];

function test(name: string, fn: TestFn): void {
  tests.push({ name, fn });
}

function assertOk(condition: boolean, message: string): void {
  if (!condition) throw new Error(message);
}

function assertEq<T>(actual: T, expected: T, message: string): void {
  if (actual !== expected) {
    throw new Error(`${message}: expected ${JSON.stringify(expected)}, got ${JSON.stringify(actual)}`);
  }
}

test("rLevelJa thresholds", () => {
  assertEq(rLevelJa(null), null, "null");
  assertEq(rLevelJa(0.1), "低", "low");
  assertEq(rLevelJa(0.5), "中", "mid");
  assertEq(rLevelJa(0.9), "高", "high");
});

test("rHeatCss warm-to-cool hue", () => {
  const low = rHeatCss(0.05)!;
  const high = rHeatCss(0.95)!;
  assertOk(low.startsWith("hsl("), "low hsl");
  assertOk(high.startsWith("hsl("), "high hsl");
  const lowHue = Number(low.slice(4).split(" ")[0]);
  const highHue = Number(high.slice(4).split(" ")[0]);
  assertOk(lowHue < 40, "low is warm");
  assertOk(highHue > 150, "high is cool");
  assertEq(rHeatCss(null), null, "null heat");
});

test("cognitiveDayAriaLabel covers record and empty", () => {
  const recorded: CognitiveDayView = {
    date: "2026-07-14",
    r_value: 0.2,
    total_expense: 4200,
    distortions: ["magnification_minimization"],
  };
  const label = cognitiveDayAriaLabel(recorded, 7);
  assertOk(label.includes("7月14日"), "date");
  assertOk(label.includes("認知資源 低"), "r");
  assertOk(label.includes("4,200円") || label.includes("4200円"), "expense");
  assertOk(label.includes("の傾向あり"), "bias");

  const empty: CognitiveDayView = {
    date: "2026-07-15",
    r_value: null,
    total_expense: 0,
    distortions: [],
  };
  assertEq(cognitiveDayAriaLabel(empty, 7), "7月15日、記録なし", "empty");
  assertOk(!dayHasRecord(empty), "empty has no record");
  assertOk(dayHasRecord(recorded), "recorded");
});

test("buildCognitiveMonthGrid monday-first july 2026", () => {
  const days: CognitiveDayView[] = [
    {
      date: "2026-07-14",
      r_value: 0.3,
      total_expense: 100,
      distortions: ["labeling"],
    },
  ];
  const cells = buildCognitiveMonthGrid(2026, 7, days);
  assertOk(cells.length % 7 === 0, "full weeks");
  // 2026-07-01 is Wednesday → lead pads = 2 (Mon,Tue)
  const pads = cells.filter((c) => c.kind === "pad").length;
  assertOk(pads >= 2, "has pads");
  const day14 = cells.find((c) => c.kind === "day" && c.date === "2026-07-14");
  assertOk(!!day14 && day14.kind === "day", "day14 present");
  if (day14 && day14.kind === "day") {
    assertEq(day14.day.total_expense, 100, "merged expense");
  }
});

test("expenseBarPct floors visual min", () => {
  assertEq(expenseBarPct(0, 1000), 0, "zero");
  assertEq(expenseBarPct(10, 1000), 8, "min visual");
  assertEq(expenseBarPct(1000, 1000), 100, "full");
});

let failed = 0;
for (const t of tests) {
  try {
    t.fn();
    console.log(`PASS ${t.name}`);
  } catch (e) {
    failed += 1;
    console.error(`FAIL ${t.name}:`, e);
  }
}
console.log(`RESULT failed=${failed} total=${tests.length}`);
if (failed > 0) {
  throw new Error(`${failed} tests failed`);
}
