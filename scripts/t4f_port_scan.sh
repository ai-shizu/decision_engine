#!/usr/bin/env bash
# T4-F-2: Device port-scan apparatus with positive control (nmap).
#
# A scanner that finds nothing is indistinguishable from a scanner that never
# reached the target — unless the same session first detects a known-open port.
#
# Phase order:
#   1. Positive control — intentionally open a listener; nmap must see it OPEN
#   2. Only then scan the stated target with an EXPLICIT port scope
#
# Env:
#   T4F_EVIDENCE_DIR     Evidence root (default: /tmp/t4f-evidence/portscan)
#   T4F_DEVICE_IP        Target device IP (required for live device scan)
#   T4F_PORT_SCOPE       One of:
#                          top1000   (default) — nmap --top-ports 1000
#                          full      — 1-65535 (slow; explicit)
#                          custom:N-M or custom:22,80,443
#   T4F_CONTROL_BIND     Control listen address (default: 127.0.0.1)
#   T4F_NMAP             Override nmap path
#   T4F_SKIP_DEVICE=1    Stop after positive control (apparatus validation)
#   T4F_HARNESS=1        Deterministic simulation (no real listen/nmap needed)
#   T4F_HARNESS_CONTROL=pass|fail
#
# Exit:
#   0 — control PASS; device scan ran or intentionally skipped
#   1 — control FAIL or usage error (device results must not be reported)
#
# Grounding: docs/T4F_ABSOLUTE_ISOLATION_RUNTIME_PLAN.md §5 T4-F-2
set -euo pipefail

NMAP="${T4F_NMAP:-/opt/homebrew/bin/nmap}"
command -v nmap >/dev/null 2>&1 && NMAP="$(command -v nmap)"
EVID="${T4F_EVIDENCE_DIR:-/tmp/t4f-evidence/portscan}"
SCOPE="${T4F_PORT_SCOPE:-top1000}"
CONTROL_BIND="${T4F_CONTROL_BIND:-127.0.0.1}"
DEVICE_IP="${T4F_DEVICE_IP:-}"
SKIP_DEVICE="${T4F_SKIP_DEVICE:-0}"
HARNESS="${T4F_HARNESS:-0}"
HARNESS_CONTROL="${T4F_HARNESS_CONTROL:-pass}"

mkdir -p "$EVID"
SESSION_ID="$(date -u +%Y%m%dT%H%M%SZ)-$$"
SESSION_DIR="$EVID/session-$SESSION_ID"
mkdir -p "$SESSION_DIR"
LOG="$SESSION_DIR/session.log"

log() { printf '%s\n' "$*" | tee -a "$LOG"; }
die() { log "FAIL: $*"; exit 1; }

prompt_commander() {
  local msg="$1"
  log ""
  log "════════════════════════════════════════════════════════════"
  log "[装置 → 指揮官] $msg"
  log "完了したら Enter を押してください。"
  log "════════════════════════════════════════════════════════════"
  if [[ "$HARNESS" == "1" ]]; then
    log "(harness: auto-acknowledge)"
    return 0
  fi
  read -r _
}

resolve_nmap_args() {
  # Writes human-readable scope + nmap argv fragments into files.
  case "$SCOPE" in
    top1000)
      echo "scope_label=nmap --top-ports 1000 (NOT the full 65535)" >"$SESSION_DIR/scan_scope.txt"
      echo "ports_examined_claim=top_1000_tcp_only" >>"$SESSION_DIR/scan_scope.txt"
      NMAP_PORT_ARGS=(--top-ports 1000)
      ;;
    full)
      echo "scope_label=tcp 1-65535 (full)" >"$SESSION_DIR/scan_scope.txt"
      echo "ports_examined_claim=all_65535_tcp" >>"$SESSION_DIR/scan_scope.txt"
      NMAP_PORT_ARGS=(-p 1-65535)
      ;;
    custom:*)
      local spec="${SCOPE#custom:}"
      echo "scope_label=custom $spec" >"$SESSION_DIR/scan_scope.txt"
      echo "ports_examined_claim=custom:$spec" >>"$SESSION_DIR/scan_scope.txt"
      NMAP_PORT_ARGS=(-p "$spec")
      ;;
    *)
      die "unknown T4F_PORT_SCOPE=$SCOPE (use top1000|full|custom:SPEC)"
      ;;
  esac
  echo "scope_env=$SCOPE" >>"$SESSION_DIR/scan_scope.txt"
  echo "stated_at_utc=$(date -u +%Y-%m-%dT%H:%M:%SZ)" >>"$SESSION_DIR/scan_scope.txt"
}

