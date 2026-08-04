#!/usr/bin/env bash
# T4-F-1: Runtime packet-capture apparatus with same-session positive control.
#
# Measures silence only after proving the instrument can hear. A zero-byte
# capture without a fired positive control in the SAME session is not evidence.
#
# Strict phase order:
#   0. Preconditions + commander physical checks (USB; radio ON for Test A)
#   1. rvictl -s <udid> + ifconfig presence (do NOT trust rvictl -l)
#   2. Positive control — intentional NON-target device traffic must be seen
#   3. Main measurement window (Test A only: radio ON). Skipped if 2 fails.
#   4. Teardown (rvictl -x) + verify no rvi* remains
#
# Commander runs live capture (tcpdump needs root/BPF). Implementers must not
# invoke sudo — use T4F_HARNESS=1 for dry-run / mutation drills.
#
# Env:
#   T4F_UDID              Device UDID (required unless harness)
#   T4F_EVIDENCE_DIR      Evidence root (default: /tmp/t4f-evidence/capture)
#   T4F_CONTROL_URL       Browser URL for control (default: http://example.com/)
#   T4F_CONTROL_SECONDS   Control capture window seconds (default: 20)
#   T4F_MEASURE_SECONDS   Main capture window seconds (default: 120)
#   T4F_SKIP_MEASURE=1    Stop after positive control (apparatus validation)
#   T4F_TEST=A            Only A is allowed for packet capture (B refused)
#   T4F_HARNESS=1         No rvictl/tcpdump/sudo; deterministic phase simulation
#   T4F_HARNESS_CONTROL=pass|fail   Harness positive-control outcome
#   T4F_RVICTL            Override rvictl path
#   T4F_TCPDUMP           Override tcpdump path
#
# Exit:
#   0  — session completed (control fired; measure ran or intentionally skipped)
#   1  — usage / preconditions / control failure / teardown failure
#   2  — refused: airplane-mode / Test B capture (vacuous green guard)
#
# Grounding: docs/T4F_ABSOLUTE_ISOLATION_RUNTIME_PLAN.md §2–§3
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
RVICTL="${T4F_RVICTL:-/Library/Apple/usr/bin/rvictl}"
TCPDUMP="${T4F_TCPDUMP:-/usr/sbin/tcpdump}"
IFCONFIG="${T4F_IFCONFIG:-/sbin/ifconfig}"
EVID="${T4F_EVIDENCE_DIR:-/tmp/t4f-evidence/capture}"
CONTROL_URL="${T4F_CONTROL_URL:-http://example.com/}"
CONTROL_SECS="${T4F_CONTROL_SECONDS:-20}"
MEASURE_SECS="${T4F_MEASURE_SECONDS:-120}"
TEST_MODE="${T4F_TEST:-A}"
HARNESS="${T4F_HARNESS:-0}"
HARNESS_CONTROL="${T4F_HARNESS_CONTROL:-pass}"
SKIP_MEASURE="${T4F_SKIP_MEASURE:-0}"
UDID="${T4F_UDID:-}"

mkdir -p "$EVID"
SESSION_ID="$(date -u +%Y%m%dT%H%M%SZ)-$$"
SESSION_DIR="$EVID/session-$SESSION_ID"
mkdir -p "$SESSION_DIR"
LOG="$SESSION_DIR/session.log"

log() { printf '%s\n' "$*" | tee -a "$LOG"; }
die() { log "FAIL: $*"; exit 1; }
refuse() { log "REFUSED: $*"; exit 2; }

sha_of() {
  if command -v shasum >/dev/null 2>&1; then
    shasum -a 256 "$1" | awk '{print $1}'
  else
    sha256sum "$1" | awk '{print $1}'
  fi
}

prompt_commander() {
  local msg="$1"
  log ""
  log "════════════════════════════════════════════════════════════"
  log "[装置 → 指揮官] $msg"
  log "完了したら Enter を押してください（申告のみでは先へ進みません）。"
  log "════════════════════════════════════════════════════════════"
  if [[ "$HARNESS" == "1" ]]; then
    log "(harness: auto-acknowledge)"
    return 0
  fi
  read -r _
}

list_rvi() {
  # Ground truth: ifconfig -l, NOT rvictl -l (rvictl -l can be empty while rvi0 is up).
  "$IFCONFIG" -l 2>/dev/null | tr ' ' '\n' | grep -E '^rvi[0-9]+$' || true
}

