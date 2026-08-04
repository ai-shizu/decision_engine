#!/usr/bin/env bash
# T4-D: Archive / IPA acceptance after a fresh signed build.
# Invokes unmodified ios_archive_scan.sh + fail-closed GGUF wrapper.
# Undefined instrument exit codes => INSTRUMENT_ERROR.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
POLICY="$ROOT/apps/desktop/src-tauri/ios/policy/ios-release.policy.json"
CANON_PRIVACY="$ROOT/apps/desktop/src-tauri/ios/canonical/PrivacyInfo.xcprivacy"
ARCHIVE_APP="${1:-$ROOT/apps/desktop/src-tauri/gen/apple/build/pkb-desktop_iOS.xcarchive/Products/Applications/Coraxis.app}"
IPA="${2:-$ROOT/apps/desktop/src-tauri/gen/apple/build/arm64/Coraxis.ipa}"
LOG_DIR="${T4D_LOG_DIR:-/private/tmp/t4d-logs}"
mkdir -p "$LOG_DIR"

die() { echo "ios_signed_archive_accept: RED: $*" >&2; exit 1; }
instrument_error() { echo "ios_signed_archive_accept: INSTRUMENT_ERROR: $*" >&2; exit 99; }

[[ -d "$ARCHIVE_APP" ]] || die "ARCHIVE_APP ABSENT: $ARCHIVE_APP"
[[ -f "$IPA" ]] || die "IPA ABSENT: $IPA"

require_cmd() {
  for c in "$@"; do
    command -v "$c" >/dev/null 2>&1 || instrument_error "missing command $c"
  done
}
require_cmd file nm otool plutil codesign shasum python3 unzip ditto

# --- explicit archive scan (T4B_ONLY unset) ---
SCAN_LOG="$LOG_DIR/archive_scan.log"
unset T4B_ONLY || true
set +e
bash "$ROOT/scripts/ios_archive_scan.sh" "$ARCHIVE_APP" >"$SCAN_LOG" 2>&1
SCAN_EC=$?
set -e
cat "$SCAN_LOG"
case "$SCAN_EC" in
  0) ;;
  1|21|22|23|24|25|26|27|28) die "ios_archive_scan exit=$SCAN_EC" ;;
  *) instrument_error "ios_archive_scan undefined exit=$SCAN_EC" ;;
esac
grep -q 'POSITIVE_CONTROLS: all fired' "$SCAN_LOG" || die "POSITIVE_CONTROLS line missing"
grep -q 'GATE: ALL GREEN (S-1..S-7)' "$SCAN_LOG" || die "final GATE line missing"
grep -q 'S-4: GREEN count=0' "$SCAN_LOG" || die "S-4 GREEN count=0 missing"
grep -Eq 'S-5:.*binary_hits: 0' "$SCAN_LOG" || grep -Eq 'binary_hits: 0' "$SCAN_LOG" \
  || die "S-5 binary_hits: 0 missing"
grep -Eq 'plist_hits: 0' "$SCAN_LOG" || die "S-5 plist_hits: 0 missing"

# --- GGUF fail-closed ---
GGUF_LOG="$LOG_DIR/gguf_three_point_require_present.log"
set +e
bash "$ROOT/scripts/ios_gguf_three_point_require_present.sh" >"$GGUF_LOG" 2>&1
GGUF_EC=$?
set -e
cat "$GGUF_LOG"
[[ "$GGUF_EC" -eq 0 ]] || die "gguf require-present exit=$GGUF_EC"

# --- IPA expand + full scan ---
IPA_WORK="$(mktemp -d "${TMPDIR:-/tmp}/t4d-ipa.XXXXXX")"
cleanup() { rm -rf "$IPA_WORK"; }
trap cleanup EXIT
ditto -x -k "$IPA" "$IPA_WORK" || instrument_error "ditto IPA extract failed"
IPA_APP="$IPA_WORK/Payload/Coraxis.app"
[[ -d "$IPA_APP" ]] || die "Payload/Coraxis.app ABSENT after IPA extract"

IPA_SCAN_LOG="$LOG_DIR/ipa_scan.log"
unset T4B_ONLY || true
set +e
bash "$ROOT/scripts/ios_archive_scan.sh" "$IPA_APP" >"$IPA_SCAN_LOG" 2>&1
IPA_SCAN_EC=$?
set -e
cat "$IPA_SCAN_LOG"
case "$IPA_SCAN_EC" in
  0) ;;
  1|21|22|23|24|25|26|27|28) die "IPA ios_archive_scan exit=$IPA_SCAN_EC" ;;
  *) instrument_error "IPA ios_archive_scan undefined exit=$IPA_SCAN_EC" ;;
esac
grep -q 'GATE: ALL GREEN (S-1..S-7)' "$IPA_SCAN_LOG" || die "IPA GATE line missing"

