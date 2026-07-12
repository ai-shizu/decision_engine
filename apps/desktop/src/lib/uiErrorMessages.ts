/**
 * Finding 13 — operation-keyed sterile UI error messages.
 * Error values must never be passed into these helpers.
 */

export type UiErrorRetryPolicy = "retry-safe" | "verify-first";

export interface UiErrorSpec {
  readonly message: string;
  readonly retryPolicy: UiErrorRetryPolicy;
}

export const UI_ERROR_SPECS = {
  PROBE_STATUS_LOAD: {
    message: "PROBEの状態を読み込めませんでした。もう一度お試しください。",
    retryPolicy: "retry-safe",
  },
  PROBE_NEXT: {
    message:
      "次の質問を取得できませんでした。現在の状態を確認し、必要な場合だけ再実行してください。",
    retryPolicy: "verify-first",
  },
  PROBE_ANSWER: {
    message:
      "回答の保存結果を確認できませんでした。現在の状態を確認し、必要な場合だけ再実行してください。",
    retryPolicy: "verify-first",
  },
  RECORD_LOAD: {
    message: "記録を読み込めませんでした。もう一度お試しください。",
    retryPolicy: "retry-safe",
  },
  RECORD_SAVE: {
    message:
      "記録の保存結果を確認できませんでした。現在の状態を確認し、必要な場合だけ再実行してください。",
    retryPolicy: "verify-first",
  },
  INTERVIEW_RESPONSE: {
    message:
      "応答結果を確認できませんでした。セッションの状態を確認し、必要な場合だけ再実行してください。",
    retryPolicy: "verify-first",
  },
  NARRATIVE_COMPILE: {
    message:
      "文章案の作成結果を確認できませんでした。現在の状態を確認し、必要な場合だけ再実行してください。",
    retryPolicy: "verify-first",
  },
  CONSULT_RESPONSE: {
    message:
      "相談結果を確認できませんでした。現在の状態を確認し、必要な場合だけ再実行してください。",
    retryPolicy: "verify-first",
  },
  ROMANCE_ANALYSIS: {
    message:
      "分析結果を確認できませんでした。現在の状態を確認し、必要な場合だけ再実行してください。",
    retryPolicy: "verify-first",
  },
  LINE_IMPORT: {
    message:
      "LINE履歴の取り込み結果を確認できませんでした。データの状態を確認し、必要な場合だけ再実行してください。",
    retryPolicy: "verify-first",
  },
  ICS_SYNC: {
    message:
      "ICS同期の結果を確認できませんでした。カレンダーの状態を確認し、必要な場合だけ再実行してください。",
    retryPolicy: "verify-first",
  },
  APPLE_CALENDAR_SYNC: {
    message:
      "Appleカレンダー同期の結果を確認できませんでした。カレンダーの状態を確認し、必要な場合だけ再実行してください。",
    retryPolicy: "verify-first",
  },
  DOCUMENT_IMPORT: {
    message:
      "ファイル処理の結果を確認できませんでした。データの状態を確認し、必要な場合だけ再実行してください。",
    retryPolicy: "verify-first",
  },
  KNOWLEDGE_FETCH: {
    message:
      "知識キューの処理結果を確認できませんでした。現在の状態を確認し、必要な場合だけ再実行してください。",
    retryPolicy: "verify-first",
  },
  PROFILE_LOAD: {
    message: "プロフィール情報を読み込めませんでした。もう一度お試しください。",
    retryPolicy: "retry-safe",
  },
  ORACLE_REPORT: {
    message:
      "分析レポートの作成結果を確認できませんでした。現在の状態を確認し、必要な場合だけ再実行してください。",
    retryPolicy: "verify-first",
  },
  TWIN_FORECAST: {
    message: "予測結果を取得できませんでした。もう一度お試しください。",
    retryPolicy: "retry-safe",
  },
  TENSOR_REBUILD: {
    message:
      "Tensor再構築の結果を確認できませんでした。現在の状態を確認し、必要な場合だけ再実行してください。",
    retryPolicy: "verify-first",
  },
  SETTINGS_LOAD: {
    message: "設定を読み込めませんでした。もう一度お試しください。",
    retryPolicy: "retry-safe",
  },
  SETTINGS_SAVE: {
    message:
      "設定の保存結果を確認できませんでした。現在の状態を確認し、必要な場合だけ再実行してください。",
    retryPolicy: "verify-first",
  },
  PROFILER_RUN: {
    message:
      "プロフィール再解析の結果を確認できませんでした。現在の状態を確認し、必要な場合だけ再実行してください。",
    retryPolicy: "verify-first",
  },
} as const satisfies Record<string, UiErrorSpec>;

export type UiErrorCode = keyof typeof UI_ERROR_SPECS;

export function uiErrorMessage(code: UiErrorCode): string {
  return UI_ERROR_SPECS[code].message;
}

export function uiErrorRetryPolicy(code: UiErrorCode): UiErrorRetryPolicy {
  return UI_ERROR_SPECS[code].retryPolicy;
}
