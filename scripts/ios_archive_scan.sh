#!/usr/bin/env bash
# T4-B: iOS archive absolute-isolation scan gate (§15.2).
#
# Scans a built Coraxis.app for network isolation violations.
# Every check runs a positive control first — a silent detector is not evidence.
#
# Exit codes:
#   0  — all S-1..S-7 GREEN
#   1  — usage / internal error / positive-control failure
#   21 — S-1 RED (app-owned network undefined symbols)
#   22 — S-2 RED (Network.framework / CFNetwork linked)
#   23 — S-3 RED (network entitlements / release get-task-allow)
#   24 — S-4 RED (network Info.plist keys)
#   25 — S-5 RED (dev URL / LAN host / CIDR strings)
#   26 — S-6 RED (bundled GGUF mismatch or ABSENT when app present)
#   27 — S-7 RED (provision expired or ABSENT)
#   28 — signing type UNKNOWN (cannot judge get-task-allow; fail-closed)
#
# Env:
#   T4B_ONLY=S-N  — mutation-drill aid: after all positive controls, measure only S-N
#
# Grounding: docs/T4B_ARCHIVE_SCAN_GATE_DIRECTIVE.md
# GGUF values must stay in lockstep with scripts/gguf_three_point_sha_gate.sh
# (that script is invoked unmodified; do not fork its constants here lightly).
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
GGUF_GATE="$ROOT/scripts/gguf_three_point_sha_gate.sh"
DEFAULT_APP="$ROOT/apps/desktop/src-tauri/gen/apple/build/pkb-desktop_iOS.xcarchive/Products/Applications/Coraxis.app"

# Frozen GGUF expectations (identical to gguf_three_point_sha_gate.sh — do not drift)
EXPECTED_GGUF_SIZE=1117320736
EXPECTED_GGUF_SHA=6a1a2eb6d15622bf3c96857206351ba97e1af16c30d7a74ee38970e434e9407e

# --- symbol / framework / entitlement / plist / string patterns ---
NM_NETWORK_RE='_socket$|_connect$|getaddrinfo|CFNetwork|nw_'
OTOOL_NETWORK_RE='Network\.framework|CFNetwork\.framework'
ENTITLEMENT_NETWORK_RE='com\.apple\.security\.network\.(client|server)|com\.apple\.developer\.networking\.|com\.apple\.developer\.associated-domains'
PLIST_NETWORK_KEY_RE='NSAppTransportSecurity|NSAllowsArbitraryLoads|NSAllowsArbitraryLoadsInWebContent|NSAllowsLocalNetworking|NSExceptionDomains|NSLocalNetworkUsageDescription|NSBonjourServices|UIRequiresPersistentWiFi'
# S-5: must catch CIDR (no :// / :port required) AND classic dev URLs — grep -a, never strings(1)
LAN_STRING_RE='10\.0\.0\.0/8|172\.16\.0\.0/12|192\.168\.0\.0/16|169\.254\.0\.0/16|://(10|172|192)\.[0-9]+\.[0-9]+\.[0-9]+(:[0-9]+)?|localhost:[0-9]+|127\.0\.0\.1:[0-9]+'

APP="${1:-$DEFAULT_APP}"
WORK="$(mktemp -d "${TMPDIR:-/tmp}/ios_archive_scan.XXXXXX")"
cleanup() { rm -rf "$WORK"; }
trap cleanup EXIT

die_usage() {
  echo "usage: $0 [/path/to/Coraxis.app]" >&2
  exit 1
}

fail_control() {
  local check="$1"
  local detail="$2"
  echo "POSITIVE_CONTROL_FAIL: ${check}: ${detail}"
  echo "GATE: detector cannot prove it detects — refusing to measure target"
  exit 1
}

sha_of() {
  local path="$1"
  if command -v shasum >/dev/null 2>&1; then
    shasum -a 256 "$path" | awk '{print $1}'
  else
    sha256sum "$path" | awk '{print $1}'
  fi
}

