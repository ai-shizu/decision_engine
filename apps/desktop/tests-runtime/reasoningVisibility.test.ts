/**
 * Pure reasoning visibility (external deps: zero aside from redact twin).
 */
import { redactHiddenReasoning } from "../src/lib/redactHiddenReasoning";
import {
  extractHiddenReasoning,
  interviewReasoningMode,
  visibleBody,
} from "../src/lib/reasoningVisibility";

type TestFn = () => void;
const tests: { name: string; fn: TestFn }[] = [];

function test(name: string, fn: TestFn): void {
  tests.push({ name, fn });
}

function assertOk(condition: boolean, message: string): void {
  if (!condition) throw new Error(message);
}

function assertEqual(actual: unknown, expected: unknown, msg: string): void {
  const a = JSON.stringify(actual);
  const e = JSON.stringify(expected);
  if (a !== e) {
    throw new Error(`${msg}: expected ${e} got ${a}`);
  }
}

test("RV-01 extract single / multi / nested / unclosed / case / absent", () => {
  assertEqual(
    extractHiddenReasoning("hello <think>secret</think> world"),
    ["secret"],
    "single",
  );
  assertEqual(
    extractHiddenReasoning("<think>a</think>mid<think>b</think>"),
    ["a", "b"],
    "multi",
  );
  assertEqual(
    extractHiddenReasoning("<think>outer<think>inner</think>tail</think>"),
    ["outerinnertail"],
    "nested flattens to one block",
  );
  assertEqual(
    extractHiddenReasoning("pre <think>partial"),
    ["partial"],
    "unclosed",
  );
  assertEqual(
    extractHiddenReasoning("x <THINK>UP</THINK> y"),
    ["UP"],
    "case",
  );
  assertEqual(extractHiddenReasoning("no tags here"), [], "absent");
});

test("RV-02 visibleBody hidden matches redactHiddenReasoning exactly", () => {
  const samples = [
    "plain",
    "before <think>hide</think> after",
    "<think>only</think>",
    "a <THINK>X</think> b",
    "open <think>no close",
    "nested <think>a<think>b</think>c</think> end",
    "",
  ];
  for (const raw of samples) {
    for (const streaming of [false, true]) {
      const left = visibleBody(raw, "hidden", streaming);
      const right = redactHiddenReasoning(raw, streaming);
      assertEqual(left, right, `parity streaming=${streaming} raw=${JSON.stringify(raw)}`);
    }
  }
});

test("RV-03 interviewReasoningMode matrix", () => {
  assertEqual(interviewReasoningMode("foundation", null), "hidden", "foundation");
  assertEqual(interviewReasoningMode("pressure", null), "hidden", "pressure");
  assertEqual(interviewReasoningMode("debrief", null), "revealed", "debrief");
  assertEqual(interviewReasoningMode(null, "closed"), "revealed", "closed outcome");
  assertEqual(interviewReasoningMode(null, null), "hidden", "1on1");
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
