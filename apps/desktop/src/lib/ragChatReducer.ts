//! Pure reducer for RAG chat streaming UI (no Zustand — AI_SKILLS E0b).

export interface RagChatMessage {
  id: string;
  role: "user" | "assistant";
  text: string;
  contextCount?: number;
  streaming?: boolean;
  /** Terminal sys-log error rendered in-bubble (monospace/red). Set ⇒ not streaming. */
  error?: string;
}

export interface RagChatState {
  messages: RagChatMessage[];
  input: string;
  streaming: boolean;
  error: string | null;
}

export type RagChatAction =
  | { type: "set_input"; value: string }
  | { type: "clear_error" }
  | { type: "send_begin"; userId: string; assistantId: string; prompt: string }
  | { type: "token"; assistantId: string; text: string }
  | { type: "token_error"; assistantId: string; message: string }
  | { type: "send_success"; assistantId: string; contextCount: number }
  | { type: "send_failure"; assistantId: string; message: string }
  | { type: "send_end" };

export function initialRagChatState(): RagChatState {
  return {
    messages: [],
    input: "",
    streaming: false,
    error: null,
  };
}

export function ragChatReducer(
  state: RagChatState,
  action: RagChatAction,
): RagChatState {
  switch (action.type) {
    case "set_input":
      return { ...state, input: action.value };
    case "clear_error":
      return { ...state, error: null };
    case "send_begin":
      return {
        ...state,
        input: "",
        error: null,
        streaming: true,
        messages: [
          ...state.messages,
          { id: action.userId, role: "user", text: action.prompt },
          {
            id: action.assistantId,
            role: "assistant",
            text: "",
            streaming: true,
          },
        ],
      };
    case "token": {
      const messages = state.messages.map((m) =>
        m.id === action.assistantId && m.role === "assistant"
          ? { ...m, text: m.text + action.text }
          : m,
      );
      return { ...state, messages };
    }
    case "token_error":
    case "send_failure": {
      // Silent-Hang fix: an error MUST terminate the assistant bubble. Clear its
      // `streaming` flag (the "…" spinner) and attach the sys-log error in-bubble.
      // Previously only `state.error` was set, so the bubble spun forever.
      const messages = state.messages.map((m) =>
        m.id === action.assistantId && m.role === "assistant"
          ? { ...m, streaming: false, error: action.message }
          : m,
      );
      return { ...state, streaming: false, error: null, messages };
    }
    case "send_success": {
      const messages = state.messages.map((m) =>
        m.id === action.assistantId
          ? {
              ...m,
              contextCount: action.contextCount,
              streaming: false,
            }
          : m,
      );
      return { ...state, messages };
    }
    case "send_end":
      return { ...state, streaming: false };
    default: {
      const _exhaustive: never = action;
      return _exhaustive;
    }
  }
}
