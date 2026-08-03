/**
 * Phase 10 — metacognitive haptics via native Taptic Engine IPC.
 * Never uses `navigator.vibrate`. Non-iOS / missing command → soft no-op.
 */

import { invoke } from "@tauri-apps/api/core";

export type HapticKind =
  | "selection"
  | "impact"
  | "impact_heavy"
  | "warning";

/** Fire a haptic; failures are swallowed (ambient UX, never blocks input). */
export async function triggerHaptic(kind: HapticKind): Promise<void> {
  try {
    await invoke("haptic_feedback", { kind });
  } catch {
    /* desktop / stub / capability gap — no vibrate fallback */
  }
}

/** Rasch Likert / selection changed. */
export function hapticSelectionChanged(): void {
  void triggerHaptic("selection");
}

/** CBT cognitive distortion detected. */
export function hapticBiasDetected(): void {
  void triggerHaptic("impact");
}

/** Digital Twin p_lapse / critical-day danger zone. */
export function hapticTwinWarning(): void {
  void triggerHaptic("warning");
  void triggerHaptic("impact_heavy");
}
