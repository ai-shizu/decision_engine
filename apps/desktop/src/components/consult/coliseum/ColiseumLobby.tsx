/**
 * Phase 14 — Coliseum Lobby: ZPD ResourceGauge + Devil Mode gate + ProtectionNotice.
 * Lock uses emerald (--ok): protection online, never crimson.
 */

import { useEffect, useMemo, useState } from "react";

import {
  evaluateDevilGate,
  formatUnit,
  rBandLabel,
  resolveModeSelection,
  type InterviewMode,
} from "../../../lib/coliseumLobbyLogic";

export type ColiseumLobbyProps = {
  onEnterArena?: (mode: InterviewMode) => void;
  /** Unit-interval Twin R(t) in [0,1]. Controlled or initial seed. */
  rT?: number;
  /** Unit-interval lapse probability in [0,1]. */
  pLapse?: number;
  /** Show corner debug sliders (default true until backend wiring). */
  showDebugControls?: boolean;
};

function ResourceGauge({ r, pLapse }: { r: number; pLapse: number }) {
  const pct = Math.round(r * 100);
  const band = rBandLabel(r);

  return (
    <div className="coliseum-frame" aria-label="ZPD resource gauge">
      <div className="coliseum-frame-label">RESOURCE GAUGE · ZPD PARAMETERS</div>

      <div className="coliseum-lobby-metrics">
        <div className="coliseum-lobby-metric">
          <span className="coliseum-text-muted">R(t)</span>
          <span className="coliseum-lobby-metric-val coliseum-text-cyan" data-testid="lobby-r-value">
            {formatUnit(r)}
          </span>
        </div>
        <div className="coliseum-lobby-metric">
          <span className="coliseum-text-muted">P_LAPSE</span>
          <span className="coliseum-lobby-metric-val coliseum-text-cyan" data-testid="lobby-plapse-value">
            {formatUnit(pLapse)}
          </span>
        </div>
        <div className="coliseum-lobby-metric">
          <span className="coliseum-text-muted">BAND</span>
          <span
            className={
              band === "DEPLETED"
                ? "coliseum-lobby-metric-val coliseum-text-ok"
                : "coliseum-lobby-metric-val coliseum-text-cyan"
            }
          >
            {band}
          </span>
        </div>
      </div>

      <div
        className="coliseum-rt-gauge coliseum-lobby-gauge"
        role="meter"
        aria-valuemin={0}
        aria-valuemax={100}
        aria-valuenow={pct}
        aria-label="Cognitive resource R(t)"
      >
        <div className="coliseum-rt-fill coliseum-rt-fill-cyan" style={{ width: `${pct}%` }} />
        <span className="coliseum-lobby-tick" style={{ left: "40%" }} title="0.40 DEPLETED" />
        <span className="coliseum-lobby-tick" style={{ left: "70%" }} title="0.70 HIGH" />
      </div>
      <div className="coliseum-lobby-tick-labels">
        <span>0.00</span>
        <span className="coliseum-text-ok">0.40 DEPLETED</span>
        <span className="coliseum-text-cyan">0.70 HIGH</span>
        <span>1.00</span>
      </div>
    </div>
  );
}

function ModeSelect({
  mode,
  devilReady,
  onSelect,
}: {
  mode: InterviewMode;
  devilReady: boolean;
  onSelect: (m: InterviewMode) => void;
}) {
  return (
    <div className="coliseum-frame" aria-label="Interview mode select">
      <div className="coliseum-frame-label">MODE SELECT · HARD GATE</div>
      <div className="coliseum-mode-row" role="group" aria-label="Interview mode">
        <button
          type="button"
          className={
            mode === "standard"
              ? "coliseum-mode-btn coliseum-mode-btn-active"
              : "coliseum-mode-btn"
          }
          aria-pressed={mode === "standard"}
          onClick={() => onSelect("standard")}
        >
          STANDARD
        </button>
        <button
          type="button"
          className={
            mode === "devil"
              ? "coliseum-mode-btn coliseum-mode-btn-devil-active"
              : devilReady
                ? "coliseum-mode-btn"
                : "coliseum-mode-btn coliseum-mode-btn-locked"
          }
          aria-pressed={mode === "devil"}
          aria-disabled={!devilReady}
          title={
            devilReady
              ? "Devil Mode READY"
              : "Protected lock — Devil unavailable (not an error)"
          }
          onClick={() => onSelect("devil")}
        >
          DEVIL {devilReady ? "· READY" : "· LOCKED"}
        </button>
      </div>
      <div className="coliseum-text-muted" style={{ fontSize: "0.65rem" }}>
        {devilReady
          ? "GATE PASS · ONI ELIGIBLE"
          : "GATE FAIL · PROTECTION ONLINE · STANDARD ONLY"}
      </div>
    </div>
  );
}

