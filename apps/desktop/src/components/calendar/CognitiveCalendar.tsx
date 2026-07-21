/**
 * Phase 12 — metacognitive calendar: Twin R(t) heatmap + expense + CBT markers.
 * Lightweight CSS Grid (no third-party calendar). VoiceOver via role=grid.
 */

import { useCallback, useEffect, useMemo, useState } from "react";

import {
  buildCognitiveMonthGrid,
  cognitiveDayAriaLabel,
  dayHasRecord,
  expenseBarPct,
  rHeatCss,
} from "../../lib/cognitiveCalendarView";
import { monthLabel, todayIso } from "../../lib/dateUtils";
import { FOREGROUND_RESTORE_EVENT } from "../../lib/foregroundRestore";
import { getCognitiveMonthView } from "../../lib/pocketBrain";
import type { CognitiveMonthView } from "../../lib/pocketBrain/types";

const WEEKDAYS = ["月", "火", "水", "木", "金", "土", "日"] as const;

function shiftMonth(year: number, month: number, delta: number): {
  year: number;
  month: number;
} {
  const d = new Date(year, month - 1 + delta, 1);
  return { year: d.getFullYear(), month: d.getMonth() + 1 };
}

export function CognitiveCalendar() {
  const today = todayIso();
  const initial = useMemo(() => {
    const [y, m] = today.split("-").map(Number);
    return { year: y, month: m };
  }, [today]);
  const [year, setYear] = useState(initial.year);
  const [month, setMonth] = useState(initial.month);
  const [view, setView] = useState<CognitiveMonthView | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const load = useCallback(async (y: number, m: number) => {
    setBusy(true);
    setError(null);
    try {
      const next = await getCognitiveMonthView(y, m);
      setView(next);
    } catch {
      setView(null);
      setError("認知カレンダーを読み込めませんでした");
    } finally {
      setBusy(false);
    }
  }, []);

  useEffect(() => {
    void load(year, month);
  }, [year, month, load]);

  useEffect(() => {
    function onRestore(): void {
      void load(year, month);
    }
    window.addEventListener(FOREGROUND_RESTORE_EVENT, onRestore);
    return () => {
      window.removeEventListener(FOREGROUND_RESTORE_EVENT, onRestore);
    };
  }, [year, month, load]);

  const days = view?.days ?? [];
  const cells = useMemo(
    () => buildCognitiveMonthGrid(year, month, days),
    [year, month, days],
  );
  const monthMax = useMemo(
    () => days.reduce((max, d) => Math.max(max, d.total_expense), 0),
    [days],
  );

  function go(delta: number): void {
    const next = shiftMonth(year, month, delta);
    setYear(next.year);
    setMonth(next.month);
  }

  return (
    <section className="panel cognitive-calendar-panel" aria-labelledby="cognitive-cal-title">
      <div className="cognitive-cal-toolbar">
        <h2 id="cognitive-cal-title">メタ認知カレンダー</h2>
        <div className="cognitive-cal-nav">
          <button type="button" className="ghost" onClick={() => go(-1)} aria-label="前月">
            ‹
          </button>
          <span className="cognitive-cal-month" aria-live="polite">
            {monthLabel(year, month)}
            {busy ? " …" : ""}
          </span>
          <button type="button" className="ghost" onClick={() => go(1)} aria-label="翌月">
            ›
          </button>
        </div>
      </div>
      <p className="hint cognitive-cal-hint">
        背景は認知資源 R(t)（低=暖色 / 高=寒色）。上部ドットは CBT バイアス、下部バーは支出。
      </p>
      {error ? <p className="error-text">{error}</p> : null}

      <div
        className="cognitive-cal-grid"
        role="grid"
        aria-label={`${monthLabel(year, month)}のメタ認知カレンダー`}
      >
        <div className="cognitive-cal-row" role="row">
          {WEEKDAYS.map((wd) => (
            <div key={wd} className="cognitive-cal-head" role="columnheader">
              {wd}
            </div>
          ))}
        </div>
        {chunkRows(cells).map((row, ri) => (
          <div key={`row-${ri}`} className="cognitive-cal-row" role="row">
            {row.map((cell) => {
              if (cell.kind === "pad") {
                return (
                  <div
                    key={cell.key}
                    className="cognitive-cal-pad"
                    role="presentation"
                    aria-hidden="true"
                  />
                );
              }
              const heat = rHeatCss(cell.day.r_value);
              const hasBias = cell.day.distortions.length > 0;
              const bar = expenseBarPct(cell.day.total_expense, monthMax);
              const isToday = cell.date === today;
              const recorded = dayHasRecord(cell.day);
              return (
                <div
                  key={cell.key}
                  className={
                    "cognitive-cal-cell" +
                    (isToday ? " is-today" : "") +
                    (recorded ? " has-record" : "")
                  }
                  role="gridcell"
                  aria-label={cognitiveDayAriaLabel(cell.day, month)}
                  tabIndex={0}
                  style={heat ? { backgroundColor: heat } : undefined}
                >
                  <div className="cognitive-cal-cell-visual" aria-hidden="true">
                    <span className="cognitive-cal-daynum">{cell.dayOfMonth}</span>
                    {hasBias ? (
                      <span className="cognitive-cal-bias-dots">
                        {cell.day.distortions.slice(0, 3).map((c) => (
                          <span key={c} className="cognitive-cal-bias-dot" title={c} />
                        ))}
                      </span>
                    ) : (
                      <span className="cognitive-cal-bias-dots" />
                    )}
                    <span className="cognitive-cal-expense">
                      {cell.day.total_expense > 0 ? (
                        <>
                          <span
                            className="cognitive-cal-expense-bar"
                            style={{ width: `${bar}%` }}
                          />
                          <span className="cognitive-cal-expense-amt">
                            {formatCompactYen(cell.day.total_expense)}
                          </span>
                        </>
                      ) : null}
                    </span>
                  </div>
                </div>
              );
            })}
          </div>
        ))}
      </div>
    </section>
  );
}

function chunkRows<T>(cells: T[]): T[][] {
  const rows: T[][] = [];
  for (let i = 0; i < cells.length; i += 7) {
    rows.push(cells.slice(i, i + 7));
  }
  return rows;
}

function formatCompactYen(n: number): string {
  if (n >= 10_000) {
    const man = n / 10_000;
    return `${man >= 10 ? Math.round(man) : man.toFixed(1)}万`;
  }
  return n.toLocaleString("ja-JP");
}