require_cmd() {
  local c
  for c in "$@"; do
    if ! command -v "$c" >/dev/null 2>&1; then
      echo "INTERNAL: required command missing: $c" >&2
      exit 1
    fi
  done
}

# ---------------------------------------------------------------------------
# Signing classification (DEV vs RELEASE vs UNKNOWN)
# ---------------------------------------------------------------------------
classify_signing() {
  # Writes: $WORK/signing_type, $WORK/signing_evidence
  local app="$1"
  local dv authority_blob type="UNKNOWN"
  local evidence=()

  # Authority lines appear only with sufficient -v (plain -dv omits them on current macOS)
  dv="$(codesign -dvvv "$app" 2>&1 || true)"
  authority_blob="$(printf '%s\n' "$dv" | grep -E '^Authority=' || true)"
  evidence+=("codesign_authority_lines=$(printf '%s' "$authority_blob" | tr '\n' '|' | sed 's/|$//')")

  if printf '%s\n' "$authority_blob" | grep -qE 'Authority=Apple Distribution|Authority=iPhone Distribution|Authority=iOS Distribution'; then
    type="RELEASE"
    evidence+=("authority_match=distribution")
  elif printf '%s\n' "$authority_blob" | grep -qE 'Authority=Apple Development|Authority=iPhone Developer'; then
    type="DEV"
    evidence+=("authority_match=development")
  fi

  # Corroborate from embedded.mobileprovision when present
  local prov="$app/embedded.mobileprovision"
  if [[ -f "$prov" ]]; then
    local prov_plist="$WORK/classify_prov.plist"
    if security cms -D -i "$prov" -o "$prov_plist" 2>/dev/null; then
      local has_devices="no" local_prov="no"
      if plutil -extract ProvisionedDevices raw "$prov_plist" >/dev/null 2>&1; then
        has_devices="yes"
      fi
      local_prov="$(plutil -extract LocalProvision raw "$prov_plist" 2>/dev/null || echo "?")"
      evidence+=("provision_ProvisionedDevices=${has_devices}" "provision_LocalProvision=${local_prov}")
      if [[ "$type" == "UNKNOWN" ]]; then
        if [[ "$has_devices" == "yes" || "$local_prov" == "true" ]]; then
          type="DEV"
          evidence+=("provision_inferred=DEV")
        fi
      fi
    else
      evidence+=("provision_decode=FAILED")
    fi
  else
    evidence+=("provision=ABSENT")
  fi

  if [[ "$type" == "UNKNOWN" ]]; then
    evidence+=("result=UNKNOWN")
  fi

  printf '%s\n' "$type" >"$WORK/signing_type"
  printf '%s\n' "${evidence[@]}" >"$WORK/signing_evidence"
}

# ---------------------------------------------------------------------------
# Detectors (operate on extracted artifacts / paths)
# ---------------------------------------------------------------------------
count_nm_network() {
  # stdin = nm -u output
  grep -icE "$NM_NETWORK_RE" || true
}

count_otool_network() {
  # stdin = otool -L output
  grep -icE "$OTOOL_NETWORK_RE" || true
}

count_entitlement_network() {
  # $1 = entitlements text dump (codesign -d --entitlements -)
  grep -icE "$ENTITLEMENT_NETWORK_RE" "$1" || true
}

entitlement_get_task_allow() {
  # prints true|false|absent
  local dump="$1"
  if grep -q 'get-task-allow' "$dump"; then
    if grep -A1 'get-task-allow' "$dump" | grep -qi 'true'; then
      echo "true"
    elif grep -A1 'get-task-allow' "$dump" | grep -qi 'false'; then
      echo "false"
    else
      # XML style <true/>/<false/>
      if grep -A1 'get-task-allow' "$dump" | grep -q '<true'; then
        echo "true"
      elif grep -A1 'get-task-allow' "$dump" | grep -q '<false'; then
        echo "false"
      else
        echo "true" # key present without clear false → treat as true (fail-closed for release)
      fi
    fi
  else
    echo "absent"
  fi
}

