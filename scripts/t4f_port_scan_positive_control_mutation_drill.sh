#!/usr/bin/env bash
# T4-F-2 — Mutation drill: port-scan positive-control failure blocks device scan.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
TARGET="$ROOT/scripts/t4f_port_scan.sh"
LOG_DIR="${T4F_LOG_DIR:-/tmp/t4f-evidence/mutations}"
EVID="${T4F_EVIDENCE_DIR:-/tmp/t4f-evidence/portscan-drill}"
mkdir -p "$LOG_DIR" "$EVID"

sha_of() { shasum -a 256 "$1" | awk '{print $1}'; }
BACKUP="$LOG_DIR/t4f_port_scan.sh.bak"
cp "$TARGET" "$BACKUP"
BEFORE="$(sha_of "$TARGET")"

run_h() {
  local label="$1" control="$2"
  rm -rf "$EVID"; mkdir -p "$EVID"
  set +e
  env T4F_HARNESS=1 T4F_HARNESS_CONTROL="$control" T4F_EVIDENCE_DIR="$EVID" \
      T4F_PORT_SCOPE=top1000 bash "$TARGET" >"$LOG_DIR/portscan_${label}.log" 2>&1
  local ec=$?
  set -e
  local s
  s="$(ls -d "$EVID"/session-* 2>/dev/null | head -n1 || true)"
  if [[ -n "$s" && -f "$s/device_scan_nmap.txt" ]]; then
    echo PRESENT >"$LOG_DIR/ps_art_${label}.txt"
  else
    echo ABSENT >"$LOG_DIR/ps_art_${label}.txt"
  fi
  echo "$ec" >"$LOG_DIR/ps_exit_${label}.txt"
}

run_h intact_fail fail
[[ "$(cat "$LOG_DIR/ps_exit_intact_fail.txt")" -ne 0 ]] || exit 1
[[ "$(cat "$LOG_DIR/ps_art_intact_fail.txt")" == "ABSENT" ]] || exit 1

python3 - "$TARGET" <<'PY'
from pathlib import Path
import sys
p = Path(sys.argv[1])
text = p.read_text(encoding="utf-8")
needle = 'if [[ "$CONTROL_OK" -ne 1 ]]; then'
replacement = '''if [[ "$CONTROL_OK" -ne 1 ]]; then
  log "MUTATION_PLANT: ignoring port-scan control failure"
  CONTROL_OK=1
fi
if [[ "0" -eq 1 ]]; then'''
if needle not in text:
    raise SystemExit("gate not found")
p.write_text(text.replace(needle, replacement, 1), encoding="utf-8")
PY

run_h mutated_fail fail
[[ "$(cat "$LOG_DIR/ps_art_mutated_fail.txt")" == "PRESENT" ]] || {
  cp "$BACKUP" "$TARGET"; exit 1
}

cp "$BACKUP" "$TARGET"
[[ "$(sha_of "$TARGET")" == "$BEFORE" ]] || exit 1
run_h restored_fail fail
[[ "$(cat "$LOG_DIR/ps_art_restored_fail.txt")" == "ABSENT" ]] || exit 1

run_h intact_pass pass
[[ "$(cat "$LOG_DIR/ps_exit_intact_pass.txt")" -eq 0 ]] || exit 1
[[ "$(cat "$LOG_DIR/ps_art_intact_pass.txt")" == "PRESENT" ]] || exit 1

echo "DRILL[t4f_port_scan_positive_control]: PASS"
exit 0
