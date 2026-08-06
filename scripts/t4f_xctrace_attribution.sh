#!/usr/bin/env bash
# T4-F-3a: Process-attributed connection capture via xctrace (out-of-app).
#
# Gives the attribution that rvi0+tcpdump correctly declared NOT SEPARABLE.
# Commander ruling: app-owned *connection count* 0 is the stronger §15.2-2 claim
# (a zero-byte path that still connected is weaker than connection 0).
#
# Strict phase order:
#   0. Preconditions (device / target app / xctrace)
#   1. Positive control — attach NON-Coraxis app; connection events must appear
#   2. Main measurement — attach Coraxis while commander exercises features
#   3. Teardown — no leftover `xctrace record`; announce abnormal exits
#
# Hang discipline (drafter measured --time-limit ignored):
#   Outer watchdog kills the recorder. Timeout => status=HUNG, never "0".
#
# Parse discipline:
#   Mechanical xctrace export + schema count. Failure => NOT PARSEABLE, never "0".
#
# Fail-soft (T4-F-1 pcapng autopsy):
#   External non-zero must not silently kill the session under set -e.
#   Never 2>/dev/null — stderr goes to evidence logs.
#   grep no-match under pipefail must not become a crash (0 connections is valid).
#
# Env:
#   T4F_UDID                 Device UDID (live)
#   T4F_EVIDENCE_DIR         Evidence root (default /tmp/t4f-evidence/xctrace)
#   T4F_CONTROL_ATTACH       Control process name (default MobileSafari)
#   T4F_TARGET_ATTACH        Target process name (default Coraxis)
#   T4F_TARGET_BUNDLE        Bundle id for presence check (default com.ai-shizu.pkb)
#   T4F_CONTROL_URL          URL for control traffic (default http://example.com/)
#   T4F_RECORD_SECONDS       --time-limit value passed to xctrace (default 30)
#   T4F_WATCHDOG_SECONDS     Outer kill deadline (default RECORD+45; must exceed)
#   T4F_SKIP_MEASURE=1       Stop after positive control
#   T4F_HARNESS=1            No live xctrace; deterministic simulation
#   T4F_HARNESS_CONTROL=pass|fail
#   T4F_HARNESS_HUNG=1       Force watchdog HUNG path (J-3)
#   T4F_HARNESS_PARSE=ok|fail|zero  Control/measure parse outcome in harness
#   T4F_XCTRACE              Override xctrace binary (mutation / hang stub)
#
# Exit:
#   0  — control fired; measure completed or skipped
#   1  — precondition / control / parse / teardown failure
#   3  — HUNG (watchdog fired)
#
# Grounding: T4-F-3a directive 2026-08-05; plan docs/T4F_ABSOLUTE_ISOLATION_RUNTIME_PLAN.md
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
EVID="${T4F_EVIDENCE_DIR:-/tmp/t4f-evidence/xctrace}"
UDID="${T4F_UDID:-}"
CONTROL_ATTACH="${T4F_CONTROL_ATTACH:-MobileSafari}"
TARGET_ATTACH="${T4F_TARGET_ATTACH:-Coraxis}"
TARGET_BUNDLE="${T4F_TARGET_BUNDLE:-com.ai-shizu.pkb}"
CONTROL_URL="${T4F_CONTROL_URL:-http://example.com/}"
RECORD_SECS="${T4F_RECORD_SECONDS:-30}"
WATCHDOG_SECS="${T4F_WATCHDOG_SECONDS:-$((RECORD_SECS + 45))}"
SKIP_MEASURE="${T4F_SKIP_MEASURE:-0}"
HARNESS="${T4F_HARNESS:-0}"
HARNESS_CONTROL="${T4F_HARNESS_CONTROL:-pass}"
HARNESS_HUNG="${T4F_HARNESS_HUNG:-0}"
HARNESS_PARSE="${T4F_HARNESS_PARSE:-ok}"
XCTRACE_BIN="${T4F_XCTRACE:-}"

mkdir -p "$EVID"
SESSION_ID="$(date -u +%Y%m%dT%H%M%SZ)-$$"
SESSION_DIR="$EVID/session-$SESSION_ID"
mkdir -p "$SESSION_DIR"
LOG="$SESSION_DIR/session.log"
T4F_CLEAN_EXIT=0
RECORD_PIDS=()

log() { printf '%s\n' "$*" | tee -a "$LOG"; }
die() { log "FAIL: $*"; exit 1; }
hung() { log "HUNG: $*"; exit 3; }

# Resolve xctrace without swallowing stderr.
resolve_xctrace() {
  if [[ -n "$XCTRACE_BIN" ]]; then
    printf '%s\n' "$XCTRACE_BIN"
    return 0
  fi
  local found=""
  found="$(xcrun --find xctrace 2>>"$SESSION_DIR/xctrace_resolve.err" || true)"
  if [[ -z "$found" || ! -x "$found" ]]; then
    return 1
  fi
  printf '%s\n' "$found"
  return 0
}