count_plist_network_keys() {
  # $1 = xml1 plist path
  grep -oE '<key>[^<]+</key>' "$1" | grep -icE "$PLIST_NETWORK_KEY_RE" || true
}

count_lan_strings() {
  # $1 = file path (binary or text); uses grep -a (NOT strings).
  # Prefer match-list | wc — BSD grep -c/-o pairing under-counts.
  # grep exit 1 on zero matches must not trip pipefail.
  local n
  n="$( { grep -aoE "$LAN_STRING_RE" "$1" 2>/dev/null || true; } | wc -l | tr -d ' ')"
  echo "${n:-0}"
}

# ---------------------------------------------------------------------------
# Positive controls — each must fire or we abort before measuring the app
# ---------------------------------------------------------------------------
run_positive_controls() {
  echo "=== POSITIVE CONTROLS (detector self-proof) ==="
  local n

  # S-1: known nm lines must match
  n="$(printf '%s\n' '_socket' '_connect' '_getaddrinfo' '_CFNetworkCopySystemProxySettings' '_nw_path_create' \
    | count_nm_network)"
  if [[ "$n" -lt 1 ]]; then
    fail_control "S-1" "nm pattern matched 0 on known-bad symbol list"
  fi
  echo "S-1 control: OK detected=${n} (planted nm symbol names)"

  # S-2: known otool lines
  n="$(printf '%s\n' \
    $'\t/System/Library/Frameworks/Network.framework/Network (compatibility version 1.0.0, current version 1.0.0)' \
    $'\t/System/Library/Frameworks/CFNetwork.framework/CFNetwork (compatibility version 1.0.0, current version 1.0.0)' \
    | count_otool_network)"
  if [[ "$n" -lt 1 ]]; then
    fail_control "S-2" "otool pattern matched 0 on known-bad framework lines"
  fi
  echo "S-2 control: OK detected=${n} (planted Network/CFNetwork link lines)"

  # S-3: planted entitlements dump
  cat >"$WORK/control_ents.txt" <<'EOF'
[Dict]
	[Key] com.apple.security.network.client
	[Value]
		[Bool] true
	[Key] com.apple.developer.networking.wifi-info
	[Value]
		[Bool] true
EOF
  n="$(count_entitlement_network "$WORK/control_ents.txt")"
  if [[ "$n" -lt 1 ]]; then
    fail_control "S-3" "entitlement pattern matched 0 on known-bad dump"
  fi
  echo "S-3 control: OK detected=${n} (planted network entitlements)"

  # S-4: planted Info.plist with network keys
  cat >"$WORK/control_info.plist" <<'EOF'
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
	<key>CFBundleIdentifier</key>
	<string>control.bad</string>
	<key>NSAppTransportSecurity</key>
	<dict>
		<key>NSAllowsLocalNetworking</key>
		<true/>
		<key>NSExceptionDomains</key>
		<dict>
			<key>10.0.0.0/8</key>
			<dict/>
		</dict>
	</dict>
	<key>NSLocalNetworkUsageDescription</key>
	<string>control</string>
</dict>
</plist>
EOF
  n="$(count_plist_network_keys "$WORK/control_info.plist")"
  if [[ "$n" -lt 1 ]]; then
    fail_control "S-4" "plist key pattern matched 0 on known-bad Info.plist"
  fi
  echo "S-4 control: OK detected=${n} (planted NSAppTransportSecurity / NSLocalNetworkUsageDescription)"

  # S-5: CIDR without :// or :port — the hole that made the old detector blind
  # Also prove grep -a finds UTF-8 (Japanese) adjacent to LAN marker (strings would miss JP)
  printf 'marker 10.0.0.0/8 開発用LAN\n' >"$WORK/control_lan.bin"
  n="$(count_lan_strings "$WORK/control_lan.bin")"
  if [[ "$n" -lt 1 ]]; then
    fail_control "S-5" "LAN/CIDR pattern matched 0 on planted CIDR (10.0.0.0/8) — old ://:port hole unrepaired"
  fi
  # Prove old regex is blind to this sample (documentation of why S-5 exists)
  local old_hits
  old_hits="$( { grep -aoE '://(10|172|192)\.[0-9]+\.[0-9]+\.[0-9]+:[0-9]+' "$WORK/control_lan.bin" 2>/dev/null || true; } | wc -l | tr -d ' ')"
  echo "S-5 control: OK detected=${n} CIDR via grep -a; old_://:port_regex_hits=${old_hits:-0} (expect 0)"

  # S-6: planted wrong-size file must fail size/sha check logic
  printf 'not-a-gguf' >"$WORK/control_gguf.bin"
  local csize csha
  csize="$(wc -c <"$WORK/control_gguf.bin" | tr -d ' ')"
  csha="$(sha_of "$WORK/control_gguf.bin")"
  if [[ "$csize" == "$EXPECTED_GGUF_SIZE" && "$csha" == "$EXPECTED_GGUF_SHA" ]]; then
    fail_control "S-6" "planted non-GGUF unexpectedly matched frozen digest"
  fi
  echo "S-6 control: OK planted mismatch size=${csize} sha=${csha} (detector would RED)"

  # S-7: planted past ExpirationDate must be detected as expired
  cat >"$WORK/control_prov.plist" <<'EOF'
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
	<key>ExpirationDate</key>
	<date>2020-01-01T00:00:00Z</date>
</dict>
</plist>
EOF
  local exp_raw now_epoch exp_epoch
  exp_raw="$(plutil -extract ExpirationDate raw "$WORK/control_prov.plist")"
  now_epoch="$(date -u +%s)"
  exp_epoch="$(date -u -j -f "%Y-%m-%dT%H:%M:%SZ" "$exp_raw" +%s 2>/dev/null \
    || date -u -d "$exp_raw" +%s 2>/dev/null || echo 0)"
  if [[ "$exp_epoch" -ge "$now_epoch" ]]; then
    fail_control "S-7" "planted 2020-01-01 expiration not recognized as past"
  fi
  echo "S-7 control: OK planted ExpirationDate=${exp_raw} is past (detector would RED)"

  echo "POSITIVE_CONTROLS: all fired — proceeding to measure target"
  echo ""
}

