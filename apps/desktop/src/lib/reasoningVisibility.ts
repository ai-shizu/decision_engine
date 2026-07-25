//! Interview / CONSULT reasoning visibility (pure — no React).
//! Tag matching mirrors `redactHiddenReasoning.ts` (do not diverge).

import { redactHiddenReasoning } from "./redactHiddenReasoning";

export type ReasoningMode = "hidden" | "revealed" | "live";

const REDACT_OPEN_TAG = "<think>";
const REDACT_CLOSE_TAG = "</think>";

function matchRedactTag(raw: string, pos: number, tag: string): number {
  const remain = raw.length - pos;
  if (remain <= 0) return -1;
  const n = Math.min(tag.length, remain);
  for (let i = 0; i < n; i++) {
    if (raw[pos + i].toLowerCase() !== tag[i].toLowerCase()) return -1;
  }
  if (n < tag.length) return 0;
  return tag.length;
}

/** `<think>…</think>` の中身だけを順に返す（redactHiddenReasoning の逆関数）。 */
export function extractHiddenReasoning(raw: string): string[] {
  const blocks: string[] = [];
  let depth = 0;
  let i = 0;
  let buf: string[] = [];
  while (i < raw.length) {
    const openFull = matchRedactTag(raw, i, REDACT_OPEN_TAG);
    if (openFull === REDACT_OPEN_TAG.length) {
      depth += 1;
      i += REDACT_OPEN_TAG.length;
      continue;
    }
    const closeFull = matchRedactTag(raw, i, REDACT_CLOSE_TAG);
    if (closeFull === REDACT_CLOSE_TAG.length) {
      if (depth > 0) {
        depth -= 1;
        if (depth === 0) {
          blocks.push(buf.join(""));
          buf = [];
        }
      }
      i += REDACT_CLOSE_TAG.length;
      continue;
    }
    if (depth > 0) {
      buf.push(raw[i]);
    }
    i += 1;
  }
  // Unclosed: take remainder through end (same rule as redact).
  if (depth > 0) {
    blocks.push(buf.join(""));
  }
  return blocks;
}

/** モードに応じた本文表示。hidden/live/revealed とも本文は redact 結果（タグは混ぜない）。 */
export function visibleBody(
  raw: string,
  mode: ReasoningMode,
  streaming: boolean,
): string {
  void mode;
  return redactHiddenReasoning(raw, streaming);
}

/** 面接ステージ → モード。debrief / closed で開示。1on1 は null,null → hidden。 */
export function interviewReasoningMode(
  stage: string | null,
  outcome: string | null,
): ReasoningMode {
  if (stage === "debrief" || outcome === "closed") return "revealed";
  return "hidden";
}
