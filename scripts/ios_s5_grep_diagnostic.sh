#!/usr/bin/env bash
# T4-D F-1: S-5 grep diagnostic matrix (diagnosis only — does NOT modify count_lan_strings).
#
# Builds three inputs (text / macho_utf8 / macho_ascii) and runs five search methods.
# Prints one line per cell:
#   S5_DIAG input=<id> method=<id> hits=<n|NOT_MEASURED> exit=<n>
#
# Exit 0 when all 15 cells were measured (even if hits=0).
# Exit 1 only when a cell is missing / not measured.
#
# Grounding: commander F-1 directive 2026-08-04. LAN_STRING_RE is loaded from
# scripts/ios_archive_scan.sh (do not fork the pattern here).
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
SCAN="$ROOT/scripts/ios_archive_scan.sh"
[[ -f "$SCAN" ]] || { echo "F1_DIAG: missing $SCAN" >&2; exit 1; }

# Load LAN_STRING_RE from the gate script — do not rewrite the pattern (R-6 spirit).
LAN_LINE="$(grep -E '^LAN_STRING_RE=' "$SCAN" | head -1 || true)"
[[ -n "$LAN_LINE" ]] || { echo "F1_DIAG: LAN_STRING_RE assignment not found in $SCAN" >&2; exit 1; }
eval "$LAN_LINE"
[[ -n "${LAN_STRING_RE:-}" ]] || { echo "F1_DIAG: LAN_STRING_RE empty after eval" >&2; exit 1; }

WORK="$(mktemp -d "${TMPDIR:-/tmp}/t4d-s5-diag.XXXXXX")"
cleanup() { rm -rf "$WORK"; }
trap cleanup EXIT

INPUTS=(text macho_utf8 macho_ascii)
METHODS=(grep_E_default grep_E_C grep_F_default grep_F_C python_bytes)
MATRIX_LOG="$WORK/matrix.txt"
: >"$MATRIX_LOG"

echo "=== F-1 S-5 grep diagnostic (not a gate) ==="
echo "scan_script=$SCAN"
echo "LAN_STRING_RE loaded from ios_archive_scan.sh (not forked)"
echo "LAN_STRING_RE=$LAN_STRING_RE"
echo ""

echo "=== ENVIRONMENT (required for reproduction) ==="
echo "--- sw_vers ---"
sw_vers || echo "sw_vers: NOT_MEASURED"
echo "--- grep --version ---"
grep --version 2>&1 || echo "grep --version: NOT_MEASURED"
echo "--- which grep / file ---"
command -v grep || true
file "$(command -v grep)" 2>/dev/null || true
echo "--- locale ---"
locale || echo "locale: NOT_MEASURED"
echo "--- LANG/LC_* from env (explicit ABSENT if none) ---"
LC_ENV="$(env | grep -E '^(LANG|LC_)' || true)"
if [[ -z "$LC_ENV" ]]; then
  echo "LANG/LC_*: 未設定"
else
  printf '%s\n' "$LC_ENV"
fi
echo "--- clang --version ---"
clang --version 2>&1 || echo "clang --version: NOT_MEASURED"
echo ""

# --- build inputs ---
TEXT_FILE="$WORK/text.bin"
printf 'marker 10.0.0.0/8 開発用LAN\n' >"$TEXT_FILE"

cat >"$WORK/macho_utf8.c" <<'EOF'
#include <stdio.h>
static const char planted[] = "marker 10.0.0.0/8 開発用LAN";
int main(void) { puts(planted); return 0; }
EOF
clang -O0 -o "$WORK/macho_utf8.bin" "$WORK/macho_utf8.c"

cat >"$WORK/macho_ascii.c" <<'EOF'
#include <stdio.h>
static const char planted[] = "marker 10.0.0.0/8 devlan";
int main(void) { puts(planted); return 0; }
EOF
clang -O0 -o "$WORK/macho_ascii.bin" "$WORK/macho_ascii.c"

path_for() {
  case "$1" in
    text) echo "$TEXT_FILE" ;;
    macho_utf8) echo "$WORK/macho_utf8.bin" ;;
    macho_ascii) echo "$WORK/macho_ascii.bin" ;;
    *) return 1 ;;
  esac
}

echo "=== INPUT ARTIFACTS ==="
for id in "${INPUTS[@]}"; do
  f="$(path_for "$id")"
  echo "--- input=$id path=$f ---"
  ls -l "$f"
  shasum -a 256 "$f"
  file "$f"
  WIN="$WORK/window_${id}.bin"
  python3 - "$f" "$WIN" <<'PY'
