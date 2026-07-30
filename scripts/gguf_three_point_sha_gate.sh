#!/usr/bin/env bash
# Tier 3 P0-2: three-point GGUF integrity gate (source / stage / archive).
#
# Xcode CpResource may skip copies when size+mtime match even if content
# changed — never use mtime. Size AND SHA-256 must both match the frozen
# expected values at every present point.
#
# Exit codes:
#   0  — source OK; every present point matches; absences reported (not contamination)
#   1  — usage / internal error
#   12 — SOURCE missing or mismatch
#   13 — STAGE mismatch (file present but wrong)  [contamination]
#   14 — ARCHIVE mismatch (file present but wrong) [contamination]
#
# Absences:
#   STAGE_ABSENT   — tauri inject_resources has not run (or assets dir cleaned)
#   ARCHIVE_ABSENT — no xcarchive product yet (distinct from contamination)
#
# Grounding: docs/TIER3_DEVICE_VALIDATION_REQUIREMENTS_DRAFT.md §9.5 / P0-2.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"

# --- Frozen expected values (auditor-measured; DO NOT CHANGE) ---
EXPECTED_SIZE=1117320736
EXPECTED_SHA=6a1a2eb6d15622bf3c96857206351ba97e1af16c30d7a74ee38970e434e9407e

SOURCE="$ROOT/apps/desktop/models/pocket-brain.gguf"
STAGE="$ROOT/apps/desktop/src-tauri/gen/apple/assets/models/pocket-brain.gguf"
ARCHIVE="$ROOT/apps/desktop/src-tauri/gen/apple/build/pkb-desktop_iOS.xcarchive/Products/Applications/Coraxis.app/assets/models/pocket-brain.gguf"

sha_of() {
  local path="$1"
  if command -v shasum >/dev/null 2>&1; then
    shasum -a 256 "$path" | awk '{print $1}'
  else
    sha256sum "$path" | awk '{print $1}'
  fi
}

check_present() {
  # args: label path exit_on_mismatch
  local label="$1"
  local path="$2"
  local mismatch_exit="$3"
  local size sha
  size="$(wc -c <"$path" | tr -d ' ')"
  sha="$(sha_of "$path")"
  if [[ "$size" != "$EXPECTED_SIZE" || "$sha" != "$EXPECTED_SHA" ]]; then
    echo "${label}: MISMATCH path=${path} size=${size} sha=${sha}"
    echo "${label}: expected size=${EXPECTED_SIZE} sha=${EXPECTED_SHA}"
    exit "$mismatch_exit"
  fi
  echo "${label}: OK size=${size} sha=${sha}"
}

echo "=== GGUF three-point SHA gate ==="
echo "expected_size=${EXPECTED_SIZE}"
echo "expected_sha=${EXPECTED_SHA}"

if [[ ! -f "$SOURCE" ]]; then
  echo "SOURCE: MISSING path=${SOURCE}"
  exit 12
fi
check_present "SOURCE" "$SOURCE" 12

if [[ ! -f "$STAGE" ]]; then
  echo "STAGE: ABSENT path=${STAGE} (not yet injected — distinct from contamination)"
else
  check_present "STAGE" "$STAGE" 13
fi

if [[ ! -f "$ARCHIVE" ]]; then
  echo "ARCHIVE: ABSENT path=${ARCHIVE} (not yet archived — distinct from contamination)"
else
  check_present "ARCHIVE" "$ARCHIVE" 14
fi

echo "GATE: source verified; present points match; absences are non-contamination"
exit 0
