#!/usr/bin/env bash
# T4-D V-3 mutation drills — disposable copies + real production gates only.
# Each drill: inject → expect RED → byte/SHA restore → same production gate GREEN.
# Fresh signed artifact full-gate GREEN is reported BLOCKED when unavailable.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
SRC_TAURI="$ROOT/apps/desktop/src-tauri"
SCAN="$ROOT/scripts/ios_archive_scan.sh"
GGUF_GATE="$ROOT/scripts/gguf_three_point_sha_gate.sh"
GGUF_REQ="$ROOT/scripts/ios_gguf_three_point_require_present.sh"
CONFIG_GATE="$ROOT/scripts/ios_config_contract_gate.sh"
LOG_DIR="${T4D_LOG_DIR:-/private/tmp/t4d-logs-fresh/mutations}"
mkdir -p "$LOG_DIR"
WORK="$(mktemp -d "${TMPDIR:-/tmp}/t4d-v3-mut.XXXXXX")"
cleanup() { rm -rf "$WORK"; }
trap cleanup EXIT

sha_of() { shasum -a 256 "$1" | awk '{print $1}'; }

pass_count=0
fail_count=0
blocked_count=0
record() {
  local name="$1" status="$2"
  echo "DRILL[$name]: $status"
  case "$status" in
    PASS) pass_count=$((pass_count+1)) ;;
    BLOCKED_EXTERNAL_PREREQUISITE) blocked_count=$((blocked_count+1)) ;;
    *) fail_count=$((fail_count+1)) ;;
  esac
}

for c in check_release_dev_isolation.py check_device_family.py check_privacy_inventory.py \
         check_export_method.py check_get_task_allow.py check_plist_trailing_lf.py; do
  [[ -f "$ROOT/scripts/ios/$c" ]] || { echo "missing production checker: $c" >&2; exit 1; }
done

FRESH_SIGNED_APP=""
# Fresh signed = built by release wrapper this session. Old archives are not evidence.
if [[ "${T4D_FRESH_SIGNED_APP:-}" != "" && -d "${T4D_FRESH_SIGNED_APP}" ]]; then
  FRESH_SIGNED_APP="$T4D_FRESH_SIGNED_APP"
fi
LEGACY_APP="$SRC_TAURI/gen/apple/build/pkb-desktop_iOS.xcarchive/Products/Applications/Coraxis.app"

# ---------------------------------------------------------------------------
# Helper: 4-point around a production gate on a disposable file
# ---------------------------------------------------------------------------
# 1) release + dev overlay mix → config-style production check via release conf copy
#    Use validate path: mutated tauri.ios.conf.json fails the same python contract
#    used by config gate (inline of that check is the production checker module).
#    Better: run a disposable config tree... For release overlay we invoke the
#    same isolation python block extracted... Audit forbids reimplementing.
#    Approach: copy desktop config files into disposable tree is heavy.
#    For overlay: mutate a copy and run `python3 scripts/ios/check_release_overlay.py`
#    which IS the production checker shared with config gate.

# Shared production checkers (not drills):
#   scripts/ios/check_release_dev_isolation.py
#   (created below as the single implementation config gate also calls)

