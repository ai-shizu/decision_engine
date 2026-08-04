#!/usr/bin/env bash
# T4-D: iOS config / static contract gate (PR lane).
# Measures regenerable canonical iOS config + FORCE_COLOR matrix + full manifest.
# Does NOT measure signing / Archive/IPA / real GGUF three-point / physical HMR.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
DESKTOP="$ROOT/apps/desktop"
SRC_TAURI="$DESKTOP/src-tauri"
POLICY="$SRC_TAURI/ios/policy/ios-release.policy.json"
CANON_PRIVACY="$SRC_TAURI/ios/canonical/PrivacyInfo.xcprivacy"
GEN_APPLE="$SRC_TAURI/gen/apple"
INFO_IOS="$SRC_TAURI/Info.ios.plist"
LOG_DIR="${T4D_LOG_DIR:-/private/tmp/t4d-logs-fresh}"
mkdir -p "$LOG_DIR"
LOG="$LOG_DIR/ios_config_contract.log"

if [[ "${T4D_CONFIG_GATE_INNER:-}" != 1 ]]; then
  export T4D_CONFIG_GATE_INNER=1
  # Default gate run: FORCE_COLOR must be unset for the recorded GREEN path.
  # `|| EC=$?` is load-bearing: under `set -e` a bare failing command aborts the
  # shell here, so `cat "$LOG"` never runs and a RED gate prints NOTHING at all —
  # the diagnosis is written to $LOG and never reaches the reader. Measured
  # 2026-08-04: exit 1, zero bytes on stdout. Keep the failure path printing.
  EC=0
  env -u FORCE_COLOR -u NO_COLOR -u CLICOLOR -u CLICOLOR_FORCE \
    bash "$0" "$@" >"$LOG" 2>&1 || EC=$?
  cat "$LOG"
  exit "$EC"
fi

die() { echo "ios_config_contract: RED: $*" >&2; exit 1; }
ok() { echo "ios_config_contract: GREEN: $*"; }

echo "=== T4-D iOS config contract ==="
echo "ROOT=$ROOT"
echo "FORCE_COLOR=${FORCE_COLOR-<unset>}"
echo "SIGNING: NOT MEASURED"
echo "ARCHIVE/IPA: NOT MEASURED"
echo "REAL GGUF THREE-POINT: NOT MEASURED"
echo "PHYSICAL-DEVICE HMR: NOT MEASURED"

command -v plutil >/dev/null 2>&1 || die "plutil missing"
command -v python3 >/dev/null 2>&1 || die "python3 missing"
command -v xcodegen >/dev/null 2>&1 || die "xcodegen missing"

python3 "$ROOT/scripts/ios/validate_ios_policy.py" "$POLICY"

EXPECTED_XCODEGEN="$(python3 -c 'import json;print(json.load(open("'"$POLICY"'"))["xcodegen_version"])')"
GOT_XCODEGEN="$(xcodegen --version | awk '{print $2}')"
[[ "$GOT_XCODEGEN" == "$EXPECTED_XCODEGEN" ]] \
  || die "XcodeGen version mismatch got=$GOT_XCODEGEN expected=$EXPECTED_XCODEGEN"
ok "XcodeGen pinned $GOT_XCODEGEN"

# --- Release/dev overlay isolation ---
python3 "$ROOT/scripts/ios/check_release_dev_isolation.py" \
  "$SRC_TAURI/tauri.ios.conf.json" "$SRC_TAURI/tauri.ios.dev.conf.json" \
  "$INFO_IOS" "$SRC_TAURI/Info.ios.dev.plist"
# Extra contract bits beyond isolation checker
python3 - <<'PY' "$SRC_TAURI/tauri.ios.conf.json"
import json, sys
from pathlib import Path
rel = json.loads(Path(sys.argv[1]).read_text(encoding="utf-8"))
ios = rel.get("bundle", {}).get("iOS", {})
if ios.get("minimumSystemVersion") != "17.0":
    raise SystemExit("minimumSystemVersion != 17.0")
if ios.get("infoPlist") != "Info.ios.plist":
    raise SystemExit("infoPlist must be Info.ios.plist")
if ios.get("template") != "src-tauri/ios/xcodegen/project.yml.template":
    raise SystemExit(f"template mismatch: {ios.get('template')!r}")
bv = ios.get("bundleVersion")
if bv is None or not str(bv).isdigit() or int(bv) < 1:
    raise SystemExit(f"bundleVersion must be positive int string: {bv!r}")
