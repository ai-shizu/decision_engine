/**
 * Pure helpers for Phase 12 metacognitive calendar (telemetry matrix + VoiceOver).
 * No React — harness-testable in tests-runtime.
 */

import { categoryAxisName } from "./biasProfileView";
import type { CognitiveDayView } from "./pocketBrain/types";

export type RLevel = "低" | "中" | "高";

/** Deterministic cell chrome band — maps to `--ok` / `--sys-cyan` / `--err-soft` / `--err`. */
export type CellTelemetryBand =
  | "empty"
  | "stable"
  | "nominal"
  | "warn"
  | "danger";

export type CalendarCell =
  | { kind: "pad"; key: string }
  | {
      kind: "day";
      key: string;
      dayOfMonth: number;
      date: string;
      day: CognitiveDayView;
    };

/** Map Twin R(t) ∈ [0,1] to a Japanese level word for aria-label. */
export function rLevelJa(r: number | null | undefined): RLevel | null {
  if (r === null || r === undefined || !Number.isFinite(r)) return null;
  if (r < 0.34) return "低";
  if (r < 0.67) return "中";
  return "高";
}

/**
 * @deprecated Consumer HSL heat fills are banned. Always returns null.
 * Prefer `cellTelemetryBand` / CSS token classes.
 */
export function rHeatCss(_r: number | null | undefined): string | null {
  return null;
}

/** True when the day carries any persisted cognitive/finance signal. */
export function dayHasRecord(day: CognitiveDayView): boolean {
  return (
    day.total_expense > 0 ||
    day.r_value !== null ||
    (day.distortions?.length ?? 0) > 0
  );
}

/**
 * Hard state band from R(t) depletion + distortion density.
 * danger ≥ warn ≥ stable ≥ nominal ≥ empty.
 */
export function cellTelemetryBand(day: CognitiveDayView): CellTelemetryBand {
  if (!dayHasRecord(day)) return "empty";
  const n = day.distortions?.length ?? 0;
  const r = day.r_value;
  const depleted =
    r !== null && r !== undefined && Number.isFinite(r) && r < 0.34;
  const strained =
    r !== null && r !== undefined && Number.isFinite(r) && r < 0.55;
  if (n >= 2 || depleted) return "danger";
  if (n >= 1 || strained) return "warn";
  if (
    n === 0 &&
    (r === null || r === undefined || !Number.isFinite(r) || r >= 0.67)
  ) {
    return "stable";
  }
  return "nominal";
}

export function cellTelemetryClassName(
  day: CognitiveDayView,
  opts: { isToday?: boolean } = {},
): string {
  const band = cellTelemetryBand(day);
  const parts = ["cognitive-cal-cell", `cognitive-cal-cell--${band}`];
  if (opts.isToday) parts.push("is-today");
  if (dayHasRecord(day)) parts.push("has-record");
  if (band === "danger") parts.push("hatch-danger");
  if (band === "warn") parts.push("hatch-warn");
  return parts.join(" ");
}

/** Twin R(t) as cold telemetry (two decimals). */
export function formatRTelemetry(r: number | null | undefined): string | null {
  if (r === null || r === undefined || !Number.isFinite(r)) return null;
  return r.toFixed(2);
}

export function formatCompactYen(n: number): string {
  if (!Number.isFinite(n) || n <= 0) return "";
  if (n >= 10_000) {
    const man = n / 10_000;
    return `${man >= 10 ? Math.round(man) : man.toFixed(1)}万`;
  }
  return n.toLocaleString("ja-JP");
}

/**
 * VoiceOver-complete label. Visual DOM should be aria-hidden; this string alone
 * must convey date, R level, expense, and distortion tendency.
 */
export function cognitiveDayAriaLabel(
  day: CognitiveDayView,
  month: number,
): string {
  const dayNum = Number(day.date.slice(8, 10));
  const datePart = `${month}月${dayNum}日`;
  if (!dayHasRecord(day)) {
    return `${datePart}、記録なし`;
  }
  const parts: string[] = [datePart];
  const level = rLevelJa(day.r_value);
  if (level) {
    parts.push(`認知資源 ${level}`);
  }
  if (day.total_expense > 0) {
    parts.push(`支出 ${day.total_expense.toLocaleString("ja-JP")}円`);
  }
  if (day.distortions.length > 0) {
    const labels = day.distortions.map((c) => categoryAxisName(c));
    parts.push(`${labels.join("・")}の傾向あり`);
  }
  return parts.join("、");
}

/** Monday-first month grid with leading/trailing pads. */
export function buildCognitiveMonthGrid(
  year: number,
  month: number,
  days: CognitiveDayView[],
): CalendarCell[] {
  const byDate = new Map(days.map((d) => [d.date, d]));
  const first = new Date(year, month - 1, 1);
  // Mon=0 … Sun=6
  const lead = (first.getDay() + 6) % 7;
  const dim =
    month === 2
      ? year % 4 === 0 && (year % 100 !== 0 || year % 400 === 0)
        ? 29
        : 28
      : [4, 6, 9, 11].includes(month)
        ? 30
        : 31;

  const cells: CalendarCell[] = [];
  for (let i = 0; i < lead; i++) {
    cells.push({ kind: "pad", key: `pad-l-${i}` });
  }
  for (let d = 1; d <= dim; d++) {
    const date = `${year}-${String(month).padStart(2, "0")}-${String(d).padStart(2, "0")}`;
    const day: CognitiveDayView = byDate.get(date) ?? {
      date,
      r_value: null,
      total_expense: 0,
      distortions: [],
    };
    cells.push({
      kind: "day",
      key: date,
      dayOfMonth: d,
      date,
      day,
    });
  }
  while (cells.length % 7 !== 0) {
    cells.push({ kind: "pad", key: `pad-t-${cells.length}` });
  }
  return cells;
}

/** Expense bar width in % relative to the month max (min visual 8% when >0). */
export function expenseBarPct(amount: number, monthMax: number): number {
  if (amount <= 0 || monthMax <= 0) return 0;
  return Math.max(8, Math.round((amount / monthMax) * 100));
}
