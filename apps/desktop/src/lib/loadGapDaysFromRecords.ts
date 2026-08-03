//! Load RECORD evidence for Gap recalculation (pure orchestration; Finding 13 sterile).

import { calendarEventDates, loadRecord } from "./engine";
import { todayIso } from "./dateUtils";
import { buildDaysFromRecords } from "./gapTensorDashboardReducer";
import type { AnalyticsDailyDay } from "./pocketBrain/types";
import type { RecordData } from "./types";

const MAX_DAYS = 30;

function shiftIsoDays(iso: string, delta: number): string {
  const d = new Date(`${iso}T12:00:00`);
  d.setDate(d.getDate() + delta);
  const y = d.getFullYear();
  const m = String(d.getMonth() + 1).padStart(2, "0");
  const day = String(d.getDate()).padStart(2, "0");
  return `${y}-${m}-${day}`;
}

function recentFallbackDates(today: string, count: number): string[] {
  const out: string[] = [];
  for (let i = 0; i < count; i += 1) {
    out.push(shiftIsoDays(today, -i));
  }
  return out;
}

/**
 * Collect recent calendar/record dates, load RECORD rows, map to gap days.
 * Engine failures degrade to last-N calendar days from today (fail-open load path).
 */
export async function loadGapDaysFromRecords(): Promise<AnalyticsDailyDay[]> {
  const today = todayIso();
  let dateSet = new Set<string>([today]);

  try {
    const dates = await calendarEventDates();
    for (const d of dates) {
      if (typeof d === "string" && d.trim().length === 10) {
        dateSet.add(d.trim());
      }
    }
  } catch {
    /* best-effort — fall through to recent window */
  }

  if (dateSet.size <= 1) {
    for (const d of recentFallbackDates(today, MAX_DAYS)) {
      dateSet.add(d);
    }
  }

  const sorted = [...dateSet].sort().reverse().slice(0, MAX_DAYS);
  const records: RecordData[] = [];

  const settled = await Promise.allSettled(sorted.map((d) => loadRecord(d)));
  for (const result of settled) {
    if (result.status === "fulfilled") {
      records.push(result.value);
    }
  }

  return buildDaysFromRecords(records);
}
