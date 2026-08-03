/**
 * Phase 14 — Circuit breaker gauge (Arena).
 * Amber at ≥78% warning; crimson at 100% trip. Stoic monospace.
 */

import { useEffect, useMemo, useRef, useState } from "react";

import {
  CIRCUIT_TRIP_PCT,
  CIRCUIT_WARN_PCT,
  circuitStatusFromDistress,
  clampDistress,
  formatCircuitTelemetry,
  mockTelemetryForDistress,
  type CircuitTelemetry,
} from "../../../lib/circuitBreakerLogic";

export type CircuitBreakerGaugeProps = {
  /** Controlled distress 0..100. Uncontrolled if omitted. */
  distressPct?: number;
  telemetry?: CircuitTelemetry;
  /** Show simulate / reset debug controls (default true until backend wiring). */
  showDebugControls?: boolean;
  /** Fired on rising edge into TRIPPED. */
  onTripped?: () => void;
};

export function CircuitBreakerGauge({
  distressPct: controlled,
  telemetry: telemetryProp,
  showDebugControls = true,
  onTripped,
}: CircuitBreakerGaugeProps) {
  const [localPct, setLocalPct] = useState(() =>
    typeof controlled === "number" ? clampDistress(controlled) : 12,
  );
  const wasTripped = useRef(false);

  useEffect(() => {
    if (typeof controlled === "number") {
      setLocalPct(clampDistress(controlled));
    }
  }, [controlled]);

  const pct = clampDistress(localPct);
  const status = circuitStatusFromDistress(pct);
  const telemetry = useMemo(
    () => telemetryProp ?? mockTelemetryForDistress(pct),
    [telemetryProp, pct],
  );

  useEffect(() => {
    if (status === "TRIPPED") {
      if (!wasTripped.current) {
        wasTripped.current = true;
        onTripped?.();
      }
    } else {
      wasTripped.current = false;
    }
  }, [status, onTripped]);

  const fillClass =
    status === "TRIPPED"
      ? "cb-fill cb-fill-trip"
      : status === "WARNING"
        ? "cb-fill cb-fill-warn"
        : "cb-fill cb-fill-nominal";

  const statusClass =
    status === "TRIPPED"
      ? "cb-status cb-status-trip"
      : status === "WARNING"
        ? "cb-status cb-status-warn"
        : "cb-status cb-status-nominal";

  function bump(delta: number) {
    setLocalPct((prev) => clampDistress(prev + delta));
  }

  return (
    <section className="cb-gauge" aria-label="Circuit breaker">
      <header className="cb-head">
        <span className="cb-title">CIRCUIT BREAKER</span>
        <span className={statusClass} data-testid="cb-status">
          {status}
        </span>
      </header>

      <div
        className="cb-track"
        role="meter"
        aria-valuemin={0}
        aria-valuemax={100}
        aria-valuenow={pct}
        aria-label="Distress level"
      >
        <div className={fillClass} style={{ width: `${pct}%` }} />
        <span
          className="cb-warn-marker"
          style={{ left: `${CIRCUIT_WARN_PCT}%` }}
          title={`WARN @ ${CIRCUIT_WARN_PCT}%`}
        />
      </div>

      <div className="cb-scale">
        <span>0%</span>
        <span className="cb-scale-warn">WARN {CIRCUIT_WARN_PCT}%</span>
        <span className="cb-scale-trip">TRIP {CIRCUIT_TRIP_PCT}%</span>
      </div>

      <div className="cb-tele" data-testid="cb-telemetry">
        {formatCircuitTelemetry(telemetry)} · distress={pct}%
      </div>

      {showDebugControls && (
        <div className="cb-debug" aria-label="Circuit breaker debug">
          <button
            type="button"
            className="cb-debug-btn"
            onClick={() => bump(10)}
          >
            [SIMULATE DISTRESS ▲]
          </button>
          <button
            type="button"
            className="cb-debug-btn"
            onClick={() => setLocalPct(0)}
          >
            [RESET]
          </button>
        </div>
      )}
    </section>
  );
}
