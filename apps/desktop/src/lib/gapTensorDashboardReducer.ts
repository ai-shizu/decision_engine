//! Pure helpers for Gap + Tensor dashboard (no Zustand — E0b).

import type {
  AnalyticsDailyDay,
  CalculateGapAnalysisResult,
  LatestGapAnalysisResult,
  TensorProfile,
} from "./pocketBrain/types";
import type { RecordData } from "./types";

export interface GapTensorDashboardState {
  phase: "idle" | "loading" | "recalculating" | "ensuring_tensor";
  tensor: TensorProfile | null;
  gap: LatestGapAnalysisResult | null;
  lastCalculate: CalculateGapAnalysisResult | null;
  /** How many Record days were fed into the last recalc (UI ambient only). */
  lastRecalcDayCount: number | null;
  error: string | null;
}

export type GapTensorDashboardAction =
  | { type: "clear_error" }
  | { type: "load_begin" }
  | {
      type: "load_success";
      tensor: TensorProfile;
      gap: LatestGapAnalysisResult | null;
    }
  | { type: "load_failure"; message: string }
  | { type: "recalc_begin" }
  | {
      type: "recalc_success";
      result: CalculateGapAnalysisResult;
      tensor: TensorProfile;
      gap: LatestGapAnalysisResult;
      dayCount: number;
    }
  | { type: "recalc_failure"; message: string }
  | { type: "ensure_begin" }
  | { type: "ensure_success"; tensor: TensorProfile }
  | { type: "ensure_failure"; message: string };

export function initialGapTensorDashboardState(): GapTensorDashboardState {
  return {
    phase: "idle",
    tensor: null,
    gap: null,
    lastCalculate: null,
    lastRecalcDayCount: null,
    error: null,
  };
}

function recordHasEvidence(record: RecordData): boolean {
  return (
    record.diary.trim().length > 0 ||
    record.events.length > 0 ||
    record.transactions.length > 0
  );
}

/** Map RECORD rows → AnalyticsDailyDay (LINE self-speech not in RECORD → empty). */
export function buildDaysFromRecords(records: RecordData[]): AnalyticsDailyDay[] {
  return records
    .filter(recordHasEvidence)
    .map((record) => ({
      date: record.date.trim(),
      diaryText: record.diary,
      lineSelfText: "",
      consultations: [],
      transactions: record.transactions.map((tx) => ({
        type: tx.type,
        category: tx.category,
        amount: tx.amount,
      })),
      calendarEvents: record.events
        .map((e) => e.title.trim())
        .filter(Boolean)
        .map((title) => ({ title })),
    }))
    .filter((day) => day.date.length === 10);
}

export function gapTensorDashboardReducer(
  state: GapTensorDashboardState,
  action: GapTensorDashboardAction,
): GapTensorDashboardState {
  switch (action.type) {
    case "clear_error":
      return { ...state, error: null };
    case "load_begin":
      return { ...state, phase: "loading", error: null };
    case "load_success":
      return {
        ...state,
        phase: "idle",
        tensor: action.tensor,
        gap: action.gap,
      };
    case "load_failure":
      return { ...state, phase: "idle", error: action.message };
    case "recalc_begin":
      return { ...state, phase: "recalculating", error: null };
    case "recalc_success":
      return {
        ...state,
        phase: "idle",
        lastCalculate: action.result,
        gap: action.gap,
        tensor: action.tensor,
        lastRecalcDayCount: action.dayCount,
      };
    case "recalc_failure":
      return { ...state, phase: "idle", error: action.message };
    case "ensure_begin":
      return { ...state, phase: "ensuring_tensor", error: null };
    case "ensure_success":
      return { ...state, phase: "idle", tensor: action.tensor };
    case "ensure_failure":
      return { ...state, phase: "idle", error: action.message };
    default: {
      const _exhaustive: never = action;
      return _exhaustive;
    }
  }
}
