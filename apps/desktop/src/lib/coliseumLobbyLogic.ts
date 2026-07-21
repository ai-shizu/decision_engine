/**
 * Phase 14 Lobby — Devil Mode ZPD gate (pure; mirrors mentor_zpd intent).
 * Lock is protection, not error. No RNG.
 */

/** Unit-interval floor for Devil eligibility (aligns with ONI_R_T_THRESHOLD=40). */
export const DEVIL_R_MIN = 0.4;

/** Max p_lapse still allowing Devil Mode. */
export const DEVIL_P_LAPSE_MAX = 0.55;

/** R(t) band labels for gauge ticks. */
export const R_BAND_DEPLETED = 0.4;
export const R_BAND_HIGH = 0.7;

export type InterviewMode = "standard" | "devil";

export type DevilGateEval = {
  ready: boolean;
  locked: boolean;
  r: number;
  pLapse: number;
  /** Canonical gate string for ProtectionNotice. */
  gateFormula: string;
};

export function clampUnit(v: number): number {
  if (!Number.isFinite(v)) return 0;
  return Math.min(1, Math.max(0, v));
}

/** GATE: R ≥ 0.40 ∧ P_LAPSE ≤ 0.55 */
export function evaluateDevilGate(rRaw: number, pLapseRaw: number): DevilGateEval {
  const r = clampUnit(rRaw);
  const pLapse = clampUnit(pLapseRaw);
  const ready = r >= DEVIL_R_MIN && pLapse <= DEVIL_P_LAPSE_MAX;
  return {
    ready,
    locked: !ready,
    r,
    pLapse,
    gateFormula: "GATE: R ≥ 0.40 ∧ P_LAPSE ≤ 0.55",
  };
}

/**
 * Resolve mode selection. Devil requests while locked fall back to STANDARD.
 */
export function resolveModeSelection(
  requested: InterviewMode,
  gate: DevilGateEval,
): InterviewMode {
  if (requested === "devil" && gate.locked) return "standard";
  return requested;
}

export function rBandLabel(r: number): "DEPLETED" | "MID" | "HIGH" {
  const v = clampUnit(r);
  if (v < R_BAND_DEPLETED) return "DEPLETED";
  if (v >= R_BAND_HIGH) return "HIGH";
  return "MID";
}

export function formatUnit(v: number, digits = 2): string {
  return clampUnit(v).toFixed(digits);
}