sha_of() {
  if command -v shasum >>"$SESSION_DIR/cmd_probe.log" 2>>"$SESSION_DIR/cmd_probe.log"; then
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

list_xctrace_record_pids() {
  # pgrep exit 1 = none found — SUCCESS here. Sandbox may error on sysmon;
  # keep only numeric PIDs so error text never looks like a leftover process.
  { pgrep -f 'xctrace[[:space:]]+record' || true; } 2>>"$SESSION_DIR/pgrep.err" \
    | { grep -E '^[0-9]+$' || true; }
}

ensure_no_xctrace_record() {
  local left
  left="$(list_xctrace_record_pids | tr '\n' ' ')"
  if [[ -n "${left// /}" ]]; then
    log "WARN: leftover xctrace record pids: $left — sending TERM"
    # shellcheck disable=SC2086
    kill -TERM $left 2>>"$SESSION_DIR/teardown_stderr.log" || true
    sleep 2
    left="$(list_xctrace_record_pids | tr '\n' ' ')"
    if [[ -n "${left// /}" ]]; then
      log "WARN: still alive after TERM — KILL: $left"
      # shellcheck disable=SC2086
      kill -KILL $left 2>>"$SESSION_DIR/teardown_stderr.log" || true
      sleep 1
    fi
  fi
  left="$(list_xctrace_record_pids | tr '\n' ' ')"
  if [[ -n "${left// /}" ]]; then
    echo "leftover_pids=$left" >"$SESSION_DIR/teardown_xctrace.txt"
    return 1
  fi
  echo "leftover_pids=NONE" >"$SESSION_DIR/teardown_xctrace.txt"
  return 0
}

t4f_on_exit() {
  # Status must be passed in: `local rc; rc=$?` clobbers $? after `local` on
  # this bash (macOS). Use: trap 't4f_on_exit $?' EXIT
  local rc="${1:-0}"
  trap - EXIT
  if [[ "$rc" -ne 0 && "${T4F_CLEAN_EXIT:-0}" != "1" ]]; then
    echo "" >&2
    echo "ABNORMAL EXIT rc=$rc — xctrace attribution session did not complete cleanly." >&2
    echo "  session dir : ${SESSION_DIR:-<unset>}" >&2
    echo "  last log    : ${LOG:-<unset>}" >&2
    echo "  record log  : ${SESSION_DIR:-}/xctrace_record_*.log" >&2
    echo "  export log  : ${SESSION_DIR:-}/xctrace_export_*.log" >&2
    local p
    local tdir="${SESSION_DIR:-$EVID}"
    mkdir -p "$tdir" || true
    local tlog="$tdir/teardown_stderr.log"
    for p in "${RECORD_PIDS[@]:-}"; do
      if [[ -n "$p" ]] && kill -0 "$p" 2>>"$tlog"; then
        kill -TERM "$p" 2>>"$tlog" || true
        echo "  stopped record pid=$p" >&2
      fi
    done
    ensure_no_xctrace_record >>"$tlog" || true
    local left_msg="unknown"
    if [[ -f "${SESSION_DIR:-}/teardown_xctrace.txt" ]]; then
      left_msg="$(cat "${SESSION_DIR}/teardown_xctrace.txt" 2>>"$tlog" || echo unknown)"
    fi
    echo "  teardown_xctrace: $left_msg" >&2
  fi
  exit "$rc"
}
trap 't4f_on_exit $?' EXIT

{
  echo "session_id=$SESSION_ID"
  echo "started_utc=$(date -u +%Y-%m-%dT%H:%M:%SZ)"
  echo "harness=$HARNESS"
  echo "control_attach=$CONTROL_ATTACH"
  echo "target_attach=$TARGET_ATTACH"
  echo "target_bundle=$TARGET_BUNDLE"
  echo "record_seconds=$RECORD_SECS"
  echo "watchdog_seconds=$WATCHDOG_SECS"
  echo "claim=app_owned_connection_count_is_stronger_than_byte_count"
  echo "in_app_nefilter=FORBIDDEN"
} >"$SESSION_DIR/meta.txt"

log "T4-F-3a xctrace attribution session $SESSION_ID"
log "evidence=$SESSION_DIR"

# =============================================================================
# Phase 0 — preconditions
# =============================================================================
log "=== PHASE 0: preconditions ==="

XCTRACE=""
if [[ "$HARNESS" == "1" ]]; then
  XCTRACE="${XCTRACE_BIN:-/usr/bin/true}"
  echo "xctrace=harness" >"$SESSION_DIR/preconditions.txt"
else
  XCTRACE="$(resolve_xctrace)" || die "xctrace not found (xcrun --find xctrace)"
  echo "xctrace=$XCTRACE" >"$SESSION_DIR/preconditions.txt"
  echo "xctrace_version=$("$XCTRACE" version 2>>"$SESSION_DIR/xctrace_version.err" || true)" >>"$SESSION_DIR/preconditions.txt"
  [[ -n "$UDID" ]] || die "T4F_UDID required for live run"
fi

# Privilege note (J-6): xctrace binary is user-executable; sudo -n fails on this host.
# Live record is still commander-owned (device prompts / hang risk / physical ops).
{
  echo "implementer_uid=$(id -u)"
  echo "sudo_n_probe=NOT_RUN_by_implementer_for_record"
  echo "xctrace_needs_root=NO_binary_is_user_executable"
  echo "live_record_executor=commander"
} >>"$SESSION_DIR/preconditions.txt"

prompt_commander "USB で実機を接続してください (UDID=${UDID:-unset}). 無線は ON (試験 A). 機内モード禁止."

if [[ "$HARNESS" == "1" ]]; then
  echo "transport=harness-wired" >"$SESSION_DIR/transport_check.txt"
  echo "target_app=harness-present" >"$SESSION_DIR/target_app_check.txt"
else
  set +e
  DEV_OUT="$(xcrun devicectl list devices 2>>"$SESSION_DIR/devicectl_list.err")"
  DEV_EC=$?
  set -e
  printf '%s\n' "$DEV_OUT" >"$SESSION_DIR/devicectl_list.txt"
  echo "devicectl_exit=$DEV_EC" >"$SESSION_DIR/transport_check.txt"
  if printf '%s\n' "$DEV_OUT" | grep -qF "$UDID"; then
    printf '%s\n' "$DEV_OUT" | grep -F -A6 -B2 "$UDID" >"$SESSION_DIR/device_snippet.txt" || true
    if grep -qiE 'transport:\s*(wired|usb)|USB|wired' "$SESSION_DIR/device_snippet.txt" \
      || grep -qiE 'wired|USB' "$SESSION_DIR/devicectl_list.txt"; then
      echo "transport_observed=wired_or_usb_hint" >>"$SESSION_DIR/transport_check.txt"
    else
      echo "transport_observed=UNKNOWN" >>"$SESSION_DIR/transport_check.txt"
      log "WARN: wired hint not found in devicectl — continuing; xctrace may still list Offline"
    fi
  else
    echo "transport_observed=UDID_NOT_LISTED" >>"$SESSION_DIR/transport_check.txt"
    log "WARN: UDID not in devicectl list"
  fi

  # xctrace's own device list (may say Offline even on USB — AI_SKILLS known trap).
  set +e
  XT_DEV="$("$XCTRACE" list devices 2>>"$SESSION_DIR/xctrace_list_devices.err")"
  XT_EC=$?
  set -e
  printf '%s\n' "$XT_DEV" >"$SESSION_DIR/xctrace_list_devices.txt"
  echo "xctrace_list_devices_exit=$XT_EC" >>"$SESSION_DIR/transport_check.txt"
  if printf '%s\n' "$XT_DEV" | grep -qF "$UDID"; then
    if printf '%s\n' "$XT_DEV" | grep -F "$UDID" | grep -qi 'offline'; then
      echo "xctrace_device_state=listed_OFFLINE" >>"$SESSION_DIR/transport_check.txt"
      log "NOTE: xctrace lists device Offline — that is NOT proof USB is absent (AI_SKILLS). Record may still fail; watchdog will catch hangs."
    else
      echo "xctrace_device_state=listed" >>"$SESSION_DIR/transport_check.txt"
    fi
  else
    echo "xctrace_device_state=UDID_ABSENT_from_xctrace_list" >>"$SESSION_DIR/transport_check.txt"
  fi

  # Target app presence via devicectl (best-effort; failure is WARN not silent green).
  set +e
  APP_OUT="$(xcrun devicectl device info apps --device "$UDID" 2>>"$SESSION_DIR/devicectl_apps.err")"
  APP_EC=$?
  set -e
  printf '%s\n' "$APP_OUT" >"$SESSION_DIR/devicectl_apps.txt"
  echo "devicectl_apps_exit=$APP_EC" >"$SESSION_DIR/target_app_check.txt"
  if printf '%s\n' "$APP_OUT" | grep -qF "$TARGET_BUNDLE"; then
    echo "target_bundle_present=YES" >>"$SESSION_DIR/target_app_check.txt"
    log "target app bundle present: $TARGET_BUNDLE"
  else
    echo "target_bundle_present=NO_OR_UNLISTED" >>"$SESSION_DIR/target_app_check.txt"
    log "WARN: bundle $TARGET_BUNDLE not found in apps list — commander must confirm Coraxis is installed"
  fi
fi

if [[ "$WATCHDOG_SECS" -le "$RECORD_SECS" ]]; then
  die "T4F_WATCHDOG_SECONDS ($WATCHDOG_SECS) must exceed T4F_RECORD_SECONDS ($RECORD_SECS) — --time-limit is untrusted"
fi

# =============================================================================
# Device process lookup
# =============================================================================
# Prints the pid of the newest process whose executable basename matches $1, or
# nothing. Never aborts the session: an unresolvable target is a reportable
# outcome, not a crash.
resolve_device_pid() {
  local want="$1"
  local out="$SESSION_DIR/device_processes_${want}.json"
  local err="$SESSION_DIR/device_processes_${want}.err"

  if ! xcrun devicectl device info processes \
      --device "$UDID" --json-output "$out" >>"$err" 2>>"$err"; then
    return 1
  fi

  python3 - "$out" "$want" <<'PY' || true
import json, sys
try:
    procs = json.load(open(sys.argv[1]))["result"]["runningProcesses"]
except Exception:
    sys.exit(0)
want = sys.argv[2]
hits = [p for p in procs
        if (p.get("executable") or "").rsplit("/", 1)[-1] == want
        and p.get("processIdentifier")]
if hits:
    # Newest pid: if the app was relaunched, the old one is the wrong target.
    print(max(int(p["processIdentifier"]) for p in hits))
PY
}

# =============================================================================
# Record + watchdog
# =============================================================================
# Returns via globals: RECORD_STATUS=ok|hung|failed, RECORD_EC=int
run_xctrace_record() {
  local label="$1"
  local attach="$2"
  local out_trace="$3"
  local rec_log="$SESSION_DIR/xctrace_record_${label}.log"
  RECORD_STATUS="failed"
  RECORD_EC=1

  rm -rf "$out_trace"
  log "record[$label]: attach=$attach time-limit=${RECORD_SECS}s watchdog=${WATCHDOG_SECS}s out=$out_trace"

  if [[ "$HARNESS" == "1" ]]; then
    if [[ "$HARNESS_HUNG" == "1" && "$label" == "control" ]]; then
      # Simulate ignored --time-limit: sleep past watchdog.
      sleep 99999 >>"$rec_log" 2>>"$rec_log" &
      local spid=$!
      RECORD_PIDS+=("$spid")
      local t=0
      while kill -0 "$spid" 2>>"$SESSION_DIR/watchdog_stderr.log"; do
        sleep 1
        t=$((t + 1))
        if [[ "$t" -ge "$WATCHDOG_SECS" ]]; then
          kill -TERM "$spid" 2>>"$SESSION_DIR/watchdog_stderr.log" || true
          sleep 1
          kill -KILL "$spid" 2>>"$SESSION_DIR/watchdog_stderr.log" || true
          RECORD_STATUS="hung"
          RECORD_EC=3
          echo "status=HUNG" >"$SESSION_DIR/record_${label}_status.txt"
          echo "watchdog_seconds=$WATCHDOG_SECS" >>"$SESSION_DIR/record_${label}_status.txt"
          return 0
        fi
      done
    fi
    # Synthetic .trace bundle (directory).
    mkdir -p "$out_trace"
    echo "harness synthetic trace label=$label attach=$attach" >"$out_trace/harness.txt"
    echo "status=ok" >"$SESSION_DIR/record_${label}_status.txt"
    echo "harness=1" >>"$SESSION_DIR/record_${label}_status.txt"
    RECORD_STATUS="ok"
    RECORD_EC=0
    return 0
  fi

  # Resolve the target to a live pid BEFORE recording.
  #
  # First live attempt against a visible device died with
  #   Cannot find process matching name: MobileSafari   (exit 19)
  # and the name was not wrong — MobileSafari was the exact executable name,
  # and it was running minutes later. iOS had simply suspended or reaped it
  # between the operator's acknowledgement and the record starting. Attaching
  # by name means that race decides the run, and reports it as a cryptic 19.
  #
  # Asking the device which processes exist turns "not running" into a sentence
  # the operator can act on, before anything is recorded.
  local attach_pid=""
  attach_pid="$(resolve_device_pid "$attach")" || true
  if [[ -z "$attach_pid" ]]; then
    RECORD_STATUS="failed"
    RECORD_EC=0
    {
      echo "status=TARGET_NOT_RUNNING"
      echo "attach_name=$attach"
      echo "note=process_absent_on_device_at_record_time; not_a_parse_or_schema_problem"
      echo "remedy=launch_and_keep_it_foreground_then_retry"
    } >"$SESSION_DIR/record_${label}_status.txt"
    log "record[$label]: '$attach' is not running on the device — cannot attach"
    log "record[$label]: launch it, keep it in the foreground, and re-run"
    return 0
  fi
  log "record[$label]: resolved $attach -> pid $attach_pid"
  echo "attach_pid=$attach_pid" >>"$SESSION_DIR/record_${label}_status.txt"

  # Live: never trust --time-limit alone.
  set +e
  "$XCTRACE" record \
    --template 'Network' \
    --device "$UDID" \
    --attach "$attach_pid" \
    --time-limit "${RECORD_SECS}s" \
    --output "$out_trace" \
    --no-prompt \
    >>"$rec_log" 2>>"$rec_log" &
  local rpid=$!
  set -e
  RECORD_PIDS+=("$rpid")
  echo "record_pid=$rpid" >>"$SESSION_DIR/record_${label}_status.txt"

  local t=0
  while kill -0 "$rpid" 2>>"$SESSION_DIR/watchdog_stderr.log"; do
    sleep 1
    t=$((t + 1))
    if [[ "$t" -ge "$WATCHDOG_SECS" ]]; then
      log "record[$label]: WATCHDOG firing after ${t}s (xctrace --time-limit not trusted)"
      kill -TERM "$rpid" 2>>"$SESSION_DIR/watchdog_stderr.log" || true
      sleep 2
      if kill -0 "$rpid" 2>>"$SESSION_DIR/watchdog_stderr.log"; then
        kill -KILL "$rpid" 2>>"$SESSION_DIR/watchdog_stderr.log" || true
      fi
      wait "$rpid" 2>>"$SESSION_DIR/watchdog_stderr.log" || true
      RECORD_STATUS="hung"
      RECORD_EC=3
      {
        echo "status=HUNG"
        echo "watchdog_seconds=$WATCHDOG_SECS"
        echo "elapsed_seconds=$t"
        echo "note=do_not_report_as_zero_connections"
      } >"$SESSION_DIR/record_${label}_status.txt"
      return 0
    fi
  done
  set +e
  wait "$rpid"
  RECORD_EC=$?
  set -e

  if [[ ! -e "$out_trace" ]]; then
    RECORD_STATUS="failed"
    {
      echo "status=FAILED_NO_OUTPUT"
      echo "record_exit=$RECORD_EC"
      echo "note=hang_or_fail_must_not_be_reported_as_zero"
    } >"$SESSION_DIR/record_${label}_status.txt"
    return 0
  fi

  # Existence of the bundle is not success. Measured 2026-08-05 on the first
  # live run: xctrace exited 13 with "Timed out waiting for device to boot",
  # still left a control.trace behind, and the rig called that status=ok. Export
  # then failed with "Document Missing Template Error" and the session reported
  # NOT_PARSEABLE — blaming the xpath for something the recorder never recorded.
  # The gate still refused, correctly, but it named the wrong component, and a
  # wrong diagnosis sends the next reader to the wrong place.
  if [[ "$RECORD_EC" -ne 0 ]]; then
    RECORD_STATUS="failed"
    {
      echo "status=RECORD_FAILED"
      echo "record_exit=$RECORD_EC"
      echo "elapsed_seconds=$t"
      echo "trace_bundle=present_but_not_a_successful_recording"
      echo "note=do_not_attribute_this_to_parsing; see xctrace_record_${label}.log"
    } >"$SESSION_DIR/record_${label}_status.txt"
    return 0
  fi

  RECORD_STATUS="ok"
  {
    echo "status=ok"
    echo "record_exit=$RECORD_EC"
    echo "elapsed_seconds=$t"
  } >"$SESSION_DIR/record_${label}_status.txt"
  return 0
}

# =============================================================================
# Mechanical connection-event extraction (J-5)
# =============================================================================
# Writes: $outdir/parse_result.txt with fields status=... connection_events=N|NOT_PARSEABLE
extract_connection_events() {
  local label="$1"
  local trace="$2"
  local outdir="$SESSION_DIR/parse_${label}"
  mkdir -p "$outdir"
  local toc="$outdir/toc.xml"
  local export_log="$SESSION_DIR/xctrace_export_${label}.log"
  local result="$outdir/parse_result.txt"

  if [[ "$HARNESS" == "1" ]]; then
    case "$HARNESS_PARSE" in
      fail)
        {
          echo "status=NOT_PARSEABLE"
          echo "connection_events=NOT_PARSEABLE"
          echo "reason=harness_forced_parse_fail"
        } >"$result"
        return 0
        ;;
      zero)
        {
          echo "status=ok"
          echo "connection_events=0"
          echo "schema=network-connection-detected"
          echo "harness=1"
        } >"$result"
        return 0
        ;;
      *)
        local n=3
        if [[ "$HARNESS_CONTROL" == "fail" && "$label" == "control" ]]; then
          n=0
        fi
        if [[ "$label" == "control" && "$HARNESS_CONTROL" == "pass" ]]; then
          n=5
        fi
        if [[ "$label" == "measure" ]]; then
          n=0
        fi
        {
          echo "status=ok"
          echo "connection_events=$n"
          echo "schema=network-connection-detected"
          echo "harness=1"
        } >"$result"
        return 0
        ;;
    esac
  fi

  if [[ ! -e "$trace" ]]; then
    {
      echo "status=NOT_PARSEABLE"
      echo "connection_events=NOT_PARSEABLE"
      echo "reason=trace_missing"
    } >"$result"
    return 0
  fi

  # Export TOC — capture non-zero without dying.
  set +e
  "$XCTRACE" export --input "$trace" --toc --output "$toc" >>"$export_log" 2>>"$export_log"
  local toc_ec=$?
  set -e
  echo "toc_export_exit=$toc_ec" >"$outdir/toc_meta.txt"
  if [[ "$toc_ec" -ne 0 || ! -s "$toc" ]]; then
    {
      echo "status=NOT_PARSEABLE"
      echo "connection_events=NOT_PARSEABLE"
      echo "reason=toc_export_failed"
      echo "toc_exit=$toc_ec"
    } >"$result"
    return 0
  fi

  # Prefer schema network-connection-detected (one row == one new connection).
  local schema="network-connection-detected"
  local has_schema=0
  if grep -qF "schema=\"$schema\"" "$toc"; then
    has_schema=1
  elif grep -qF "$schema" "$toc"; then
    has_schema=1
  fi
  # grep no-match => exit 1; `if grep -q` absorbs it (0 connections / missing schema must not crash).

  if [[ "$has_schema" -ne 1 ]]; then
    # Fall back: any table schema containing "network-connection".
    local alt=""
    alt="$( { grep -oE 'schema="[^"]*network-connection[^"]*"' "$toc" || true; } | head -n1 | sed 's/schema="//;s/"$//' )"
    if [[ -n "$alt" ]]; then
      schema="$alt"
      has_schema=1
      echo "schema_fallback=$schema" >>"$outdir/toc_meta.txt"
    fi
  fi

  if [[ "$has_schema" -ne 1 ]]; then
    {
      echo "status=NOT_PARSEABLE"
      echo "connection_events=NOT_PARSEABLE"
      echo "reason=network_connection_schema_absent_from_toc"
    } >"$result"
    # Keep toc for commander; do not invent 0.
    return 0
  fi

  local xpath="/trace-toc/run[@number=\"1\"]/data/table[@schema=\"$schema\"]"
  local table_xml="$outdir/table_${schema}.xml"
  set +e
  "$XCTRACE" export --input "$trace" --xpath "$xpath" --output "$table_xml" >>"$export_log" 2>>"$export_log"
  local xp_ec=$?
  set -e
  echo "xpath_export_exit=$xp_ec" >>"$outdir/toc_meta.txt"
  echo "xpath=$xpath" >>"$outdir/toc_meta.txt"

  if [[ "$xp_ec" -ne 0 || ! -s "$table_xml" ]]; then
    {
      echo "status=NOT_PARSEABLE"
      echo "connection_events=NOT_PARSEABLE"
      echo "reason=xpath_export_failed"
      echo "xpath_exit=$xp_ec"
      echo "schema=$schema"
    } >"$result"
    return 0
  fi

  # Count rows mechanically. Empty table => 0 (valid). Parse tools must not crash us.
  local count
  count="$(python3 - "$table_xml" <<'PY' 2>>"$outdir/python_count.err"
