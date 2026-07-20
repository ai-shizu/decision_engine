/**
 * Finding 13 — UI fixed error dictionary boundary (external deps: zero).
 */
import {
  UI_ERROR_SPECS,
  uiErrorMessage,
  uiErrorRetryPolicy,
  type UiErrorCode,
} from "../src/lib/uiErrorMessages";

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

const RETRY_SAFE: UiErrorCode[] = [
  "PROBE_STATUS_LOAD",
  "RECORD_LOAD",
  "PROFILE_LOAD",
  "TWIN_FORECAST",
  "SETTINGS_LOAD",
  "RAG_CHAT",
];

const VERIFY_FIRST: UiErrorCode[] = [
  "PROBE_NEXT",
  "PROBE_ANSWER",
  "RECORD_SAVE",
  "INTERVIEW_RESPONSE",
  "NARRATIVE_COMPILE",
  "CONSULT_RESPONSE",
  "ROMANCE_ANALYSIS",
  "LINE_IMPORT",
  "ICS_SYNC",
  "APPLE_CALENDAR_SYNC",
  "DOCUMENT_IMPORT",
  "KNOWLEDGE_FETCH",
  "KNOWLEDGE_RESEARCH",
  "ORACLE_REPORT",
  "TENSOR_REBUILD",
  "SETTINGS_SAVE",
  "PROFILER_RUN",
];

test("T-01 exact 23 keys", () => {
  const keys = Object.keys(UI_ERROR_SPECS).sort();
  assertEqual(keys.length, 23, "key count");
  const expected = [...RETRY_SAFE, ...VERIFY_FIRST].sort();
  assertEqual(JSON.stringify(keys), JSON.stringify(expected), "key set");
});

test("T-02 all messages non-empty fixed strings", () => {
  for (const code of Object.keys(UI_ERROR_SPECS) as UiErrorCode[]) {
    const msg = UI_ERROR_SPECS[code].message;
    assertOk(typeof msg === "string" && msg.length > 0, `${code} message`);
    assertEqual(uiErrorMessage(code), msg, `${code} helper`);
  }
});

test("T-03 all keys have policy", () => {
  for (const code of Object.keys(UI_ERROR_SPECS) as UiErrorCode[]) {
    const p = UI_ERROR_SPECS[code].retryPolicy;
    assertOk(p === "retry-safe" || p === "verify-first", `${code} policy`);
    assertEqual(uiErrorRetryPolicy(code), p, `${code} helper policy`);
  }
});

test("T-04 retry-safe key set exact", () => {
  const got = (Object.keys(UI_ERROR_SPECS) as UiErrorCode[])
    .filter((k) => UI_ERROR_SPECS[k].retryPolicy === "retry-safe")
    .sort();
  assertEqual(JSON.stringify(got), JSON.stringify([...RETRY_SAFE].sort()), "retry-safe");
});

test("T-05 verify-first key set exact", () => {
  const got = (Object.keys(UI_ERROR_SPECS) as UiErrorCode[])
    .filter((k) => UI_ERROR_SPECS[k].retryPolicy === "verify-first")
    .sort();
  assertEqual(JSON.stringify(got), JSON.stringify([...VERIFY_FIRST].sort()), "verify-first");
});

test("T-06 verify-first messages guide safe re-run", () => {
  for (const code of VERIFY_FIRST) {
    assertOk(
      UI_ERROR_SPECS[code].message.includes("必要な場合だけ再実行"),
      `${code} verify wording`,
    );
  }
});

test("T-07 messages contain no secret-like tokens", () => {
  const banned = ["SECRET", "password", "token", "diary", "LINE本文"];
  for (const code of Object.keys(UI_ERROR_SPECS) as UiErrorCode[]) {
    const msg = UI_ERROR_SPECS[code].message;
    for (const b of banned) {
      assertOk(!msg.includes(b), `${code} has ${b}`);
    }
  }
});

test("T-08 messages have no newlines or control chars", () => {
  for (const code of Object.keys(UI_ERROR_SPECS) as UiErrorCode[]) {
    const msg = UI_ERROR_SPECS[code].message;
    assertOk(!/[\n\r\t\x00-\x1f]/.test(msg), `${code} control`);
  }
});

test("T-09 messages have no path forms", () => {
  for (const code of Object.keys(UI_ERROR_SPECS) as UiErrorCode[]) {
    const msg = UI_ERROR_SPECS[code].message;
    assertOk(!/[A-Za-z]:\\/.test(msg), `${code} win path`);
    assertOk(!/(?:^|\s)\/(?:Users|home|tmp|var)\//.test(msg), `${code} unix path`);
  }
});

test("T-10 messages have no exception class tokens", () => {
  const banned = ["ValueError", "RuntimeError", "Traceback", "Error:"];
  for (const code of Object.keys(UI_ERROR_SPECS) as UiErrorCode[]) {
    const msg = UI_ERROR_SPECS[code].message;
    for (const b of banned) {
      assertOk(!msg.includes(b), `${code} has ${b}`);
    }
  }
});

test("T-11 extra secret args must not leak into message", () => {
  const fn = uiErrorMessage as unknown as (...args: unknown[]) => string;
  const out = fn("RECORD_LOAD", "SECRET_DIARY_xyz", { path: "C:\\secret\\diary.md" });
  assertEqual(out, UI_ERROR_SPECS.RECORD_LOAD.message, "no leak");
  assertOk(!out.includes("SECRET"), "secret absent");
  assertOk(!out.includes("diary.md"), "path absent");
});

test("T-12 unknown codes are not normal keys", () => {
  assertOk(!("UNKNOWN_CODE" in UI_ERROR_SPECS), "unknown not in specs");
  const keys = Object.keys(UI_ERROR_SPECS);
  assertOk(!keys.includes("UNKNOWN_CODE"), "unknown not enumerable");
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
