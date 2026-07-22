//! GD SPEAK-DSL stream parser (Inner Coliseum multi-agent GD). Pure & TOTAL:
//! never throws, always returns >=0 transcript bubbles, even when the 1.5B
//! model ignores the output format entirely. Re-parses the whole accumulated
//! buffer per token (bounded by max_tokens, so this stays cheap) rather than
//! tracking split-token parser state across calls.
//!
//! Tiered recovery (strict prompt, lenient parser — never symmetric):
//!   1. STRICT   `@A>` (fullwidth ＠Ａ＞ tolerated)   — model behaving.
//!   2. LENIENT  `[A]` / `A:` / `A：`                 — only if zero strict hits.
//!   3. COLLAPSE the whole buffer into one bubble     — zero headers at all.

/**
 * GD-relevant subset of TranscriptStream's role union. Kept as a local, pure
 * mirror (not imported from the .tsx component) so this file stays JSX-free —
 * matches the repo's lib/*.ts vs components/*.tsx boundary. Structurally
 * identical to `TranscriptRole`, so it is assignable wherever that is expected.
 */
export type GdTranscriptRole =
  | "PARTICIPANT_A"
  | "PARTICIPANT_B"
  | "PARTICIPANT_C"
  | "PARTICIPANT_D"
  | "PARTICIPANT_E"
  | "USER"
  | "FACILITATOR";

export type GdTranscriptStage =
  | "FOUNDATION"
  | "PRESSURE"
  | "DISCUSSION"
  | "DEBRIEF"
  | "CLOSED";

export interface GdTranscriptMessage {
  turnId: string;
  role: GdTranscriptRole;
  stage: GdTranscriptStage;
  text: string;
}

const ROLE_BY_LETTER: Record<string, GdTranscriptRole> = {
  A: "PARTICIPANT_A",
  B: "PARTICIPANT_B",
  C: "PARTICIPANT_C",
  D: "PARTICIPANT_D",
  E: "PARTICIPANT_E",
};

// Line-anchored; horizontal whitespace only ([ \t　]) so a header can never
// eat the previous line's newline. Fullwidth @／＞ tolerated (small-model
// output drifts between ASCII and fullwidth punctuation). The letter class is
// intentionally A-Z, not just A-E: a 1.5B model can hallucinate a letter
// outside the configured roster (e.g. `@F>` with only 4 agents in play), and
// that must still split into its own header — only role *attribution* falls
// back to the previous speaker; header *detection* must not miss it.
const STRICT_RE = /(?:^|\n)[ \t　]*[@＠][ \t　]*([A-Za-z])[ \t　]*[>＞]/g;
const LENIENT_RE = /(?:^|\n)[ \t　]*\[?[ \t　]*([A-Za-z])[ \t　]*[>＞\]:：]/g;

export interface ParseGdOptions {
  /** Letters currently in play (see `gdValidLetters`); others fall back. */
  validLetters: string[];
  /** Stage stamped on every bubble (timer-driven, not model-driven). */
  stage?: GdTranscriptStage;
  /** turnId prefix; keep stable across a round for React reconciliation. */
  idPrefix?: string;
  /** Role for unattributable text (default: first valid participant). */
  fallbackRole?: GdTranscriptRole;
}

interface Header {
  at: number;
  bodyAt: number;
  letter: string;
}

function collectHeaders(re: RegExp, raw: string): Header[] {
  const out: Header[] = [];
  re.lastIndex = 0;
  let m: RegExpExecArray | null;
  while ((m = re.exec(raw)) !== null) {
    out.push({ at: m.index, bodyAt: re.lastIndex, letter: m[1].toUpperCase() });
    if (re.lastIndex === m.index) re.lastIndex += 1; // zero-width guard
  }
  return out;
}

function resolveFallbackRole(options: ParseGdOptions): GdTranscriptRole {
  if (options.fallbackRole) return options.fallbackRole;
  const first = options.validLetters[0]?.toUpperCase();
  return (first && ROLE_BY_LETTER[first]) || "PARTICIPANT_A";
}

/** Parse an accumulated GD model buffer into per-speaker transcript bubbles. */
export function parseGdStream(
  raw: string,
  options: ParseGdOptions,
): GdTranscriptMessage[] {
  const stage: GdTranscriptStage = options.stage ?? "DISCUSSION";
  const prefix = options.idPrefix ?? "gd";
  const fallbackRole = resolveFallbackRole(options);

  try {
    const valid = new Set(options.validLetters.map((l) => l.toUpperCase()));

    let headers = collectHeaders(STRICT_RE, raw);
    if (headers.length === 0) headers = collectHeaders(LENIENT_RE, raw);

    // Tier 3: total format collapse — surface the raw output, never blank.
    if (headers.length === 0) {
      const body = raw.trim();
      return body
        ? [{ turnId: `${prefix}-0`, role: fallbackRole, stage, text: body }]
        : [];
    }

    const messages: GdTranscriptMessage[] = [];
    let prevRole: GdTranscriptRole = fallbackRole;
    for (let i = 0; i < headers.length; i += 1) {
      const end = i + 1 < headers.length ? headers[i + 1].at : raw.length;
      const text = raw.slice(headers[i].bodyAt, end).trim();
      const letter = headers[i].letter;
      const role: GdTranscriptRole = valid.has(letter)
        ? ROLE_BY_LETTER[letter]
        : prevRole; // unknown letter → previous speaker, never crash
      prevRole = role;
      // Drop an empty non-final header (its body just hasn't streamed in yet);
      // keep the last one even if empty so its bubble appears and grows live.
      if (!text && i < headers.length - 1) continue;
      messages.push({ turnId: `${prefix}-${messages.length}`, role, stage, text });
    }
    return messages;
  } catch {
    // Absolute failsafe: the parser must never break the stream.
    const body = raw.trim();
    return body
      ? [{ turnId: `${prefix}-0`, role: fallbackRole, stage, text: body }]
      : [];
  }
}
