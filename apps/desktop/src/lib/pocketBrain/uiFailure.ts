/**
 * Coraxis on-device UI failure copy (Finding 13). Internal invoke path: pocket-brain.
 * Call-site keyed sterile strings only — never pass exception values.
 */
export const PB_UI_FAIL = {
  probeStatus: "状態を確認できませんでした。保管庫のロックを確認し、必要な場合だけ再実行してください。",
  probeNext: "次の質問を取得できませんでした。しばらくしてからもう一度お試しください。",
  probeSubmit: "回答の送信に失敗しました。しばらくしてからもう一度お試しください。",
  gapTensorLoad:
    "最新の分析データを読み込めませんでした。保管庫のロックを確認し、必要な場合だけ再実行してください。",
  gapRecalc:
    "Gap の再計算結果を確認できませんでした。入力内容を確認し、必要な場合だけ再実行してください。",
  tensorEnsure:
    "バランス分析の確定結果を確認できませんでした。現在の状態を確認し、必要な場合だけ再実行してください。",
} as const;

/** Ambient progress lines (not errors). */
export const PB_UI_BUSY = {
  probeStatus: "状態を確認しています…",
  gapTensorLoad: "最新の分析データを読み込んでいます…",
} as const;
