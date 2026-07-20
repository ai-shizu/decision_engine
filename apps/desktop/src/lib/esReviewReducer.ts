//! Pure reducer for Pocket Brain ES review streaming UI (no Zustand — E0b).

import { emptyCompanyFacts } from "./interviewStage";
import type { CompanyFacts, SimSessionResult } from "./pocketBrain/types";

export interface EsReviewMessage {
  id: string;
  role: "user" | "reviewer";
  text: string;
  streaming?: boolean;
}

export interface EsReviewMeta {
  companyName: string;
  factsSource: string;
  contextCount: number;
  contextIds: string[];
}

export interface EsReviewState {
  esDraft: string;
  experienceQuery: string;
  facts: CompanyFacts;
  messages: EsReviewMessage[];
  meta: EsReviewMeta | null;
  streaming: boolean;
  error: string | null;
}

export type EsReviewAction =
  | { type: "set_draft"; value: string }
  | { type: "set_experience_query"; value: string }
  | { type: "patch_facts"; patch: Partial<CompanyFacts> }
  | { type: "clear_error" }
  | { type: "reset_feedback" }
  | { type: "review_begin"; userId: string; reviewerId: string; draft: string }
  | { type: "token"; reviewerId: string; text: string }
  | { type: "token_error"; message: string }
  | { type: "review_success"; reviewerId: string; result: SimSessionResult }
  | { type: "review_failure"; message: string }
  | { type: "review_end" };

export function initialEsReviewState(): EsReviewState {
  return {
    esDraft: "",
    experienceQuery: "",
    facts: emptyCompanyFacts(),
    messages: [],
    meta: null,
    streaming: false,
    error: null,
  };
}

export function esReviewReducer(
  state: EsReviewState,
  action: EsReviewAction,
): EsReviewState {
  switch (action.type) {
    case "set_draft":
      return { ...state, esDraft: action.value };
    case "set_experience_query":
      return { ...state, experienceQuery: action.value };
    case "patch_facts":
      return { ...state, facts: { ...state.facts, ...action.patch } };
    case "clear_error":
      return { ...state, error: null };
    case "reset_feedback":
      return {
        ...state,
        messages: [],
        meta: null,
        error: null,
        streaming: false,
      };
    case "review_begin":
      return {
        ...state,
        error: null,
        streaming: true,
        meta: null,
        messages: [
          { id: action.userId, role: "user", text: action.draft },
          {
            id: action.reviewerId,
            role: "reviewer",
            text: "",
            streaming: true,
          },
        ],
      };
    case "token": {
      const messages = state.messages.map((m) =>
        m.id === action.reviewerId && m.role === "reviewer"
          ? { ...m, text: m.text + action.text }
          : m,
      );
      return { ...state, messages };
    }
    case "token_error":
      return { ...state, error: action.message };
    case "review_success": {
      const messages = state.messages.map((m) =>
        m.id === action.reviewerId
          ? { ...m, streaming: false }
          : m,
      );
      return {
        ...state,
        messages,
        meta: {
          companyName: action.result.company_name,
          factsSource: action.result.facts_source,
          contextCount: action.result.context_count,
          contextIds: action.result.context_ids,
        },
      };
    }
    case "review_failure":
      return {
        ...state,
        error: action.message,
        messages: state.messages.filter(
          (m) => !(m.streaming === true && m.text.trim() === ""),
        ),
      };
    case "review_end":
      return { ...state, streaming: false };
    default: {
      const _exhaustive: never = action;
      return _exhaustive;
    }
  }
}
