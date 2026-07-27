/**
 * Stimulus deck — null fields render as "—", never invented.
 * No "this is a probe" chrome (measurement contamination guard).
 */

import type { StimulusView } from "../../../lib/parseBlackboxArena";

export interface BlackboxStimulusDeckProps {
  stimuli: readonly StimulusView[];
}

function cell(value: number | null): string {
  return value === null ? "—" : String(value);
}

export function BlackboxStimulusDeck({ stimuli }: BlackboxStimulusDeckProps) {
  return (
    <section className="bxs-stimuli" aria-label="Active events">
      <header className="bxs-panel-head">
        <span className="bxs-panel-title">EVENTS</span>
        <span className="bxs-panel-meta">{stimuli.length}</span>
      </header>
      {stimuli.length === 0 ? (
        <p className="bxs-empty">NO ACTIVE EVENTS</p>
      ) : (
        <ul className="bxs-stimulus-list">
          {stimuli.map((s) => (
            <li key={s.stimulusSeq} className="bxs-stimulus-card" data-kind={s.kind}>
              <div className="bxs-stimulus-kind">{s.kind}</div>
              <div className="bxs-stimulus-grid">
                <span>SEQ {s.stimulusSeq}</span>
                <span>OFFER {cell(s.offerId)}</span>
                <span>PROJ {cell(s.projectId)}</span>
                <span>POS {cell(s.positionId)}</span>
                <span>ANCHOR {cell(s.anchorMinor)}</span>
                <span>GAIN {cell(s.gainMinor)}</span>
                <span>LOSS {cell(s.lossMinor)}</span>
                <span>P {cell(s.winProbabilityMicro)} µ</span>
                <span>PREM {cell(s.premiumMinor)}</span>
                <span>LEFT {cell(s.ticksRemaining)}</span>
                <span>U/R {cell(s.unrealisedMinor)}</span>
              </div>
            </li>
          ))}
        </ul>
      )}
    </section>
  );
}
