//! Pure reducer for Gap + Tensor dashboard (no Zustand — E0b).

import type {
  AnalyticsDailyDay,
  CalculateGapAnalysisResult,
  LatestGapAnalysisResult,
  TensorProfile,
} from "./pocketBrain/types";
import { todayIso } from "./dateUtils";

export interface GapEvidenceDraft {
  date: string;
  diaryText: string;
  lineSelfText: string;
  expenseCategory: string;
  expenseAmount: string;
  calendarTitle: string;
}

export interface GapTensorDashboardState {
  phase: "idle" | "loading" | "recalculating" | "ensuring_tensor";
  tensor: TensorProfile | null;
  gap: LatestGapAnalysisResult | null;
  lastCalculate: CalculateGapAnalysisResult | null;
  draft: GapEvidenceDraft;
  error: string | null;
}

export type GapTensorDashboardAction =
  | { type: "patch_draft"; patch: Partial<GapEvidenceDraft> }
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
    }
  | { type: "recalc_failure"; message: string }
  | { type: "ensure_begin" }
  | { type: "ensure_success"; tensor: TensorProfile }
  | { type: "ensure_failure"; message: string };

export function initialGapEvidenceDraft(): GapEvidenceDraft {
  return {
    date: todayIso(),
    diaryText: "",
    lineSelfText: "",
    expenseCategory: "",
    expenseAmount: "",
    calendarTitle: "",
  };
}

export function initialGapTensorDashboardState(): GapTensorDashboardState {
  return {
    phase: "idle",
    tensor: null,
    gap: null,
    lastCalculate: null,
    draft: initialGapEvidenceDraft(),
    error: null,
  };
}

/** Build CalculateGapRequest.days from the evidence composer (non-empty). */
export function buildDaysFromDraft(draft: GapEvidenceDraft): AnalyticsDailyDay[] {
  const date = draft.date.trim();
  const amount = Number.parseInt(draft.expenseAmount.trim(), 10);
  const transactions =
    draft.expenseCategory.trim() && Number.isFinite(amount) && amount > 0
      ? [
          {
            type: "expense",
            category: draft.expenseCategory.trim(),
            amount,
          },
        ]
      : [];
  const calendarEvents = draft.calendarTitle.trim()
    ? [{ title: draft.calendarTitle.trim() }]
    : [];

  return [
    {
      date,
      diaryText: draft.diaryText,
      lineSelfText: draft.lineSelfText,
      consultations: [],
      transactions,
      calendarEvents,
    },
  ];
}

export function draftReadyForRecalc(draft: GapEvidenceDraft): boolean {
  return draft.date.trim().length === 10;
}

export function gapTensorDashboardReducer(
  state: GapTensorDashboardState,
  action: GapTensorDashboardAction,
): GapTensorDashboardState {
  switch (action.type) {
    case "patch_draft":
      return { ...state, draft: { ...state.draft, ...action.patch } };
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