# ---------------------------------------------------------------------------
drill_dev_overlay() {
  local name="dev_overlay_into_release"
  local f="$WORK/tauri.ios.conf.json"
  cp "$SRC_TAURI/tauri.ios.conf.json" "$f"
  local before; before="$(sha_of "$f")"
  echo "DRILL[$name] before_sha=$before"
  python3 - <<'PY' "$f"
import json, sys
from pathlib import Path
p = Path(sys.argv[1])
d = json.loads(p.read_text(encoding="utf-8"))
d.setdefault("build", {})["devUrl"] = "http://localhost:1420"
p.write_text(json.dumps(d, indent=2) + "\n", encoding="utf-8")
PY
  echo "DRILL[$name] mutate: inject build.devUrl"
  set +e
  python3 "$ROOT/scripts/ios/check_release_dev_isolation.py" \
    "$f" "$SRC_TAURI/tauri.ios.dev.conf.json" \
    "$SRC_TAURI/Info.ios.plist" "$SRC_TAURI/Info.ios.dev.plist" \
    >"$LOG_DIR/${name}.red.log" 2>&1
  local ec=$?
  set -e
  echo "DRILL[$name] expected_exit!=0 actual_exit=$ec"
  cat "$LOG_DIR/${name}.red.log"
  cp "$SRC_TAURI/tauri.ios.conf.json" "$f"
  local after; after="$(sha_of "$f")"
  echo "DRILL[$name] after_restore_sha=$after"
  [[ "$before" == "$after" ]] || { record "$name" FAIL; return; }
  set +e
  python3 "$ROOT/scripts/ios/check_release_dev_isolation.py" \
    "$SRC_TAURI/tauri.ios.conf.json" "$SRC_TAURI/tauri.ios.dev.conf.json" \
    "$SRC_TAURI/Info.ios.plist" "$SRC_TAURI/Info.ios.dev.plist" \
    >"$LOG_DIR/${name}.green.log" 2>&1
  local gec=$?
  set -e
  cat "$LOG_DIR/${name}.green.log"
  [[ "$ec" -ne 0 && "$gec" -eq 0 ]] && record "$name" PASS || record "$name" FAIL
}

drill_device_family() {
  local name="targeted_device_family_1_2"
  local f="$WORK/project.pbxproj"
  cp "$SRC_TAURI/gen/apple/pkb-desktop.xcodeproj/project.pbxproj" "$f"
  local before; before="$(sha_of "$f")"
  echo "DRILL[$name] before_sha=$before"
  perl -pi -e 's/TARGETED_DEVICE_FAMILY = 1;/TARGETED_DEVICE_FAMILY = "1,2";/g' "$f"
  echo "DRILL[$name] mutate: TARGETED_DEVICE_FAMILY=1,2"
  set +e
  python3 "$ROOT/scripts/ios/check_device_family.py" "$f" >"$LOG_DIR/${name}.red.log" 2>&1
  local ec=$?
  set -e
  echo "DRILL[$name] expected_exit!=0 actual_exit=$ec"
  cat "$LOG_DIR/${name}.red.log"
  cp "$SRC_TAURI/gen/apple/pkb-desktop.xcodeproj/project.pbxproj" "$f"
  local after; after="$(sha_of "$f")"
  [[ "$before" == "$after" ]] || { record "$name" FAIL; return; }
  set +e
  python3 "$ROOT/scripts/ios/check_device_family.py" "$f" >"$LOG_DIR/${name}.green.log" 2>&1
  local gec=$?
  set -e
  cat "$LOG_DIR/${name}.green.log"
  [[ "$ec" -ne 0 && "$gec" -eq 0 ]] && record "$name" PASS || record "$name" FAIL
}

drill_privacy() {
  local name="privacyinfo_reason_mismatch"
  local f="$WORK/PrivacyInfo.xcprivacy"
  cp "$SRC_TAURI/ios/canonical/PrivacyInfo.xcprivacy" "$f"
  local before; before="$(sha_of "$f")"
  echo "DRILL[$name] before_sha=$before"
  python3 - <<'PY' "$f"
import plistlib, re, sys
from pathlib import Path
raw = re.sub(br"<!--.*?-->", b"", Path(sys.argv[1]).read_bytes(), flags=re.S)
pl = plistlib.loads(raw)
pl["NSPrivacyAccessedAPITypes"][0]["NSPrivacyAccessedAPITypeReasons"] = ["0A2A.1"]
Path(sys.argv[1]).write_bytes(plistlib.dumps(pl))
PY
  echo "DRILL[$name] mutate: reason mismatch"
  set +e
  python3 "$ROOT/scripts/ios/check_privacy_inventory.py" "$f" \
    "$SRC_TAURI/ios/policy/ios-release.policy.json" >"$LOG_DIR/${name}.red.log" 2>&1
  local ec=$?
  set -e
  echo "DRILL[$name] expected_exit!=0 actual_exit=$ec"
  cat "$LOG_DIR/${name}.red.log"
  cp "$SRC_TAURI/ios/canonical/PrivacyInfo.xcprivacy" "$f"
  local after; after="$(sha_of "$f")"
  [[ "$before" == "$after" ]] || { record "$name" FAIL; return; }
  set +e
  python3 "$ROOT/scripts/ios/check_privacy_inventory.py" "$f" \
    "$SRC_TAURI/ios/policy/ios-release.policy.json" >"$LOG_DIR/${name}.green.log" 2>&1
  local gec=$?
  set -e
  [[ "$ec" -ne 0 && "$gec" -eq 0 ]] && record "$name" PASS || record "$name" FAIL
}

