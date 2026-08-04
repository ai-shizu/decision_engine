#!/usr/bin/env bash
# T4-D D-3: archive-scan-wiring — prove ios_archive_scan.sh is CI-reachable
# via hosted fixtures (T4B_ONLY=S-N). Does NOT modify ios_archive_scan.sh.
#
# Exit 0 only when every expected RED/GREEN exit matches.
# Prints SCANNED / S-3 / S-6 / NOT MEASURED scope lines (directive §5.2 / user §2.7).
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
SCAN="$ROOT/scripts/ios_archive_scan.sh"
FIX_ROOT="$(mktemp -d "${TMPDIR:-/tmp}/t4d-archive-scan-wiring.XXXXXX")"
cleanup() { rm -rf "$FIX_ROOT"; }
trap cleanup EXIT

chmod +x "$SCAN" 2>/dev/null || true

APP_BASE() {
  local name="$1"
  local app="$FIX_ROOT/${name}/Coraxis.app"
  mkdir -p "$app"
  printf '%s\n' "$app"
}

write_clean_plist() {
  local plist="$1"
  cat >"$plist" <<'EOF'
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
	<key>CFBundleExecutable</key>
	<string>Coraxis</string>
	<key>CFBundleIdentifier</key>
	<string>app.coraxis.archive-scan-wiring</string>
	<key>CFBundleName</key>
	<string>Coraxis</string>
	<key>CFBundlePackageType</key>
	<string>APPL</string>
	<key>CFBundleVersion</key>
	<string>1</string>
</dict>
</plist>
EOF
}

build_clean_bin() {
  local out="$1"
  local src="$FIX_ROOT/_clean.c"
  cat >"$src" <<'EOF'
int main(void) { return 0; }
EOF
  clang -o "$out" "$src"
}

# --- fixture builders ---

mk_green_base() {
  local name="$1"
  local app
  app="$(APP_BASE "$name")"
  build_clean_bin "$app/Coraxis"
  write_clean_plist "$app/Info.plist"
  # Future-dated synthetic provision (self-signed CMS) for S-7 GREEN when present
  printf '%s\n' "$app"
}

sign_cms_provision() {
  # Args: plist_in provision_out
  local plist_in="$1"
  local prov_out="$2"
  local certdir="$FIX_ROOT/_cms_certs"
  mkdir -p "$certdir"
  if [[ ! -f "$certdir/signer.key" ]]; then
    openssl req -x509 -newkey rsa:2048 -nodes \
      -keyout "$certdir/signer.key" -out "$certdir/signer.crt" \
      -days 3650 -subj "/CN=T4D-ArchiveScanWiring-Fixture/" >/dev/null 2>&1
  fi
  # DER CMS detached=no (same shape security cms -D accepts)
  openssl smime -sign -binary -nodetach \
    -signer "$certdir/signer.crt" -inkey "$certdir/signer.key" \
    -in "$plist_in" -outform der -out "$prov_out"
}

mk_s1_red() {
  local app
  app="$(APP_BASE "red-s1")"
  cat >"$FIX_ROOT/_s1.c" <<'EOF'
#include <sys/socket.h>
#include <netdb.h>
#include <arpa/inet.h>
int main(void) {
  int fd = socket(AF_INET, SOCK_STREAM, 0);
  (void)fd;
  struct addrinfo *res = 0;
  getaddrinfo("example.invalid", "80", 0, &res);
  connect(fd, 0, 0);
  return 0;
}
EOF
  clang -o "$app/Coraxis" "$FIX_ROOT/_s1.c"
  write_clean_plist "$app/Info.plist"
  printf '%s\n' "$app"
}

mk_s2_red() {
  local app
  app="$(APP_BASE "red-s2")"
  cat >"$FIX_ROOT/_s2.c" <<'EOF'
#include <Network/Network.h>
int main(void) {
  nw_parameters_t p = nw_parameters_create_secure_tcp(NW_PARAMETERS_DISABLE_PROTOCOL,
                                                   NW_PARAMETERS_DEFAULT_CONFIGURATION);
  (void)p;
  return 0;
}
EOF
  clang -framework Network -o "$app/Coraxis" "$FIX_ROOT/_s2.c"
  write_clean_plist "$app/Info.plist"
  printf '%s\n' "$app"
}

