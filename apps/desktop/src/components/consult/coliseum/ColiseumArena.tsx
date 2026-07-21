/**
 * Phase 14 — Arena: transcript stream + circuit breaker + I-22 AsymmetryProbe.
 */

import { useCallback, useState } from "react";

import { AsymmetryProbe } from "./AsymmetryProbe";
import { CircuitBreakerGauge } from "./CircuitBreakerGauge";
import {
  TranscriptStream,
  type TranscriptMessage,
} from "./TranscriptStream";

const MOCK_MESSAGES: TranscriptMessage[] = [
  {
    turnId: "t-0",
    role: "INTERVIEWER",
    stage: "FOUNDATION",
    text: "まず論点を MECE に分割してください。",
  },
  {
    turnId: "t-1",
    role: "CANDIDATE",
    stage: "FOUNDATION",
    text: "需要・供給・規制の三層で切ります。",
  },
  {
    turnId: "t-2",
    role: "INTERVIEWER",
    stage: "PRESSURE",
    text: "定量: オーダーは何度ですか。感度を示せ。",
  },
  {
    turnId: "t-3",
    role: "CANDIDATE",
    stage: "PRESSURE",
    text: "10^7 規模。感度は価格弾性に依存。",
  },
  {
    turnId: "t-4",
    role: "INTERVIEWER",
    stage: "PRESSURE",
    text: "その前提が崩れたとき、結論はどう変わる？",
  },
  {
    turnId: "t-5",
    role: "CANDIDATE",
    stage: "PRESSURE",
    text: "需要側が半減すればオーダーは一桁下がる。",
  },
];

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
  messages = MOCK_MESSAGES,
}: {
  onRequestDebrief?: () => void;
  /** Override compiled tactics for live sessions later. */
  activeTactics?: string[];
  messages?: TranscriptMessage[];
}) {
  const [tripped, setTripped] = useState(false);

  const handleTripped = useCallback(() => {
    setTripped(true);
  }, []);

  return (
    <section className="coliseum-panel coliseum-arena" aria-label="Coliseum arena">
      <header className="coliseum-panel-head">
        <span className="coliseum-panel-id">VIEW/ARENA</span>
        <span
          className={
            tripped
              ? "coliseum-panel-tag coliseum-tag-err"
              : "coliseum-panel-tag coliseum-tag-ok"
          }
        >
          {tripped ? "CIRCUIT TRIPPED" : "PRESSURE STREAM"}
        </span>
      </header>

      <CircuitBreakerGauge onTripped={handleTripped} />

      <div className="coliseum-grid-2">
        <div className="coliseum-frame coliseum-frame-tall coliseum-frame-tx">
          <TranscriptStream messages={messages} />
          <div className="coliseum-arena-actions">
            <button
              type="button"
              className="coliseum-btn-cyan"
              onClick={onRequestDebrief}
              disabled={tripped}
            >
              {tripped ? "FORCED → DEBRIEF (TRIPPED)" : "ADVANCE → DEBRIEF"}
            </button>
            {tripped && (
              <button
                type="button"
                className="coliseum-btn-cyan"
                onClick={onRequestDebrief}
              >
                ENTER DEBRIEF NOW
              </button>
            )}
          </div>
        </div>

        <div className="coliseum-frame coliseum-frame-tall coliseum-frame-asym">
          <AsymmetryProbe activeTactics={activeTactics} />
        </div>
      </div>
    </section>
  );
}
