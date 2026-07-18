import {
  extractionReducer,
  formatAmountDisplay,
  INITIAL_EXTRACTION_STATE,
  isUnknownField,
  type ExtractionState,
  type KakeiboEntryV1,
} from "../src/lib/extractionReducer";
import { formatEntryForClipboard } from "../src/lib/extractionSink";

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
    throw new Error(`${message}: expected ${String(expected)}, got ${String(actual)}`);
  }
}

const SAMPLE: KakeiboEntryV1 = {
  date: "2026-07-18",
  amount: 298,
  category: "食費",
  payee: "スーパー",
  memo: "牛乳",
};

const UNKNOWNISH: KakeiboEntryV1 = {
  date: "unknown",
  amount: null,
  category: "unknown",
  payee: "unknown",
  memo: "unknown",
};

function extracting(requestId: number, input = "買った"): ExtractionState {
  return extractionReducer(INITIAL_EXTRACTION_STATE, {
    type: "extractionStarted",
    requestId,
    input,
  });
}

test("E-01 idle → extracting", () => {
  const next = extracting(1, "牛乳を買った");
  assertEq(next.phase, "extracting", "phase");
  assertEq(next.requestId, 1, "requestId");
  assertOk(next.result === null, "result null");
  assertOk(next.error === null, "error null");
  assertEq(next.streamedText, "", "stream cleared");
});

test("E-02 extracting → success clears requestId", () => {
  const next = extractionReducer(extracting(2), {
    type: "extractionSucceeded",
    requestId: 2,
    result: SAMPLE,
  });
  assertEq(next.phase, "success", "phase");
  assertOk(next.result !== null && next.result.amount === 298, "result");
  assertOk(next.error === null, "error null");
  assertOk(next.requestId === null, "requestId cleared");
});

test("E-03 extracting → error clears requestId", () => {
  const next = extractionReducer(extracting(3), {
    type: "extractionFailed",
    requestId: 3,
    error: "boom",
  });
  assertEq(next.phase, "error", "phase");
  assertEq(next.error, "boom", "error");
  assertOk(next.result === null, "result null");
  assertOk(next.requestId === null, "requestId cleared");
});

test("E-04 extracting → idle (cancel) clears requestId", () => {
  const next = extractionReducer(extracting(4, "keep me"), {
    type: "extractionCancelled",
    requestId: 4,
  });
  assertEq(next.phase, "idle", "phase");
  assertEq(next.input, "keep me", "input preserved");
  assertOk(next.result === null, "no stale result");
  assertEq(next.streamedText, "", "stream cleared");
  assertOk(next.requestId === null, "requestId cleared");
});

test("E-05 tokens concatenate in order", () => {
  let s = extracting(5);
  s = extractionReducer(s, { type: "tokenReceived", requestId: 5, text: '{"a"' });
  s = extractionReducer(s, { type: "tokenReceived", requestId: 5, text: ":1}" });
  assertEq(s.streamedText, '{"a":1}', "concat");
});

test("E-06 stale requestId token ignored", () => {
  const s = extracting(6);
  const next = extractionReducer(s, {
    type: "tokenReceived",
    requestId: 99,
    text: "STALE",
  });
  assertEq(next.streamedText, "", "stale ignored");
  assertEq(next.phase, "extracting", "still extracting");
});

test("E-07 stale success/error/cancel ignored", () => {
  const s = extracting(7);
  const afterSuccess = extractionReducer(s, {
    type: "extractionSucceeded",
    requestId: 1,
    result: SAMPLE,
  });
  assertEq(afterSuccess.phase, "extracting", "stale success");

  const afterError = extractionReducer(s, {
    type: "extractionFailed",
    requestId: 1,
    error: "nope",
  });
  assertEq(afterError.phase, "extracting", "stale error");

  const afterCancel = extractionReducer(s, {
    type: "extractionCancelled",
    requestId: 1,
  });
  assertEq(afterCancel.phase, "extracting", "stale cancel");
});

test("E-08 validated required for success (type+reducer path)", () => {
  const idle = extractionReducer(INITIAL_EXTRACTION_STATE, {
    type: "extractionSucceeded",
    requestId: 8,
    result: SAMPLE,
  });
  assertEq(idle.phase, "idle", "success on idle ignored");
  assertOk(idle.result === null, "no result without extracting");
});

test("E-09 inputChanged from success discards prior result", () => {
  let s = extractionReducer(extracting(9), {
    type: "extractionSucceeded",
    requestId: 9,
    result: SAMPLE,
  });
  assertEq(s.phase, "success", "precondition");
  s = extractionReducer(s, { type: "inputChanged", input: "新しい入力" });
  assertEq(s.phase, "idle", "idle");
  assertOk(s.result === null, "result cleared");
  assertEq(s.streamedText, "", "stream cleared");
  assertOk(s.error === null, "error cleared");
  assertEq(s.input, "新しい入力", "input");
  assertOk(s.requestId === null, "requestId null");
});

