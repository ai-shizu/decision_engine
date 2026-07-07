import type { KeyboardEvent as ReactKeyboardEvent } from "react";

/**
 * W-25 (SPEC_FOXTROT_UI.md §2.1.1): 日本語 IME の変換確定 Enter を
 * QuickAdd 発火と誤認しない。isComposing が主判定、keyCode 229 は
 * WebView/OS 差異への保険。単一行 input の onKeyDown からのみ呼ぶこと
 * (diary の textarea には適用しない — Enter は改行が正)。
 */
export function isCommitEnter(e: ReactKeyboardEvent): boolean {
  return e.key === "Enter" && !e.shiftKey && !e.nativeEvent.isComposing && e.keyCode !== 229;
}
