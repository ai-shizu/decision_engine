/**
 * Phase 12 — metacognitive calendar: Twin R(t) telemetry matrix + expense + CBT.
 * Lightweight CSS Grid (no third-party calendar). VoiceOver via role=grid.
 */

import { useCallback, useEffect, useMemo, useState } from "react";

import {
  buildCognitiveMonthGrid,
  cellTelemetryClassName,
  cognitiveDayAriaLabel,
  expenseBarPct,
  formatCompactYen,
  formatRTelemetry,
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
    <section
      className="panel cognitive-calendar-panel"
      aria-labelledby="cognitive-cal-title"
    >
      <div className="cognitive-cal-toolbar">
        <h2 id="cognitive-cal-title">METACOG · MATRIX</h2>
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
      <div className="ascii-flow" role="separator" aria-hidden="true">
        -&gt; R(t) · DISTORTION · EXPENSE -&gt;
      </div>
      <p className="hint cognitive-cal-hint">
        border = state · hatch = hazard · cyan = telemetry
      </p>
      {error ? <p className="error-text">{error}</p> : null}

      <div className="cognitive-cal-legend" aria-hidden="true">
        <span className="cognitive-cal-legend-item cognitive-cal-legend--stable term-tag term-tag--ok">
          [ STABLE ]
        </span>
        <span className="cognitive-cal-legend-item cognitive-cal-legend--nominal term-tag term-tag--info">
          [ NOMINAL ]
        </span>
        <span className="cognitive-cal-legend-item cognitive-cal-legend--warn term-tag term-tag--warn">
          [ WARN ]
        </span>
        <span className="cognitive-cal-legend-item cognitive-cal-legend--danger term-tag term-tag--danger">
          [ DANGER ]
        </span>
      </div>

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
              const biasN = cell.day.distortions.length;
              const bar = expenseBarPct(cell.day.total_expense, monthMax);
              const isToday = cell.date === today;
              const rTel = formatRTelemetry(cell.day.r_value);
              return (
                <div
                  key={cell.key}
                  className={cellTelemetryClassName(cell.day, { isToday })}
                  role="gridcell"
                  aria-label={cognitiveDayAriaLabel(cell.day, month)}
                  tabIndex={0}
                >
                  <div className="cognitive-cal-cell-visual" aria-hidden="true">
                    <div className="cognitive-cal-cell-top">
                      <span className="cognitive-cal-daynum">{cell.dayOfMonth}</span>
                      {rTel ? (
                        <span className="cognitive-cal-r">R {rTel}</span>
                      ) : null}
                    </div>
                    {biasN > 0 ? (
                      <span
                        className={
                          biasN >= 2
                            ? "cognitive-cal-bias term-tag term-tag--danger"
                            : "cognitive-cal-bias term-tag term-tag--warn"
                        }
                      >
                        [ D×{biasN} ]
                        <span className="cognitive-cal-bias-ticks">
                          {cell.day.distortions.slice(0, 3).map((c) => (
                            <span
                              key={c}
                              className="cognitive-cal-bias-tick"
                              title={c}
                            />
                          ))}
                        </span>
                      </span>
                    ) : (
                      <span className="cognitive-cal-bias cognitive-cal-bias--none term-tag term-tag--muted">
                        [ — ]
                      </span>
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
                      ) : (
                        <span className="cognitive-cal-expense-amt cognitive-cal-expense-amt--empty">
                          —
                        </span>
                      )}
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
