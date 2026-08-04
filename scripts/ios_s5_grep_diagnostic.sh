#!/usr/bin/env bash
# T4-D F-2-e: S-5 locale regression for count_lan_strings (byte-scan detector).
#
# Asserts: for each input class (text / macho / binary_plist), count_lan_strings
# returns the SAME occurrence count under every locale axis:
#   LC_ALL=C / LC_ALL=en_US.UTF-8 / LC_ALL unset (env -u LC_ALL -u LANG)
#
# grep -E / grep -F columns are INFORMATIONAL ONLY (never used for pass/fail).
# A missing cell is NOT_MEASURED (never written as 0).
#
# Exit non-zero if any count_lan_strings cell disagrees across locales.
#
# Grounding: commander F-2 directive 2026-08-04. LAN_STRING_RE from ios_archive_scan.sh only.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
SCAN="$ROOT/scripts/ios_archive_scan.sh"
HELPER="$ROOT/scripts/ios/count_lan_strings.py"
[[ -f "$SCAN" ]] || { echo "F2_REGRESS: missing $SCAN" >&2; exit 1; }
[[ -f "$HELPER" ]] || { echo "F2_REGRESS: missing $HELPER" >&2; exit 1; }

LAN_LINE="$(grep -E '^LAN_STRING_RE=' "$SCAN" | head -1 || true)"
[[ -n "$LAN_LINE" ]] || { echo "F2_REGRESS: LAN_STRING_RE assignment not found in $SCAN" >&2; exit 1; }
eval "$LAN_LINE"
[[ -n "${LAN_STRING_RE:-}" ]] || { echo "F2_REGRESS: LAN_STRING_RE empty after eval" >&2; exit 1; }

WORK="$(mktemp -d "${TMPDIR:-/tmp}/t4d-s5-regress.XXXXXX")"
cleanup() { rm -rf "$WORK"; }
trap cleanup EXIT

INPUTS=(text macho binary_plist)
LOCALES=(C en_US.UTF-8 unset)
MATRIX_LOG="$WORK/matrix.txt"
: >"$MATRIX_LOG"

echo "=== F-2-e S-5 locale regression (count_lan_strings assert; grep informational) ==="
echo "scan_script=$SCAN"
echo "helper=$HELPER"
echo "LAN_STRING_RE loaded from ios_archive_scan.sh (not forked)"
echo "LAN_STRING_RE=$LAN_STRING_RE"
echo ""

echo "=== ENVIRONMENT ==="
echo "--- sw_vers ---"
sw_vers || echo "sw_vers: NOT_MEASURED"
echo "--- grep --version ---"
grep --version 2>&1 || echo "grep --version: NOT_MEASURED"
echo "--- which grep / file ---"
command -v grep || true
file "$(command -v grep)" 2>/dev/null || true
echo "--- locale (host) ---"
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
echo "--- python3 ---"
python3 --version 2>&1 || echo "python3: NOT_MEASURED"
echo ""

# --- build inputs (3 production-relevant classes) ---
TEXT_FILE="$WORK/text.bin"
printf 'marker 10.0.0.0/8 開発用LAN\n' >"$TEXT_FILE"

cat >"$WORK/macho.c" <<'EOF'
#include <stdio.h>
static const char planted[] = "marker 10.0.0.0/8 開発用LAN";
int main(void) { puts(planted); return 0; }
EOF
clang -O0 -o "$WORK/macho.bin" "$WORK/macho.c"

cat >"$WORK/info.xml.plist" <<'EOF'
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
	<key>CFBundleIdentifier</key>
	<string>regress.lan</string>
	<key>PlantedLANMarker</key>
	<string>marker 10.0.0.0/8 http://192.168.1.1:1420</string>
