/**
 * Phase 14 UI — SovereignBar double-tap arm logic (pure; tests-runtime friendly).
 * No RNG. Confirmation window is deterministic ms.
 */

export const SOVEREIGN_ARM_WINDOW_MS = 3000;

export type SovereignArmState = {
  armed: boolean;
  /** Absolute epoch ms when arm expires; 0 when idle. */
  armedUntilMs: number;
};

export const SOVEREIGN_ARM_IDLE: SovereignArmState = {
  armed: false,
  armedUntilMs: 0,
};

export type SovereignTapResult = {
  next: SovereignArmState;
  /** True only on second tap within the arm window → HALT confirmed. */
  confirmed: boolean;
};

/**
 * Resolve a SURRENDER tap. First tap arms; second within window confirms.
 * Expired arm restarts as a fresh first tap (does not confirm).
 */
export function resolveSovereignTap(
  state: SovereignArmState,
  nowMs: number,
  windowMs: number = SOVEREIGN_ARM_WINDOW_MS,
): SovereignTapResult {
  if (state.armed && nowMs <= state.armedUntilMs) {
    return { next: SOVEREIGN_ARM_IDLE, confirmed: true };
  }
  return {
    next: { armed: true, armedUntilMs: nowMs + windowMs },
    confirmed: false,
  };
}

export function isSovereignArmed(state: SovereignArmState, nowMs: number): boolean {
  return state.armed && nowMs <= state.armedUntilMs;
}