require_rvi_present() {
  local found
  found="$(list_rvi)"
  if [[ -z "$found" ]]; then
    return 1
  fi
  printf '%s\n' "$found"
  return 0
}

packet_count_pcap() {
  local pcap="$1"
  if [[ ! -s "$pcap" ]]; then
    echo 0
    return 0
  fi
  # Count only — never dump payloads into evidence (C-7).
  "$TCPDUMP" -nn -r "$pcap" 2>/dev/null | wc -l | tr -d ' '
}

summarize_pcap_endpoints() {
  # Unique IP endpoints only (no ports mixed into privacy surface beyond count).
  local pcap="$1"
  local out="$2"
  if [[ ! -s "$pcap" ]]; then
    echo "packet_lines=0" >"$out"
    echo "unique_endpoints=0" >>"$out"
    return 0
  fi
  local lines endpoints
  lines="$("$TCPDUMP" -nn -r "$pcap" 2>/dev/null | wc -l | tr -d ' ')"
  endpoints="$("$TCPDUMP" -nn -r "$pcap" 2>/dev/null \
    | grep -oE '([0-9]{1,3}\.){3}[0-9]{1,3}' \
    | sort -u | wc -l | tr -d ' ')"
  {
    echo "packet_lines=$lines"
    echo "unique_ipv4_endpoints=$endpoints"
    echo "note=payloads_not_retained; endpoints_are_counts_only"
  } >"$out"
}

write_app_owned_criterion() {
  cat >"$SESSION_DIR/app_owned_criterion.txt" <<'EOF'
T4-F app-owned attribution criterion (defined BEFORE measurement)

Q1: Can rvi0 + tcpdump separate process / bundle id?
A1: NO. BPF on the Remote Virtual Interface sees device-wide L2/L3 frames.
    There is no PID, no bundle id, and no per-app tag in the capture.

Q2: What may be reported as a number?
A2: Device-wide packet/byte counts for a stated time window, after a fired
    same-session positive control. Those numbers are DEVICE-WIDE, not app-owned.

Q3: What must NOT be written as 0?
A3: "app-owned network bytes = 0" from rvi0/tcpdump alone.
    That claim is NOT SEPARABLE with this apparatus.

Q4: When is "app-owned = 0" allowed?
A4: Only with an additional attribution method that is itself positively
    controlled (e.g. Instruments Network profiling with process filter) —
    OUT OF SCOPE for this tcpdump/rvi0 apparatus. Until then: NOT SEPARABLE.

EOF
}

# --- refuse vacuous Test B capture (C-4) ---
if [[ "$TEST_MODE" != "A" ]]; then
  refuse "T4F_TEST=$TEST_MODE — packet capture is Test A only (radio ON). Airplane-mode capture is vacuous green (plan §2). Use a functional checklist for Test B; do not capture."
fi

{
  echo "session_id=$SESSION_ID"
  echo "started_utc=$(date -u +%Y-%m-%dT%H:%M:%SZ)"
  echo "test_mode=$TEST_MODE"
  echo "harness=$HARNESS"
  echo "control_url=$CONTROL_URL"
  echo "repo_root=$ROOT"
} >"$SESSION_DIR/meta.txt"

write_app_owned_criterion
log "T4-F-1 capture session $SESSION_ID"
log "evidence=$SESSION_DIR"

# =============================================================================
# Phase 0 — physical preconditions (do not trust commander claims alone)
# =============================================================================
log "=== PHASE 0: preconditions ==="

if [[ "$HARNESS" != "1" ]]; then
  [[ -x "$RVICTL" ]] || die "rvictl missing/unexecutable: $RVICTL"
  [[ -x "$TCPDUMP" ]] || die "tcpdump missing: $TCPDUMP"
  [[ -n "$UDID" ]] || die "T4F_UDID is required for live capture"
  # tcpdump needs BPF; refuse to pretend if we cannot open it (commander must sudo).
  if [[ "$(id -u)" -ne 0 ]]; then
    die "live capture requires root (sudo) for BPF/tcpdump — commander must run this script under sudo. Implementer must not sudo. For dry-run: T4F_HARNESS=1"
  fi
fi

prompt_commander "USB で実機を接続し、信頼してください（transport: wired が必要。localNetwork では rvictl 不可）。"

if [[ "$HARNESS" == "1" ]]; then
  echo "transport=harness-simulated-wired" >"$SESSION_DIR/transport_check.txt"
  log "transport check: harness OK"
