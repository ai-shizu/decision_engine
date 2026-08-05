#!/usr/bin/env bash
# T4-F-3a J-2 / J-3 / J-4 / J-5b — mutation drills for xctrace attribution apparatus.
#
# 1) Control fail → no measurement_summary.txt
# 2) Sabotage gate → measurement artifact PRESENT
# 3) Restore → fail again ABSENT
# 4) Watchdog HUNG → exit 3, never reported as 0
# 5) Parse fail (fail-soft) → exit non-zero, NOT PARSEABLE, no silent death
# 6) No operational 2>/dev/null in the apparatus
# 7) After runs, no leftover `xctrace record` (J-4)
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
TARGET="$ROOT/scripts/t4f_xctrace_attribution.sh"
LOG_DIR="${T4F_LOG_DIR:-/tmp/t4f-evidence/xctrace-mutations}"
EVID="${T4F_EVIDENCE_DIR:-/tmp/t4f-evidence/xctrace-drill}"
mkdir -p "$LOG_DIR" "$EVID"

sha_of() { shasum -a 256 "$1" | awk '{print $1}'; }
[[ -f "$TARGET" ]] || { echo "missing $TARGET" >&2; exit 1; }

BACKUP="$LOG_DIR/t4f_xctrace_attribution.sh.bak"
cp "$TARGET" "$BACKUP"
BEFORE="$(sha_of "$TARGET")"
echo "DRILL[t4f_xctrace] before_sha=$BEFORE"

# --- J-5b: forbid 2>/dev/null in operational code (comments mentioning the ban OK) ---
if grep -nE '[^#[:space:]].*2>/dev/null|2>/dev/null' "$TARGET" | grep -vE '^\s*[0-9]+:\s*#' | grep -v 'Never 2>/dev/null'; then
  echo "DRILL[t4f_xctrace]: FAIL operational 2>/dev/null present" >&2
  exit 1
fi
echo "DRILL step0 OK: no operational 2>/dev/null"

run_h() {
  local label="$1"
  shift
  local out="$LOG_DIR/xctrace_${label}.log"
  rm -rf "$EVID"
  mkdir -p "$EVID"
  set +e
  env T4F_HARNESS=1 \
      T4F_EVIDENCE_DIR="$EVID" \
      T4F_UDID=00008150-000269103CF0401C \
      T4F_RECORD_SECONDS=1 \
      T4F_WATCHDOG_SECONDS=3 \
      "$@" \
      bash "$TARGET" >"$out" 2>&1
  local ec=$?
  set -e
  echo "DRILL run label=$label exit=$ec"
  local sessions
  sessions="$(ls -d "$EVID"/session-* 2>/dev/null | head -n1 || true)"
  if [[ -n "$sessions" && -f "$sessions/measurement_summary.txt" ]]; then
    echo "PRESENT" >"$LOG_DIR/art_${label}.txt"
    echo "measurement_artifact=PRESENT"
  else
    echo "ABSENT" >"$LOG_DIR/art_${label}.txt"
    echo "measurement_artifact=ABSENT"
  fi
  echo "$ec" >"$LOG_DIR/exit_${label}.txt"
  # J-4: no leftover recorder
  local left
  left="$( { pgrep -f 'xctrace[[:space:]]+record' || true; } 2>>"$LOG_DIR/pgrep.err" | grep -E '^[0-9]+$' || true)"
  if [[ -n "$left" ]]; then
    echo "DRILL[t4f_xctrace]: FAIL leftover xctrace record: $left" >&2
    exit 1
  fi
}

# --- J-2 step1: intact fail ---
run_h intact_fail T4F_HARNESS_CONTROL=fail T4F_HARNESS_PARSE=ok T4F_SKIP_MEASURE=0
[[ "$(cat "$LOG_DIR/exit_intact_fail.txt")" -ne 0 ]] || exit 1
[[ "$(cat "$LOG_DIR/art_intact_fail.txt")" == "ABSENT" ]] || exit 1
echo "DRILL step1 OK: control fail → artifact ABSENT"

# --- J-2 step2: sabotage gate ---
python3 - "$TARGET" <<'PY'
from pathlib import Path
import sys
p = Path(sys.argv[1])
text = p.read_text(encoding="utf-8")
needle = 'if [[ "$CONTROL_OK" -ne 1 ]]; then'
replacement = '''if [[ "$CONTROL_OK" -ne 1 ]]; then
  log "MUTATION_PLANT: ignoring xctrace positive-control failure"
  CONTROL_OK=1
fi
if [[ "0" -eq 1 ]]; then'''
if needle not in text:
    raise SystemExit("gate needle not found")