mk_s4_red() {
  local app
  app="$(mk_green_base "red-s4")"
  plutil -insert NSAllowsLocalNetworking -bool true "$app/Info.plist"
  plutil -insert NSLocalNetworkUsageDescription -string "wiring-fixture" "$app/Info.plist"
  printf '%s\n' "$app"
}

mk_s5_red() {
  local app
  app="$(mk_green_base "red-s5")"
  # Plant CIDR into binary resource via overwrite of a data section companion file
  # S-5 greps Coraxis binary and Info.plist — embed string in a tiny .c string literal
  cat >"$FIX_ROOT/_s5.c" <<'EOF'
#include <stdio.h>
static const char *k = "marker 10.0.0.0/8 http://192.168.1.1:1420";
int main(void) { puts(k); return 0; }
EOF
  clang -o "$app/Coraxis" "$FIX_ROOT/_s5.c"
  write_clean_plist "$app/Info.plist"
  printf '%s\n' "$app"
}

mk_s7_red() {
  local app
  app="$(mk_green_base "red-s7")"
  cat >"$FIX_ROOT/_prov_expired.plist" <<'EOF'
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
	<key>ExpirationDate</key>
	<date>2020-01-01T00:00:00Z</date>
	<key>Name</key>
	<string>t4d-wiring-expired</string>
	<key>UUID</key>
	<string>00000000-0000-4000-8000-000000000001</string>
</dict>
</plist>
EOF
  sign_cms_provision "$FIX_ROOT/_prov_expired.plist" "$app/embedded.mobileprovision"
  printf '%s\n' "$app"
}

mk_s7_green() {
  local app
  app="$(mk_green_base "green-s7")"
  cat >"$FIX_ROOT/_prov_future.plist" <<'EOF'
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
	<key>ExpirationDate</key>
	<date>2099-01-01T00:00:00Z</date>
	<key>Name</key>
	<string>t4d-wiring-future</string>
	<key>UUID</key>
	<string>00000000-0000-4000-8000-000000000002</string>
</dict>
</plist>
EOF
  sign_cms_provision "$FIX_ROOT/_prov_future.plist" "$app/embedded.mobileprovision"
  printf '%s\n' "$app"
}

mk_green_no_prov() {
  local app
  app="$(mk_green_base "green-clean")"
  printf '%s\n' "$app"
}

mk_adhoc_s3() {
  local app
  app="$(mk_green_base "adhoc-s3")"
  codesign -s - --force "$app" >/dev/null 2>&1 || codesign -s - --force "$app"
  printf '%s\n' "$app"
}

expect_exit() {
  local label="$1"
  local expected="$2"
  local only="$3"
  local app="$4"
  local log="$FIX_ROOT/logs/${label}.log"
  mkdir -p "$FIX_ROOT/logs"
  set +e
  T4B_ONLY="$only" bash "$SCAN" "$app" >"$log" 2>&1
  local rc=$?
  set -e
  echo "FIXTURE ${label}: T4B_ONLY=${only} expected=${expected} actual=${rc} app=${app}"
  if [[ "$rc" -ne "$expected" ]]; then
    echo "MISMATCH ${label}: expected exit ${expected}, got ${rc}" >&2
    echo "----- log -----" >&2
    cat "$log" >&2
    exit 1
  fi
  # Keep a short trailer for CI log readability
  tail -n 30 "$log" | sed "s/^/[${label}] /"
}

echo "=== T4-D archive-scan-wiring (fixture / T4B_ONLY) ==="
echo "scan_script=$SCAN"
echo "fix_root=$FIX_ROOT"
echo ""

# Build fixtures
APP_S1="$(mk_s1_red)"
APP_S2="$(mk_s2_red)"
APP_S4="$(mk_s4_red)"
APP_S5="$(mk_s5_red)"
APP_S7R="$(mk_s7_red)"
APP_S7G="$(mk_s7_green)"
APP_G="$(mk_green_no_prov)"
APP_ADHOC="$(mk_adhoc_s3)"

# RED measurements (required)
expect_exit "RED-S-1" 21 S-1 "$APP_S1"
expect_exit "RED-S-2" 22 S-2 "$APP_S2"
expect_exit "RED-S-4" 24 S-4 "$APP_S4"
expect_exit "RED-S-5" 25 S-5 "$APP_S5"
expect_exit "RED-S-7" 27 S-7 "$APP_S7R"