# ---------------------------------------------------------------------------
# Checks against APP
# ---------------------------------------------------------------------------
FIRST_FAIL=0
record_fail() {
  local code="$1"
  if [[ "$FIRST_FAIL" -eq 0 ]]; then
    FIRST_FAIL="$code"
  fi
}

check_s1() {
  local bin="$APP/Coraxis"
  echo "--- S-1: app-owned network undefined symbols (nm -u) ---"
  echo "scan_target: $bin"
  if [[ ! -f "$bin" ]]; then
    echo "S-1: ABSENT path=${bin} (not measured as 0 — Mach-O missing)"
    record_fail 21
    return
  fi
  local nm_out hits
  nm_out="$(nm -u "$bin" 2>/dev/null || true)"
  hits="$(printf '%s\n' "$nm_out" | count_nm_network)"
  if [[ "$hits" -gt 0 ]]; then
    echo "S-1: RED count=${hits}"
    printf '%s\n' "$nm_out" | grep -iE "$NM_NETWORK_RE" | head -20
    record_fail 21
  else
    echo "S-1: GREEN count=0 (measured)"
  fi
}

check_s2() {
  local bin="$APP/Coraxis"
  echo "--- S-2: Network.framework / CFNetwork link (otool -L) ---"
  echo "scan_target: $bin"
  if [[ ! -f "$bin" ]]; then
    echo "S-2: ABSENT path=${bin} (not measured as 0 — Mach-O missing)"
    record_fail 22
    return
  fi
  local ot_out hits
  ot_out="$(otool -L "$bin" 2>/dev/null || true)"
  hits="$(printf '%s\n' "$ot_out" | count_otool_network)"
  if [[ "$hits" -gt 0 ]]; then
    echo "S-2: RED count=${hits}"
    printf '%s\n' "$ot_out" | grep -iE "$OTOOL_NETWORK_RE"
    record_fail 22
  else
    echo "S-2: GREEN count=0 (measured — Network.framework/CFNetwork not linked)"
  fi
}