{
  echo "session_id=$SESSION_ID"
  echo "started_utc=$(date -u +%Y-%m-%dT%H:%M:%SZ)"
  echo "harness=$HARNESS"
  echo "nmap=$NMAP"
} >"$SESSION_DIR/meta.txt"

log "T4-F-2 port scan session $SESSION_ID"
resolve_nmap_args
log "scan scope: $(tr '\n' ' ' <"$SESSION_DIR/scan_scope.txt")"

CONTROL_PORT=""
CONTROL_PID=""
cleanup_control() {
  if [[ -n "${CONTROL_PID:-}" ]]; then
    kill "$CONTROL_PID" 2>/dev/null || true
    wait "$CONTROL_PID" 2>/dev/null || true
    CONTROL_PID=""
  fi
}
trap cleanup_control EXIT

# =============================================================================
# Phase 1 — positive control: known-open port must be detected
# =============================================================================
log "=== PHASE 1: positive control (known-open listener) ==="

CONTROL_OK=0

if [[ "$HARNESS" == "1" ]]; then
  if [[ "$HARNESS_CONTROL" == "pass" ]]; then
    echo "control_port=65521" >"$SESSION_DIR/positive_control_summary.txt"
    echo "control_state=open" >>"$SESSION_DIR/positive_control_summary.txt"
    echo "harness=pass" >>"$SESSION_DIR/positive_control_summary.txt"
    CONTROL_OK=1
    log "POSITIVE_CONTROL: PASS (harness)"
  else
    echo "control_port=65521" >"$SESSION_DIR/positive_control_summary.txt"
    echo "control_state=not_detected" >>"$SESSION_DIR/positive_control_summary.txt"
    echo "harness=fail" >>"$SESSION_DIR/positive_control_summary.txt"
    CONTROL_OK=0
    log "POSITIVE_CONTROL: FAIL (harness forced)"
  fi
else
  [[ -x "$NMAP" ]] || [[ -n "$(command -v "$NMAP" 2>/dev/null)" ]] || die "nmap not found (expected /opt/homebrew/bin/nmap)"
  if ! command -v "$NMAP" >/dev/null 2>&1 && [[ ! -x "$NMAP" ]]; then
    die "nmap not executable: $NMAP"
  fi

  # Pick an ephemeral high port; bind localhost HTTP listener (Mac-side OK per directive).
  CONTROL_PORT="$(python3 - <<'PY'
import socket
s = socket.socket()
s.bind(("127.0.0.1", 0))
print(s.getsockname()[1])
s.close()
PY
)"
  log "control: listening on ${CONTROL_BIND}:${CONTROL_PORT}"

  python3 - "$CONTROL_BIND" "$CONTROL_PORT" <<'PY' &
import socket, sys
bind, port = sys.argv[1], int(sys.argv[2])
s = socket.socket()
s.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
s.bind((bind, port))
s.listen(1)
while True:
    try:
        c, _ = s.accept()
        c.close()
    except Exception:
        break
PY
  CONTROL_PID=$!
  sleep 0.3
  if ! kill -0 "$CONTROL_PID" 2>/dev/null; then
    die "failed to start control listener"
  fi

  # Narrow scan: only the control port (proves detection, not device posture).
  set +e
  "$NMAP" -Pn -n -p "$CONTROL_PORT" "$CONTROL_BIND" \
    >"$SESSION_DIR/positive_control_nmap.txt" 2>&1
  NMAP_EC=$?
  set -e
  echo "nmap_exit=$NMAP_EC" >>"$SESSION_DIR/positive_control_summary.txt"
  echo "control_bind=$CONTROL_BIND" >>"$SESSION_DIR/positive_control_summary.txt"
  echo "control_port=$CONTROL_PORT" >>"$SESSION_DIR/positive_control_summary.txt"

  if grep -Eq "${CONTROL_PORT}/tcp[[:space:]]+open" "$SESSION_DIR/positive_control_nmap.txt"; then
    echo "control_state=open" >>"$SESSION_DIR/positive_control_summary.txt"
    CONTROL_OK=1
    log "POSITIVE_CONTROL: PASS (detected open ${CONTROL_BIND}:${CONTROL_PORT})"
  else
    echo "control_state=not_detected" >>"$SESSION_DIR/positive_control_summary.txt"
    CONTROL_OK=0
    log "POSITIVE_CONTROL: FAIL — scanner did not see known-open port"
    log "---- nmap output ----"
    tee -a "$LOG" <"$SESSION_DIR/positive_control_nmap.txt" >/dev/null || true
  fi

  cleanup_control
  trap - EXIT