else
  # Prefer direct measurement over rvictl -l.
  set +e
  DEV_OUT="$(xcrun devicectl list devices 2>&1)"
  DEV_EC=$?
  set -e
  printf '%s\n' "$DEV_OUT" >"$SESSION_DIR/devicectl_list.txt"
  echo "devicectl_exit=$DEV_EC" >>"$SESSION_DIR/transport_check.txt"
  if printf '%s\n' "$DEV_OUT" | grep -F "$UDID" >/dev/null 2>&1; then
    # Extract nearby lines for the UDID; look for wired/usb hints.
    printf '%s\n' "$DEV_OUT" | grep -F -A5 -B2 "$UDID" >"$SESSION_DIR/device_snippet.txt" || true
    if grep -qiE 'transport:\s*(wired|usb)|connectionType.*usb|USB' "$SESSION_DIR/device_snippet.txt" \
      || grep -qiE 'wired|USB' "$SESSION_DIR/devicectl_list.txt"; then
      echo "transport_observed=wired_or_usb_hint" >>"$SESSION_DIR/transport_check.txt"
      log "transport: wired/USB hint observed for UDID"
    else
      echo "transport_observed=UNKNOWN_or_not_wired" >>"$SESSION_DIR/transport_check.txt"
      log "WARN: could not confirm wired transport from devicectl output — continuing only if rvi comes up"
    fi
  else
    echo "transport_observed=UDID_NOT_LISTED" >>"$SESSION_DIR/transport_check.txt"
    log "WARN: UDID not listed by devicectl — rvictl may still work if USB paired"
  fi
fi

prompt_commander "試験 A: 無線を ON にしてください（Wi-Fi または cellular）。機内モードは禁止です。設定画面で接続中であることを確認してください。"

echo "radio_commander_ack_utc=$(date -u +%Y-%m-%dT%H:%M:%SZ)" >"$SESSION_DIR/radio_ack.txt"
echo "radio_requirement=ON_for_test_A" >>"$SESSION_DIR/radio_ack.txt"
echo "airplane_mode=FORBIDDEN_for_capture" >>"$SESSION_DIR/radio_ack.txt"
log "radio: commander acknowledged ON (liveness will be proven by positive control traffic)"

# =============================================================================
# Phase 1 — bring up rvi0; ground truth via ifconfig
# =============================================================================
log "=== PHASE 1: rvi bring-up ==="
RVI_IFACE=""

teardown_rvi() {
  if [[ "$HARNESS" == "1" ]]; then
    rm -f "$SESSION_DIR/harness_rvi_present"
    log "teardown: harness rvi cleared"
    return 0
  fi
  if [[ -n "$UDID" ]]; then
    set +e
    "$RVICTL" -x "$UDID" >>"$LOG" 2>&1
    set -e
  fi
  sleep 1
  local left
  left="$(list_rvi)"
  if [[ -n "$left" ]]; then
    log "WARN: rvi still present after -x: $left"
    return 1
  fi
  log "teardown: no rvi* in ifconfig -l"
  return 0
}

if [[ "$HARNESS" == "1" ]]; then
  touch "$SESSION_DIR/harness_rvi_present"
  RVI_IFACE="rvi0"
  echo "rvi_iface=$RVI_IFACE" >"$SESSION_DIR/rvi_bringup.txt"
  echo "ground_truth=harness" >>"$SESSION_DIR/rvi_bringup.txt"
  log "rvi: harness simulated $RVI_IFACE"
else
  # Clear any stale rvi first (best-effort)
  set +e
  "$RVICTL" -x "$UDID" >>"$LOG" 2>&1
  set -e
  sleep 1
  set +e
  "$RVICTL" -s "$UDID" >>"$SESSION_DIR/rvictl_start.txt" 2>&1
  RVI_EC=$?
  set -e
  echo "rvictl_start_exit=$RVI_EC" >>"$SESSION_DIR/rvi_bringup.txt"
  sleep 1
  RVI_LIST="$(require_rvi_present)" || die "rvictl -s reported done but ifconfig -l has no rvi* (do not trust rvictl -l)"
  RVI_IFACE="$(printf '%s\n' "$RVI_LIST" | head -n1)"
  {
    echo "rvi_iface=$RVI_IFACE"
    echo "rvi_all=$(printf '%s' "$RVI_LIST" | tr '\n' ',')"
    echo "ground_truth=ifconfig -l"
    echo "rvictl_list_note=NOT_USED_as_presence_oracle"
  } >>"$SESSION_DIR/rvi_bringup.txt"
  log "rvi present (ifconfig): $RVI_IFACE"
fi