check_s3() {
  echo "--- S-3: network entitlements + get-task-allow vs signing ---"
  echo "scan_target: $APP (codesign entitlements)"
  if [[ ! -d "$APP" ]]; then
    echo "S-3: ABSENT path=${APP}"
    record_fail 23
    return
  fi

  classify_signing "$APP"
  local sig_type
  sig_type="$(cat "$WORK/signing_type")"
  echo "signing_type: ${sig_type}"
  while IFS= read -r line; do
    echo "signing_evidence: $line"
  done <"$WORK/signing_evidence"

  if [[ "$sig_type" == "UNKNOWN" ]]; then
    echo "S-3: UNKNOWN signing type — fail-closed (will not guess get-task-allow policy)"
    record_fail 28
    return
  fi

  local ent_dump="$WORK/ents.txt"
  if ! codesign -d --entitlements - "$APP" >"$ent_dump" 2>"$WORK/ents.err"; then
    echo "S-3: RED codesign entitlements extraction failed"
    cat "$WORK/ents.err" || true
    record_fail 23
    return
  fi

  local net_hits gta
  net_hits="$(count_entitlement_network "$ent_dump")"
  gta="$(entitlement_get_task_allow "$ent_dump")"
  echo "network_entitlement_hits: ${net_hits}"
  echo "get-task-allow: ${gta}"

  local red=0
  if [[ "$net_hits" -gt 0 ]]; then
    echo "S-3: network entitlements present:"
    grep -iE "$ENTITLEMENT_NETWORK_RE" "$ent_dump" || true
    red=1
  fi

  if [[ "$sig_type" == "RELEASE" && "$gta" == "true" ]]; then
    echo "S-3: RELEASE signing with get-task-allow=true is a violation"
    red=1
  elif [[ "$sig_type" == "DEV" && "$gta" == "true" ]]; then
    echo "S-3: DEV signing get-task-allow=true — allowed (not a violation)"
  elif [[ "$sig_type" == "RELEASE" ]]; then
    echo "S-3: RELEASE get-task-allow=${gta} — OK for isolation gate"
  fi

  if [[ "$red" -eq 1 ]]; then
    echo "S-3: RED"
    record_fail 23
  else
    echo "S-3: GREEN (network entitlements=0; get-task-allow policy satisfied for ${sig_type})"
  fi
}

check_s4() {
  local plist="$APP/Info.plist"
  echo "--- S-4: network Info.plist keys ---"
  echo "scan_target: $plist"
  if [[ ! -f "$plist" ]]; then
    echo "S-4: ABSENT path=${plist} (not measured as 0)"
    record_fail 24
    return
  fi
  local xml="$WORK/Info.xml.plist"
  plutil -convert xml1 -o "$xml" "$plist"
  local hits
  hits="$(count_plist_network_keys "$xml")"
  if [[ "$hits" -gt 0 ]]; then
    echo "S-4: RED count=${hits}"
    grep -E '<key>[^<]+</key>' "$xml" | grep -iE "$PLIST_NETWORK_KEY_RE" || true
    record_fail 24
  else
    echo "S-4: GREEN count=0 (measured)"
  fi
}

