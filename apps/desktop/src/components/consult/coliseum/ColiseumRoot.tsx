/**
 * Phase 14 / 5-B — Inner Coliseum shell: arena selector (GD | BLACKBOX).
 * GD remains under COMING SOON freeze; BLACKBOX is live outside that wrap.
 */

import { useCallback, useState } from "react";

import {
  gdSetupReady,
  initialGdSetupConfig,
  type GdPhase,
  type GdSetupConfig,
} from "../../../lib/gdSetupState";
import { useGdSession } from "../../../lib/useGdSession";
import { BlackboxArena } from "./BlackboxArena";
import { ColiseumArena } from "./ColiseumArena";
import { ColiseumDebrief } from "./ColiseumDebrief";
import { ColiseumLobby } from "./ColiseumLobby";
import { GdSetupPanel } from "./GdSetupPanel";
import { SovereignBar } from "./SovereignBar";

export type ColiseumView = "lobby" | "arena" | "debrief";
export type ColiseumArenaKind = "gd" | "blackbox";

const VIEWS: { id: ColiseumView; label: string }[] = [
  { id: "lobby", label: "LOBBY" },
  { id: "arena", label: "ARENA" },
  { id: "debrief", label: "DEBRIEF" },
];

const ARENAS: { id: ColiseumArenaKind; label: string }[] = [
  { id: "blackbox", label: "BLACKBOX" },
  { id: "gd", label: "GD · FROZEN" },
];

export function ColiseumRoot({
  onSurrender,
}: {
  /** Fired after two-tap SURRENDER confirmation (hard stop). */
  onSurrender?: () => void;
} = {}) {
  const [arena, setArena] = useState<ColiseumArenaKind>("blackbox");
  const [phase, setPhase] = useState<GdPhase>("setup");
  const [setup, setSetup] = useState<GdSetupConfig>(() => initialGdSetupConfig());
  const [view, setView] = useState<ColiseumView>("lobby");
  const [halted, setHalted] = useState(false);
  const [haltToken, setHaltToken] = useState(0);
  const [blackboxCampaignId, setBlackboxCampaignId] = useState<string | null>(
    null,
  );
  const gdSession = useGdSession(setup);

  const armed = phase === "armed";

  const onCampaignIdChange = useCallback((id: string | null) => {
    setBlackboxCampaignId(id);
  }, []);

  function handleInitialize() {
    if (!gdSetupReady(setup)) return;
    setPhase("armed");
    setView("lobby");
  }

  function handleSurrender() {
    setHalted(true);
    if (arena === "blackbox") {
      setHaltToken((n) => n + 1);
    } else {
      setView("debrief");
    }
    onSurrender?.();
  }

  function trySetView(next: ColiseumView) {
    if (!armed) return;
    setView(next);
  }

  return (
    <div
      className="coliseum-root"
      data-arena={arena}
      data-view={arena === "gd" ? (armed ? view : "setup") : "blackbox"}
      data-phase={arena === "gd" ? phase : "live"}
      data-halted={halted ? "1" : "0"}
    >
      <header className="coliseum-nav" role="navigation" aria-label="Coliseum view">
        <div className="coliseum-nav-brand">
          <span className="coliseum-glyph-ok">▮</span>
          <span className="coliseum-nav-title">
            {arena === "blackbox"
              ? "INNER COLISEUM · BLACKBOX"
              : armed
                ? "INNER COLISEUM · GD"
                : "GD ENVIRONMENT"}
          </span>
          <span className="coliseum-nav-meta">
            {arena === "blackbox"
              ? "IN-PROCESS SESSION"
              : armed
                ? `${setup.participants}席 · ${setup.timeLimitMin}分`
                : "SETUP REQUIRED"}
          </span>
        </div>
        <div className="coliseum-nav-tabs" role="tablist" aria-label="Arena select">
          {ARENAS.map((a) => {
            const active = arena === a.id;
            return (
              <button
                key={a.id}
                type="button"
                role="tab"
                aria-selected={active}
                className={
                  active
                    ? "coliseum-nav-tab coliseum-nav-tab-active"
                    : "coliseum-nav-tab"
                }
                onClick={() => {
                  setArena(a.id);
                  setHalted(false);
                }}
              >
                {a.label}
              </button>
            );
          })}
        </div>
        {arena === "gd" && (
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
        )}
        {halted && (
          <div className="coliseum-halt-banner" role="status">
            SOVEREIGN HALT · SESSION FORCED STOP
          </div>
        )}
      </header>

      <main className="coliseum-stage" aria-live="polite">
        {arena === "blackbox" && (
          <BlackboxArena
            haltRequest={haltToken}
            onCampaignIdChange={onCampaignIdChange}
          />
        )}

        {arena === "gd" && (
          <div className="gd-frozen">
            <div className="gd-frozen-content" aria-hidden="true">
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
                  messages={
                    gdSession.state.messages.length > 0
                      ? gdSession.state.messages
                      : undefined
                  }
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
              {armed && view === "debrief" && (
                <ColiseumDebrief halted={halted} />
              )}

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
            </div>
            <div className="gd-frozen-overlay" role="status" aria-label="GD 近日公開">
              <div className="gd-frozen-badge">
                <span className="gd-frozen-kicker">INNER COLISEUM · GD</span>
                <span className="gd-frozen-title">COMING SOON</span>
                <span className="gd-frozen-sub">
                  近日公開 — BLACKBOX アリーナは上部タブから起動
                </span>
              </div>
            </div>
          </div>
        )}
      </main>

      <SovereignBar
        onConfirmHalt={handleSurrender}
        disabled={
          halted ||
          (arena === "gd" ? !armed : blackboxCampaignId === null)
        }
      />
    </div>
  );
}