drill_export_method() {
  local name="export_method_debugging"
  local f="$WORK/ExportOptions.plist"
  local golden="$WORK/ExportOptions.plist.golden"
  cat >"$golden" <<'EOF'
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict><key>method</key><string>app-store-connect</string></dict></plist>
EOF
  cp "$golden" "$f"
  local before; before="$(sha_of "$f")"
  echo "DRILL[$name] before_sha=$before"
  /usr/libexec/PlistBuddy -c 'Set :method debugging' "$f"
  echo "DRILL[$name] mutate: method=debugging"
  set +e
  python3 "$ROOT/scripts/ios/check_export_method.py" "$f" >"$LOG_DIR/${name}.red.log" 2>&1
  local ec=$?
  set -e
  echo "DRILL[$name] expected_exit!=0 actual_exit=$ec"
  cp "$golden" "$f"
  local after; after="$(sha_of "$f")"
  echo "DRILL[$name] after_restore_sha=$after"
  [[ "$before" == "$after" ]] || { record "$name" FAIL; return; }
  set +e
  python3 "$ROOT/scripts/ios/check_export_method.py" "$f" >"$LOG_DIR/${name}.green.log" 2>&1
  local gec=$?
  set -e
  echo "DRILL[$name] restore_green_exit=$gec"
  [[ "$ec" -ne 0 && "$gec" -eq 0 ]] && record "$name" PASS || record "$name" FAIL
}

drill_plist_lf() {
  local name="plist_trailing_lf_byte"
  local f="$WORK/Info.plist"
  printf '%s\n' '<?xml version="1.0" encoding="UTF-8"?>' '<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">' '<plist version="1.0">' '<dict/>' '</plist>' >"$f"
  local before; before="$(sha_of "$f")"
  echo "DRILL[$name] before_sha=$before"
  perl -pi -e 'chomp if eof' "$f"
  echo "DRILL[$name] mutate: strip trailing LF"
  set +e
  python3 "$ROOT/scripts/ios/check_plist_trailing_lf.py" "$f" >"$LOG_DIR/${name}.red.log" 2>&1
  local ec=$?
  set -e
  echo "DRILL[$name] expected_exit!=0 actual_exit=$ec"
  # restore exact bytes
  printf '%s\n' '<?xml version="1.0" encoding="UTF-8"?>' '<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">' '<plist version="1.0">' '<dict/>' '</plist>' >"$f"
  local after; after="$(sha_of "$f")"
  echo "DRILL[$name] after_restore_sha=$after"
  [[ "$before" == "$after" ]] || { record "$name" FAIL; return; }
  set +e
  python3 "$ROOT/scripts/ios/check_plist_trailing_lf.py" "$f" >"$LOG_DIR/${name}.green.log" 2>&1
  local gec=$?
  set -e
  echo "DRILL[$name] restore_green_exit=$gec"
  [[ "$ec" -ne 0 && "$gec" -eq 0 ]] && record "$name" PASS || record "$name" FAIL
}