print("tauri.ios.conf.json iOS contract: GREEN")
PY
ok "release/dev overlay isolation"
grep -q 'tauri.ios.dev.conf.json' "$DESKTOP/scripts/tauri-ios-dev.sh" || die "tauri-ios-dev.sh overlay missing"
pkg_script="$(python3 -c 'import json;print(json.load(open("'"$DESKTOP/package.json"'"))["scripts"]["tauri:ios-dev"])')"
[[ "$pkg_script" == "bash scripts/tauri-ios-dev.sh" ]] || die "tauri:ios-dev path changed"
ok "tauri:ios-dev unmodified"

# release wrapper rejects argv
if bash "$ROOT/scripts/ios_release_build.sh" --debug >/tmp/t4d_wrap.out 2>&1; then
  die "release wrapper accepted --debug"
fi
grep -q 'arbitrary arguments are forbidden' /tmp/t4d_wrap.out || die "wrapper reject message missing"
ok "release wrapper rejects forbidden argv"

# plutil
while IFS= read -r -d '' f; do
  plutil -lint "$f" >/dev/null || die "plutil failed: $f"
done < <(find "$SRC_TAURI/Info.ios.plist" "$SRC_TAURI/Info.ios.dev.plist" \
  "$SRC_TAURI/ios" "$GEN_APPLE/pkb-desktop_iOS/Info.plist" \
  "$GEN_APPLE/ExportOptions.plist" -name '*.plist' -print0 2>/dev/null)
plutil -lint "$CANON_PRIVACY" >/dev/null || die "PrivacyInfo lint failed"
ok "plutil lint"

# Privacy inventory (shared production checker)
python3 "$ROOT/scripts/ios/check_privacy_inventory.py" "$CANON_PRIVACY" "$POLICY"
python3 "$ROOT/scripts/ios/check_plist_trailing_lf.py" \
  "$GEN_APPLE/pkb-desktop_iOS/Info.plist"
ok "PrivacyInfo inventory + Info.plist trailing LF"

# Tauri CLI lock version
CLI_VER="$(python3 - <<'PY' "$DESKTOP/package-lock.json"
import json, sys
lock = json.load(open(sys.argv[1], encoding="utf-8"))
ver = (lock.get("packages") or {}).get("node_modules/@tauri-apps/cli", {}).get("version")
assert ver
print(ver)
PY
)"
INSTALLED="$(cd "$DESKTOP" && npx --no-install tauri --version 2>/dev/null | head -1 || true)"
echo "$INSTALLED" | grep -F "$CLI_VER" >/dev/null || die "Tauri CLI != lock $CLI_VER ($INSTALLED)"
ok "Tauri CLI $CLI_VER"

# --- FORCE_COLOR matrix: unset / 0 / 1 must byte-match ---
MATRIX_ROOT="$(mktemp -d "${TMPDIR:-/tmp}/t4d-fc-matrix.XXXXXX")"
run_matrix() {
  local label="$1"
  shift
  local out="$MATRIX_ROOT/$label"
  echo "FORCE_COLOR matrix: generating label=$label ($*)"
  # shellcheck disable=SC2086
  env "$@" bash "$ROOT/scripts/ios_regenerate_tree.sh" "$out/apple"
}
run_matrix unset -u FORCE_COLOR
run_matrix zero FORCE_COLOR=0
run_matrix one FORCE_COLOR=1

python3 "$ROOT/scripts/ios/ios_cmp_gen_apple_manifest.py" \
  "$MATRIX_ROOT/unset/apple" "$MATRIX_ROOT/zero/apple"
python3 "$ROOT/scripts/ios/ios_cmp_gen_apple_manifest.py" \
  "$MATRIX_ROOT/unset/apple" "$MATRIX_ROOT/one/apple"
# Assert literal ${FORCE_COLOR} present; bare 0/1 absent in rust script
for label in unset zero one; do
  PBX="$MATRIX_ROOT/$label/apple/pkb-desktop.xcodeproj/project.pbxproj"
  grep -q '\${FORCE_COLOR}' "$PBX" || die "matrix $label missing literal \${FORCE_COLOR}"
  if grep -E -- '--configuration \$\{CONFIGURATION:\?\} (0|1) \$\{ARCHS' "$PBX" >/dev/null; then
    die "matrix $label has bare FORCE_COLOR positional"
  fi
  grep -q 'Coraxis.app' "$PBX" || die "matrix $label missing Coraxis.app"
  grep -q 'pkb-desktop_iOS.app' "$PBX" && die "matrix $label still has pkb-desktop_iOS.app" || true
done
ok "FORCE_COLOR matrix byte-identical (unset/0/1)"

