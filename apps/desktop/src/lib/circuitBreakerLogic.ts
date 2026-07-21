/**
 * Phase 14 Arena — circuit-breaker distress bands (pure; F-14).
 * Warning starts at 78%; trip at 100%.
 */

export const CIRCUIT_WARN_PCT = 78;
export const CIRCUIT_TRIP_PCT = 100;

export type CircuitStatus = "NOMINAL" | "WARNING" | "TRIPPED";

export type CircuitTelemetry = {
  /** Length delta vs prior candidate answer (%). */
  lenDeltaPct: number;
  /** Withdrawal lexicon hits this streak. */
  withdrawal: number;
  /** Silence / pause seconds. */
  silenceSec: number;
};

export function clampDistress(pct: number): number {
  if (!Number.isFinite(pct)) return 0;
  return Math.min(CIRCUIT_TRIP_PCT, Math.max(0, Math.round(pct)));
}

export function circuitStatusFromDistress(pct: number): CircuitStatus {
  const v = clampDistress(pct);
  if (v >= CIRCUIT_TRIP_PCT) return "TRIPPED";
  if (v >= CIRCUIT_WARN_PCT) return "WARNING";
  return "NOMINAL";
}

export function formatCircuitTelemetry(t: CircuitTelemetry): string {
  const len =
    t.lenDeltaPct > 0
      ? `len Δ +${t.lenDeltaPct}%`
      : `len Δ ${t.lenDeltaPct}%`;
  return `${len} · withdrawal ${t.withdrawal} · silence ${t.silenceSec.toFixed(1)}s`;
}

/** Deterministic mock telemetry that escalates with distress (debug sim). */
export function mockTelemetryForDistress(pct: number): CircuitTelemetry {
  const v = clampDistress(pct);
  return {
    lenDeltaPct: v < CIRCUIT_WARN_PCT ? -1 : v < CIRCUIT_TRIP_PCT ? -4 : -12,
    withdrawal: v >= CIRCUIT_TRIP_PCT ? 1 : 0,
    silenceSec: v < CIRCUIT_WARN_PCT ? 0.2 : v < CIRCUIT_TRIP_PCT ? 0.9 : 2.4,
  };
}
