/**
 * Phase 10 — foreground restore sequencing + twin warning threshold (pure).
 */

import {
  isVaultUnlockedStatus,
  maxPLapseFromArray,
  runForegroundRestore,
  twinNeedsWarning,
  type AnalyticsFreshness,
  type ForegroundRestoreDeps,
} from "../src/lib/foregroundRestore";
import type { VaultStatus } from "../src/lib/parseVault";

type TestFn = () => void | Promise<void>;
const tests: { name: string; fn: TestFn }[] = [];

function test(name: string, fn: TestFn): void {
  tests.push({ name, fn });
}

function assertOk(condition: boolean, message: string): void {
  if (!condition) throw new Error(message);
}

test("P10-01 vault unlocked status helper", () => {
  assertOk(isVaultUnlockedStatus("unlocked"), "unlocked");
  assertOk(!isVaultUnlockedStatus("locked"), "locked");
  assertOk(!isVaultUnlockedStatus("unavailable"), "unavailable");
});

test("P10-02 twin warning from critical days or p_lapse", () => {
  assertOk(twinNeedsWarning(null, 1), "critical days");
  assertOk(twinNeedsWarning(0.51, 0), "high p_lapse");
  assertOk(!twinNeedsWarning(0.49, 0), "below threshold");
  assertOk(!twinNeedsWarning(null, 0), "empty");
});

test("P10-03 maxPLapseFromArray", () => {
  assertOk(maxPLapseFromArray([0.1, 0.4, 0.2]) === 0.4, "max");
  assertOk(maxPLapseFromArray([]) === null, "empty");
  assertOk(maxPLapseFromArray([null, 0.3]) === 0.3, "skip null");
});

test("P10-04 restore order vault → llm → analytics", async () => {
  const order: string[] = [];
  const freshness: AnalyticsFreshness = {
    twinIdentify: null,
    biasProfile: null,
    maxPLapse: 0.2,
    criticalDayCount: 0,
  };
  const deps: ForegroundRestoreDeps = {
    probeVault: async () => {
      order.push("vault");
      return "unlocked" as VaultStatus;
    },
    probeLlmLoaded: async () => {
      order.push("llm_probe");
      return false;
    },
    warmLlm: async () => {
      order.push("llm_warm");
    },
    refreshAnalytics: async () => {
      order.push("analytics");
      return freshness;
    },
  };
  const report = await runForegroundRestore(deps);
  assertOk(
    order.join(",") === "vault,llm_probe,llm_warm,llm_probe,analytics",
    `order=${order.join(",")}`,
  );
  assertOk(report.vaultUnlocked, "unlocked");
  assertOk(report.llmWarmed, "warmed");
  assertOk(report.analyticsOk, "analytics");
  assertOk(!report.twinWarning, "no twin warn");
});

test("P10-05 restore continues after vault failure", async () => {
  const order: string[] = [];
  const deps: ForegroundRestoreDeps = {
    probeVault: async () => {
      order.push("vault");
      throw new Error("vault down");
    },
    probeLlmLoaded: async () => {
      order.push("llm");
      return true;
    },
    warmLlm: async () => {
      order.push("warm");
    },
    refreshAnalytics: async () => {
      order.push("analytics");
      return {
        twinIdentify: null,
        biasProfile: null,
        maxPLapse: 0.9,
        criticalDayCount: 2,
      };
    },
  };
  const report = await runForegroundRestore(deps);
  assertOk(order[0] === "vault", "vault first");
  assertOk(order.includes("llm"), "llm still ran");
  assertOk(order.includes("analytics"), "analytics still ran");
  assertOk(report.errors.some((e) => e.startsWith("vault:")), "vault error");
  assertOk(report.twinWarning, "twin warning from analytics");
});

void (async () => {
  let failed = 0;
  for (const { name, fn } of tests) {
    try {
      await fn();
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
})();