</dict>
</plist>
EOF
plutil -convert binary1 -o "$WORK/info.binary.plist" "$WORK/info.xml.plist"
# Guard: CJK in the string forces UTF-16 in bplist00 and would make ASCII CIDR invisible.
if ! python3 -c "import sys; d=open(sys.argv[1],'rb').read(); sys.exit(0 if b'10.0.0.0/8' in d else 1)" \
    "$WORK/info.binary.plist"; then
  echo "F2_REGRESS: binary_plist input lacks ASCII 10.0.0.0/8 (UTF-16 elision)" >&2
  exit 1
fi

path_for() {
  case "$1" in
    text) echo "$TEXT_FILE" ;;
    macho) echo "$WORK/macho.bin" ;;
    binary_plist) echo "$WORK/info.binary.plist" ;;
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
  python3 - "$f" <<'PY'
import sys
from pathlib import Path
d = Path(sys.argv[1]).read_bytes()
needle = b"10.0.0.0/8"
off = d.find(needle)
print(f"python_find_offset={off}")
print(f"size_bytes={len(d)}")
PY
  echo ""
done

# Informational grep (never assert)
info_grep() {
  local mode="$1"  # E|F
  local loc="$2"   # C|en_US.UTF-8|unset
  local file="$3"
  local hits="NOT_MEASURED" exitc="NOT_MEASURED"
  set +e
  case "$loc:$mode" in
    C:E)
      hits="$( { LC_ALL=C grep -aoE "$LAN_STRING_RE" "$file" 2>/dev/null || true; } | wc -l | tr -d ' ')"
      LC_ALL=C grep -aoE "$LAN_STRING_RE" "$file" >/dev/null 2>&1
      exitc=$?
      ;;
    C:F)
      hits="$( { LC_ALL=C grep -aoF '10.0.0.0/8' "$file" 2>/dev/null || true; } | wc -l | tr -d ' ')"
      LC_ALL=C grep -aoF '10.0.0.0/8' "$file" >/dev/null 2>&1
      exitc=$?
      ;;
    en_US.UTF-8:E)
      hits="$( { LC_ALL=en_US.UTF-8 grep -aoE "$LAN_STRING_RE" "$file" 2>/dev/null || true; } | wc -l | tr -d ' ')"
      LC_ALL=en_US.UTF-8 grep -aoE "$LAN_STRING_RE" "$file" >/dev/null 2>&1
      exitc=$?
      ;;
    en_US.UTF-8:F)
      hits="$( { LC_ALL=en_US.UTF-8 grep -aoF '10.0.0.0/8' "$file" 2>/dev/null || true; } | wc -l | tr -d ' ')"
      LC_ALL=en_US.UTF-8 grep -aoF '10.0.0.0/8' "$file" >/dev/null 2>&1
      exitc=$?
      ;;
    unset:E)
      hits="$( { env -u LC_ALL -u LANG grep -aoE "$LAN_STRING_RE" "$file" 2>/dev/null || true; } | wc -l | tr -d ' ')"
      env -u LC_ALL -u LANG grep -aoE "$LAN_STRING_RE" "$file" >/dev/null 2>&1
      exitc=$?
      ;;
    unset:F)
      hits="$( { env -u LC_ALL -u LANG grep -aoF '10.0.0.0/8' "$file" 2>/dev/null || true; } | wc -l | tr -d ' ')"
      env -u LC_ALL -u LANG grep -aoF '10.0.0.0/8' "$file" >/dev/null 2>&1
      exitc=$?
      ;;
  esac
  set -e
  if [[ -z "$hits" ]]; then hits="NOT_MEASURED"; fi
  if [[ -z "$exitc" ]]; then exitc="NOT_MEASURED"; fi
  printf 'S5_INFO input=%s locale=%s method=grep_%s hits=%s exit=%s\n' \
    "$4" "$loc" "$mode" "$hits" "$exitc"
}

