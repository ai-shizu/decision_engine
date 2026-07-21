import {
  buildCognitiveMonthGrid,
  cellTelemetryBand,
  cellTelemetryClassName,
  cognitiveDayAriaLabel,
  dayHasRecord,
  expenseBarPct,
  formatCompactYen,
  formatRTelemetry,
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
    throw new Error(
      `${message}: expected ${JSON.stringify(expected)}, got ${JSON.stringify(actual)}`,
    );
  }
}

test("rLevelJa thresholds", () => {
  assertEq(rLevelJa(null), null, "null");
  assertEq(rLevelJa(0.1), "低", "low");
  assertEq(rLevelJa(0.5), "中", "mid");
  assertEq(rLevelJa(0.9), "高", "high");
});

test("rHeatCss retired — always null", () => {
  assertEq(rHeatCss(0.05), null, "low");
  assertEq(rHeatCss(0.95), null, "high");
  assertEq(rHeatCss(null), null, "null");
});

test("cellTelemetryBand state function", () => {
  const empty: CognitiveDayView = {
    date: "2026-07-01",
    r_value: null,
    total_expense: 0,
    distortions: [],
  };
  assertEq(cellTelemetryBand(empty), "empty", "empty");

  const stable: CognitiveDayView = {
    date: "2026-07-02",
    r_value: 0.8,
    total_expense: 100,
    distortions: [],
  };
  assertEq(cellTelemetryBand(stable), "stable", "stable");

  const warnBias: CognitiveDayView = {
    date: "2026-07-03",
    r_value: 0.8,
    total_expense: 0,
    distortions: ["labeling"],
  };
  assertEq(cellTelemetryBand(warnBias), "warn", "one distortion");

  const warnR: CognitiveDayView = {
    date: "2026-07-04",
    r_value: 0.4,
    total_expense: 50,
    distortions: [],
  };
  assertEq(cellTelemetryBand(warnR), "warn", "strained R");

  const dangerR: CognitiveDayView = {
    date: "2026-07-05",
    r_value: 0.2,
    total_expense: 0,
    distortions: [],
  };
  assertEq(cellTelemetryBand(dangerR), "danger", "depleted R");

  const dangerBias: CognitiveDayView = {
    date: "2026-07-06",
    r_value: 0.9,
    total_expense: 0,
    distortions: ["labeling", "catastrophizing"],
  };
  assertEq(cellTelemetryBand(dangerBias), "danger", "density");

  const cls = cellTelemetryClassName(dangerR, { isToday: true });
  assertOk(cls.includes("cognitive-cal-cell--danger"), "class band");
  assertOk(cls.includes("is-today"), "today");
});

test("formatRTelemetry / formatCompactYen", () => {
  assertEq(formatRTelemetry(0.2), "0.20", "r");
  assertEq(formatRTelemetry(null), null, "null r");
  assertEq(formatCompactYen(4200), "4,200", "yen");
  assertOk(formatCompactYen(12_000).includes("万"), "man");
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