# --- tool self-exit 0 ---
BIN="$ARCHIVE_APP/Coraxis"
[[ -f "$BIN" ]] || BIN="$(find "$ARCHIVE_APP" -maxdepth 1 -type f -perm +111 | head -1)"
[[ -n "$BIN" && -f "$BIN" ]] || die "app binary ABSENT"
file "$BIN" >/dev/null || instrument_error "file failed"
nm -u "$BIN" >/dev/null || instrument_error "nm failed"
otool -L "$BIN" >/dev/null || instrument_error "otool failed"
plutil -lint "$ARCHIVE_APP/Info.plist" >/dev/null || instrument_error "plutil Info.plist failed"

# --- codesign ---
codesign --verify --deep --strict "$ARCHIVE_APP" || die "codesign --verify archive app failed"
codesign --verify --deep --strict "$IPA_APP" || die "codesign --verify IPA app failed"

CS_OUT="$(codesign -dvvv "$ARCHIVE_APP" 2>&1 || true)"
echo "$CS_OUT" | grep -Eq 'Authority=Apple Distribution|Authority=iPhone Distribution|Authority=iOS Distribution' \
  || die "Apple Distribution authority not found"

# --- entitlements / get-task-allow ---
ENT_PLIST="$LOG_DIR/archive.entitlements.plist"
codesign -d --entitlements :- "$ARCHIVE_APP" >"$ENT_PLIST" 2>/dev/null \
  || die "codesign entitlements dump failed"
python3 - <<'PY' "$ENT_PLIST"
import plistlib, sys
from pathlib import Path
raw = Path(sys.argv[1]).read_bytes()
pl = plistlib.loads(raw)
# get-task-allow must be false or absent
if pl.get("get-task-allow") is True:
    raise SystemExit("get-task-allow=true")
for k in list(pl):
    if "network" in k.lower() or "associated-domains" in k:
        raise SystemExit(f"network entitlement present: {k}")
print("entitlements network=0 get-task-allow OK")
PY

# --- Info.plist device family / min OS / required 3 keys / ITS vs APPROVED decision ---
python3 - <<'PY' "$ARCHIVE_APP/Info.plist" "$POLICY" \
  "$ROOT/apps/desktop/src-tauri/ios/decisions/export-compliance.decision.json" \
  "$ROOT/apps/desktop/src-tauri/Info.ios.plist" \
  "$IPA_APP/Info.plist"
import plistlib, json, sys, re
from pathlib import Path

def load(p):
    return plistlib.loads(re.sub(br"<!--.*?-->", b"", Path(p).read_bytes(), flags=re.S))

arch = load(sys.argv[1])
pol = json.loads(Path(sys.argv[2]).read_text(encoding="utf-8"))
expo = json.loads(Path(sys.argv[3]).read_text(encoding="utf-8"))
src = load(sys.argv[4])
ipa = load(sys.argv[5])

udf = arch.get("UIDeviceFamily")
if udf != [1]:
    raise SystemExit(f"UIDeviceFamily != [1]: {udf!r}")
min_os = str(arch.get("MinimumOSVersion") or arch.get("LSMinimumSystemVersion") or "")
if min_os != pol["minimum_ios_version"]:
    raise SystemExit(f"MinimumOSVersion {min_os!r}")
if arch.get("CFBundleIdentifier") != pol["product_bundle_identifier"]:
    raise SystemExit("CFBundleIdentifier mismatch")

for pl_name, pl in (("source", src), ("archive", arch), ("ipa", ipa)):
    for k in pol["required_info_plist_keys"]:
        if k not in pl:
            raise SystemExit(f"{pl_name} Info.plist missing {k}")
    if pl["ITSAppUsesNonExemptEncryption"] != src["ITSAppUsesNonExemptEncryption"]:
        raise SystemExit(f"{pl_name} ITS != source")

if expo.get("status") != "APPROVED":
    raise SystemExit("EXPORT_COMPLIANCE: RED (UNAPPROVED) — archive accept refused")
for field in ("decision_id", "approver", "jurisdiction", "crypto_inventory_digest"):
    if expo.get(field) in (None, ""):
        raise SystemExit(f"EXPORT_COMPLIANCE missing {field}")
if expo.get("its_app_uses_non_exempt_encryption") is None:
    raise SystemExit("EXPORT_COMPLIANCE boolean null")
if arch["ITSAppUsesNonExemptEncryption"] != expo["its_app_uses_non_exempt_encryption"]:
    raise SystemExit("archive ITS != APPROVED decision boolean")
if ipa["ITSAppUsesNonExemptEncryption"] != expo["its_app_uses_non_exempt_encryption"]:
    raise SystemExit("IPA ITS != APPROVED decision boolean")
