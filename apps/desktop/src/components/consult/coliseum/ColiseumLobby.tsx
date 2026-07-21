/**
 * Phase 14 skeleton — Lobby: R(t) gauge + Devil Mode entry gate.
 */

export function ColiseumLobby({
  onEnterArena,
  rT = 72,
  devilLocked = true,
}: {
  onEnterArena?: () => void;
  /** Quantized Twin R(t) on 0..100 (telemetry placeholder). */
  rT?: number;
  devilLocked?: boolean;
}) {
  const clamped = Math.max(0, Math.min(100, Math.round(rT)));
  const depleted = clamped <= 40;

  return (
    <section className="coliseum-panel" aria-label="Coliseum lobby">
      <header className="coliseum-panel-head">
        <span className="coliseum-panel-id">VIEW/LOBBY</span>
        <span className="coliseum-panel-tag coliseum-tag-cyan">ZPD GATE</span>
      </header>

      <div className="coliseum-grid-2">
        <div className="coliseum-frame">
          <div className="coliseum-frame-label">R(t) · COGNITIVE RESOURCE</div>
          <div
            className="coliseum-rt-gauge"
            role="meter"
            aria-valuemin={0}
            aria-valuemax={100}
            aria-valuenow={clamped}
            aria-label="Cognitive resource R(t)"
          >
            <div
              className={
                depleted
                  ? "coliseum-rt-fill coliseum-rt-fill-warn"
                  : "coliseum-rt-fill coliseum-rt-fill-ok"
              }
              style={{ width: `${clamped}%` }}
            />
          </div>
          <div className="coliseum-rt-readout">
            <span className={depleted ? "coliseum-text-amber" : "coliseum-text-ok"}>
              R={clamped}
            </span>
            <span className="coliseum-text-muted">THRESHOLD=40 · F-14 QUANTIZED</span>
          </div>
        </div>

        <div className="coliseum-frame">
          <div className="coliseum-frame-label">ENTRY GATE · DEVIL MODE</div>
          <div className="coliseum-lock-row">
            <span
              className={
                devilLocked || depleted
                  ? "coliseum-lock-badge coliseum-lock-closed"
                  : "coliseum-lock-badge coliseum-lock-open"
              }
            >
              {devilLocked || depleted ? "LOCKED" : "OPEN"}
            </span>
            <span className="coliseum-text-muted">
              {depleted
                ? "ZPD DEPLETED — STRUCTURAL DOWNGRADE"
                : devilLocked
                  ? "PLACEHOLDER · OPT-IN PENDING"
                  : "ONI ELIGIBLE"}
            </span>
          </div>
          <button
            type="button"
            className="coliseum-btn-cyan"
            disabled={depleted}
            onClick={onEnterArena}
          >
            ENTER ARENA
          </button>
        </div>
      </div>

      <pre className="coliseum-mock-block" aria-hidden="true">
{`┌─ TELEMETRY ─────────────────────────────────────┐
│ model_hash ……… [pending artifact freeze]       │
│ per_turn_seeds … COLISEUM_SEED×1_000_003+i     │
│ I-22 …………… fossils → AbstractTacticSet only │
└─────────────────────────────────────────────────┘`}
      </pre>
    </section>
  );
}
