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

function isPartialOpenPrefix(raw: string, pos: number): boolean {
  const fragment = raw.slice(pos);
  if (!fragment || fragment.length >= REDACT_OPEN_TAG.length) return false;
  return fragment.toLowerCase() === REDACT_OPEN_TAG.slice(0, fragment.length).toLowerCase();
}

/** Hidden-reasoning blocks are removed before any AI/feedback text reaches the DOM. */
export function redactHiddenReasoning(raw: string, streaming = false): string {
  const out: string[] = [];
  let depth = 0;
  let i = 0;
  while (i < raw.length) {
    const openFull = matchRedactTag(raw, i, REDACT_OPEN_TAG);
    if (openFull === REDACT_OPEN_TAG.length) {
      depth += 1;
      i += REDACT_OPEN_TAG.length;
      continue;
    }
    const closeFull = matchRedactTag(raw, i, REDACT_CLOSE_TAG);
    if (closeFull === REDACT_CLOSE_TAG.length) {
      if (depth > 0) depth -= 1;
      i += REDACT_CLOSE_TAG.length;
      continue;
    }
    if (depth === 0) {
      if (streaming && isPartialOpenPrefix(raw, i)) break;
      out.push(raw[i]);
    }
    i += 1;
  }
  return out.join("");
}