# =============================================================================
# Phase 2 — positive control (NON-target app, device-originated, reproducible)
# =============================================================================
log "=== PHASE 2: positive control ==="
log "Control design: commander opens Safari (NOT Coraxis) to $CONTROL_URL"
log "Evidence retains counts/endpoint cardinality only — not payloads (C-7)."

CONTROL_PCAP="$SESSION_DIR/positive_control.pcap"
CONTROL_SUMMARY="$SESSION_DIR/positive_control_summary.txt"
MEASUREMENT_STARTED=0
CONTROL_OK=0

if [[ "$HARNESS" == "1" ]]; then
  if [[ "$HARNESS_CONTROL" == "pass" ]]; then
    # Synthetic non-empty pcap-ish marker: harness does not write real pcap.
    echo "harness_control_packets=12" >"$CONTROL_SUMMARY"
    echo "harness_control=pass" >>"$CONTROL_SUMMARY"
    echo "source=Safari_or_system_browser_NOT_Coraxis" >>"$CONTROL_SUMMARY"
    CONTROL_OK=1
    log "POSITIVE_CONTROL: PASS (harness)"
  else
    echo "harness_control_packets=0" >"$CONTROL_SUMMARY"
    echo "harness_control=fail" >>"$CONTROL_SUMMARY"
    CONTROL_OK=0
    log "POSITIVE_CONTROL: FAIL (harness forced)"
  fi
else
  prompt_commander "まもなく陽性対照キャプチャを ${CONTROL_SECS}s 開始します。準備: Safari を開き、アドレス欄に貼れるよう $CONTROL_URL を用意してください。Coraxis は起動しないでください。"

  # Start capture, then ask commander to generate traffic.
  rm -f "$CONTROL_PCAP"
  "$TCPDUMP" -i "$RVI_IFACE" -nn -w "$CONTROL_PCAP" >>"$SESSION_DIR/tcpdump_control.log" 2>&1 &
  TCPDUMP_PID=$!
  sleep 1
  if ! kill -0 "$TCPDUMP_PID" 2>/dev/null; then
    teardown_rvi || true
    die "tcpdump failed to start on $RVI_IFACE (see tcpdump_control.log)"
  fi

  prompt_commander "今すぐ Safari で $CONTROL_URL を開いてください（読み込み完了まで待つ）。Coraxis 以外のブラウザであること。完了後 Enter。"

  log "control: waiting ${CONTROL_SECS}s capture window..."
  sleep "$CONTROL_SECS"
  set +e
  kill "$TCPDUMP_PID" 2>/dev/null
  wait "$TCPDUMP_PID" 2>/dev/null
  set -e

  PC_COUNT="$(packet_count_pcap "$CONTROL_PCAP")"
  summarize_pcap_endpoints "$CONTROL_PCAP" "$CONTROL_SUMMARY"
  echo "control_url=$CONTROL_URL" >>"$CONTROL_SUMMARY"
  echo "control_source_requirement=non_Coraxis_browser_on_device" >>"$CONTROL_SUMMARY"
  echo "pcap_sha256=$(sha_of "$CONTROL_PCAP")" >>"$CONTROL_SUMMARY"
  # Privacy: drop control pcap after summary — counts stay; payloads do not linger (C-7).
  rm -f "$CONTROL_PCAP"
  echo "pcap_retained=no_deleted_after_count" >>"$CONTROL_SUMMARY"

  log "positive control packet_lines=$PC_COUNT"
  if [[ "$PC_COUNT" -gt 0 ]]; then
    CONTROL_OK=1
    log "POSITIVE_CONTROL: PASS (packets observed on $RVI_IFACE)"
  else
    CONTROL_OK=0
    log "POSITIVE_CONTROL: FAIL (0 packets — instrument cannot prove it hears)"
  fi
fi

echo "control_ok=$CONTROL_OK" >"$SESSION_DIR/positive_control_result.txt"
echo "finished_utc=$(date -u +%Y-%m-%dT%H:%M:%SZ)" >>"$SESSION_DIR/positive_control_result.txt"

if [[ "$CONTROL_OK" -ne 1 ]]; then
  log "GATE: positive control did not fire — refusing main measurement (silence is not evidence)"
  echo "measurement=REFUSED_CONTROL_FAIL" >"$SESSION_DIR/measurement_status.txt"
  teardown_rvi || true
  # Prove we did not create a measurement artifact.
  if [[ -e "$SESSION_DIR/measurement_summary.txt" ]]; then
    die "internal: measurement artifact exists after control failure"
  fi
  exit 1