import sys
from pathlib import Path
src, win_path = Path(sys.argv[1]), Path(sys.argv[2])
d = src.read_bytes()
needle = b"10.0.0.0/8"
off = d.find(needle)
print(f"python_find_offset={off}")
print(f"size_bytes={len(d)}")
if off < 0:
    print("xxd_window: ABSENT (needle not found)")
else:
    start = max(0, off - 32)
    end = min(len(d), off + len(needle) + 32)
    win_path.write_bytes(d[start:end])
    print(f"xxd_window_range=[{start},{end}) relative_marker_at={off - start}")
PY
  if [[ -f "$WIN" ]]; then
    echo "xxd (±32B around marker):"
    xxd "$WIN" || true
  fi
  echo ""
done

run_method() {
  local method="$1"
  local file="$2"
  local input_id="$3"
  local hits="" exitc=""

  case "$method" in
    grep_E_default)
      set +e
      hits="$( { grep -aoE "$LAN_STRING_RE" "$file" 2>/dev/null || true; } | wc -l | tr -d ' ')"
      grep -aoE "$LAN_STRING_RE" "$file" >/dev/null 2>&1
      exitc=$?
      set -e
      ;;
    grep_E_C)
      set +e
      hits="$( { LC_ALL=C grep -aoE "$LAN_STRING_RE" "$file" 2>/dev/null || true; } | wc -l | tr -d ' ')"
      LC_ALL=C grep -aoE "$LAN_STRING_RE" "$file" >/dev/null 2>&1
      exitc=$?
      set -e
      ;;
    grep_F_default)
      set +e
      hits="$( { grep -aoF '10.0.0.0/8' "$file" 2>/dev/null || true; } | wc -l | tr -d ' ')"
      grep -aoF '10.0.0.0/8' "$file" >/dev/null 2>&1
      exitc=$?
      set -e
      ;;
    grep_F_C)
      set +e
      hits="$( { LC_ALL=C grep -aoF '10.0.0.0/8' "$file" 2>/dev/null || true; } | wc -l | tr -d ' ')"
      LC_ALL=C grep -aoF '10.0.0.0/8' "$file" >/dev/null 2>&1
      exitc=$?
      set -e
      ;;
    python_bytes)
      set +e
      hits="$(python3 - "$file" <<'PY'
from pathlib import Path
import sys
d = Path(sys.argv[1]).read_bytes()
needle = b"10.0.0.0/8"
n = 0
i = 0
while True:
    j = d.find(needle, i)
    if j < 0:
        break
    n += 1
    i = j + 1
print(n)
PY
)"
      exitc=$?
      set -e
      ;;
    *)
      hits="NOT_MEASURED"
      exitc=1
      ;;
  esac

  if [[ -z "${hits}" ]]; then
    hits="NOT_MEASURED"
  fi
  if [[ -z "${exitc}" ]]; then
    exitc="NOT_MEASURED"
  fi
  local line
  line="$(printf 'S5_DIAG input=%s method=%s hits=%s exit=%s' "$input_id" "$method" "$hits" "$exitc")"
  printf '%s\n' "$line"
  printf '%s\n' "$line" >>"$MATRIX_LOG"
}

echo "=== MATRIX (3 inputs × 5 methods = 15 cells) ==="
for id in "${INPUTS[@]}"; do
  f="$(path_for "$id")"
  for method in "${METHODS[@]}"; do
    run_method "$method" "$f" "$id"
  done
done

echo ""
echo "=== MATRIX COMPLETENESS ==="
MISSING=0
for id in "${INPUTS[@]}"; do
  for method in "${METHODS[@]}"; do
    if ! grep -qE "^S5_DIAG input=${id} method=${method} " "$MATRIX_LOG"; then
      echo "MISSING_CELL input=$id method=$method"
      MISSING=1
      continue
    fi
    line="$(grep -E "^S5_DIAG input=${id} method=${method} " "$MATRIX_LOG" | head -1)"
    if [[ "$line" == *"hits=NOT_MEASURED"* ]]; then
      echo "UNMEASURED_CELL $line"
      MISSING=1
    fi
  done
done

CELL_COUNT="$(grep -cE '^S5_DIAG ' "$MATRIX_LOG" || true)"
echo "cell_count=$CELL_COUNT expected=15"
if [[ "$CELL_COUNT" -ne 15 ]]; then
  echo "MISSING_CELL count=$CELL_COUNT"
  MISSING=1
fi

if [[ "$MISSING" -ne 0 ]]; then
  echo "F1_DIAG: RED incomplete matrix"
  exit 1
fi

echo "F1_DIAG: matrix complete (15/15 measured) — diagnostic job GREEN (not a shipping gate)"
exit 0
