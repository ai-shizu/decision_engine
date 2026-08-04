#!/usr/bin/env bash
# T4-D F-2-e mutation drill: prove the locale regression CAN go RED.
#
# 1) COUNT_LAN_STRINGS_HELPER=grep_mutant → expect F2_REGRESS RED (locale divergence)
# 2) default byte-scan helper → expect F2_REGRESS GREEN
#
# Does not permanently alter production detector. Exit 0 only if both phases behave.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
REGRESS="$ROOT/scripts/ios_s5_grep_diagnostic.sh"
MUTANT="$ROOT/scripts/ios/count_lan_strings_grep_mutant.py"
HELPER="$ROOT/scripts/ios/count_lan_strings.py"

[[ -f "$REGRESS" ]] || { echo "MUTATION: missing $REGRESS" >&2; exit 1; }
[[ -f "$MUTANT" ]] || { echo "MUTATION: missing $MUTANT" >&2; exit 1; }
[[ -f "$HELPER" ]] || { echo "MUTATION: missing $HELPER" >&2; exit 1; }

WORK="$(mktemp -d "${TMPDIR:-/tmp}/t4d-s5-mutation.XXXXXX")"
cleanup() { rm -rf "$WORK"; }
trap cleanup EXIT

echo "=== F-2-e MUTATION DRILL ==="
echo "mutant=$MUTANT"
echo "helper=$HELPER"
echo ""

echo "--- PHASE 1: grep mutant under locale matrix (expect RED) ---"
set +e
COUNT_LAN_STRINGS_HELPER="$MUTANT" bash "$REGRESS" >"$WORK/mutant.log" 2>&1
RC_MUT=$?
set -e
echo "mutant_exit=$RC_MUT (expect non-zero)"
tail -n 40 "$WORK/mutant.log" | sed 's/^/[mutant] /'
if [[ "$RC_MUT" -eq 0 ]]; then
  echo "MUTATION_FAIL: grep mutant did not turn regression RED — permanent-green risk"
  exit 1
fi
echo "MUTATION_OK: grep mutant produced RED (exit=$RC_MUT)"
echo ""

echo "--- PHASE 2: byte-scan helper (expect GREEN) ---"
set +e
unset COUNT_LAN_STRINGS_HELPER
bash "$REGRESS" >"$WORK/bytes.log" 2>&1
RC_OK=$?
set -e
echo "bytescan_exit=$RC_OK (expect 0)"
tail -n 40 "$WORK/bytes.log" | sed 's/^/[bytes] /'
if [[ "$RC_OK" -ne 0 ]]; then
  echo "MUTATION_FAIL: byte-scan regression unexpectedly RED"
  exit 1
fi
echo "MUTATION_OK: byte-scan produced GREEN (exit=0)"
echo ""
echo "F2_MUTATION: GREEN (RED-capable + GREEN-capable proven)"
exit 0
