import {
  shouldApplyVaultSnapshot,
  vaultPanelTone,
  vaultSystemErrorLine,
} from "../src/lib/vaultPanelView";

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

test("V-P01 hardware HUD tones follow vault semantics", () => {
  assertEq(vaultPanelTone("unlocking", null), "active", "unlocking is cyan");
  assertEq(vaultPanelTone("locking", null), "active", "locking is cyan");
  assertEq(vaultPanelTone("locked", null), "locked", "locked is red");
  assertEq(vaultPanelTone("unprovisioned", null), "locked", "sealed is red");
  assertEq(vaultPanelTone("unlocked", null), "unlocked", "ready is green");
  assertEq(
    vaultPanelTone("unlocked", "storage_failed"),
    "error",
    "error overrides ready",
  );
});

test("V-P02 errors use the sterile terminal envelope", () => {
  const line = vaultSystemErrorLine("authentication_failed");
  assertOk(
    line.startsWith("> SYS_ERR :: [AUTHENTICATION_FAILED] "),
    "terminal prefix",
  );
  assertOk(line.includes("認証を確認できませんでした"), "sterile user guidance");
});

test("V-P03 IPC snapshot cannot rewind a delivered event", () => {
  assertOk(
    shouldApplyVaultSnapshot(7, 7),
    "snapshot is fallback before the first event",
  );
  assertOk(
    !shouldApplyVaultSnapshot(7, 8),
    "observed lifecycle event wins over snapshot",
  );
});

let failed = 0;
for (const { name, fn } of tests) {
  try {
    fn();
    console.log(`PASS ${name}`);
  } catch (error: unknown) {
    failed += 1;
    console.error(`FAIL ${name}`, error);
  }
}
console.log(`RESULT failed=${failed} total=${tests.length}`);
if (failed > 0) {
  throw new Error(`${failed} tests failed`);
}
