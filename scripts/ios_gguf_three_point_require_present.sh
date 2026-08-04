#!/usr/bin/env bash
# T4-D: Fail-closed wrapper around unmodified gguf_three_point_sha_gate.sh.
# Requires exact SOURCE/STAGE/ARCHIVE OK lines.
#
# Fixture seam (do not mv/destroy real stage):
#   GGUF_GATE_SCRIPT  — path to a copy of gguf_three_point_sha_gate.sh
#                       whose dirname/../ is the fixture ROOT layout
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
GATE="${GGUF_GATE_SCRIPT:-$ROOT/scripts/gguf_three_point_sha_gate.sh}"
OUT="$(mktemp)"

die() { echo "ios_gguf_require_present: RED: $*" >&2; exit 1; }
[[ -f "$GATE" ]] || die "missing gate script: $GATE"

set +e
bash "$GATE" >"$OUT" 2>&1
EC=$?
set -e
cat "$OUT"
[[ "$EC" -eq 0 ]] || die "underlying gate exit=$EC"

for label in SOURCE STAGE ARCHIVE; do
  grep -E "^${label}: OK " "$OUT" >/dev/null || die "required line missing: '${label}: OK ...'"
done
if grep -E 'ABSENT|MISSING|MISMATCH' "$OUT" >/dev/null; then
  die "ABSENT|MISSING|MISMATCH present in gate output"
fi
echo "ios_gguf_require_present: SOURCE/STAGE/ARCHIVE all OK (fail-closed)"
exit 0