import sys, re
from pathlib import Path
text = Path(sys.argv[1]).read_text(encoding="utf-8", errors="replace")
# Prefer <row ...> style; also accept <node> rows used by some exports.
rows = len(re.findall(r"<row[\s>]", text))
if rows == 0:
    rows = len(re.findall(r"<node\b", text))
# Privacy: do not print addresses — count only.
print(rows)
PY
)" || count=""

  if [[ -z "$count" ]] || ! [[ "$count" =~ ^[0-9]+$ ]]; then
    {
      echo "status=NOT_PARSEABLE"
      echo "connection_events=NOT_PARSEABLE"
      echo "reason=row_count_failed"
      echo "schema=$schema"
    } >"$result"
    return 0
  fi

  {
    echo "status=ok"
    echo "connection_events=$count"
    echo "schema=$schema"
    echo "note=counts_only_no_remote_addresses_in_summary"
  } >"$result"
  return 0
}

read_parse_field() {
  local file="$1"
  local key="$2"
  { grep -E "^${key}=" "$file" || true; } | head -n1 | cut -d= -f2-
}

# =============================================================================
# Phase 1 — positive control
# =============================================================================
log "=== PHASE 1: positive control (attach $CONTROL_ATTACH) ==="
CONTROL_OK=0
CONTROL_TRACE="$SESSION_DIR/control.trace"