if src["ITSAppUsesNonExemptEncryption"] != expo["its_app_uses_non_exempt_encryption"]:
    raise SystemExit("source ITS != APPROVED decision boolean")
print("Info.plist 3-key + ITS APPROVED match across source/archive/IPA: GREEN")
PY

# --- eligibility policy match (capabilities) ---
python3 - <<'PY' "$ARCHIVE_APP/Info.plist" "$POLICY" "$ROOT/apps/desktop/src-tauri/ios/decisions/eligibility.decision.json"
import plistlib, json, sys, re
from pathlib import Path
raw = Path(sys.argv[1]).read_bytes()
raw = re.sub(br"<!--.*?-->", b"", raw, flags=re.S)
pl = plistlib.loads(raw)
pol = json.loads(Path(sys.argv[2]).read_text(encoding="utf-8"))
elig = json.loads(Path(sys.argv[3]).read_text(encoding="utf-8"))
caps = list(pl.get("UIRequiredDeviceCapabilities") or [])
base = list(pol["required_device_capabilities_base"])
if elig.get("status") != "APPROVED":
    raise SystemExit("RELEASE_ELIGIBILITY: RED (UNAPPROVED) — archive accept refused")
if elig.get("option") == "A":
    need = base + ["iphone-performance-gaming-tier"]
else:
    need = base
if sorted(caps) != sorted(need):
    raise SystemExit(f"capabilities {caps} != required {need}")
print("eligibility capabilities match")
PY

# --- PrivacyInfo exact root path only (no nested find fallback) ---
PRIV_ARCH="$ARCHIVE_APP/PrivacyInfo.xcprivacy"
[[ -f "$PRIV_ARCH" ]] || die "PrivacyInfo.xcprivacy ABSENT at archive app root (exact path)"
PRIV_IPA="$IPA_APP/PrivacyInfo.xcprivacy"
[[ -f "$PRIV_IPA" ]] || die "PrivacyInfo.xcprivacy ABSENT at IPA app root (exact path)"
CANON_SHA="$(shasum -a 256 "$CANON_PRIVACY" | awk '{print $1}')"
ARCH_SHA="$(shasum -a 256 "$PRIV_ARCH" | awk '{print $1}')"
IPA_SHA="$(shasum -a 256 "$PRIV_IPA" | awk '{print $1}')"
[[ "$CANON_SHA" == "$ARCH_SHA" ]] || die "PrivacyInfo SHA mismatch canonical vs archive"
[[ "$CANON_SHA" == "$IPA_SHA" ]] || die "PrivacyInfo SHA mismatch canonical vs IPA"

# --- ExportOptions exact path only (no find fallback) ---
EXPORT_OPTS="${EXPORT_OPTIONS_PATH:-$ROOT/apps/desktop/src-tauri/gen/apple/build/arm64/ExportOptions.plist}"
[[ -f "$EXPORT_OPTS" ]] || die "ExportOptions.plist ABSENT at exact path: $EXPORT_OPTS"
python3 "$ROOT/scripts/ios/check_export_method.py" "$EXPORT_OPTS"

# --- Mach-O device arm64 only ---
FILE_OUT="$(file "$BIN")"
echo "$FILE_OUT"
echo "$FILE_OUT" | grep -qi 'arm64' || die "binary not arm64"
if echo "$FILE_OUT" | grep -qi 'x86_64'; then
  die "binary contains x86_64 slice"
fi
# lipo if available
if command -v lipo >/dev/null 2>&1; then
  LIPO_OUT="$(lipo -info "$BIN" 2>&1 || true)"
  echo "$LIPO_OUT"
  echo "$LIPO_OUT" | grep -q 'arm64' || die "lipo missing arm64"
  if echo "$LIPO_OUT" | grep -Eq 'x86_64|armv7'; then
    die "lipo reports non-device slice"
  fi
fi

# --- IPA GGUF size/SHA ---
IPA_GGUF="$(find "$IPA_APP" -name pocket-brain.gguf | head -1 || true)"
[[ -n "$IPA_GGUF" && -f "$IPA_GGUF" ]] || die "IPA GGUF ABSENT"
EXPECTED_SIZE=1117320736
EXPECTED_SHA=6a1a2eb6d15622bf3c96857206351ba97e1af16c30d7a74ee38970e434e9407e
SIZE="$(wc -c <"$IPA_GGUF" | tr -d ' ')"
SHA="$(shasum -a 256 "$IPA_GGUF" | awk '{print $1}')"
[[ "$SIZE" == "$EXPECTED_SIZE" && "$SHA" == "$EXPECTED_SHA" ]] \
  || die "IPA GGUF mismatch size=$SIZE sha=$SHA"

echo "ios_signed_archive_accept: ALL ACCEPTANCE CHECKS GREEN"
exit 0
