/**
 * Interview sterile fallback mapping (Finding 13).
 */
import { interviewFallbackFor } from "../src/lib/interviewFallback";
import { UI_ERROR_SPECS } from "../src/lib/uiErrorMessages";

type TestFn = () => void;
const tests: { name: string; fn: TestFn }[] = [];

function test(name: string, fn: TestFn): void {
  tests.push({ name, fn });
}

function assertOk(condition: boolean, message: string): void {
  if (!condition) throw new Error(message);
}

function assertEqual(actual: unknown, expected: unknown, msg: string): void {
  if (actual !== expected) {
    throw new Error(
      `${msg}: expected ${JSON.stringify(expected)} got ${JSON.stringify(actual)}`,
    );
  }
}

test("IF-01 five classifications", () => {
  const cold = interviewFallbackFor("MODEL_NOT_LOADED");
  assertEqual(
    cold.message,
    UI_ERROR_SPECS.INTERVIEW_FALLBACK_MODEL_COLD.message,
    "cold msg",
  );
  assertEqual(cold.resendable, true, "cold resend");

  const vault = interviewFallbackFor("error: VAULT_LOCKED while reading");
  assertEqual(
    vault.message,
    UI_ERROR_SPECS.INTERVIEW_FALLBACK_VAULT_LOCKED.message,
    "vault msg",
  );
  assertEqual(vault.resendable, false, "vault no resend");

  const budget = interviewFallbackFor(
    "prompt exceeds context budget: 2394 > 1792",
  );
  assertEqual(
    budget.message,
    UI_ERROR_SPECS.INTERVIEW_FALLBACK_TOO_LONG.message,
    "budget msg",
  );
  assertEqual(budget.resendable, true, "budget resend");

  const cancelled = interviewFallbackFor("Cancelled by user");
  assertEqual(cancelled.message, "発言が取り消されました。", "cancelled msg");
  assertEqual(cancelled.resendable, true, "cancelled resend");

  const other = interviewFallbackFor("stream terminal timeout");
  assertEqual(
    other.message,
    UI_ERROR_SPECS.INTERVIEW_FALLBACK_THINKING.message,
    "default msg",
  );
  assertEqual(other.resendable, true, "default resend");
});

test("IF-02 non-string inputs fall to default without throw", () => {
  for (const raw of [null, undefined, 42, new Error("boom"), { x: 1 }]) {
    const fb = interviewFallbackFor(raw);
    assertEqual(
      fb.message,
      UI_ERROR_SPECS.INTERVIEW_FALLBACK_THINKING.message,
      `default for ${String(raw)}`,
    );
    assertEqual(fb.resendable, true, "resendable");
  }
  const fromErr = interviewFallbackFor(new Error("VAULT_LOCKED"));
  assertEqual(fromErr.resendable, false, "Error with vault code");
});

test("IF-03 only VAULT_LOCKED is non-resendable", () => {
  const samples = [
    "MODEL_NOT_LOADED",
    "prompt exceeds context budget",
    "cancelled",
    "timeout",
    "OOM",
    "",
  ];
  for (const s of samples) {
    assertEqual(interviewFallbackFor(s).resendable, true, s);
  }
  assertEqual(interviewFallbackFor("VAULT_LOCKED").resendable, false, "vault");
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
