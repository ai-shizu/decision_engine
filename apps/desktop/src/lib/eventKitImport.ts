/**
 * Pure helpers for EventKit → daily Vault ingest (M20 Part 2 UI).
 * No React / no Tauri — boundary-testable.
 */

import { toIsoDate } from "./dateUtils";
import type { CalendarEventOut } from "./pocketBrain/types";

/** One day bucket ready for `sync_daily_context` `events_json`. */
export interface DailyCalendarBucket {
  date: string;
  events: Array<{ time: string; title: string }>;
}

/** Default import window: past 365 days … next 730 days (~3 years, Unix UTC). */
export function eventKitImportRange(
  nowMs: number = Date.now(),
): { startUnix: number; endUnix: number } {
  const dayMs = 24 * 60 * 60 * 1000;
  const startUnix = Math.floor((nowMs - 365 * dayMs) / 1000);
  const endUnix = Math.floor((nowMs + 730 * dayMs) / 1000);
  return { startUnix, endUnix };
}

export function unixToLocalIsoDate(unixSec: number): string {
  return toIsoDate(new Date(unixSec * 1000));
}

function formatEventTime(ev: CalendarEventOut): string {
  if (ev.all_day) return "終日";
  const d = new Date(ev.start * 1000);
  const hh = String(d.getHours()).padStart(2, "0");
  const mm = String(d.getMinutes()).padStart(2, "0");
  return `${hh}:${mm}`;
}

/**
 * Group EventKit events by local calendar day for `sync_daily_context`.
 * Empty titles are dropped. Days with zero kept events are omitted.
 */
export function groupEventsForDailySync(
  events: CalendarEventOut[],
): DailyCalendarBucket[] {
  const map = new Map<string, Array<{ time: string; title: string }>>();
  for (const ev of events) {
    const title = (ev.title ?? "").trim();
    if (!title) continue;
    if (!Number.isFinite(ev.start)) continue;
    const date = unixToLocalIsoDate(ev.start);
    const row = { time: formatEventTime(ev), title };
    const list = map.get(date);
    if (list) list.push(row);
    else map.set(date, [row]);
  }
  return [...map.entries()]
    .sort(([a], [b]) => a.localeCompare(b))
    .map(([date, dayEvents]) => ({ date, events: dayEvents }));
}

/** Sterile copy for EventKit authorization soft-fails (Finding 13). */
export function sterileEventKitAuthMessage(status: string): string {
  switch (status) {
    case "denied":
      return "カレンダーへのアクセスが拒否されています。設定 → プライバシーとセキュリティ → カレンダー で Coraxis を許可し、必要な場合だけ再実行してください。";
    case "restricted":
      return "カレンダーへのアクセスが制限されています。端末の制限設定を確認し、必要な場合だけ再実行してください。";
    case "write_only":
      return "カレンダーは書き込みのみ許可されています。フルアクセスを許可し、必要な場合だけ再実行してください。";
    case "not_determined":
      return "カレンダーへのアクセス許可が未設定です。表示されたダイアログで許可し、必要な場合だけ再実行してください。";
    default:
      return "カレンダー予定を取得できませんでした。権限と状態を確認し、必要な場合だけ再実行してください。";
  }
}
