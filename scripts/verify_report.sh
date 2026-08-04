#!/usr/bin/env bash
# T4-D D-7: Report ↔ evidence integrity verifier (directive §10.2 / R-1 / R-6).
#
# Usage:
#   bash scripts/verify_report.sh <report.md> <evidence_dir>
#
# Exit:
#   0 — REPORT_INTEGRITY: GREEN
#   1 — REPORT_INTEGRITY: RED
#
# Rules:
#   1. Extract every 40-hex / 64-hex / ISO8601 timestamp from the report.
#   2. Each 40-hex: git cat-file -t.
#        - Missing objects → RED, EXCEPT those listed in evidence
#          expected_absent_sha.txt / extracted from nonexistent_claims.txt
#          (must remain absent; if any "expected absent" exists → RED).
#   3. Each 64-hex must appear in evidence receipt.json and/or log_sha256.txt.
#   4. Each ISO8601 timestamp must appear somewhere under evidence_dir.
#
# Grounding: docs/T4D_IOS_ARCHIVE_CI_DIRECTIVE.md §10.2
set -euo pipefail

die() { echo "verify_report: RED: $*" >&2; exit 1; }

[[ $# -eq 2 ]] || die "usage: verify_report.sh <report.md> <evidence_dir>"
REPORT="$1"
EVID="$2"
[[ -f "$REPORT" ]] || die "report missing: $REPORT"
[[ -d "$EVID" ]] || die "evidence dir missing: $EVID"

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

WORK="$(mktemp -d "${TMPDIR:-/tmp}/verify-report.XXXXXX")"
cleanup() { rm -rf "$WORK"; }
trap cleanup EXIT

python3 - <<'PY' "$REPORT" "$WORK"
import re, sys
from pathlib import Path

text = Path(sys.argv[1]).read_text(encoding="utf-8")
out = Path(sys.argv[2])

# 64-hex first so 40-hex extraction does not slice digests
hex64 = sorted(set(re.findall(r"\b[0-9a-f]{64}\b", text)))
hex40_all = sorted(set(re.findall(r"\b[0-9a-f]{40}\b", text)))
# A 64-hex contains 40-hex substrings; do not treat those slices as git objects.
hex40 = sorted(h for h in hex40_all if not any(h in g for g in hex64))
# ISO8601 (date required; time optional; Z or offset)
iso = sorted(set(re.findall(
    r"\b\d{4}-\d{2}-\d{2}(?:[T ]\d{2}:\d{2}:\d{2}(?:\.\d+)?(?:Z|[+-]\d{2}:?\d{2})?)?\b",
    text,
)))

(out / "hex40.txt").write_text("\n".join(hex40) + ("\n" if hex40 else ""), encoding="utf-8")
(out / "hex64.txt").write_text("\n".join(hex64) + ("\n" if hex64 else ""), encoding="utf-8")
(out / "iso8601.txt").write_text("\n".join(iso) + ("\n" if iso else ""), encoding="utf-8")
print("extracted hex40=%d hex64=%d iso8601=%d" % (len(hex40), len(hex64), len(iso)))
PY

# --- build expected-absent set from evidence (R-6: no keyboard) ---
: >"$WORK/expected_absent.txt"
if [[ -f "$EVID/expected_absent_sha.txt" ]]; then
  grep -Eo '[0-9a-f]{40}' "$EVID/expected_absent_sha.txt" >>"$WORK/expected_absent.txt" || true
fi
if [[ -f "$EVID/nonexistent_claims.txt" ]]; then
  grep -Eo '[0-9a-f]{40}' "$EVID/nonexistent_claims.txt" >>"$WORK/expected_absent.txt" || true
fi
sort -u "$WORK/expected_absent.txt" -o "$WORK/expected_absent.txt"

# --- corpus for 64-hex + ISO matching ---
CORPUS="$WORK/corpus.txt"
: >"$CORPUS"
for f in "$EVID/receipt.json" "$EVID/log_sha256.txt"; do
  if [[ -f "$f" ]]; then
    cat "$f" >>"$CORPUS"
    printf '\n' >>"$CORPUS"
  fi
done
# ISO may live in any evidence text file
find "$EVID" -type f -print0 | while IFS= read -r -d '' f; do
  cat "$f" >>"$WORK/evid_all.txt" 2>/dev/null || true
  printf '\n' >>"$WORK/evid_all.txt"
done

RED=0
echo "=== verify_report: object existence (40-hex) ==="
: >"$WORK/missing.txt"
: >"$WORK/present.txt"
: >"$WORK/absent_ok.txt"
: >"$WORK/absent_unexpected_present.txt"

while IFS= read -r sha || [[ -n "${sha:-}" ]]; do
  [[ -z "$sha" ]] && continue
  set +e
  typ="$(git cat-file -t "$sha" 2>"$WORK/cat.err")"
  ec=$?
  set -e
  if [[ "$ec" -eq 0 ]]; then
    echo "PRESENT $sha type=$typ"
    echo "$sha" >>"$WORK/present.txt"
    if grep -qx "$sha" "$WORK/expected_absent.txt"; then
      echo "RED: SHA listed as expected-absent but EXISTS in object store: $sha"
      echo "$sha" >>"$WORK/absent_unexpected_present.txt"
      RED=1
    fi
  else
    if grep -qx "$sha" "$WORK/expected_absent.txt"; then
      echo "ABSENT_OK (documented nonexistent) $sha"
      echo "$sha" >>"$WORK/absent_ok.txt"
    else
      echo "MISSING $sha"
      echo "$sha" >>"$WORK/missing.txt"
      RED=1
    fi
  fi
done <"$WORK/hex40.txt"

echo "=== verify_report: digest presence (64-hex) ==="
if [[ ! -s "$CORPUS" ]]; then
  if [[ -s "$WORK/hex64.txt" ]]; then
    echo "RED: report has 64-hex digests but evidence lacks receipt.json and log_sha256.txt"
    RED=1
  fi
else
  while IFS= read -r dig || [[ -n "${dig:-}" ]]; do
    [[ -z "$dig" ]] && continue
    if grep -Fq "$dig" "$CORPUS"; then
      echo "DIGEST_OK $dig"
    else
      echo "DIGEST_MISSING $dig"
      RED=1
    fi
  done <"$WORK/hex64.txt"
fi

echo "=== verify_report: ISO8601 presence ==="
while IFS= read -r ts || [[ -n "${ts:-}" ]]; do
  [[ -z "$ts" ]] && continue
  if [[ -f "$WORK/evid_all.txt" ]] && grep -Fq "$ts" "$WORK/evid_all.txt"; then
    echo "TIME_OK $ts"
  else
    echo "TIME_MISSING $ts"
    RED=1
  fi
done <"$WORK/iso8601.txt"

# --- R-7: declared SHA-256 must match re-hash of adjacent code block ---
echo "=== verify_report: content digest binding (R-7) ==="
python3 - <<'PY' "$REPORT" "$EVID" "$WORK/r7.txt"
import hashlib, re, sys
from pathlib import Path

report = Path(sys.argv[1]).read_text(encoding="utf-8")
evid = Path(sys.argv[2])
out = Path(sys.argv[3])
# Pattern: optional heading with path, then SHA-256: `digest`, then fenced block
pat = re.compile(
    r"(?:###\s+`(?P<path>[^`]+)`\s*\n\s*)?"
    r"SHA-256:\s*`(?P<digest>[0-9a-f]{64})`\s*\n\s*"
    r"```(?:text|txt|bash|sh|json|markdown)?\n(?P<body>.*?)```",
    re.S,
)
mismatches = []
checked = 0
for m in pat.finditer(report):
    digest = m.group("digest")
    body = m.group("body")
    path = m.group("path")
    # Normalize: code fence bodies in our reports end with newline included
    body_bytes = body.encode("utf-8")
    got = hashlib.sha256(body_bytes).hexdigest()
    checked += 1
    if got != digest:
        mismatches.append("CONTENT_DIGEST_MISMATCH declared=%s recomputed=%s path=%s" % (
            digest, got, path or "-"))
        continue
    if path:
        p = Path(path)
        # Absolute path or relative to evidence dir / cwd
        candidates = [p, evid / p.name, evid / path]
        existing = next((c for c in candidates if c.is_file()), None)
        if existing is not None:
            file_hash = hashlib.sha256(existing.read_bytes()).hexdigest()
            if file_hash != digest:
                mismatches.append(
                    "CONTENT_DIGEST_MISMATCH file_bytes path=%s file_sha=%s declared=%s" % (
                        existing, file_hash, digest))
out.write_text("\n".join(mismatches) + ("\n" if mismatches else ""), encoding="utf-8")
print("r7_pairs_checked=%d mismatches=%d" % (checked, len(mismatches)))
for line in mismatches:
    print(line)
PY
if [[ -s "$WORK/r7.txt" ]]; then
  RED=1
fi

echo "=== verify_report: summary ==="
echo "missing_undeclared=$(wc -l <"$WORK/missing.txt" | tr -d ' ')"
echo "absent_ok=$(wc -l <"$WORK/absent_ok.txt" | tr -d ' ')"
echo "absent_but_present=$(wc -l <"$WORK/absent_unexpected_present.txt" | tr -d ' ')"

if [[ "$RED" -ne 0 ]]; then
  echo "REPORT_INTEGRITY: RED"
  exit 1
fi
echo "REPORT_INTEGRITY: GREEN"
exit 0
