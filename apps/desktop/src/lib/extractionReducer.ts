/**
 * Pure extraction UI reducer (M5 Phase 3).
 *
 * No React / Tauri / clipboard / DOM imports — unit-testable via tests-runtime.
 * Trust boundary: only `KakeiboEntryV1` carried by success actions is structured data.
 */

/** Mirrors Rust `KakeiboEntryV1` (amount null = unknown). Canonical TS shape. */
export interface KakeiboEntryV1 {
  date: string;
  amount: number | null;
  category: string;
  payee: string;
  memo: string;
}

export type CopyStatus = "idle" | "copying" | "copied" | "copy_failed";

type ExtractionBase = {
  readonly input: string;
  readonly streamedText: string;
  readonly copyStatus: CopyStatus;
  readonly copyError: string | null;
  /** Non-fatal IPC banner while still extracting (e.g. cancel invoke failed). */
  readonly cancelError: string | null;
};

/**
 * Discriminated union: `requestId` is a number only while `extracting`.
 * `idle` / `success` / `error` always carry `requestId: null`.
 */
export type ExtractionState =
  | (ExtractionBase & {
      readonly phase: "idle";
      readonly result: null;
      readonly error: null;
      readonly requestId: null;
    })
  | (ExtractionBase & {
      readonly phase: "extracting";
      readonly result: null;
      readonly error: null;
      readonly requestId: number;
    })
  | (ExtractionBase & {
      readonly phase: "success";
      readonly result: KakeiboEntryV1;
      readonly error: null;
      readonly requestId: null;
    })
  | (ExtractionBase & {
      readonly phase: "error";
      readonly result: null;
      readonly error: string;
      readonly requestId: null;
    });

export type ExtractionAction =
  | { readonly type: "inputChanged"; readonly input: string }
  | { readonly type: "extractionStarted"; readonly requestId: number; readonly input: string }
  | { readonly type: "tokenReceived"; readonly requestId: number; readonly text: string }
  | {
      readonly type: "extractionSucceeded";
      readonly requestId: number;
      readonly result: KakeiboEntryV1;
    }
  | { readonly type: "extractionFailed"; readonly requestId: number; readonly error: string }
  | { readonly type: "extractionCancelled"; readonly requestId: number }
  | { readonly type: "cancelFailed"; readonly requestId: number; readonly error: string }
  | { readonly type: "copyStarted" }
  | { readonly type: "copySucceeded" }
  | { readonly type: "copyFailed"; readonly error: string }
  | { readonly type: "reset" };

export const INITIAL_EXTRACTION_STATE: ExtractionState = {
  phase: "idle",
  input: "",
  streamedText: "",
  result: null,
  error: null,
  requestId: null,
  copyStatus: "idle",
  copyError: null,
  cancelError: null,
};

function toIdle(input: string): ExtractionState {
  return {
    phase: "idle",
    input,
    streamedText: "",
    result: null,
    error: null,
    requestId: null,
    copyStatus: "idle",
    copyError: null,
    cancelError: null,
  };
}

function isActiveRequest(
  state: ExtractionState,
  requestId: number,
): state is ExtractionState & { phase: "extracting"; requestId: number } {
  return state.phase === "extracting" && state.requestId === requestId;
}

export function extractionReducer(
  state: ExtractionState,
  action: ExtractionAction,
): ExtractionState {
  switch (action.type) {
    case "inputChanged":
      // While extracting, keep phase + requestId (double-submit guard) and only update text.
      if (state.phase === "extracting") {
        return {
          ...state,
          input: action.input,
        };
      }
      return toIdle(action.input);

    case "extractionStarted":
      return {
        phase: "extracting",
        input: action.input,
        streamedText: "",
        result: null,
        error: null,
        requestId: action.requestId,
        copyStatus: "idle",
        copyError: null,
        cancelError: null,
      };

    case "tokenReceived":
      if (!isActiveRequest(state, action.requestId)) {
        return state;
      }
      return {
        ...state,
        streamedText: state.streamedText + action.text,
      };

    case "extractionSucceeded":
      if (!isActiveRequest(state, action.requestId)) {
        return state;
      }
      return {
        phase: "success",
        input: state.input,
        streamedText: state.streamedText,
        result: action.result,
        error: null,
        requestId: null,
        copyStatus: "idle",
        copyError: null,
        cancelError: null,
      };

    case "extractionFailed":
      if (!isActiveRequest(state, action.requestId)) {
        return state;
      }
      return {
        phase: "error",
        input: state.input,
        streamedText: state.streamedText,
        result: null,
        error: action.error.length > 0 ? action.error : "extraction failed",
        requestId: null,
        copyStatus: "idle",
        copyError: null,
        cancelError: null,
      };

    case "extractionCancelled":
      if (!isActiveRequest(state, action.requestId)) {
        return state;
      }
      return toIdle(state.input);

    case "cancelFailed":
      if (!isActiveRequest(state, action.requestId)) {
        return state;
      }
      return {
        ...state,
        cancelError:
          action.error.length > 0
            ? action.error
            : "キャンセルの送信に失敗しました",
      };

    case "copyStarted":
      if (state.phase !== "success") {
        return state;
      }
      return {
        ...state,
        copyStatus: "copying",
        copyError: null,
      };

    case "copySucceeded":
      if (state.phase !== "success") {
        return state;
      }
      return {
        ...state,
        copyStatus: "copied",
        copyError: null,
      };

    case "copyFailed":
      if (state.phase !== "success") {
        return state;
      }
      return {
        ...state,
        copyStatus: "copy_failed",
        copyError: action.error.length > 0 ? action.error : "copy failed",
      };

    case "reset":
      return toIdle("");

    default: {
      const _exhaustive: never = action;
      return _exhaustive;
    }
  }
}

/** Display-only amount formatting (does not mutate copy/persist values). */
export function formatAmountDisplay(amount: number | null): string {
  if (amount === null) {
    return "unknown";
  }
  return String(amount);
}

export function isUnknownField(value: string): boolean {
  return value === "unknown";
}