drill_archive_s4_s5() {
  local name="archive_plist_network_key"
  local name2="archive_binary_hmr_inject"
  local src_app=""
  if [[ -n "$FRESH_SIGNED_APP" ]]; then
    src_app="$FRESH_SIGNED_APP"
  elif [[ -d "$LEGACY_APP" ]]; then
    src_app="$LEGACY_APP"
    echo "NOTE: using legacy archive only as disposable RED fixture source (not fresh-signed GREEN evidence)"
  else
    echo "DRILL[$name]: no archive fixture"
    record "$name" BLOCKED_EXTERNAL_PREREQUISITE
    record "$name2" BLOCKED_EXTERNAL_PREREQUISITE
    return
  fi

  local APP="$WORK/Coraxis.app"
  rm -rf "$APP"
  ditto "$src_app" "$APP"
  local PLIST="$APP/Info.plist"
  local before; before="$(sha_of "$PLIST")"
  echo "DRILL[$name] before_sha=$before"
  /usr/libexec/PlistBuddy -c 'Add :NSAppTransportSecurity dict' "$PLIST" 2>/dev/null || true
  /usr/libexec/PlistBuddy -c 'Add :NSAppTransportSecurity:NSAllowsLocalNetworking bool true' "$PLIST" 2>/dev/null || true
  echo "DRILL[$name] mutate: inject NSAppTransportSecurity"
  set +e
  env T4B_ONLY=S-4 bash "$SCAN" "$APP" >"$LOG_DIR/${name}.red.log" 2>&1
  local ec=$?
  set -e
  echo "DRILL[$name] expected_exit=24 actual_exit=$ec"
  tail -30 "$LOG_DIR/${name}.red.log"
  cp "$src_app/Info.plist" "$PLIST"
  local after; after="$(sha_of "$PLIST")"
  [[ "$before" == "$after" ]] || { record "$name" FAIL; return; }
  set +e
  env T4B_ONLY=S-4 bash "$SCAN" "$APP" >"$LOG_DIR/${name}.green.log" 2>&1
  local gec=$?
  set -e
  echo "DRILL[$name] restore_green_exit=$gec"
  tail -20 "$LOG_DIR/${name}.green.log"
  [[ "$ec" -eq 24 && "$gec" -eq 0 ]] && record "$name" PASS || record "$name" FAIL

  # S-5 binary
  local BIN
  BIN="$(find "$APP" -maxdepth 1 -type f -perm +111 | head -1)"
  [[ -n "$BIN" ]] || { record "$name2" FAIL; return; }
  local bbefore; bbefore="$(sha_of "$BIN")"
  printf 'http://127.0.0.1:1421' >>"$BIN"
  echo "DRILL[$name2] mutate: append HMR URL"
  set +e
  env T4B_ONLY=S-5 bash "$SCAN" "$APP" >"$LOG_DIR/${name2}.red.log" 2>&1
  local ec2=$?
  set -e
  echo "DRILL[$name2] expected_exit=25 actual_exit=$ec2"
  grep -E 'binary_hits|S-5' "$LOG_DIR/${name2}.red.log" | head -20 || true
  cp "$(find "$src_app" -maxdepth 1 -type f -perm +111 | head -1)" "$BIN"
  local bafter; bafter="$(sha_of "$BIN")"
  [[ "$bbefore" == "$bafter" ]] || { record "$name2" FAIL; return; }
  set +e
  env T4B_ONLY=S-5 bash "$SCAN" "$APP" >"$LOG_DIR/${name2}.green.log" 2>&1
  local gec2=$?
  set -e
  [[ "$ec2" -eq 25 && "$gec2" -eq 0 ]] && record "$name2" PASS || record "$name2" FAIL

  # Fresh signed full gate GREEN
  if [[ -n "$FRESH_SIGNED_APP" ]]; then
    set +e
    unset T4B_ONLY
    bash "$SCAN" "$FRESH_SIGNED_APP" >"$LOG_DIR/fresh_signed_full_green.log" 2>&1
    local fec=$?
    set -e
    [[ "$fec" -eq 0 ]] && grep -q 'GATE: ALL GREEN (S-1..S-7)' "$LOG_DIR/fresh_signed_full_green.log" \
      && record fresh_signed_full_gate PASS || record fresh_signed_full_gate FAIL
  else
    echo "fresh signed full gate: BLOCKED_EXTERNAL_PREREQUISITE"
    record fresh_signed_full_gate BLOCKED_EXTERNAL_PREREQUISITE
  fi
}