check_s5() {
  local bin="$APP/Coraxis"
  local plist="$APP/Info.plist"
  echo "--- S-5: dev URL / LAN host / CIDR strings (grep -a, not strings) ---"
  echo "scan_target: $bin"
  echo "scan_target: $plist"
  local total=0
  local any_present=0

  if [[ -f "$bin" ]]; then
    any_present=1
    local h
    h="$(count_lan_strings "$bin")"
    echo "S-5 binary_hits: ${h}"
    total=$((total + h))
  else
    echo "S-5 binary: ABSENT path=${bin}"
  fi

  if [[ -f "$plist" ]]; then
    any_present=1
    local h
    h="$(count_lan_strings "$plist")"
    echo "S-5 plist_hits: ${h}"
    total=$((total + h))
  else
    echo "S-5 plist: ABSENT path=${plist}"
  fi

  if [[ "$any_present" -eq 0 ]]; then
    echo "S-5: ABSENT (no scan targets present — not measured as 0)"
    record_fail 25
    return
  fi

  if [[ "$total" -gt 0 ]]; then
    echo "S-5: RED total_hits=${total}"
    # Show matching lines/context where feasible (plist xml / binary strings via grep -a)
    if [[ -f "$plist" ]]; then
      grep -aoE "$LAN_STRING_RE" "$plist" | sort -u | head -20 || true
    fi
    if [[ -f "$bin" ]]; then
      grep -aoE "$LAN_STRING_RE" "$bin" | sort -u | head -20 || true
    fi
    record_fail 25
  else
    echo "S-5: GREEN total_hits=0 (measured via grep -a on listed targets)"
  fi
}

check_s6() {
  local gguf="$APP/assets/models/pocket-brain.gguf"
  local s6_red=0
  echo "--- S-6: bundled GGUF three-point + in-app digest ---"
  echo "scan_target: $gguf"
  echo "scan_helper: $GGUF_GATE (unmodified)"

  if [[ ! -f "$gguf" ]]; then
    echo "S-6: ABSENT path=${gguf} (app present but GGUF missing — distinct from measured-0)"
    s6_red=1
  else
    local size sha
    size="$(wc -c <"$gguf" | tr -d ' ')"
    sha="$(sha_of "$gguf")"
    if [[ "$size" != "$EXPECTED_GGUF_SIZE" || "$sha" != "$EXPECTED_GGUF_SHA" ]]; then
      echo "S-6: RED in-app MISMATCH size=${size} sha=${sha}"
      echo "S-6: expected size=${EXPECTED_GGUF_SIZE} sha=${EXPECTED_GGUF_SHA}"
      s6_red=1
    else
      echo "S-6: in-app OK size=${size} sha=${sha}"
    fi
  fi

  # Always invoke the unmodified three-point gate (SOURCE/STAGE/default ARCHIVE)
  set +e
  local gguf_out gguf_rc
  gguf_out="$(bash "$GGUF_GATE" 2>&1)"
  gguf_rc=$?
  set -e
  printf '%s\n' "$gguf_out" | sed 's/^/S-6 gguf_gate: /'
  if [[ "$gguf_rc" -ne 0 ]]; then
    echo "S-6: RED three-point gate exit=${gguf_rc}"
    s6_red=1
  else
    echo "S-6: three-point gate exit=0"
  fi

  if [[ "$s6_red" -eq 1 ]]; then
    echo "S-6: RED"
    record_fail 26
  else
    echo "S-6: GREEN"
  fi
}

