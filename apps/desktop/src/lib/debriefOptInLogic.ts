/**
 * Phase 14 Debrief — Layer-2 two-step opt-in (pure; one-way reveal).
 */

export type Layer2OptInPhase = "sealed" | "consented" | "revealed";

export function canRevealLayer2(phase: Layer2OptInPhase): boolean {
  return phase === "consented";
}

export function applyLayer2Consent(
  phase: Layer2OptInPhase,
  checked: boolean,
): Layer2OptInPhase {
  if (phase === "revealed") return "revealed";
  return checked ? "consented" : "sealed";
}

/** Reveal is irreversible — once revealed, stays revealed. */
export function applyLayer2Reveal(phase: Layer2OptInPhase): Layer2OptInPhase {
  if (phase === "consented" || phase === "revealed") return "revealed";
  return phase;
}

export function formatTurnBadge(turnId: string): string {
  const m = /^t-?(\d+)$/i.exec(turnId.trim());
  if (m) return `[T-${m[1].padStart(2, "0")}]`;
  if (turnId.startsWith("[") && turnId.endsWith("]")) return turnId.toUpperCase();
  return `[${turnId.toUpperCase()}]`;
}