drill_gguf_absent_fixture() {
  local name="gguf_stage_archive_absent"
  # Disposable ROOT layout + copy of unmodified gate script (no real stage mv)
  local FIX="$WORK/gguf_fix"
  mkdir -p "$FIX/scripts" \
    "$FIX/apps/desktop/models" \
    "$FIX/apps/desktop/src-tauri/gen/apple/assets/models" \
    "$FIX/apps/desktop/src-tauri/gen/apple/build/pkb-desktop_iOS.xcarchive/Products/Applications/Coraxis.app/assets/models"
  cp "$GGUF_GATE" "$FIX/scripts/gguf_three_point_sha_gate.sh"
  # SOURCE present OK
  if [[ -f "$ROOT/apps/desktop/models/pocket-brain.gguf" ]]; then
    ln -s "$ROOT/apps/desktop/models/pocket-brain.gguf" "$FIX/apps/desktop/models/pocket-brain.gguf"
  else
    echo "DRILL[$name]: no source GGUF"
    record "$name" BLOCKED_EXTERNAL_PREREQUISITE
    return
  fi
  # STAGE/ARCHIVE intentionally absent
  set +e
  GGUF_GATE_SCRIPT="$FIX/scripts/gguf_three_point_sha_gate.sh" \
    bash "$GGUF_REQ" >"$LOG_DIR/${name}.red.log" 2>&1
  local ec=$?
  set -e
  echo "DRILL[$name] expected_exit!=0 actual_exit=$ec"
  cat "$LOG_DIR/${name}.red.log" | tail -20
  # Restore GREEN on fixture: populate STAGE+ARCHIVE via symlinks to source
  mkdir -p "$FIX/apps/desktop/src-tauri/gen/apple/assets/models"
  ln -sf "$ROOT/apps/desktop/models/pocket-brain.gguf" \
    "$FIX/apps/desktop/src-tauri/gen/apple/assets/models/pocket-brain.gguf"
  ln -sf "$ROOT/apps/desktop/models/pocket-brain.gguf" \
    "$FIX/apps/desktop/src-tauri/gen/apple/build/pkb-desktop_iOS.xcarchive/Products/Applications/Coraxis.app/assets/models/pocket-brain.gguf"
  set +e
  GGUF_GATE_SCRIPT="$FIX/scripts/gguf_three_point_sha_gate.sh" \
    bash "$GGUF_REQ" >"$LOG_DIR/${name}.green.log" 2>&1
  local gec=$?
  set -e
  cat "$LOG_DIR/${name}.green.log" | tail -15
  [[ "$ec" -ne 0 && "$gec" -eq 0 ]] && record "$name" PASS || record "$name" FAIL
}