fi

echo "control_ok=$CONTROL_OK" >"$SESSION_DIR/positive_control_result.txt"
echo "finished_utc=$(date -u +%Y-%m-%dT%H:%M:%SZ)" >>"$SESSION_DIR/positive_control_result.txt"

if [[ "$CONTROL_OK" -ne 1 ]]; then
  log "GATE: positive control failed — refusing to report any device scan"
  echo "device_scan=REFUSED_CONTROL_FAIL" >"$SESSION_DIR/device_scan_status.txt"
  if [[ -e "$SESSION_DIR/device_scan_nmap.txt" ]]; then
    die "internal: device scan artifact exists after control failure"
  fi
  exit 1
fi

# =============================================================================
# Phase 2 — device scan (explicit scope)
# =============================================================================
log "=== PHASE 2: device scan ==="

if [[ "$SKIP_DEVICE" == "1" ]]; then
  log "T4F_SKIP_DEVICE=1 — apparatus validation only"
  {
    echo "status=SKIPPED_BY_REQUEST"
    echo "scope_file=scan_scope.txt"
  } >"$SESSION_DIR/device_scan_status.txt"
  log "SESSION OK (control only)"
  exit 0
fi

if [[ "$HARNESS" == "1" ]]; then
  {
    echo "status=HARNESS_STUB"
    echo "device_ip=127.0.0.1"
    echo "open_ports=none_claimed"
    echo "scope=$(grep scope_label "$SESSION_DIR/scan_scope.txt")"
  } >"$SESSION_DIR/device_scan_status.txt"
  echo "harness stub — not a live device posture claim" >"$SESSION_DIR/device_scan_nmap.txt"
  log "SESSION OK (harness stub device scan)"
  exit 0
fi

prompt_commander "実機が同一 LAN 上で到達可能な IP を確認してください（設定 → Wi-Fi → 情報）。機内モードではスキャンは届きません。"

[[ -n "$DEVICE_IP" ]] || die "T4F_DEVICE_IP is required for live device scan (or set T4F_SKIP_DEVICE=1)"

echo "device_ip=$DEVICE_IP" >"$SESSION_DIR/device_scan_status.txt"
echo "scope=$(grep scope_label "$SESSION_DIR/scan_scope.txt" | head -n1)" >>"$SESSION_DIR/device_scan_status.txt"

log "scanning $DEVICE_IP with scope=$SCOPE ..."
set +e
"$NMAP" -Pn -n "${NMAP_PORT_ARGS[@]}" "$DEVICE_IP" \
  >"$SESSION_DIR/device_scan_nmap.txt" 2>&1
DEV_EC=$?
set -e
echo "nmap_exit=$DEV_EC" >>"$SESSION_DIR/device_scan_status.txt"
echo "finished_utc=$(date -u +%Y-%m-%dT%H:%M:%SZ)" >>"$SESSION_DIR/device_scan_status.txt"

# Extract open ports list (state only — no service banners required for posture claim).
grep -E '^[0-9]+/tcp[[:space:]]+open' "$SESSION_DIR/device_scan_nmap.txt" \
  >"$SESSION_DIR/device_open_ports.txt" || true
OPEN_COUNT="$(wc -l <"$SESSION_DIR/device_open_ports.txt" | tr -d ' ')"
echo "open_port_lines=$OPEN_COUNT" >>"$SESSION_DIR/device_scan_status.txt"
echo "status=CAPTURED" >>"$SESSION_DIR/device_scan_status.txt"

log "device scan done: open_port_lines=$OPEN_COUNT (scope stated in scan_scope.txt)"
log "SESSION OK"
log "session_dir=$SESSION_DIR"
exit 0
