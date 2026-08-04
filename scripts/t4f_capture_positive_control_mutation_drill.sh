#!/usr/bin/env bash
# T4-F-1 I-2 — Mutation drill: positive-control failure must block measurement.
#
# 1) Harness CONTROL=fail → exit 1, no measurement_summary.txt
# 2) Sabotage the gate (proceed despite control fail) → measurement artifact appears
# 3) Restore → CONTROL=fail again refuses measurement
#
# Proves the gate is load-bearing (same discipline as arena_dead_reason_mutation_drill).
# Does NOT require sudo, rvi0, or a physical device (C-3).
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
TARGET="$ROOT/scripts/t4f_capture_session.sh"
LOG_DIR="${T4F_LOG_DIR:-/tmp/t4f-evidence/mutations}"
EVID="${T4F_EVIDENCE_DIR:-/tmp/t4f-evidence/capture-drill}"
mkdir -p "$LOG_DIR" "$EVID"

sha_of() { shasum -a 256 "$1" | awk '{print $1}'; }

[[ -f "$TARGET" ]] || { echo "missing $TARGET" >&2; exit 1; }

BACKUP="$LOG_DIR/t4f_capture_session.sh.bak"
cp "$TARGET" "$BACKUP"
BEFORE="$(sha_of "$TARGET")"
echo "DRILL[t4f_capture_positive_control] before_sha=$BEFORE"

run_harness() {
  local label="$1"
  local control="$2"
  local out="$LOG_DIR/capture_${label}.log"
  rm -rf "$EVID"
  mkdir -p "$EVID"
  set +e
  env T4F_HARNESS=1 \
      T4F_HARNESS_CONTROL="$control" \
      T4F_EVIDENCE_DIR="$EVID" \
      T4F_TEST=A \
      T4F_SKIP_MEASURE=0 \
      bash "$TARGET" >"$out" 2>&1
  local ec=$?
  set -e
  echo "DRILL run label=$label control=$control exit=$ec"
  # shellcheck disable=SC2012
  local sessions
  sessions="$(ls -d "$EVID"/session-* 2>/dev/null | head -n1 || true)"
  echo "session_dir=$sessions"
  if [[ -n "$sessions" && -f "$sessions/measurement_summary.txt" ]]; then
    echo "measurement_artifact=PRESENT"
    echo "PRESENT" >"$LOG_DIR/artifact_${label}.txt"
  else
    echo "measurement_artifact=ABSENT"
    echo "ABSENT" >"$LOG_DIR/artifact_${label}.txt"
  fi
  echo "$ec" >"$LOG_DIR/exit_${label}.txt"
  return 0
}

# --- Step 1: intact gate + failing control → must refuse measurement ---
run_harness "intact_fail" "fail"
INTACT_EC="$(cat "$LOG_DIR/exit_intact_fail.txt")"
INTACT_ART="$(cat "$LOG_DIR/artifact_intact_fail.txt")"
if [[ "$INTACT_EC" -eq 0 ]]; then
  echo "DRILL[t4f_capture_positive_control]: FAIL intact gate accepted failed control" >&2
  exit 1
fi
if [[ "$INTACT_ART" != "ABSENT" ]]; then
  echo "DRILL[t4f_capture_positive_control]: FAIL measurement ran despite control fail" >&2
  exit 1
fi
echo "DRILL step1 OK: control fail → exit=$INTACT_EC artifact=ABSENT"

# --- Step 2: sabotage gate — treat control fail as continue ---
python3 - "$TARGET" <<'PY'
from pathlib import Path
import sys
p = Path(sys.argv[1])
text = p.read_text(encoding="utf-8")
needle = 'if [[ "$CONTROL_OK" -ne 1 ]]; then'
# Make the gate a no-op continue (mutation plant).
replacement = '''if [[ "$CONTROL_OK" -ne 1 ]]; then
  log "MUTATION_PLANT: ignoring positive-control failure (drill sabotage)"
  CONTROL_OK=1
fi
if [[ "0" -eq 1 ]]; then'''
if needle not in text:
    raise SystemExit("gate needle not found — drill cannot plant")
# Only replace the Phase-2 gate (first occurrence after positive control).
text2 = text.replace(needle, replacement, 1)
if text2 == text:
    raise SystemExit("plant failed")
p.write_text(text2, encoding="utf-8")
print("planted: control-fail continues to measurement")
PY

run_harness "mutated_fail" "fail"
MUT_EC="$(cat "$LOG_DIR/exit_mutated_fail.txt")"
MUT_ART="$(cat "$LOG_DIR/artifact_mutated_fail.txt")"
# Sabotaged script should reach measurement despite control=fail.
if [[ "$MUT_ART" != "PRESENT" ]]; then
  cp "$BACKUP" "$TARGET"
  echo "DRILL[t4f_capture_positive_control]: FAIL mutation did not reach measurement" >&2
  exit 1
fi
echo "DRILL step2 OK: sabotaged gate → artifact=PRESENT (exit=$MUT_EC) — proves gate is load-bearing"

# --- Step 3: restore + failing control again refuses ---
cp "$BACKUP" "$TARGET"
AFTER="$(sha_of "$TARGET")"
[[ "$BEFORE" == "$AFTER" ]] || {
  echo "DRILL[t4f_capture_positive_control]: FAIL restore mismatch" >&2
  exit 1
}

run_harness "restored_fail" "fail"
REST_EC="$(cat "$LOG_DIR/exit_restored_fail.txt")"
REST_ART="$(cat "$LOG_DIR/artifact_restored_fail.txt")"
if [[ "$REST_EC" -eq 0 || "$REST_ART" != "ABSENT" ]]; then
  echo "DRILL[t4f_capture_positive_control]: FAIL restore did not re-arm gate" >&2
  exit 1
fi

# Bonus: pass path reaches measurement (ordering sanity)
run_harness "intact_pass" "pass"
PASS_EC="$(cat "$LOG_DIR/exit_intact_pass.txt")"
PASS_ART="$(cat "$LOG_DIR/artifact_intact_pass.txt")"
if [[ "$PASS_EC" -ne 0 || "$PASS_ART" != "PRESENT" ]]; then
  echo "DRILL[t4f_capture_positive_control]: FAIL pass path did not measure" >&2
  exit 1
fi

# Airplane / Test B must refuse capture (C-4)
rm -rf "$EVID"
mkdir -p "$EVID"
set +e
env T4F_HARNESS=1 T4F_TEST=B T4F_EVIDENCE_DIR="$EVID" bash "$TARGET" \
  >"$LOG_DIR/capture_test_b_refuse.log" 2>&1
B_EC=$?
set -e
if [[ "$B_EC" -ne 2 ]]; then
  echo "DRILL[t4f_capture_positive_control]: FAIL Test B should exit 2 (refused), got $B_EC" >&2
  exit 1
fi

echo "DRILL[t4f_capture_positive_control]: PASS"
echo "intact_fail_exit=$INTACT_EC restored_fail_exit=$REST_EC pass_exit=$PASS_EC test_b_exit=$B_EC"
exit 0