prompt_commander "陽性対照: $CONTROL_ATTACH を前面にし、$CONTROL_URL を開ける準備をしてください。Coraxis は起動しないでください。"

if [[ "$HARNESS" != "1" ]]; then
  prompt_commander "録画開始後すぐに $CONTROL_ATTACH で $CONTROL_URL を開いてください。ウィンドウは約 ${RECORD_SECS}s(outer watchdog ${WATCHDOG_SECS}s)。"
fi

run_xctrace_record "control" "$CONTROL_ATTACH" "$CONTROL_TRACE"

if [[ "$RECORD_STATUS" == "hung" ]]; then
  ensure_no_xctrace_record || true
  hung "positive-control recorder exceeded watchdog — not reporting connection 0"
fi
if [[ "$RECORD_STATUS" != "ok" ]]; then
  echo "control_record=FAILED" >"$SESSION_DIR/positive_control_result.txt"
  ensure_no_xctrace_record || true
  die "positive-control record failed (status=$RECORD_STATUS ec=$RECORD_EC) — refusing measurement"
fi

extract_connection_events "control" "$CONTROL_TRACE"
CONTROL_PARSE="$SESSION_DIR/parse_control/parse_result.txt"
CONTROL_STATUS="$(read_parse_field "$CONTROL_PARSE" status)"
CONTROL_EVENTS="$(read_parse_field "$CONTROL_PARSE" connection_events)"

