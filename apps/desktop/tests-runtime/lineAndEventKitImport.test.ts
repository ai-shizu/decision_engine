/**
 * Boundary tests: LINE import sterile feedback + EventKit day grouping.
 */
import {
  extractLineImportCode,
  sterileLineImportFromUnknown,
  sterileLineImportMessage,
} from "../src/lib/lineImportFeedback";
import {
  eventKitImportRange,
  groupEventsForDailySync,
  sterileEventKitAuthMessage,
  unixToLocalIsoDate,
} from "../src/lib/eventKitImport";
import type { CalendarEventOut } from "../src/lib/pocketBrain/types";

type TestFn = () => void;
const tests: { name: string; fn: TestFn }[] = [];

function test(name: string, fn: TestFn): void {
  tests.push({ name, fn });
}

function assertOk(cond: boolean, msg: string): void {
  if (!cond) throw new Error(msg);
}

function assertEqual(actual: unknown, expected: unknown, msg: string): void {
  if (actual !== expected) {
    throw new Error(`${msg}: expected ${JSON.stringify(expected)} got ${JSON.stringify(actual)}`);
  }
}

test("line: extracts controlled code from invoke wrapper message", () => {
  assertEqual(
    extractLineImportCode("[ingest_line_history] LINE_IMPORT:TOO_LARGE"),
    "TOO_LARGE",
    "code",
  );
  assertEqual(extractLineImportCode("random boom"), null, "no code");
});

test("line: sterile messages never echo raw exception text", () => {
  const msg = sterileLineImportFromUnknown(
    new Error("[ingest_line_history] LINE_IMPORT:TOO_LARGE"),
  );
  assertOk(msg.includes("4MB") || msg.includes("大きすぎ"), "too large copy");
  assertOk(!msg.includes("ingest_line_history"), "no cmd leak");
  assertOk(!msg.includes("/var/"), "no path leak");
});

test("line: unknown code falls back to generic sterile copy", () => {
  assertOk(
    sterileLineImportMessage("TOTALLY_FAKE").includes("確認できませんでした"),
    "generic",
  );
});

test("eventkit: import range spans ~3 years", () => {
  const now = Date.UTC(2026, 6, 23, 12, 0, 0);
  const { startUnix, endUnix } = eventKitImportRange(now);
  assertOk(endUnix > startUnix, "ordered");
  assertOk(now / 1000 - startUnix >= 364 * 86400, "past ~365d");
  assertOk(endUnix - now / 1000 >= 729 * 86400, "future ~730d");
});

test("eventkit: groups by local day and drops empty titles", () => {
  const events: CalendarEventOut[] = [
    {
      id: "1",
      title: "面接",
      start: Date.UTC(2026, 6, 23, 1, 0, 0) / 1000, // 10:00 JST if local=JST
      end: Date.UTC(2026, 6, 23, 2, 0, 0) / 1000,
      all_day: false,
      calendar_title: "Work",
      notes: null,
    },
    {
      id: "2",
      title: "   ",
      start: Date.UTC(2026, 6, 23, 3, 0, 0) / 1000,
      end: Date.UTC(2026, 6, 23, 4, 0, 0) / 1000,
      all_day: false,
      calendar_title: null,
      notes: null,
    },
    {
      id: "3",
      title: "終日イベント",
      start: Date.UTC(2026, 6, 24, 0, 0, 0) / 1000,
      end: Date.UTC(2026, 6, 25, 0, 0, 0) / 1000,
      all_day: true,
      calendar_title: null,
      notes: null,
    },
  ];
  const buckets = groupEventsForDailySync(events);
  assertEqual(buckets.length, 2, "two days");
  assertOk(buckets[0]!.events.length === 1, "empty title dropped");
  assertEqual(buckets[1]!.events[0]!.time, "終日", "all-day label");
});

test("eventkit: auth messages are sterile and actionable", () => {
  const denied = sterileEventKitAuthMessage("denied");
  assertOk(denied.includes("カレンダー"), "mentions calendar");
  assertOk(denied.includes("再実行"), "verify-first tone");
  assertOk(!denied.includes("EKAuthorization"), "no raw enum");
});

test("eventkit: unixToLocalIsoDate stable shape", () => {
  const iso = unixToLocalIsoDate(1_753_228_800); // fixed unix
  assertOk(/^\d{4}-\d{2}-\d{2}$/.test(iso), "iso shape");
});

let failed = 0;
for (const t of tests) {
  try {
    t.fn();
    console.log(`PASS ${t.name}`);
  } catch (e) {
    failed += 1;
    const msg = e instanceof Error ? e.message : String(e);
    console.log(`FAIL ${t.name}: ${msg}`);
  }
}
console.log(`RESULT failed=${failed} total=${tests.length}`);
if (failed > 0) {
  throw new Error(`${failed} tests failed`);
}