test("E-10 copy success", () => {
  let s = extractionReducer(extracting(10), {
    type: "extractionSucceeded",
    requestId: 10,
    result: SAMPLE,
  });
  s = extractionReducer(s, { type: "copyStarted" });
  assertEq(s.copyStatus, "copying", "copying");
  s = extractionReducer(s, { type: "copySucceeded" });
  assertEq(s.copyStatus, "copied", "copied");
  assertOk(s.copyError === null, "no copy error");
});

test("E-11 copy failure", () => {
  let s = extractionReducer(extracting(11), {
    type: "extractionSucceeded",
    requestId: 11,
    result: SAMPLE,
  });
  s = extractionReducer(s, { type: "copyStarted" });
  s = extractionReducer(s, { type: "copyFailed", error: "denied" });
  assertEq(s.copyStatus, "copy_failed", "status");
  assertEq(s.copyError, "denied", "error");
  assertEq(s.phase, "success", "still success");
});

test("E-12 retry after error", () => {
  let s = extractionReducer(extracting(12), {
    type: "extractionFailed",
    requestId: 12,
    error: "fail once",
  });
  assertEq(s.phase, "error", "error");
  s = extractionReducer(s, {
    type: "extractionStarted",
    requestId: 13,
    input: s.input,
  });
  assertEq(s.phase, "extracting", "retry extracting");
  assertOk(s.error === null, "error cleared");
  assertOk(s.result === null, "result null");
  s = extractionReducer(s, {
    type: "extractionSucceeded",
    requestId: 13,
    result: SAMPLE,
  });
  assertEq(s.phase, "success", "retry success");
});

test("E-13 unknown fields are not invented or mutated", () => {
  const s = extractionReducer(extracting(13), {
    type: "extractionSucceeded",
    requestId: 13,
    result: UNKNOWNISH,
  });
  assertEq(s.phase, "success", "success");
  if (s.phase !== "success") {
    throw new Error("expected success phase");
  }
  assertEq(s.result.date, "unknown", "date");
  assertOk(s.result.amount === null, "amount null");
  assertEq(s.result.category, "unknown", "category");
  assertEq(s.result.payee, "unknown", "payee");
  assertEq(s.result.memo, "unknown", "memo");
  assertEq(formatAmountDisplay(null), "unknown", "display amount");
  assertOk(isUnknownField("unknown"), "isUnknown");
  assertOk(!isUnknownField("食費"), "known field");
  const clip = formatEntryForClipboard(UNKNOWNISH);
  assertOk(clip.includes('"amount": null'), "clipboard keeps null");
  assertOk(clip.includes('"date": "unknown"'), "clipboard keeps unknown string");
});

test("E-14 copy actions noop outside success", () => {
  const idle = extractionReducer(INITIAL_EXTRACTION_STATE, { type: "copyStarted" });
  assertEq(idle.copyStatus, "idle", "idle noop");
  const err = extractionReducer(extracting(14), {
    type: "extractionFailed",
    requestId: 14,
    error: "x",
  });
  const after = extractionReducer(err, { type: "copySucceeded" });
  assertEq(after.copyStatus, "idle", "error noop");
});

test("E-15 reset clears all", () => {
  let s = extractionReducer(extracting(15), {
    type: "extractionSucceeded",
    requestId: 15,
    result: SAMPLE,
  });
  s = extractionReducer(s, { type: "reset" });
  assertEq(s.phase, "idle", "idle");
  assertEq(s.input, "", "input");
  assertOk(s.result === null, "result");
  assertOk(s.requestId === null, "requestId");
});

test("E-16 inputChanged while extracting keeps phase and requestId", () => {
  const s0 = extracting(16, "元の入力");
  const s1 = extractionReducer(s0, {
    type: "tokenReceived",
    requestId: 16,
    text: "partial",
  });
  const next = extractionReducer(s1, {
    type: "inputChanged",
    input: "編集中の入力",
  });
  assertEq(next.phase, "extracting", "still extracting");
  assertEq(next.requestId, 16, "requestId kept");
  assertEq(next.input, "編集中の入力", "input updated");
  assertEq(next.streamedText, "partial", "stream preserved");
});

test("E-17 cancelFailed keeps extracting and sets cancelError", () => {
  const s = extracting(17);
  const next = extractionReducer(s, {
    type: "cancelFailed",
    requestId: 17,
    error: "キャンセルの送信に失敗しました",
  });
  assertEq(next.phase, "extracting", "still extracting");
  assertEq(next.requestId, 17, "requestId kept");
  assertEq(next.cancelError, "キャンセルの送信に失敗しました", "banner");
});

test("E-18 stale cancelFailed ignored", () => {
  const s = extracting(18);
  const next = extractionReducer(s, {
    type: "cancelFailed",
    requestId: 1,
    error: "nope",
  });
  assertEq(next.phase, "extracting", "phase");
  assertOk(next.cancelError === null, "no banner");
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
