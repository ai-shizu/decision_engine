//! Pure reducer for M17 multistage interview UI (no Zustand — AI_SKILLS E0b).

import { coerceInterviewStage, emptyCompanyFacts } from "./interviewStage";
import type {
  CompanyFacts,
  InterviewSession,
  InterviewStage,
  MultistageInterviewResult,
} from "./pocketBrain/types";

export interface MultistageChatMessage {
  id: string;
  role: "candidate" | "interviewer" | "system";
  text: string;
  stage: InterviewStage;
  streaming?: boolean;
}

export interface MultistageInterviewState {
  sessionId: string | null;
  stage: InterviewStage | null;
  status: string | null;
  turnInStage: number;
  totalTurns: number;
  companyName: string | null;
  outcome: string | null;
  messages: MultistageChatMessage[];
  input: string;
  openingMessage: string;
  facts: CompanyFacts;
  streaming: boolean;
  error: string | null;
}

export type MultistageInterviewAction =
  | { type: "set_input"; value: string }
  | { type: "set_opening"; value: string }
  | { type: "patch_facts"; patch: Partial<CompanyFacts> }
  | { type: "clear_error" }
  | { type: "reset" }
  | { type: "start_begin"; assistantId: string }
  | {
      type: "start_success";
      assistantId: string;
      result: MultistageInterviewResult;
    }
  | {
      type: "advance_begin";
      userId: string;
      assistantId: string;
      answer: string;
    }
  | {
      type: "advance_success";
      assistantId: string;
      result: MultistageInterviewResult;
    }
  | { type: "session_closed"; result: MultistageInterviewResult }
  | { type: "token"; assistantId: string; text: string }
  | { type: "token_error"; message: string }
  | { type: "send_failure"; message: string }
  | { type: "send_end" }
  | { type: "hydrate_session"; session: InterviewSession };

export function initialMultistageInterviewState(): MultistageInterviewState {
  return {
    sessionId: null,
    stage: null,
    status: null,
    turnInStage: 0,
    totalTurns: 0,
    companyName: null,
    outcome: null,
    messages: [],
    input: "",
    openingMessage: "",
    facts: emptyCompanyFacts(),
    streaming: false,
    error: null,
  };
}

function applyResultMeta(
  state: MultistageInterviewState,
  result: MultistageInterviewResult,
): MultistageInterviewState {
  return {
    ...state,
    sessionId: result.session_id,
    stage: coerceInterviewStage(result.stage),
    status: result.status,
    turnInStage: result.turn_in_stage,
    totalTurns: result.total_turns,
    companyName: result.company_name,
    outcome: result.outcome,
  };
}

export function multistageInterviewReducer(
  state: MultistageInterviewState,
  action: MultistageInterviewAction,
): MultistageInterviewState {
  switch (action.type) {
    case "set_input":
      return { ...state, input: action.value };
    case "set_opening":
      return { ...state, openingMessage: action.value };
    case "patch_facts":
      return { ...state, facts: { ...state.facts, ...action.patch } };
    case "clear_error":
      return { ...state, error: null };
    case "reset":
      return {
        ...initialMultistageInterviewState(),
        facts: state.facts,
        openingMessage: state.openingMessage,
      };
    case "start_begin":
      return {
        ...state,
        error: null,
        streaming: true,
        messages: [
          {
            id: action.assistantId,
            role: "interviewer",
            text: "",
            stage: "foundation",
            streaming: true,
          },
        ],
      };
    case "start_success": {
      const stage = coerceInterviewStage(action.result.stage);
      const messages = state.messages.map((m) =>
        m.id === action.assistantId
          ? { ...m, stage, streaming: false }
          : m,
      );
      return applyResultMeta({ ...state, messages }, action.result);
    }
    case "advance_begin": {
      const stage = state.stage ?? "foundation";
      return {
        ...state,
        input: "",
        error: null,
        streaming: true,
        messages: [
          ...state.messages,
          {
            id: action.userId,
            role: "candidate",
            text: action.answer,
            stage,
          },
          {
            id: action.assistantId,
            role: "interviewer",
            text: "",
            stage,
            streaming: true,
          },
        ],
      };
    }
    case "advance_success": {
      const stage = coerceInterviewStage(action.result.stage);
      const messages = state.messages.map((m) =>
        m.id === action.assistantId
          ? {
              ...m,
              stage,
              streaming: false,
              role:
                stage === "debrief" || action.result.outcome === "entered_debrief"
                  ? ("interviewer" as const)
                  : m.role,
            }
          : m,
      );
      return applyResultMeta({ ...state, messages }, action.result);
    }
    case "session_closed": {
      const stage = coerceInterviewStage(action.result.stage);
      const withoutStreaming = state.messages
        .filter((m) => !(m.streaming && m.text.trim() === ""))
        .map((m) => ({ ...m, streaming: false }));
      return applyResultMeta(
        {
          ...state,
          streaming: false,
          messages: [
            ...withoutStreaming,
            {
              id: `sys-closed-${action.result.session_id}`,
              role: "system",
              text: "セッションは Closed になりました。",
              stage,
            },
          ],
        },
        action.result,
      );
    }
    case "token": {
      const messages = state.messages.map((m) =>
        m.id === action.assistantId
          ? { ...m, text: m.text + action.text }
          : m,
      );
      return { ...state, messages };
    }
    case "token_error":
      return { ...state, error: action.message };
    case "send_failure":
      return {
        ...state,
        error: action.message,
        messages: state.messages.filter(
          (m) => !(m.streaming === true && m.text.trim() === ""),
        ),
      };
    case "send_end":
      return { ...state, streaming: false };
    case "hydrate_session": {
      const session = action.session;
      const stage = coerceInterviewStage(session.stage);
      return {
        ...state,
        sessionId: session.id,
        stage,
        status: session.status,
        turnInStage: session.turn_in_stage,
        totalTurns: session.total_turns,
        companyName: session.company_name,
        messages: session.transcript.map((t, i) => ({
          id: `tx-${i}`,
          role:
            t.role === "candidate"
              ? ("candidate" as const)
              : t.role === "interviewer"
                ? ("interviewer" as const)
                : ("system" as const),
          text: t.text,
          stage: coerceInterviewStage(t.stage),
        })),
      };
    }
    default: {
      const _exhaustive: never = action;
      return _exhaustive;
    }
  }
}
