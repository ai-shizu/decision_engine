/** Turn log — AdvanceView headlines + quarter-close emphasis. */

import type { TurnLogEntry } from "../../../lib/blackboxArenaReducer";

export interface BlackboxTurnLogProps {
  entries: readonly TurnLogEntry[];
}

export function BlackboxTurnLog({ entries }: BlackboxTurnLogProps) {
  return (
    <section className="bxs-turnlog" aria-label="Turn log">
      <header className="bxs-panel-head">
        <span className="bxs-panel-title">TURN LOG</span>
        <span className="bxs-panel-meta">{entries.length}</span>
      </header>
      {entries.length === 0 ? (
        <p className="bxs-empty">NO SETTLED TURNS</p>
      ) : (
        <ul className="bxs-turnlog-list">
          {[...entries].reverse().map((e) => (
            <li key={e.tick} className="bxs-turnlog-row">
              <div>
                T{e.tick} · REV {e.revenueMinor} · COGS {e.cogsMinor} · UNMET{" "}
                {e.unmetUnits}
              </div>
              {e.periodClose && (
                <div className="bxs-quarter-close">
                  QUARTER CLOSE · NI {e.periodClose.netIncomeMinor} · ΔCASH{" "}
                  {e.periodClose.netCashChangeMinor} · DEP{" "}
                  {e.periodClose.depreciationMinor} · INT{" "}
                  {e.periodClose.interestMinor} · TAX {e.periodClose.taxMinor}
                </div>
              )}
            </li>
          ))}
        </ul>
      )}
    </section>
  );
}