fi

# Radio liveness is evidenced by control traffic itself (device-originated packets).
echo "radio_liveness=proven_by_positive_control_packets" >>"$SESSION_DIR/radio_ack.txt"

# =============================================================================
# Phase 3 — main measurement (only after control PASS)
# =============================================================================
log "=== PHASE 3: main measurement ==="
echo "measurement=STARTED" >"$SESSION_DIR/measurement_status.txt"
MEASUREMENT_STARTED=1

if [[ "$SKIP_MEASURE" == "1" ]]; then
  log "T4F_SKIP_MEASURE=1 — apparatus validation only; skipping long measure window"
  {
    echo "status=SKIPPED_BY_REQUEST"
    echo "app_owned_bytes=NOT_SEPARABLE"
    echo "device_wide_bytes=NOT_MEASURED_this_run"
    echo "reason=T4F_SKIP_MEASURE"
  } >"$SESSION_DIR/measurement_summary.txt"
else
  prompt_commander "本計測ウィンドウ（${MEASURE_SECS}s）を開始します。無線は ON のまま。Coraxis で試験 A の操作を行ってください。機内モードにしないでください。"

  MEASURE_PCAP="$SESSION_DIR/measurement.pcap"
  if [[ "$HARNESS" == "1" ]]; then
    {
      echo "status=HARNESS_STUB"
      echo "device_wide_packet_lines=0"
      echo "device_wide_note=harness_does_not_claim_live_silence"
      echo "app_owned_bytes=NOT_SEPARABLE"
      echo "app_owned_note=rvi_tcpdump_has_no_process_attribution"
    } >"$SESSION_DIR/measurement_summary.txt"
    log "measurement: harness stub written (NOT a live silence claim)"
  else
    rm -f "$MEASURE_PCAP"
    "$TCPDUMP" -i "$RVI_IFACE" -nn -w "$MEASURE_PCAP" >>"$SESSION_DIR/tcpdump_measure.log" 2>&1 &
    MPID=$!
    sleep 1
    if ! kill -0 "$MPID" 2>/dev/null; then
      teardown_rvi || true
      die "tcpdump failed to start for measurement"
    fi
    log "measurement: capturing ${MEASURE_SECS}s on $RVI_IFACE..."
    sleep "$MEASURE_SECS"
    set +e
    kill "$MPID" 2>/dev/null
    wait "$MPID" 2>/dev/null
    set -e

    M_COUNT="$(packet_count_pcap "$MEASURE_PCAP")"
    summarize_pcap_endpoints "$MEASURE_PCAP" "$SESSION_DIR/measurement_endpoints.txt"
    # Byte length of pcap file is NOT "network bytes" — record both honestly.
    M_PCAP_BYTES="$(wc -c <"$MEASURE_PCAP" | tr -d ' ')"
    {
      echo "status=CAPTURED"
      echo "device_wide_packet_lines=$M_COUNT"
      echo "pcap_file_bytes=$M_PCAP_BYTES"
      echo "pcap_sha256=$(sha_of "$MEASURE_PCAP")"
      echo "app_owned_bytes=NOT_SEPARABLE"
      echo "app_owned_note=rvi_tcpdump_has_no_process_attribution; do_not_report_app_owned_zero"
      echo "silence_claim=FORBIDDEN_without_attribution_method"
    } >"$SESSION_DIR/measurement_summary.txt"
    # Retain measurement pcap for commander audit; payloads may include ambient
    # system traffic — do not paste contents into reports (C-7). Counts only in report.
    log "measurement: device_wide packet_lines=$M_COUNT app_owned=NOT_SEPARABLE"
  fi
fi

echo "measurement=COMPLETED" >"$SESSION_DIR/measurement_status.txt"
echo "measurement_started=$MEASUREMENT_STARTED" >>"$SESSION_DIR/measurement_status.txt"

# =============================================================================
# Phase 4 — teardown
# =============================================================================
log "=== PHASE 4: teardown ==="
if ! teardown_rvi; then
  die "teardown incomplete — rvi still present"
fi
echo "teardown=OK" >"$SESSION_DIR/teardown.txt"
echo "finished_utc=$(date -u +%Y-%m-%dT%H:%M:%SZ)" >>"$SESSION_DIR/teardown.txt"
echo "rvi_after=$(list_rvi | tr '\n' ',' )" >>"$SESSION_DIR/teardown.txt"

log "SESSION OK: positive control → measurement → teardown"
log "session_dir=$SESSION_DIR"
exit 0
