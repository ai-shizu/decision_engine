/**
 * Phase 14 — Inner Coliseum shell: GD setup gate → LOBBY / ARENA / DEBRIEF.
 * Terminal aesthetics only (monospace / radius 0 / state-colored CSS vars).
 */

import { useState } from "react";

import {
  gdSetupReady,
  initialGdSetupConfig,
  type GdPhase,
  type GdSetupConfig,
} from "../../../lib/gdSetupState";
import { useGdSession } from "../../../lib/useGdSession";
import { ColiseumArena } from "./ColiseumArena";
import { ColiseumDebrief } from "./ColiseumDebrief";
import { ColiseumLobby } from "./ColiseumLobby";
import { GdSetupPanel } from "./GdSetupPanel";
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
  const [phase, setPhase] = useState<GdPhase>("setup");
  const [setup, setSetup] = useState<GdSetupConfig>(() => initialGdSetupConfig());
  const [view, setView] = useState<ColiseumView>("lobby");
  const [halted, setHalted] = useState(false);
  const gdSession = useGdSession(setup);

  const armed = phase === "armed";

  function handleInitialize() {
    if (!gdSetupReady(setup)) return;
    setPhase("armed");
    setView("lobby");
  }

  function handleSurrender() {
    setHalted(true);
    setView("debrief");
    onSurrender?.();
  }

  function trySetView(next: ColiseumView) {
    if (!armed) return;
    setView(next);
  }

  // GD frozen (期待感の醸成): the whole GD surface is blurred + made
  // non-interactive under a COMING SOON overlay so the operator focuses fully on
  // INTERVIEW. The underlying tree is aria-hidden and pointer-events:none (CSS);
  // the overlay layer on top absorbs all interaction.
  return (
    <div className="gd-frozen">
      <div className="gd-frozen-content" aria-hidden="true">
        <div
          className="coliseum-root"
          data-view={armed ? view : "setup"}
          data-phase={phase}
          data-halted={halted ? "1" : "0"}
        >
          <header className="coliseum-nav" role="navigation" aria-label="Coliseum view">
            <div className="coliseum-nav-brand">
              <span className="coliseum-glyph-ok">▮</span>
              <span className="coliseum-nav-title">
                {armed ? "INNER COLISEUM · GD" : "GD ENVIRONMENT"}
              </span>
              <span className="coliseum-nav-meta">
                {armed
                  ? `${setup.participants}席 · ${setup.timeLimitMin}分`
                  : "SETUP REQUIRED"}
              </span>
            </div>
            <div className="coliseum-nav-tabs" role="tablist" aria-label="Coliseum stages">
              {VIEWS.map((v) => {
                const locked = !armed;
                const active = armed && view === v.id;
                return (
                  <button
                    key={v.id}
                    type="button"
                    role="tab"
                    aria-selected={active}
                    aria-disabled={locked}
                    disabled={locked}
                    className={
                      locked
                        ? "coliseum-nav-tab coliseum-nav-tab-locked"
                        : active
                          ? "coliseum-nav-tab coliseum-nav-tab-active"
                          : "coliseum-nav-tab"
                    }
                    onClick={() => trySetView(v.id)}
                  >
                    {locked ? `[ LOCK ] ${v.label}` : v.label}
                  </button>
                );
              })}
            </div>
            {halted && (
              <div className="coliseum-halt-banner" role="status">
                SOVEREIGN HALT · SESSION FORCED TO DEBRIEF
              </div>
            )}
          </header>

          <main className="coliseum-stage" aria-live="polite">
            {!armed && (
              <GdSetupPanel
                config={setup}
                onChange={setSetup}
                onInitialize={handleInitialize}
              />
            )}

            {armed && view === "lobby" && (
              <ColiseumLobby onEnterArena={() => setView("arena")} />
            )}
            {armed && view === "arena" && (
              <ColiseumArena
                mode="gd"
                gdConfig={setup}
                messages={gdSession.state.messages.length > 0 ? gdSession.state.messages : undefined}
                composer={{
                  value: gdSession.state.input,
                  onChange: gdSession.setInput,
                  onSend: gdSession.send,
                  streaming: gdSession.state.streaming,
                  error: gdSession.state.error,
                }}
                onRequestDebrief={() => setView("debrief")}
              />
            )}
            {armed && view === "debrief" && <ColiseumDebrief halted={halted} />}
          </main>

          {armed && (
            <div className="gd-armed-strip" aria-label="GD context summary">
              <span className="gd-armed-theme">{setup.theme.trim()}</span>
              <button
                type="button"
                className="gd-reconfig-btn"
                onClick={() => {
                  setPhase("setup");
                  setHalted(false);
                }}
              >
                [ RECONFIGURE ]
              </button>
            </div>
          )}

          <SovereignBar onConfirmHalt={handleSurrender} disabled={halted || !armed} />
        </div>
      </div>
      <div className="gd-frozen-overlay" role="status" aria-label="GD 近日公開">
        <div className="gd-frozen-badge">
          <span className="gd-frozen-kicker">INNER COLISEUM · GD</span>
          <span className="gd-frozen-title">COMING SOON</span>
          <span className="gd-frozen-sub">近日公開 — 現在は INTERVIEW に全集中</span>
        </div>
      </div>
    </div>
  );
}
