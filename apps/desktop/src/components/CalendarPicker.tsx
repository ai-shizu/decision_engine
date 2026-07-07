import { useMemo, useState } from "react";
import { monthLabel, todayIso } from "../lib/dateUtils";

interface Props {
  selected: string;
  eventDates: Set<string>;
  onSelect: (date: string) => void;
}

export function CalendarPicker({ selected, eventDates, onSelect }: Props) {
  const initial = selected ? new Date(`${selected}T12:00:00`) : new Date();
  const [viewYear, setViewYear] = useState(initial.getFullYear());
  const [viewMonth, setViewMonth] = useState(initial.getMonth() + 1);

  const cells = useMemo(() => {
    const first = new Date(viewYear, viewMonth - 1, 1);
    const startPad = (first.getDay() + 6) % 7;
    const daysInMonth = new Date(viewYear, viewMonth, 0).getDate();
    const out: Array<{ date: string | null; inMonth: boolean }> = [];
    for (let i = 0; i < startPad; i++) out.push({ date: null, inMonth: false });
    for (let d = 1; d <= daysInMonth; d++) {
      const m = String(viewMonth).padStart(2, "0");
      const day = String(d).padStart(2, "0");
      out.push({ date: `${viewYear}-${m}-${day}`, inMonth: true });
    }
    return out;
  }, [viewYear, viewMonth]);

  function prevMonth() {
    if (viewMonth === 1) {
      setViewYear((y) => y - 1);
      setViewMonth(12);
    } else {
      setViewMonth((m) => m - 1);
    }
  }

  function nextMonth() {
    if (viewMonth === 12) {
      setViewYear((y) => y + 1);
      setViewMonth(1);
    } else {
      setViewMonth((m) => m + 1);
    }
  }

  const today = todayIso();

  return (
    <div className="calendar">
      <div className="calendar-toolbar">
        <button type="button" onClick={prevMonth} aria-label="前月">
          ‹
        </button>
        <span>{monthLabel(viewYear, viewMonth)}</span>
        <button type="button" onClick={nextMonth} aria-label="翌月">
          ›
        </button>
      </div>
      <div className="calendar-grid">
        {["月", "火", "水", "木", "金", "土", "日"].map((w) => (
          <span key={w} className="cal-head">
            {w}
          </span>
        ))}
        {cells.map((cell, i) =>
          cell.date ? (
            <button
              key={cell.date + i}
              type="button"
              className={[
                "cal-day",
                cell.date === selected ? "selected" : "",
                cell.date === today ? "today" : "",
                eventDates.has(cell.date) ? "has-events" : "",
              ]
                .filter(Boolean)
                .join(" ")}
              onClick={() => onSelect(cell.date!)}
            >
              {parseInt(cell.date.slice(8), 10)}
            </button>
          ) : (
            <span key={`pad-${i}`} className="cal-pad" />
          ),
        )}
      </div>
    </div>
  );
}