check_s7() {
  local prov="$APP/embedded.mobileprovision"
  echo "--- S-7: provisioning profile expiration ---"
  echo "scan_target: $prov"
  if [[ ! -f "$prov" ]]; then
    echo "S-7: ABSENT path=${prov} (not measured as unexpired)"
    record_fail 27
    return
  fi
  local prov_plist="$WORK/prov.plist"
  if ! security cms -D -i "$prov" -o "$prov_plist" 2>"$WORK/prov.err"; then
    echo "S-7: RED failed to decode mobileprovision"
    cat "$WORK/prov.err" || true
    record_fail 27
    return
  fi
  local exp_raw now_epoch exp_epoch
  exp_raw="$(plutil -extract ExpirationDate raw "$prov_plist" 2>/dev/null || true)"
  if [[ -z "$exp_raw" ]]; then
    echo "S-7: RED ExpirationDate key ABSENT inside provision"
    record_fail 27
    return
  fi
  now_epoch="$(date -u +%s)"
  exp_epoch="$(date -u -j -f "%Y-%m-%dT%H:%M:%SZ" "$exp_raw" +%s 2>/dev/null \
    || date -u -d "$exp_raw" +%s 2>/dev/null || echo "")"
  if [[ -z "$exp_epoch" ]]; then
    echo "S-7: RED could not parse ExpirationDate=${exp_raw}"
    record_fail 27
    return
  fi
  echo "S-7: ExpirationDate=${exp_raw} now_epoch=${now_epoch} exp_epoch=${exp_epoch}"
  if [[ "$exp_epoch" -le "$now_epoch" ]]; then
    echo "S-7: RED profile expired"
    record_fail 27
  else
    echo "S-7: GREEN profile unexpired"
  fi
}

# ---------------------------------------------------------------------------
# main
# ---------------------------------------------------------------------------
if [[ "${1:-}" == "-h" || "${1:-}" == "--help" ]]; then
  die_usage
fi

require_cmd nm otool codesign plutil security grep mktemp date
if ! command -v shasum >/dev/null 2>&1 && ! command -v sha256sum >/dev/null 2>&1; then
  echo "INTERNAL: need shasum or sha256sum" >&2
  exit 1
fi

if [[ ! -d "$APP" ]]; then
  echo "TARGET: ABSENT path=${APP}"
  echo "GATE: cannot scan — Coraxis.app directory missing (not reported as all-green 0)"
  exit 1
fi

echo "=== iOS archive absolute-isolation scan (T4-B) ==="
echo "app: $APP"
echo ""
echo "=== SCAN SCOPE (green is meaningless without this list) ==="
echo "  1. $APP/Coraxis"
echo "  2. $APP/Info.plist"
echo "  3. $APP/embedded.mobileprovision"
echo "  4. $APP/assets/models/pocket-brain.gguf"
echo "helper: $GGUF_GATE"
echo ""

# Presence inventory (ABSENT vs present — never conflate with measured-0)
for rel in Coraxis Info.plist embedded.mobileprovision assets/models/pocket-brain.gguf; do
  if [[ -e "$APP/$rel" ]]; then
    echo "scope_member: PRESENT $APP/$rel"
  else
    echo "scope_member: ABSENT  $APP/$rel"
  fi
done
echo ""

run_positive_controls

# Optional single-check mode for mutation drills: T4B_ONLY=S-4
ONLY="${T4B_ONLY:-}"

run_or_skip() {
  local id="$1"
  local fn="$2"
  if [[ -n "$ONLY" && "$ONLY" != "$id" ]]; then
    echo "--- ${id}: SKIPPED (T4B_ONLY=${ONLY}) ---"
    echo ""
    return
  fi
  "$fn"
  echo ""
}

run_or_skip S-1 check_s1
run_or_skip S-2 check_s2
run_or_skip S-3 check_s3
run_or_skip S-4 check_s4
run_or_skip S-5 check_s5
run_or_skip S-6 check_s6
run_or_skip S-7 check_s7

if [[ "$FIRST_FAIL" -eq 0 ]]; then
  if [[ -n "$ONLY" ]]; then
    echo "GATE: GREEN for ${ONLY} — scope listed above was measured"
  else
    echo "GATE: ALL GREEN (S-1..S-7) — scope listed above was measured"
  fi
  exit 0
fi

echo "GATE: RED first_fail_exit=${FIRST_FAIL}"
exit "$FIRST_FAIL"
