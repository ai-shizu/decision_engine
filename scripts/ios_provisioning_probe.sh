#!/usr/bin/env bash
# T4-D D-1: Classify a provisioning profile and count remaining lifetime.
#
# Usage:
#   bash scripts/ios_provisioning_probe.sh <path-to-.app|.mobileprovision|.plist>
#
# .plist is a fixture seam (already-decoded provision XML/binary plist) for
# mutation drills — never treat a fixture as a substitute for CMS decode of a
# real embedded.mobileprovision in Release acceptance.
#
# Exit codes (fail-closed):
#   0  — classified OK (may print WARN_EXPIRING when seconds_remaining < 72h)
#  22  — profile_source ABSENT (path missing)
#  23  — RED for Release Archive: DEVELOPMENT or get-task-allow=true
#  24  — EXPIRED (seconds_remaining <= 0)
#  28  — UNKNOWN / CMS decode failure (fail-closed; not 0, not 23)
#
# Grounding: docs/T4D_IOS_ARCHIVE_CI_DIRECTIVE.md §3.1
set -euo pipefail

die_usage() {
  echo "usage: ios_provisioning_probe.sh <path-to-.app|.mobileprovision|.plist>" >&2
  exit 1
}

[[ $# -eq 1 ]] || die_usage
INPUT="$1"

emit() { printf '%s=%s\n' "$1" "$2"; }

# --- resolve profile_source ---
PROFILE_SOURCE="ABSENT"
MODE="" # cms | plist
if [[ ! -e "$INPUT" ]]; then
  emit profile_source ABSENT
  emit profile_class UNKNOWN
  emit get_task_allow ABSENT
  emit provisioned_devices ABSENT
  emit expiration_utc ABSENT
  emit seconds_remaining ABSENT
  emit validity_days_total ABSENT
  echo "RESULT=ABSENT"
  exit 22
fi

ABS_INPUT="$(cd "$(dirname "$INPUT")" && pwd)/$(basename "$INPUT")"
if [[ -d "$ABS_INPUT" ]]; then
  CAND="$ABS_INPUT/embedded.mobileprovision"
  if [[ ! -f "$CAND" ]]; then
    emit profile_source ABSENT
    emit profile_class UNKNOWN
    emit get_task_allow ABSENT
    emit provisioned_devices ABSENT
    emit expiration_utc ABSENT
    emit seconds_remaining ABSENT
    emit validity_days_total ABSENT
    echo "NOTE: .app present but embedded.mobileprovision missing"
    echo "RESULT=ABSENT"
    exit 22
  fi
  PROFILE_SOURCE="$(cd "$(dirname "$CAND")" && pwd)/$(basename "$CAND")"
  MODE=cms
elif [[ -f "$ABS_INPUT" ]]; then
  PROFILE_SOURCE="$ABS_INPUT"
  case "$ABS_INPUT" in
    *.plist) MODE=plist ;;
    *.mobileprovision) MODE=cms ;;
    *)
      # Heuristic: try CMS first; if that fails, fail-closed UNKNOWN
      MODE=cms
      ;;
  esac
else
  emit profile_source ABSENT
  emit profile_class UNKNOWN
  emit get_task_allow ABSENT
  emit provisioned_devices ABSENT
  emit expiration_utc ABSENT
  emit seconds_remaining ABSENT
  emit validity_days_total ABSENT
  echo "RESULT=ABSENT"
  exit 22
fi

emit profile_source "$PROFILE_SOURCE"

WORK="$(mktemp -d "${TMPDIR:-/tmp}/ios-prov-probe.XXXXXX")"
cleanup() { rm -rf "$WORK"; }
trap cleanup EXIT
PLIST="$WORK/provision.plist"

if [[ "$MODE" == "plist" ]]; then
  cp "$PROFILE_SOURCE" "$PLIST"
else
  set +e
  security cms -D -i "$PROFILE_SOURCE" -o "$PLIST" >"$WORK/cms.out" 2>"$WORK/cms.err"
  CMS_EC=$?
  set -e
  if [[ "$CMS_EC" -ne 0 || ! -s "$PLIST" ]]; then
    emit profile_class UNKNOWN
    emit get_task_allow ABSENT
    emit provisioned_devices ABSENT
    emit expiration_utc ABSENT
    emit seconds_remaining ABSENT
    emit validity_days_total ABSENT
    echo "cms_decode_exit=$CMS_EC"
    if [[ -s "$WORK/cms.err" ]]; then
      echo "cms_stderr<<EOF"
      cat "$WORK/cms.err"
      echo "EOF"
    fi
    echo "RESULT=UNKNOWN"
    exit 28
  fi
fi

# --- parse via python (dates / entitlements / classification) ---
# Prints KEY=VALUE lines; exit code of python is classification helper only.
set +e
python3 - <<'PY' "$PLIST" >"$WORK/parsed.env"
import plistlib
import sys
from datetime import datetime, timezone
from pathlib import Path

path = Path(sys.argv[1])
try:
    pl = plistlib.loads(path.read_bytes())
except Exception as e:
    print("PARSE_ERROR=%s" % e)
    sys.exit(28)

ents = pl.get("Entitlements") or {}
gta = ents.get("get-task-allow", None)
if gta is True:
    gta_s = "true"
elif gta is False:
    gta_s = "false"
else:
    gta_s = "ABSENT"

devices = pl.get("ProvisionedDevices")
if devices is None:
    devices_s = "ABSENT"
elif isinstance(devices, list):
    devices_s = str(len(devices))
else:
    devices_s = "ABSENT"

all_devices = pl.get("ProvisionsAllDevices")
local_prov = pl.get("LocalProvision")
name = pl.get("Name") or ""