# --- Two fresh trees + tracked full manifest ---
TREE_A="$(mktemp -d "${TMPDIR:-/tmp}/t4d-tree-a.XXXXXX")"
TREE_B="$(mktemp -d "${TMPDIR:-/tmp}/t4d-tree-b.XXXXXX")"
env -u FORCE_COLOR bash "$ROOT/scripts/ios_regenerate_tree.sh" "$TREE_A/apple"
env -u FORCE_COLOR bash "$ROOT/scripts/ios_regenerate_tree.sh" "$TREE_B/apple"
python3 "$ROOT/scripts/ios/ios_cmp_gen_apple_manifest.py" "$TREE_A/apple" "$TREE_B/apple"
python3 "$ROOT/scripts/ios/ios_cmp_gen_apple_manifest.py" "$TREE_A/apple" "$GEN_APPLE"
ok "full gen/apple manifest A==B==tracked (build/ excluded)"

# --- Generated Info.plist 3 keys ---
python3 - <<'PY' "$GEN_APPLE/pkb-desktop_iOS/Info.plist" "$POLICY" "$INFO_IOS" "$SRC_TAURI/ios/decisions/export-compliance.decision.json"
import plistlib, json, re, sys
from pathlib import Path
gen = plistlib.loads(re.sub(br"<!--.*?-->", b"", Path(sys.argv[1]).read_bytes(), flags=re.S))
pol = json.loads(Path(sys.argv[2]).read_text(encoding="utf-8"))
src = plistlib.loads(re.sub(br"<!--.*?-->", b"", Path(sys.argv[3]).read_bytes(), flags=re.S))
expo = json.loads(Path(sys.argv[4]).read_text(encoding="utf-8"))
for k in pol["required_info_plist_keys"]:
    if k not in gen:
        raise SystemExit(f"generated Info.plist missing {k}")
    if k not in src:
        raise SystemExit(f"source Info.ios.plist missing {k}")
    if gen[k] != src[k]:
        raise SystemExit(f"{k} gen!=source: {gen[k]!r} vs {src[k]!r}")
# ITS: never treat UNAPPROVED as ratified
if expo["status"] != "APPROVED":
    print("EXPORT_COMPLIANCE: RED (UNAPPROVED) — Archive must not claim ITS match as approved")
    if expo.get("its_app_uses_non_exempt_encryption") is not None:
        raise SystemExit("decision boolean must be null while UNAPPROVED")
else:
    if gen["ITSAppUsesNonExemptEncryption"] != expo["its_app_uses_non_exempt_encryption"]:
        raise SystemExit("APPROVED ITS boolean mismatch vs generated plist")
print("Info.plist 3-key contract: GREEN")
PY

# PrivacyInfo exact root path + Resources membership (structural)
[[ -f "$GEN_APPLE/PrivacyInfo.xcprivacy" ]] || die "PrivacyInfo.xcprivacy not at gen/apple root"
grep -q 'path: PrivacyInfo.xcprivacy' "$GEN_APPLE/project.yml" || die "project.yml PrivacyInfo path missing"
grep -q 'PrivacyInfo.xcprivacy in Resources' "$GEN_APPLE/pkb-desktop.xcodeproj/project.pbxproj" \
  || die "pbxproj PrivacyInfo Resources membership missing"
cmp -s "$CANON_PRIVACY" "$GEN_APPLE/PrivacyInfo.xcprivacy" || die "PrivacyInfo bytes != canonical"
ok "PrivacyInfo exact root path + membership"

# Product Coraxis.app
grep -q 'Coraxis.app' "$GEN_APPLE/pkb-desktop.xcodeproj/project.pbxproj" || die "pbxproj missing Coraxis.app"
grep -q 'pkb-desktop_iOS.app' "$GEN_APPLE/pkb-desktop.xcodeproj/project.pbxproj" \
  && die "pbxproj still has pkb-desktop_iOS.app" || true
SCHEME="$GEN_APPLE/pkb-desktop.xcodeproj/xcshareddata/xcschemes/pkb-desktop_iOS.xcscheme"
grep -q 'BuildableName = "Coraxis.app"' "$SCHEME" || die "scheme BuildableName != Coraxis.app"
ok "product identity Coraxis.app"

# device family
grep -E 'TARGETED_DEVICE_FAMILY *= *"?1"?' "$GEN_APPLE/pkb-desktop.xcodeproj/project.pbxproj" >/dev/null \
  || die "TARGETED_DEVICE_FAMILY != 1"
grep -E 'TARGETED_DEVICE_FAMILY *= *"1,2"' "$GEN_APPLE/pkb-desktop.xcodeproj/project.pbxproj" >/dev/null \
  && die "TARGETED_DEVICE_FAMILY still 1,2" || true

echo "=== iOS config contract: ALL STATIC CHECKS GREEN ==="
echo "SIGNING: NOT MEASURED"
echo "ARCHIVE/IPA: NOT MEASURED"
echo "REAL GGUF THREE-POINT: NOT MEASURED"
echo "PHYSICAL-DEVICE HMR: NOT MEASURED"
exit 0