p.write_text(text.replace(needle, replacement, 1), encoding="utf-8")
print("planted control-fail bypass")
PY

run_h mutated_fail T4F_HARNESS_CONTROL=fail T4F_HARNESS_PARSE=ok T4F_SKIP_MEASURE=0
[[ "$(cat "$LOG_DIR/art_mutated_fail.txt")" == "PRESENT" ]] || {
  cp "$BACKUP" "$TARGET"
  echo "DRILL[t4f_xctrace]: FAIL mutation did not reach measurement" >&2
  exit 1
}
echo "DRILL step2 OK: sabotaged gate → PRESENT"

cp "$BACKUP" "$TARGET"
[[ "$(sha_of "$TARGET")" == "$BEFORE" ]] || exit 1

run_h restored_fail T4F_HARNESS_CONTROL=fail T4F_HARNESS_PARSE=ok
[[ "$(cat "$LOG_DIR/art_restored_fail.txt")" == "ABSENT" ]] || exit 1
echo "DRILL step3 OK: restore re-armed"

# Pass path reaches measurement
run_h intact_pass T4F_HARNESS_CONTROL=pass T4F_HARNESS_PARSE=ok T4F_SKIP_MEASURE=0
[[ "$(cat "$LOG_DIR/exit_intact_pass.txt")" -eq 0 ]] || exit 1
[[ "$(cat "$LOG_DIR/art_intact_pass.txt")" == "PRESENT" ]] || exit 1
# Pass measurement should be numeric 0 in harness (Coraxis quiet), not HUNG/NOT_PARSEABLE
PASS_SESS="$(ls -d "$EVID"/session-* | head -n1)"
grep -q 'app_owned_connection_events=0' "$PASS_SESS/measurement_summary.txt"
echo "DRILL step4 OK: pass path measures (harness events=0 is ok AFTER control)"

# --- J-3: hang watchdog ---
run_h hung_control T4F_HARNESS_CONTROL=pass T4F_HARNESS_HUNG=1 T4F_HARNESS_PARSE=ok
HUNG_EC="$(cat "$LOG_DIR/exit_hung_control.txt")"
[[ "$HUNG_EC" -eq 3 ]] || {
  echo "DRILL[t4f_xctrace]: FAIL expected exit 3 for HUNG, got $HUNG_EC" >&2
  exit 1
}
[[ "$(cat "$LOG_DIR/art_hung_control.txt")" == "ABSENT" ]] || {
  echo "DRILL[t4f_xctrace]: FAIL hung control must not produce measurement summary as success" >&2
  # Note: hung() may write measurement_summary with HUNG only on measure phase;
  # control-phase hung exits before measurement=STARTED content — ABSENT expected.
  exit 1
}
grep -q 'HUNG' "$LOG_DIR/xctrace_hung_control.log"
echo "DRILL step5 OK: watchdog HUNG exit=3"

# --- J-5b: parse fail does not silent-die; reports NOT PARSEABLE ---
run_h parse_fail T4F_HARNESS_CONTROL=pass T4F_HARNESS_PARSE=fail T4F_SKIP_MEASURE=0
PF_EC="$(cat "$LOG_DIR/exit_parse_fail.txt")"
[[ "$PF_EC" -ne 0 ]] || exit 1
[[ "$(cat "$LOG_DIR/art_parse_fail.txt")" == "ABSENT" ]] || exit 1
grep -q 'NOT_PARSEABLE\|POSITIVE_CONTROL: FAIL' "$LOG_DIR/xctrace_parse_fail.log"
# Confirm ABNORMAL EXIT trap text appears on unexpected die paths is optional;
# parse-fail on control is intentional gate (CLEAN_EXIT). Also verify stderr logs exist pattern.
echo "DRILL step6 OK: parse fail → non-zero, no measurement claim"

# Leftover check once more
left="$( { pgrep -f 'xctrace[[:space:]]+record' || true; } 2>>"$LOG_DIR/pgrep.err" | grep -E '^[0-9]+$' || true)"
[[ -z "$left" ]] || exit 1

echo "DRILL[t4f_xctrace]: PASS"
exit 0