{
  echo "attach=$CONTROL_ATTACH"
  echo "parse_status=$CONTROL_STATUS"
  echo "connection_events=$CONTROL_EVENTS"
  echo "finished_utc=$(date -u +%Y-%m-%dT%H:%M:%SZ)"
} >"$SESSION_DIR/positive_control_result.txt"

if [[ "$CONTROL_STATUS" != "ok" ]]; then
  log "POSITIVE_CONTROL: FAIL ($CONTROL_STATUS / $CONTROL_EVENTS)"
  CONTROL_OK=0
elif [[ "$CONTROL_EVENTS" =~ ^[0-9]+$ ]] && [[ "$CONTROL_EVENTS" -gt 0 ]]; then
  log "POSITIVE_CONTROL: PASS (connection_events=$CONTROL_EVENTS on $CONTROL_ATTACH)"
  CONTROL_OK=1
else
  log "POSITIVE_CONTROL: FAIL (connection_events=$CONTROL_EVENTS — instrument did not see control traffic)"
  CONTROL_OK=0
fi

echo "control_ok=$CONTROL_OK" >>"$SESSION_DIR/positive_control_result.txt"

if [[ "$CONTROL_OK" -ne 1 ]]; then
  log "GATE: positive control did not fire — refusing main measurement"
  echo "measurement=REFUSED_CONTROL_FAIL" >"$SESSION_DIR/measurement_status.txt"
  ensure_no_xctrace_record || die "teardown: xctrace record still present after control fail"
  if [[ -e "$SESSION_DIR/measurement_summary.txt" ]]; then
    die "internal: measurement artifact exists after control failure"
  fi
  # Intentional gate failure — not an abnormal crash.
  T4F_CLEAN_EXIT=1
  exit 1
