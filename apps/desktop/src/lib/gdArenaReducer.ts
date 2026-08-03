//! Pure reducer for GD Arena multi-agent streaming (Inner Coliseum). Mirrors
//! ragChatReducer / multistageInterviewReducer: the reducer never talks to
//! IPC, it only reshapes state from actions dispatched by useGdSession.

import type { GdTranscriptMessage } from "./gdStreamParser";

export interface GdArenaState {
  messages: GdTranscriptMessage[];
  input: string;
  streaming: boolean;
  error: string | null;
  /** messages.length before the in-flight round's parsed agent bubbles. */
  roundBaseLen: number;
}

export type GdArenaAction =
  | { type: "set_input"; value: string }
  | { type: "clear_error" }
  | { type: "send_begin"; userMessage: GdTranscriptMessage }
  | { type: "round_tokens"; parsed: GdTranscriptMessage[] }
  | { type: "send_failure"; message: string }
  | { type: "send_end" };

export function initialGdArenaState(): GdArenaState {
  return {
    messages: [],
    input: "",
    streaming: false,
    error: null,
    roundBaseLen: 0,
  };
}

export function gdArenaReducer(
  state: GdArenaState,
  action: GdArenaAction,
): GdArenaState {
  switch (action.type) {
    case "set_input":
      return { ...state, input: action.value };
    case "clear_error":
      return { ...state, error: null };
    case "send_begin": {
      const messages = [...state.messages, action.userMessage];
      return {
        ...state,
        messages,
        input: "",
        error: null,
        streaming: true,
        roundBaseLen: messages.length,
      };
    }
    case "round_tokens": {
      const messages = [
        ...state.messages.slice(0, state.roundBaseLen),
        ...action.parsed,
      ];
      return { ...state, messages };
    }
    case "send_failure":
      return { ...state, streaming: false, error: action.message };
    case "send_end":
      return { ...state, streaming: false };
    default: {
      const _exhaustive: never = action;
      return _exhaustive;
    }
  }
}
