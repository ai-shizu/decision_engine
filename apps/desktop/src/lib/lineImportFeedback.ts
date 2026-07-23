/**
 * On-device LINE import feedback — sterile copy only (Finding 13).
 * Maps controlled Rust codes `LINE_IMPORT:CODE` to fixed Japanese strings.
 * Never accepts or echoes raw exception text / paths / payloads.
 */

const LINE_IMPORT_CODE_PREFIX = "LINE_IMPORT:";

/** Fixed sterile messages keyed by controlled Rust suffix codes. */
const LINE_IMPORT_DETAIL: Record<string, string> = {
  EMPTY:
    "LINE履歴ファイルが空でした。エクスポートした .txt を選び直してください。必要な場合だけ再実行してください。",
  TOO_LARGE:
    "LINE履歴ファイルが大きすぎます（上限 16MB）。期間を分けてエクスポートし、必要な場合だけ再実行してください。",
  BLANK:
    "LINE履歴を読み取れませんでした（文字化けまたは空内容）。別の文字コードのエクスポートを試し、必要な場合だけ再実行してください。",
  BAD_SOURCE_ID:
    "LINE履歴のファイル名を処理できませんでした。名前を変えて保存し、必要な場合だけ再実行してください。",
  BAD_PATH:
    "LINE履歴の一時ファイルを開けませんでした。もう一度選択し、必要な場合だけ再実行してください。",
  READ:
    "LINE履歴ファイルの読み込みに失敗しました。ファイルを選び直し、必要な場合だけ再実行してください。",
  NO_CHUNKS:
    "LINE履歴から取り込み単位を作れませんでした。内容を確認し、必要な場合だけ再実行してください。",
  VAULT_LOCKED:
    "Vault がロックされているため LINE履歴を保存できませんでした。解錠後に、必要な場合だけ再実行してください。",
  EMBED:
    "LINE履歴の埋め込み処理に失敗しました。モデル配置を確認し、必要な場合だけ再実行してください。",
  PIPELINE:
    "LINE履歴のオンデバイス取り込みに失敗しました。状態を確認し、必要な場合だけ再実行してください。",
  JOIN:
    "LINE履歴の取り込み処理が中断されました。状態を確認し、必要な場合だけ再実行してください。",
};

const LINE_IMPORT_GENERIC =
  "LINE履歴の取り込み結果を確認できませんでした。データの状態を確認し、必要な場合だけ再実行してください。";

/**
 * Extract `LINE_IMPORT:CODE` from a PocketBrain / invoke error message.
 * Returns the CODE only when it is an exact known suffix — never returns raw text.
 */
export function extractLineImportCode(message: string): string | null {
  const idx = message.lastIndexOf(LINE_IMPORT_CODE_PREFIX);
  if (idx < 0) return null;
  const code = message.slice(idx + LINE_IMPORT_CODE_PREFIX.length).trim();
  if (!code || !/^[A-Z][A-Z0-9_]*$/.test(code)) return null;
  return code;
}

/** Sterile UI line for a controlled LINE import failure (never echoes `err`). */
export function sterileLineImportMessage(code: string | null | undefined): string {
  if (!code) return LINE_IMPORT_GENERIC;
  return LINE_IMPORT_DETAIL[code] ?? LINE_IMPORT_GENERIC;
}

/**
 * From an unknown catch value, produce sterile import-log text.
 * Does not stringify the error into the UI — only matches controlled codes.
 */
export function sterileLineImportFromUnknown(_err: unknown): string {
  // Intentionally ignore message body for Finding 13 — only parse controlled codes
  // when the value is an Error whose message embeds LINE_IMPORT:CODE.
  if (_err instanceof Error) {
    const code = extractLineImportCode(_err.message);
    return sterileLineImportMessage(code);
  }
  if (typeof _err === "string") {
    return sterileLineImportMessage(extractLineImportCode(_err));
  }
  return LINE_IMPORT_GENERIC;
}
