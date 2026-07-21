/**
 * Phase 10 — mount-once hook: visibilitychange → ordered restore coordinator.
 * Does not disable inputs; twin warning is ambient haptic only.
 */

import { useEffect, useRef } from "react";

import { todayIso } from "./dateUtils";
import { warmConsultRuntime } from "./engine";
import {
  dispatchForegroundRestore,
  maxPLapseFromArray,
  runForegroundRestore,
  type AnalyticsFreshness,
} from "./foregroundRestore";
import { hapticTwinWarning } from "./haptics";
import { llmIsLoaded, loadModel } from "./llm";
import {
  evaluateDigitalTwinScenario,
  getCognitiveBiasProfile,
  getTwinIdentifyStatus,
} from "./pocketBrain";
import { vaultStatus } from "./vault";

async function refreshAnalyticsFreshness(): Promise<AnalyticsFreshness> {
  const [twinIdentify, biasProfile, twinEval] = await Promise.all([
    getTwinIdentifyStatus().catch(() => null),
    getCognitiveBiasProfile().catch(() => null),
    evaluateDigitalTwinScenario({ today: todayIso(), horizonDays: 7 }).catch(
      () => null,
    ),
  ]);
  const maxPLapse = twinEval
    ? maxPLapseFromArray(twinEval.twin.forecast.p_lapse)
    : null;
  const criticalDayCount = twinEval
    ? twinEval.twin.forecast.critical_days.length
    : 0;
  return {
    twinIdentify,
    biasProfile,
    maxPLapse,
    criticalDayCount,
  };
}

/**
 * Subscribe to document visibility; on return-to-foreground run the ordered
 * restore (vault → LLM → analytics) and notify panels via CustomEvent.
 */
export function useForegroundRestore(enabled: boolean): void {
  const runningRef = useRef(false);
  const warnedRef = useRef(false);

  useEffect(() => {
    if (!enabled) return;

    async function restore(): Promise<void> {
      if (runningRef.current) return;
      if (typeof document !== "undefined" && document.visibilityState !== "visible") {
        return;
      }
      runningRef.current = true;
      try {
        const report = await runForegroundRestore({
          probeVault: vaultStatus,
          probeLlmLoaded: () =>
            llmIsLoaded().catch(() => false),
          warmLlm: () =>
            loadModel({ n_gpu_layers: 999, use_mmap: true }),
          warmConsultRuntime: () =>
            warmConsultRuntime(true).then(() => undefined),
          refreshAnalytics: refreshAnalyticsFreshness,
        });
        dispatchForegroundRestore(report);
        if (report.twinWarning && !warnedRef.current) {
          warnedRef.current = true;
          hapticTwinWarning();
        }
        if (!report.twinWarning) {
          warnedRef.current = false;
        }
      } finally {
        runningRef.current = false;
      }
    }

    function onVisibility(): void {
      if (document.visibilityState === "visible") {
        void restore();
      }
    }

    document.addEventListener("visibilitychange", onVisibility);
    return () => {
      document.removeEventListener("visibilitychange", onVisibility);
    };
  }, [enabled]);
}