fi

# Privacy: drop control trace after count (bundle may contain remote addrs).
if [[ "$HARNESS" != "1" && -e "$CONTROL_TRACE" ]]; then
  rm -rf "$CONTROL_TRACE"
  echo "control_trace_retained=no_deleted_after_count" >>"$SESSION_DIR/positive_control_result.txt"
fi

# =============================================================================
# Phase 2 — main measurement (Coraxis + feature exercise)
# =============================================================================
log "=== PHASE 2: main measurement (attach $TARGET_ATTACH) ==="
echo "measurement=STARTED" >"$SESSION_DIR/measurement_status.txt"

# Operation scope — idle splash is NOT §15.2-2 evidence (C-6).
cat >"$SESSION_DIR/operations_checklist.txt" <<EOF
T4-F-3a operations checklist (commander fills DURING the measure window)

App: $TARGET_ATTACH / $TARGET_BUNDLE
Window: ~${RECORD_SECS}s record (watchdog ${WATCHDOG_SECS}s)

Required during measurement (mark done=yes/no):
[ ] launch Coraxis from cold or bring to foreground
[ ] open Record / diary entry UI and type a short note (do not need to save if offline path blocks)
[ ] open Consult / chat UI and send one local prompt if model available; if model ABSENT mark skipped
[ ] open Profile / settings and toggle a non-network preference if present
[ ] open any Import / Archive UI and cancel without choosing a cloud source