function ProtectionNotice({
  gateFormula,
  r,
  pLapse,
}: {
  gateFormula: string;
  r: number;
  pLapse: number;
}) {
  return (
    <aside
      className="coliseum-protection-notice"
      role="status"
      aria-live="polite"
      data-testid="lobby-protection-notice"
    >
      <div className="coliseum-protection-title">PROTECTION · ACTIVE</div>
      <p className="coliseum-protection-body">
        System locked to prevent cognitive damage — this is not an error.
      </p>
      <p className="coliseum-protection-egress">Standard remains available.</p>
      <pre className="coliseum-protection-math">
        {gateFormula}
        {"\n"}
        {`EVAL: R=${formatUnit(r)}  P_LAPSE=${formatUnit(pLapse)}  → LOCKED`}
      </pre>
    </aside>
  );
}

export function ColiseumLobby({
  onEnterArena,
  rT: rProp,
  pLapse: pProp,
  showDebugControls = true,
}: ColiseumLobbyProps) {
  const [rLocal, setRLocal] = useState(() =>
    typeof rProp === "number" ? rProp : 0.72,
  );
  const [pLocal, setPLocal] = useState(() =>
    typeof pProp === "number" ? pProp : 0.22,
  );
  const [mode, setMode] = useState<InterviewMode>("standard");

  // Controlled sync when parent later wires live Twin telemetry.
  useEffect(() => {
    if (typeof rProp === "number") setRLocal(rProp);
  }, [rProp]);
  useEffect(() => {
    if (typeof pProp === "number") setPLocal(pProp);
  }, [pProp]);

  const gate = useMemo(() => evaluateDevilGate(rLocal, pLocal), [rLocal, pLocal]);

  useEffect(() => {
    setMode((prev) => resolveModeSelection(prev, gate));
  }, [gate]);

  function handleModeSelect(requested: InterviewMode) {
    setMode(resolveModeSelection(requested, gate));
  }

  return (
    <section className="coliseum-panel coliseum-lobby" aria-label="Coliseum lobby">
      <header className="coliseum-panel-head">
        <span className="coliseum-panel-id">VIEW/LOBBY</span>
        <span
          className={
            gate.locked
              ? "coliseum-panel-tag coliseum-tag-ok"
              : "coliseum-panel-tag coliseum-tag-cyan"
          }
        >
          {gate.locked ? "PROTECTION ONLINE" : "ZPD CLEAR"}
        </span>
      </header>

      <div className="coliseum-grid-2">
        <ResourceGauge r={gate.r} pLapse={gate.pLapse} />
        <ModeSelect mode={mode} devilReady={gate.ready} onSelect={handleModeSelect} />
      </div>

      {gate.locked && (
        <ProtectionNotice
          gateFormula={gate.gateFormula}
          r={gate.r}
          pLapse={gate.pLapse}
        />
      )}

      <div className="coliseum-lobby-actions">
        <button
          type="button"
          className="coliseum-btn-cyan"
          onClick={() => onEnterArena?.(mode)}
        >
          ENTER ARENA · {mode === "devil" ? "DEVIL" : "STANDARD"}
        </button>
      </div>

      {showDebugControls && (
        <div className="coliseum-lobby-debug" aria-label="Lobby debug simulators">
          <div className="coliseum-frame-label">DEBUG · SIMULATE TELEMETRY</div>
          <label className="coliseum-lobby-slider">
            <span className="coliseum-text-cyan">R(t)={formatUnit(rLocal)}</span>
            <input
              type="range"
              min={0}
              max={100}
              step={1}
              value={Math.round(rLocal * 100)}
              onChange={(e) => setRLocal(Number(e.target.value) / 100)}
              aria-label="Simulate R(t)"
            />
          </label>
          <label className="coliseum-lobby-slider">
            <span className="coliseum-text-cyan">P_LAPSE={formatUnit(pLocal)}</span>
            <input
              type="range"
              min={0}
              max={100}
              step={1}
              value={Math.round(pLocal * 100)}
              onChange={(e) => setPLocal(Number(e.target.value) / 100)}
              aria-label="Simulate P_LAPSE"
            />
          </label>
        </div>
      )}
    </section>
  );
}
