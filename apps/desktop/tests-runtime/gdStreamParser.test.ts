import { parseGdStream } from "../src/lib/gdStreamParser";

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

const L = { validLetters: ["A", "B", "C"] };

test("GD-P01 strict lines split into per-speaker bubbles with correct roles", () => {
  const r = parseGdStream("@A> 分割しよう。\n@B> 数字は？", L);
  assertEq(r.length, 2, "two bubbles");
  assertEq(r[0].role, "PARTICIPANT_A", "A -> PARTICIPANT_A");
  assertEq(r[1].role, "PARTICIPANT_B", "B -> PARTICIPANT_B");
  assertEq(r[0].text, "分割しよう。", "body trimmed");
});

test("GD-P02 unknown letter falls back to previous speaker (no crash)", () => {
  const r = parseGdStream("@A> x\n@Z> y", L); // Z is not in play
  assertEq(r.length, 2, "still two bubbles");
  assertEq(r[1].role, "PARTICIPANT_A", "unknown letter falls back to previous speaker");
});

test("GD-P03 total format collapse yields one fallback bubble, never blank", () => {
  const r = parseGdStream("すみません、フォーマットが分かりません。", L);
  assertEq(r.length, 1, "one bubble");
  assertEq(r[0].role, "PARTICIPANT_A", "fallback role = first valid letter");
  assertOk(r[0].text.length > 0, "raw output is surfaced, not dropped");
});

test("GD-P04 lenient recovery when no strict header is present", () => {
  const r = parseGdStream("[A] 論点整理\nB: 反論", L);
  assertEq(r.length, 2, "bracket/colon forms recovered");
  assertEq(r[0].role, "PARTICIPANT_A", "[A] -> PARTICIPANT_A");
  assertEq(r[1].role, "PARTICIPANT_B", "B: -> PARTICIPANT_B");
});

test("GD-P05 empty input yields no bubbles (never throws)", () => {
  assertEq(parseGdStream("", L).length, 0, "empty buffer -> []");
});

test("GD-P06 bubble ids stay stable as the buffer grows mid-stream", () => {
  const a = parseGdStream("@A> 途中", { ...L, idPrefix: "r0" });
  const b = parseGdStream("@A> 途中まで完成\n@B> 次", { ...L, idPrefix: "r0" });
  assertEq(a[0].turnId, b[0].turnId, "first bubble id stable across frames");
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
