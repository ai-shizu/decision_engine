/**
 * Phase 14 — Arena: virtual transcript + I-22 AsymmetryProbe.
 */

import { AsymmetryProbe } from "./AsymmetryProbe";

const MOCK_TURNS = [
  { id: "t-0", role: "INTERVIEWER", text: "まず論点を MECE に分割してください。" },
  { id: "t-1", role: "CANDIDATE", text: "需要・供給・規制の三層で切ります。" },
  { id: "t-2", role: "INTERVIEWER", text: "定量: オーダーは何度ですか。" },
  { id: "t-3", role: "CANDIDATE", text: "10^7 規模。感度は価格弾性に依存。" },
] as const;

/** Mock AbstractTacticSet ids (Finding 1 compile output — no vault fossils). */
const MOCK_ACTIVE_TACTICS = [
  "probe_overgeneralization",
  "stress_quant_backing",
  "force_nuanced_tradeoff",
  "probe_reproducibility",
] as const;

export function ColiseumArena({
  onRequestDebrief,
  activeTactics = [...MOCK_ACTIVE_TACTICS],
}: {
  onRequestDebrief?: () => void;
  /** Override compiled tactics for live sessions later. */
  activeTactics?: string[];
}) {
  return (
    <section className="coliseum-panel" aria-label="Coliseum arena">
      <header className="coliseum-panel-head">
        <span className="coliseum-panel-id">VIEW/ARENA</span>
        <span className="coliseum-panel-tag coliseum-tag-ok">PRESSURE STREAM</span>
      </header>

      <div className="coliseum-grid-2">
        <div className="coliseum-frame coliseum-frame-tall">
          <div className="coliseum-frame-label">VIRTUAL TRANSCRIPT · turn_id BOUND</div>
          <ul className="coliseum-transcript" aria-label="Mock transcript">
            {MOCK_TURNS.map((t) => (
              <li key={t.id} className="coliseum-turn">
                <span className="coliseum-turn-id">{t.id}</span>
                <span
                  className={
                    t.role === "INTERVIEWER"
                      ? "coliseum-turn-role coliseum-text-cyan"
                      : "coliseum-turn-role coliseum-text-ok"
                  }
                >
                  {t.role}
                </span>
                <span className="coliseum-turn-text">{t.text}</span>
              </li>
            ))}
          </ul>
          <button type="button" className="coliseum-btn-cyan" onClick={onRequestDebrief}>
            ADVANCE → DEBRIEF
          </button>
        </div>

        <div className="coliseum-frame coliseum-frame-tall coliseum-frame-asym">
          <AsymmetryProbe activeTactics={activeTactics} />
        </div>
      </div>
    </section>
  );
}