# Asserted detector under a locale
run_detector() {
  local loc="$1"
  local file="$2"
  local input_id="$3"
  local hits="NOT_MEASURED" exitc="NOT_MEASURED"
  set +e
  case "$loc" in
    C)
      hits="$(LC_ALL=C bash "$SCAN" --count-lan-strings "$file" 2>/dev/null)"
      exitc=$?
      ;;
    en_US.UTF-8)
      hits="$(LC_ALL=en_US.UTF-8 bash "$SCAN" --count-lan-strings "$file" 2>/dev/null)"
      exitc=$?
      ;;
    unset)
      hits="$(env -u LC_ALL -u LANG bash "$SCAN" --count-lan-strings "$file" 2>/dev/null)"
      exitc=$?
      ;;
  esac
  set -e
  # Strip trailing whitespace / non-digit noise; empty → NOT_MEASURED
  hits="$(printf '%s' "$hits" | tr -d ' \t\r\n')"
  if [[ -z "$hits" || ! "$hits" =~ ^[0-9]+$ ]]; then
    hits="NOT_MEASURED"
  fi
  if [[ -z "$exitc" ]]; then
    exitc="NOT_MEASURED"
  fi
  local line
  line="$(printf 'S5_ASSERT input=%s locale=%s method=count_lan_strings hits=%s exit=%s' \
    "$input_id" "$loc" "$hits" "$exitc")"
  printf '%s\n' "$line"
  printf '%s\n' "$line" >>"$MATRIX_LOG"
}

echo "=== MATRIX (3 inputs × 3 locales) — count_lan_strings ASSERTED; grep INFORMATIONAL ==="
for id in "${INPUTS[@]}"; do
  f="$(path_for "$id")"
  for loc in "${LOCALES[@]}"; do
    run_detector "$loc" "$f" "$id"
    info_grep E "$loc" "$f" "$id"
    info_grep F "$loc" "$f" "$id"
  done
done

echo ""
echo "=== ASSERT: count_lan_strings identical across locales per input ==="
FAIL=0
for id in "${INPUTS[@]}"; do
  vals=()
  for loc in "${LOCALES[@]}"; do
    line="$(grep -E "^S5_ASSERT input=${id} locale=${loc} " "$MATRIX_LOG" | head -1 || true)"
    if [[ -z "$line" ]]; then
      echo "MISSING_CELL input=$id locale=$loc"
      FAIL=1
      continue
    fi
    hits="$(printf '%s\n' "$line" | sed -n 's/.* hits=\([^ ]*\).*/\1/p')"
    if [[ "$hits" == "NOT_MEASURED" ]]; then
      echo "UNMEASURED_CELL $line"
      FAIL=1
      continue
    fi
    vals+=("$hits")
    echo "measured input=$id locale=$loc hits=$hits"
  done
  if [[ "${#vals[@]}" -eq 3 ]]; then
    if [[ "${vals[0]}" != "${vals[1]}" || "${vals[1]}" != "${vals[2]}" ]]; then
      echo "ASSERT_FAIL input=$id count_lan_strings diverged: C=${vals[0]} en_US.UTF-8=${vals[1]} unset=${vals[2]}"
      FAIL=1
    else
      if [[ "${vals[0]}" -lt 1 ]]; then
        echo "ASSERT_FAIL input=$id count_lan_strings=${vals[0]} (planted marker must be ≥1)"
        FAIL=1
      else
        echo "ASSERT_OK input=$id count_lan_strings=${vals[0]} (identical across 3 locales)"
      fi
    fi
  fi
done

CELL_COUNT="$(grep -cE '^S5_ASSERT ' "$MATRIX_LOG" || true)"
echo "assert_cell_count=$CELL_COUNT expected=9"
if [[ "$CELL_COUNT" -ne 9 ]]; then
  echo "MISSING_CELL count=$CELL_COUNT"
  FAIL=1
fi

if [[ "$FAIL" -ne 0 ]]; then
  echo "F2_REGRESS: RED count_lan_strings locale divergence or incomplete matrix"
  exit 1
fi

echo "F2_REGRESS: GREEN count_lan_strings stable across 3 locales × 3 inputs (grep columns informational only)"
exit 0
