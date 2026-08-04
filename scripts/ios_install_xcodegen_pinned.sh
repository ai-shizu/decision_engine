#!/usr/bin/env bash
# Install XcodeGen at the exact policy-pinned version with a frozen archive SHA-256.
# Do NOT use `brew install xcodegen` — Homebrew core may lag or serve the wrong bottle.
#
# Usage: ios_install_xcodegen_pinned.sh
# Env:
#   XCODEGEN_INSTALL_PREFIX  — default: ${RUNNER_TEMP:-/tmp}/t4d-xcodegen
#   PATH is extended with $PREFIX/bin by printing export line when sourced? No — caller
#   must eval or use the printed bin dir. This script installs and prints:
#     XCODEGEN_BIN=<path>
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
POLICY="$ROOT/apps/desktop/src-tauri/ios/policy/ios-release.policy.json"

EXPECTED_VERSION="$(python3 -c 'import json;print(json.load(open("'"$POLICY"'"))["xcodegen_version"])')"
# Frozen provenance for yonaskolb/XcodeGen release asset xcodegen.zip @ 2.46.0
# Measured 2026-08-04 from https://github.com/yonaskolb/XcodeGen/releases/download/2.46.0/xcodegen.zip
EXPECTED_SHA256="4d9e34b62172d645eed6457cac13fc222569974098ef4ee9c3368bedf0196806"
URL="https://github.com/yonaskolb/XcodeGen/releases/download/${EXPECTED_VERSION}/xcodegen.zip"

PREFIX="${XCODEGEN_INSTALL_PREFIX:-${RUNNER_TEMP:-/tmp}/t4d-xcodegen}"
mkdir -p "$PREFIX"
STAGE="$(mktemp -d "${TMPDIR:-/tmp}/xcodegen-pin.XXXXXX")"
cleanup() { rm -rf "$STAGE"; }
trap cleanup EXIT

ZIP="$STAGE/xcodegen.zip"
echo "xcodegen_pin: fetching $URL"
# Network failure must fail the job (NOT MEASURED is forbidden here).
if ! curl -fsSL --retry 3 --retry-delay 2 -o "$ZIP" "$URL"; then
  echo "xcodegen_pin: FAIL download $URL" >&2
  exit 1
fi

GOT_SHA="$(shasum -a 256 "$ZIP" | awk '{print $1}')"
echo "xcodegen_pin: sha256=$GOT_SHA expected=$EXPECTED_SHA256"
if [[ "$GOT_SHA" != "$EXPECTED_SHA256" ]]; then
  echo "xcodegen_pin: FAIL digest mismatch — refusing unauthenticated toolchain" >&2
  exit 1
fi

unzip -q "$ZIP" -d "$STAGE/out"
BIN_SRC="$STAGE/out/xcodegen/bin/xcodegen"
test -f "$BIN_SRC"
chmod +x "$BIN_SRC"

mkdir -p "$PREFIX/bin" "$PREFIX/share"
cp "$BIN_SRC" "$PREFIX/bin/xcodegen"
# SettingPresets live next to share/xcodegen (release layout)
if [[ -d "$STAGE/out/xcodegen/share/xcodegen" ]]; then
  rm -rf "$PREFIX/share/xcodegen"
  cp -R "$STAGE/out/xcodegen/share/xcodegen" "$PREFIX/share/xcodegen"
fi

GOT_VER="$("$PREFIX/bin/xcodegen" --version | awk '{print $2}')"
echo "xcodegen_pin: version got=$GOT_VER expected=$EXPECTED_VERSION"
if [[ "$GOT_VER" != "$EXPECTED_VERSION" ]]; then
  echo "xcodegen_pin: FAIL version mismatch after install" >&2
  exit 1
fi

# Expose for subsequent steps
if [[ -n "${GITHUB_PATH:-}" ]]; then
  echo "$PREFIX/bin" >>"$GITHUB_PATH"
fi
export PATH="$PREFIX/bin:$PATH"
echo "XCODEGEN_BIN=$PREFIX/bin/xcodegen"
echo "xcodegen_pin: GREEN"