drill_gguf_mutate_fixture() {
  local name="gguf_one_byte_mutation"
  local FIX="$WORK/gguf_mut"
  mkdir -p "$FIX/scripts" "$FIX/apps/desktop/models" \
    "$FIX/apps/desktop/src-tauri/gen/apple/assets/models" \
    "$FIX/apps/desktop/src-tauri/gen/apple/build/pkb-desktop_iOS.xcarchive/Products/Applications/Coraxis.app/assets/models"
  cp "$GGUF_GATE" "$FIX/scripts/gguf_three_point_sha_gate.sh"
  if [[ ! -f "$ROOT/apps/desktop/models/pocket-brain.gguf" ]]; then
    record "$name" BLOCKED_EXTERNAL_PREREQUISITE
    return
  fi
  ln -s "$ROOT/apps/desktop/models/pocket-brain.gguf" "$FIX/apps/desktop/models/pocket-brain.gguf"
  # STAGE is a real disposable copy we can mutate
  cp "$ROOT/apps/desktop/models/pocket-brain.gguf" \
    "$FIX/apps/desktop/src-tauri/gen/apple/assets/models/pocket-brain.gguf"
  ln -s "$ROOT/apps/desktop/models/pocket-brain.gguf" \
    "$FIX/apps/desktop/src-tauri/gen/apple/build/pkb-desktop_iOS.xcarchive/Products/Applications/Coraxis.app/assets/models/pocket-brain.gguf"
  local STAGE="$FIX/apps/desktop/src-tauri/gen/apple/assets/models/pocket-brain.gguf"
  local before; before="$(sha_of "$STAGE")"
  python3 - <<'PY' "$STAGE"
import sys
from pathlib import Path
p = Path(sys.argv[1])
b = bytearray(p.read_bytes())
b[0] ^= 0x01
p.write_bytes(bytes(b))
PY
  echo "DRILL[$name] mutate: XOR first byte of disposable STAGE"
  set +e
  bash "$FIX/scripts/gguf_three_point_sha_gate.sh" >"$LOG_DIR/${name}.red.log" 2>&1
  local ec=$?
  set -e
  echo "DRILL[$name] expected_exit=13 actual_exit=$ec"
  cat "$LOG_DIR/${name}.red.log" | tail -15
  cp "$ROOT/apps/desktop/models/pocket-brain.gguf" "$STAGE"
  local after; after="$(sha_of "$STAGE")"
  [[ "$before" == "$after" ]] || { record "$name" FAIL; return; }
  set +e
  GGUF_GATE_SCRIPT="$FIX/scripts/gguf_three_point_sha_gate.sh" \
    bash "$GGUF_REQ" >"$LOG_DIR/${name}.green.log" 2>&1
  local gec=$?
  set -e
  [[ "$ec" -eq 13 && "$gec" -eq 0 ]] && record "$name" PASS || record "$name" FAIL
}

drill_get_task_allow() {
  local name="get_task_allow_true_fixture"
  local f="$WORK/entitlements.plist"
  local golden="$WORK/entitlements.plist.golden"
  cat >"$golden" <<'EOF'
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict><key>get-task-allow</key><false/></dict></plist>
EOF
  cp "$golden" "$f"
  local before; before="$(sha_of "$f")"
  echo "DRILL[$name] before_sha=$before"
  /usr/libexec/PlistBuddy -c 'Set :get-task-allow true' "$f"
  echo "DRILL[$name] mutate: get-task-allow=true"
  set +e
  python3 "$ROOT/scripts/ios/check_get_task_allow.py" "$f" >"$LOG_DIR/${name}.red.log" 2>&1
  local ec=$?
  set -e
  echo "DRILL[$name] expected_exit!=0 actual_exit=$ec"
  cp "$golden" "$f"
  local after; after="$(sha_of "$f")"
  echo "DRILL[$name] after_restore_sha=$after"
  [[ "$before" == "$after" ]] || { record "$name" FAIL; return; }
  set +e
  python3 "$ROOT/scripts/ios/check_get_task_allow.py" "$f" >"$LOG_DIR/${name}.green.log" 2>&1
  local gec=$?
  set -e
  echo "DRILL[$name] restore_green_exit=$gec"
  [[ "$ec" -ne 0 && "$gec" -eq 0 ]] && record "$name" PASS || record "$name" FAIL
}

drill_dev_overlay
drill_device_family
drill_privacy
drill_export_method
drill_plist_lf
drill_archive_s4_s5
drill_gguf_absent_fixture
drill_gguf_mutate_fixture
drill_get_task_allow

echo "=== MUTATION V-3 summary pass=$pass_count fail=$fail_count blocked=$blocked_count ==="
[[ "$fail_count" -eq 0 ]]
exit 0