# GREEN positive controls per check (T4B_ONLY isolates from S-3/S-6)
expect_exit "GREEN-S-1" 0 S-1 "$APP_G"
expect_exit "GREEN-S-2" 0 S-2 "$APP_G"
expect_exit "GREEN-S-4" 0 S-4 "$APP_G"
expect_exit "GREEN-S-5" 0 S-5 "$APP_G"
expect_exit "GREEN-S-7" 0 S-7 "$APP_S7G"

# S-3: measure ad-hoc — do not assume 23 vs 28
echo ""
echo "=== S-3 ad-hoc measurement (no Apple signing identity on hosted) ==="
S3_LOG="$FIX_ROOT/logs/S3-adhoc.log"
set +e
T4B_ONLY=S-3 bash "$SCAN" "$APP_ADHOC" >"$S3_LOG" 2>&1
S3_RC=$?
set -e
echo "S-3 ad-hoc actual_exit=${S3_RC}"
tail -n 40 "$S3_LOG" | sed 's/^/[S3-adhoc] /'
if [[ "$S3_RC" -eq 23 ]]; then
  echo "S-3: MEASURED RED exit=23 (ad-hoc classified; get-task-allow/network entitlements path)"
elif [[ "$S3_RC" -eq 28 ]]; then
  echo "S-3: NOT MEASURED (no signing identity — fail-closed to 28)"
  echo "S-3: MEASURED fail-closed exit=28 (ad-hoc → signing_type=UNKNOWN)"
else
  echo "S-3 unexpected exit=${S3_RC}" >&2
  cat "$S3_LOG" >&2
  exit 1
fi
# Expectation lock: hosted / this machine must get 28 for ad-hoc (fail-closed)
if [[ "$S3_RC" -ne 28 ]]; then
  echo "NOTE: S-3 returned ${S3_RC}; wiring still accepts measured 23, but CI expectation is 28 for ad-hoc" >&2
fi
# On CI we assert 28 for the ad-hoc path (current measured behavior)
if [[ "${T4D_EXPECT_S3_ADHOC_EXIT:-28}" -ne "$S3_RC" ]]; then
  echo "S-3 ad-hoc exit mismatch: expected ${T4D_EXPECT_S3_ADHOC_EXIT:-28} got ${S3_RC}" >&2
  exit 1
fi

# Full scan (no T4B_ONLY): expect fail-closed 28 (S-3 UNKNOWN before later checks matter)
echo ""
echo "=== Full scan (no T4B_ONLY) — expect fail-closed 28 ==="
FULL_LOG="$FIX_ROOT/logs/full-scan.log"
set +e
bash "$SCAN" "$APP_G" >"$FULL_LOG" 2>&1
FULL_RC=$?
set -e
echo "FULL_SCAN actual_exit=${FULL_RC}"
tail -n 50 "$FULL_LOG" | sed 's/^/[FULL] /'
if [[ "$FULL_RC" -ne 28 ]]; then
  echo "FULL_SCAN expected 28 (fail-closed), got ${FULL_RC}" >&2
  cat "$FULL_LOG" >&2
  exit 1
fi
echo "FULL_SCAN: fail-closed to 28 observed (S-3 UNKNOWN without Apple signing identity)"

echo ""
echo "SCANNED: red-s1(S-1) red-s2(S-2) red-s4(S-4) red-s5(S-5) red-s7(S-7) green-clean(S-1..S-5) green-s7(S-7) adhoc-s3(S-3) green-clean(full)"
if [[ "$S3_RC" -eq 28 ]]; then
  echo "S-3: NOT MEASURED (no signing identity — fail-closed to 28)"
else
  echo "S-3: MEASURED exit=${S3_RC}"
fi
echo "S-6: NOT MEASURED (real GGUF absent — 1.1GB asset root not configured)"
echo "NOT MEASURED: 物理デバイス HMR"
echo "NOT MEASURED: 署名 Archive / IPA（C-2=UNPAID_PERSONAL_TEAM により恒久）"
echo "GATE: archive-scan-wiring GREEN (fixtures matched expected exits)"
exit 0
