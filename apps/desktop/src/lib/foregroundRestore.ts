/**
 * Phase 10 — ordered foreground restore after iOS Jetsam / vault auto-lock.
 *
 * Pure orchestration (deps injected) so `tests-runtime` can cover sequencing
 * without React / Tauri. Order is non-negotiable:
 *   1) Vault lock probe
 *   2) LLM / embed presence + optional re-warm
 *   3) Twin + CBT analytics freshness
 */

import type { VaultStatus } from "./parseVault";
import type {
  CognitiveBiasProfile,
  TwinIdentifyStatus,
} from "./pocketBrain/types";

/** CustomEvent name: App → panels after a successful restore pass. */
export const FOREGROUND_RESTORE_EVENT = "coraxis:foreground-restore";

/** p_lapse (or critical-day count) above this → Warning haptic. */
export const P_LAPSE_WARNING_THRESHOLD = 0.5;

export interface ForegroundRestoreDeps {
  probeVault: () => Promise<VaultStatus>;
  /** true when GGUF is resident in the worker. */
  probeLlmLoaded: () => Promise<boolean>;
  /** Re-mmap model when purged; must be best-effort. */
  warmLlm: () => Promise<void>;
  /** Soft sidecar warm (desktop); may no-op on iOS. */
  warmConsultRuntime?: () => Promise<void>;
  refreshAnalytics: () => Promise<AnalyticsFreshness>;
}

export interface AnalyticsFreshness {
  twinIdentify: TwinIdentifyStatus | null;
  biasProfile: CognitiveBiasProfile | null;
  /** Max forecast p_lapse in [0,1], or null if unavailable. */
  maxPLapse: number | null;
  criticalDayCount: number;
}

export interface ForegroundRestoreReport {
  vaultStatus: VaultStatus | null;
  vaultUnlocked: boolean;
  llmWasLoaded: boolean;
  llmWarmed: boolean;
  analyticsOk: boolean;
  twinWarning: boolean;
  freshness: AnalyticsFreshness | null;
  errors: string[];
}

export interface ForegroundRestoreDetail {
  report: ForegroundRestoreReport;
}

export function isVaultUnlockedStatus(status: VaultStatus): boolean {
  return status === "unlocked";
}

export function twinNeedsWarning(
  maxPLapse: number | null,
  criticalDayCount: number,
  threshold: number = P_LAPSE_WARNING_THRESHOLD,
): boolean {
  if (criticalDayCount > 0) return true;
  if (maxPLapse === null || !Number.isFinite(maxPLapse)) return false;
  return maxPLapse >= threshold;
}

/**
 * Run restore steps in fixed order. Never throws — failures accumulate in
 * `errors` and later steps still attempt (best-effort ambient recovery).
 */
export async function runForegroundRestore(
  deps: ForegroundRestoreDeps,
): Promise<ForegroundRestoreReport> {
  const errors: string[] = [];
  let vaultStatus: VaultStatus | null = null;
  let vaultUnlocked = false;
  let llmWasLoaded = false;
  let llmWarmed = false;
  let analyticsOk = false;
  let freshness: AnalyticsFreshness | null = null;
  let twinWarning = false;

  // 1) Vault re-probe
  try {
    vaultStatus = await deps.probeVault();
    vaultUnlocked = isVaultUnlockedStatus(vaultStatus);
  } catch (e) {
    errors.push(`vault:${String(e)}`);
  }

  // 2) LLM presence + re-warm if needed
  try {
    llmWasLoaded = await deps.probeLlmLoaded();
    if (!llmWasLoaded) {
      await deps.warmLlm();
      llmWarmed = true;
      llmWasLoaded = await deps.probeLlmLoaded().catch(() => true);
    }
    if (deps.warmConsultRuntime) {
      await deps.warmConsultRuntime().catch(() => {
        /* sidecar absent on iOS */
      });
    }
  } catch (e) {
    errors.push(`llm:${String(e)}`);
  }

  // 3) Analytics freshness (Twin identify + CBT bias + optional p_lapse)
  try {
    freshness = await deps.refreshAnalytics();
    analyticsOk = true;
    twinWarning = twinNeedsWarning(
      freshness.maxPLapse,
      freshness.criticalDayCount,
    );
  } catch (e) {
    errors.push(`analytics:${String(e)}`);
  }

  return {
    vaultStatus,
    vaultUnlocked,
    llmWasLoaded,
    llmWarmed,
    analyticsOk,
    twinWarning,
    freshness,
    errors,
  };
}

/** Dispatch browser CustomEvent for panel re-sync (Vault / Gap / PocketBrain). */
export function dispatchForegroundRestore(
  report: ForegroundRestoreReport,
): void {
  if (typeof window === "undefined") return;
  const detail: ForegroundRestoreDetail = { report };
  window.dispatchEvent(
    new CustomEvent(FOREGROUND_RESTORE_EVENT, { detail }),
  );
}

export function maxPLapseFromArray(
  values: readonly (number | null | undefined)[] | null | undefined,
): number | null {
  if (!values || values.length === 0) return null;
  let max = -Infinity;
  for (const v of values) {
    if (typeof v === "number" && Number.isFinite(v) && v > max) {
      max = v;
    }
  }
  return max === -Infinity ? null : max;
}
