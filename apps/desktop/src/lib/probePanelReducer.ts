//! Pure reducer for Pocket Brain PROBE funnel (M15 / M18-D). No Zustand.

import type {
  ProbeAnswerResultV1,
  ProbeQuestionOut,
  ProbeQuestionV1,
  ProbeStatusV1,
} from "./pocketBrain/types";

export type ProbePanelPhase =
  | "idle"
  | "loading"
  | "fetching_question"
  | "submitting"
  | "complete";

export interface ProbePanelState {
  phase: ProbePanelPhase;
  status: ProbeStatusV1 | null;
  bank: ProbeQuestionOut[];
  question: ProbeQuestionV1 | null;
  answer: string;
  lastResult: ProbeAnswerResultV1 | null;
  error: string | null;
}

export type ProbePanelAction =
  | { type: "set_answer"; value: string }
  | { type: "clear_error" }
  | { type: "load_begin" }
  | {
      type: "load_success";
      status: ProbeStatusV1;
      bank: ProbeQuestionOut[];
    }
  | { type: "load_failure"; message: string }
  | { type: "next_begin" }
  | { type: "next_success"; question: ProbeQuestionV1; status: ProbeStatusV1 }
  | { type: "next_failure"; message: string }
  | { type: "submit_begin" }
  | {
      type: "submit_success";
      result: ProbeAnswerResultV1;
      status: ProbeStatusV1;
    }
  | { type: "submit_failure"; message: string }
  | { type: "reset_to_lobby" };

export function initialProbePanelState(): ProbePanelState {
  return {
    phase: "idle",
    status: null,
    bank: [],
    question: null,
    answer: "",
    lastResult: null,
    error: null,
  };
}

export function probePanelReducer(
  state: ProbePanelState,
  action: ProbePanelAction,
): ProbePanelState {
  switch (action.type) {
    case "set_answer":
      return { ...state, answer: action.value, error: null };
    case "clear_error":
      return { ...state, error: null };
    case "load_begin":
      return { ...state, phase: "loading", error: null };
    case "load_success":
      return {
        ...state,
        phase: "idle",
        status: action.status,
        bank: action.bank,
      };
    case "load_failure":
      return { ...state, phase: "idle", error: action.message };
    case "next_begin":
      return { ...state, phase: "fetching_question", error: null };
    case "next_success":
      return {
        ...state,
        phase: "idle",
        question: action.question,
        status: action.status,
        answer: "",
        lastResult: null,
      };
    case "next_failure":
      return { ...state, phase: "idle", error: action.message };
    case "submit_begin":
      return { ...state, phase: "submitting", error: null };
    case "submit_success": {
      const complete = action.result.next_question === null;
      return {
        ...state,
        phase: complete ? "complete" : "idle",
        lastResult: action.result,
        question: action.result.next_question,
        status: action.status,
        answer: "",
      };
    }
    case "submit_failure":
      return { ...state, phase: "idle", error: action.message };
    case "reset_to_lobby":
      return {
        ...state,
        phase: "idle",
        question: null,
        answer: "",
        lastResult: null,
        error: null,
      };
    default: {
      const _exhaustive: never = action;
      return _exhaustive;
    }
  }
}

export function probeBusy(phase: ProbePanelPhase): boolean {
  return (
    phase === "loading" ||
    phase === "fetching_question" ||
    phase === "submitting"
  );
}