Do NOT:
- idle on splash only
- enable airplane mode for this capture (Test A)
- open Safari during Coraxis attach (contaminates attribution window)

After the window, copy this file to operations_log.txt and set each line done=yes|no|skipped_reason=...
EOF

if [[ "$SKIP_MEASURE" == "1" ]]; then
  log "T4F_SKIP_MEASURE=1 — apparatus validation only"
  {
    echo "status=SKIPPED_BY_REQUEST"
    echo "app_owned_connection_events=NOT_MEASURED"
    echo "operations=NOT_RUN"
  } >"$SESSION_DIR/measurement_summary.txt"
else
  prompt_commander "本計測: Coraxis ($TARGET_ATTACH) を前面にしてください。録画中に operations_checklist.txt の項目を操作します。アイドル放置は無効です。"

  if [[ "$HARNESS" == "1" ]]; then
    cp "$SESSION_DIR/operations_checklist.txt" "$SESSION_DIR/operations_log.txt"
    {
      echo "harness_operations=simulated"
      echo "done_launch=yes"
      echo "done_record_ui=yes"
      echo "done_consult=skipped_reason=harness"
      echo "done_profile=yes"
      echo "done_import_ui=skipped_reason=harness"
      echo "NOT_EXERCISED=live_llm_infer_live_line_import_archive_roundtrip"
    } >>"$SESSION_DIR/operations_log.txt"
  else
    prompt_commander "録画を開始します。開始直後から checklist を実行してください。"
  fi

  MEASURE_TRACE="$SESSION_DIR/measure.trace"
  run_xctrace_record "measure" "$TARGET_ATTACH" "$MEASURE_TRACE"

  if [[ "$RECORD_STATUS" == "hung" ]]; then
    ensure_no_xctrace_record || true
    {
      echo "status=HUNG"
      echo "app_owned_connection_events=HUNG"
      echo "note=watchdog_fired_do_not_read_as_zero"
    } >"$SESSION_DIR/measurement_summary.txt"
    hung "measurement recorder exceeded watchdog"
  fi
  if [[ "$RECORD_STATUS" != "ok" ]]; then
    {
      echo "status=RECORD_FAILED"
      echo "app_owned_connection_events=NOT_PARSEABLE"
      echo "record_status=$RECORD_STATUS"
    } >"$SESSION_DIR/measurement_summary.txt"
    ensure_no_xctrace_record || true
    die "measurement record failed"
  fi

  if [[ "$HARNESS" != "1" ]]; then
    prompt_commander "録画終了。operations_checklist.txt を operations_log.txt にコピーし、各項目を done=yes|no|skipped_reason=... で埋めてから Enter。"
    if [[ ! -f "$SESSION_DIR/operations_log.txt" ]]; then
      cp "$SESSION_DIR/operations_checklist.txt" "$SESSION_DIR/operations_log.txt"
      echo "WARNING: commander did not supply operations_log.txt — checklist copied as placeholder" >>"$SESSION_DIR/operations_log.txt"
    fi
  fi

  extract_connection_events "measure" "$MEASURE_TRACE"
  MEAS_PARSE="$SESSION_DIR/parse_measure/parse_result.txt"
  MEAS_STATUS="$(read_parse_field "$MEAS_PARSE" status)"
  MEAS_EVENTS="$(read_parse_field "$MEAS_PARSE" connection_events)"

  if [[ "$MEAS_STATUS" != "ok" ]]; then
    {
      echo "status=NOT_PARSEABLE"
      echo "app_owned_connection_events=NOT_PARSEABLE"
      echo "parse_status=$MEAS_STATUS"
      echo "attach=$TARGET_ATTACH"
      echo "claim_note=do_not_report_as_zero"
    } >"$SESSION_DIR/measurement_summary.txt"
    # Retain measure.trace for commander re-export; still fail the session.
    ensure_no_xctrace_record || true
    die "measurement NOT PARSEABLE — refusing to report 0"
  fi

  {
    echo "status=CAPTURED"
    echo "app_owned_connection_events=$MEAS_EVENTS"
    echo "schema=$(read_parse_field "$MEAS_PARSE" schema)"
    echo "attach=$TARGET_ATTACH"
    echo "bundle=$TARGET_BUNDLE"
    echo "ruling=connection_zero_is_stronger_than_byte_zero"
    echo "privacy=counts_only"
  } >"$SESSION_DIR/measurement_summary.txt"

  # Privacy: strip measure trace after successful count (C-8). Commander can
  # set T4F_KEEP_TRACE=1 to retain for audit.
  if [[ "${T4F_KEEP_TRACE:-0}" != "1" && "$HARNESS" != "1" && -e "$MEASURE_TRACE" ]]; then
    rm -rf "$MEASURE_TRACE"
    echo "measure_trace_retained=no_deleted_after_count" >>"$SESSION_DIR/measurement_summary.txt"
  fi

  log "measurement: app_owned_connection_events=$MEAS_EVENTS (attach=$TARGET_ATTACH)"
fi

echo "measurement=COMPLETED" >"$SESSION_DIR/measurement_status.txt"

# =============================================================================
# Phase 3 — teardown
# =============================================================================
log "=== PHASE 3: teardown ==="
if ! ensure_no_xctrace_record; then
  die "teardown incomplete — xctrace record process still present"
fi
echo "teardown=OK" >"$SESSION_DIR/teardown.txt"
echo "finished_utc=$(date -u +%Y-%m-%dT%H:%M:%SZ)" >>"$SESSION_DIR/teardown.txt"

log "SESSION OK: positive control → measurement → teardown"
log "session_dir=$SESSION_DIR"
T4F_CLEAN_EXIT=1
exit 0
