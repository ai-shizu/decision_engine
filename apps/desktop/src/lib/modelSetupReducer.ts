/**
 * Pure reducer for the offline model setup gate (no React / no Zustand).
 * Network download states are intentionally absent — local import only.
 */

export type ModelSetupPhase =
  | "checking"
  | "needed"
  | "importing"
  | "ready"
  | "unavailable";

export interface ModelSetupState {
  phase: ModelSetupPhase;
  progressPercent: number;
  relativePath: string;
  recommendedPageUrl: string;
  error: string | null;
}

export type ModelSetupAction =
  | { type: "check_start" }
  | {
      type: "check_result";
      exists: boolean;
      relativePath: string;
      recommendedPageUrl: string;
    }
  | { type: "check_skipped" }
  | { type: "import_start" }
  | { type: "import_progress"; percent: number }
  | { type: "import_done" }
  | { type: "import_error"; message: string }
  | { type: "clear_error" };

export const INITIAL_MODEL_SETUP: ModelSetupState = {
  phase: "checking",
  progressPercent: 0,
  relativePath: "models/pocket-brain.gguf",
  recommendedPageUrl: "https://huggingface.co/Qwen/Qwen2.5-7B-Instruct-GGUF",
  error: null,
};

export function modelSetupReducer(
  state: ModelSetupState,
  action: ModelSetupAction,
): ModelSetupState {
  switch (action.type) {
    case "check_start":
      return { ...INITIAL_MODEL_SETUP, phase: "checking" };
    case "check_result":
      return {
        ...state,
        phase: action.exists ? "ready" : "needed",
        relativePath: action.relativePath,
        recommendedPageUrl: action.recommendedPageUrl,
        progressPercent: action.exists ? 100 : 0,
        error: null,
      };
    case "check_skipped":
      // pocket-brain feature absent — do not block the shell.
      return { ...state, phase: "unavailable", error: null };
    case "import_start":
      return {
        ...state,
        phase: "importing",
        progressPercent: 0,
        error: null,
      };
    case "import_progress":
      return {
        ...state,
        phase: "importing",
        progressPercent: Math.max(0, Math.min(100, action.percent)),
      };
    case "import_done":
      return {
        ...state,
        phase: "ready",
        progressPercent: 100,
        error: null,
      };
    case "import_error":
      return {
        ...state,
        phase: "needed",
        progressPercent: 0,
        error: action.message,
      };
    case "clear_error":
      return { ...state, error: null };
    default: {
      const _exhaustive: never = action;
      return _exhaustive;
    }
  }
}

/** Soft fixed copy — never surface raw exception / path / secrets. */
export function softImportError(_raw: string): string {
  return "モデルの取り込みに失敗しました。ファイルを確認して再度お試しください。";
}
