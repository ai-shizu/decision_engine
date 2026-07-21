/**
 * Phase 14 — Inner Coliseum shell: view router + always-mounted SovereignBar.
 * Terminal aesthetics only (monospace / radius 0 / state-colored CSS vars).
 */

import { useState } from "react";

import { ColiseumArena } from "./ColiseumArena";
import { ColiseumDebrief } from "./ColiseumDebrief";
import { ColiseumLobby } from "./ColiseumLobby";
import { SovereignBar } from "./SovereignBar";

export type ColiseumView = "lobby" | "arena" | "debrief";

const VIEWS: { id: ColiseumView; label: string }[] = [
  { id: "lobby", label: "LOBBY" },
  { id: "arena", label: "ARENA" },
  { id: "debrief", label: "DEBRIEF" },
];

export function ColiseumRoot({
  onSurrender,
}: {
  /** Fired after two-tap SURRENDER confirmation (hard stop). */
  onSurrender?: () => void;
} = {}) {
  const [view, setView] = useState<ColiseumView>("lobby");
  const [halted, setHalted] = useState(false);

  function handleSurrender() {
    setHalted(true);
    setView("debrief");
    onSurrender?.();
  }

  return (
    <div className="coliseum-root" data-view={view} data-halted={halted ? "1" : "0"}>
      <header className="coliseum-nav" role="navigation" aria-label="Coliseum view (debug)">
        <div className="coliseum-nav-brand">
          <span className="coliseum-glyph-ok">▮</span>
          <span className="coliseum-nav-title">INNER COLISEUM</span>
          <span className="coliseum-nav-meta">I-22 · F-14 · PHASE14</span>
        </div>
        <div className="coliseum-nav-tabs" role="tablist" aria-label="Coliseum stages">
          {VIEWS.map((v) => (
            <button
              key={v.id}
              type="button"
              role="tab"
              aria-selected={view === v.id}
              className={
                view === v.id
                  ? "coliseum-nav-tab coliseum-nav-tab-active"
                  : "coliseum-nav-tab"
              }
              onClick={() => setView(v.id)}
            >
              {v.label}
            </button>
          ))}
        </div>
        {halted && (
          <div className="coliseum-halt-banner" role="status">
            SOVEREIGN HALT · SESSION FORCED TO DEBRIEF
          </div>
        )}
      </header>

      <main className="coliseum-stage" aria-live="polite">
        {view === "lobby" && <ColiseumLobby onEnterArena={() => setView("arena")} />}
        {view === "arena" && <ColiseumArena onRequestDebrief={() => setView("debrief")} />}
        {view === "debrief" && <ColiseumDebrief halted={halted} />}
      </main>

      {/* Always mounted — fixed sovereign hard-stop (never unmounted by view). */}
      <SovereignBar onConfirmHalt={handleSurrender} disabled={halted} />
    </div>
  );
}