creation = pl.get("CreationDate")
expiration = pl.get("ExpirationDate")

def to_utc_iso(dt):
    if dt is None:
        return None
    if isinstance(dt, datetime):
        if dt.tzinfo is None:
            dt = dt.replace(tzinfo=timezone.utc)
        else:
            dt = dt.astimezone(timezone.utc)
        return dt.strftime("%Y-%m-%dT%H:%M:%SZ"), int(dt.timestamp())
    return None

cre = to_utc_iso(creation)
exp = to_utc_iso(expiration)
now = datetime.now(timezone.utc)
now_epoch = int(now.timestamp())

if exp is None:
    exp_iso = "ABSENT"
    seconds_remaining = "ABSENT"
    exp_epoch = None
else:
    exp_iso, exp_epoch = exp
    seconds_remaining = str(exp_epoch - now_epoch)

if cre is None or exp_epoch is None:
    validity_days = "ABSENT"
else:
    _, cre_epoch = cre
    validity_days = str(int((exp_epoch - cre_epoch) // 86400))

# Classification (fail-closed to UNKNOWN when ambiguous)
profile_class = "UNKNOWN"
if all_devices is True:
    profile_class = "ENTERPRISE"
elif gta is True or local_prov is True or (
    isinstance(devices, list) and len(devices) > 0 and "Team Provisioning Profile" in name
):
    profile_class = "DEVELOPMENT"
elif isinstance(devices, list) and len(devices) > 0 and gta is not True:
    # Device-list present, not development entitlements → Ad Hoc
    profile_class = "ADHOC"
elif devices is None and gta is not True and all_devices is not True:
    # No device list, not enterprise, not get-task-allow → App Store distribution shape
    profile_class = "APPSTORE"
elif devices is None and gta is False:
    profile_class = "APPSTORE"

# Ambiguity guard: empty device list without other signals
if profile_class == "UNKNOWN":
    pass

print("get_task_allow=%s" % gta_s)
print("provisioned_devices=%s" % (
    "ALL_DEVICES" if all_devices is True else devices_s
))
print("expiration_utc=%s" % exp_iso)
print("seconds_remaining=%s" % seconds_remaining)
print("validity_days_total=%s" % validity_days)
print("profile_class=%s" % profile_class)
print("profile_name=%s" % name.replace("\n", " "))
if validity_days == "7":
    print("NOTE=validity_days_total=7 suggests unpaid Apple Personal Team (7-day profiles)")
sys.exit(0)
PY
PARSE_EC=$?
set -e

if [[ "$PARSE_EC" -eq 28 ]] || grep -q '^PARSE_ERROR=' "$WORK/parsed.env" 2>/dev/null; then
  emit profile_class UNKNOWN
  emit get_task_allow ABSENT
  emit provisioned_devices ABSENT
  emit expiration_utc ABSENT
  emit seconds_remaining ABSENT
  emit validity_days_total ABSENT
  cat "$WORK/parsed.env" || true
  echo "RESULT=UNKNOWN"
  exit 28
fi

get_field() {
  # Exact key extract; values may contain spaces/colons — no shell source.
  local k="$1"
  local line
  line="$(grep -E "^${k}=" "$WORK/parsed.env" | head -1 || true)"
  if [[ -z "$line" ]]; then
    printf '%s\n' "ABSENT"
    return 0
  fi
  printf '%s\n' "${line#*=}"
}

profile_class="$(get_field profile_class)"
get_task_allow="$(get_field get_task_allow)"
provisioned_devices="$(get_field provisioned_devices)"
expiration_utc="$(get_field expiration_utc)"
seconds_remaining="$(get_field seconds_remaining)"
validity_days_total="$(get_field validity_days_total)"
profile_name="$(get_field profile_name)"
note_line="$(get_field NOTE)"

emit profile_class "${profile_class}"
emit get_task_allow "${get_task_allow}"
emit provisioned_devices "${provisioned_devices}"
emit expiration_utc "${expiration_utc}"
emit seconds_remaining "${seconds_remaining}"
emit validity_days_total "${validity_days_total}"
if [[ "$profile_name" != "ABSENT" && -n "$profile_name" ]]; then
  emit profile_name "$profile_name"
fi
if [[ "$note_line" != "ABSENT" && -n "$note_line" ]]; then
  echo "NOTE=$note_line"
fi

# --- fail-closed decisions (order matches §3.2 drills) ---
if [[ "$profile_class" == "UNKNOWN" ]]; then
  echo "RESULT=UNKNOWN"
  exit 28
fi

if [[ "$seconds_remaining" == "ABSENT" ]]; then
  echo "RESULT=UNKNOWN"
  exit 28
fi

# EXPIRED before DEVELOPMENT RED so past-Expiration fixtures return 24
if [[ "$seconds_remaining" =~ ^-?[0-9]+$ ]] && [[ "$seconds_remaining" -le 0 ]]; then
  echo "RESULT=EXPIRED"
  exit 24
fi

# Release Archive must not use Development / get-task-allow=true
if [[ "$profile_class" == "DEVELOPMENT" || "$get_task_allow" == "true" ]]; then
  echo "RESULT=RED_DEVELOPMENT_OR_GET_TASK_ALLOW"
  exit 23
fi

if [[ "$seconds_remaining" =~ ^[0-9]+$ ]] && [[ "$seconds_remaining" -lt 259200 ]]; then
  echo "WARN_EXPIRING=seconds_remaining<$seconds_remaining (threshold 259200=72h)"
  echo "RESULT=OK_WARN_EXPIRING"
  exit 0
fi

echo "RESULT=OK"
exit 0
