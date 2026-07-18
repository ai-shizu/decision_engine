/**
 * Extraction persistence seam (M5 Phase 3).
 *
 * Only clipboard preview is implemented. DB / localStorage / IndexedDB are
 * intentionally out of scope (M3).
 */

import type { KakeiboEntryV1 } from "./extractionReducer";

export interface ExtractionSink {
  persist(entry: KakeiboEntryV1): Promise<void>;
}

/** Format the validated entry for clipboard (pretty JSON, values unchanged). */
export function formatEntryForClipboard(entry: KakeiboEntryV1): string {
  return JSON.stringify(entry, null, 2);
}

/**
 * Clipboard-only sink via the Web Clipboard API (no extra npm plugins).
 * Callers must catch rejections and dispatch `copyFailed`.
 */
export function createClipboardExtractionSink(): ExtractionSink {
  return {
    async persist(entry: KakeiboEntryV1): Promise<void> {
      const text = formatEntryForClipboard(entry);
      if (
        typeof navigator === "undefined" ||
        !navigator.clipboard ||
        typeof navigator.clipboard.writeText !== "function"
      ) {
        throw new Error("clipboard API unavailable");
      }
      await navigator.clipboard.writeText(text);
    },
  };
}
