#!/usr/bin/env bash
# A-1 deletability proof (SPEC_FLAVOR_LAYER.md §13 / F-3 T-6 / T-6b).
#
# Shell-driven (SPEC §9.2): never invoke cargo from inside cargo test.
# Three arms, each compared to baseline:
#   A-1-1  blackbox-sim only
#   A-1-2  flavor-live + completion None      (mechanism runs; nothing Accepted)
#   A-1-2b flavor-live + canned + take         (flavor exists; A-1-e)
#
# Criteria A-1-a..e. Exit non-zero on any failure.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
TAURI="$ROOT/apps/desktop/src-tauri"
OUT="${FLAVOR_A1_OUT:-$ROOT/target/flavor-a1}"
BASELINE="$OUT/a1_1_baseline.txt"
LIVE_NONE="$OUT/a1_2_flavor_live_none.txt"
LIVE_CANNED="$OUT/a1_2b_flavor_live_canned.txt"
MANIFEST="$TAURI/Cargo.toml"

mkdir -p "$OUT"

echo "=== A-1-1: blackbox-sim only (baseline) ==="
cargo run --manifest-path "$MANIFEST" --bin flavor-a1-digest \
  --no-default-features --features blackbox-sim --release \
  -- --out "$BASELINE" --arm none

echo "=== A-1-2: flavor-live, completion None ==="
NONE_LOG="$(mktemp)"
cargo run --manifest-path "$MANIFEST" --bin flavor-a1-digest \
  --no-default-features --features flavor-live --release \
  -- --out "$LIVE_NONE" --arm none 2>&1 | tee "$NONE_LOG"

echo "=== A-1-2b: flavor-live, canned + take ==="
CANNED_LOG="$(mktemp)"
cargo run --manifest-path "$MANIFEST" --bin flavor-a1-digest \
  --no-default-features --features flavor-live --release \
  -- --out "$LIVE_CANNED" --arm canned 2>&1 | tee "$CANNED_LOG"

base_lines="$(grep -c . "$BASELINE" || true)"
none_lines="$(grep -c . "$LIVE_NONE" || true)"
canned_lines="$(grep -c . "$LIVE_CANNED" || true)"

echo "A-1-b series lengths: baseline=${base_lines} none=${none_lines} canned=${canned_lines}"
if [[ "$base_lines" -eq 0 || "$none_lines" -eq 0 || "$canned_lines" -eq 0 ]]; then
  echo "A-1-b RED: empty series is not a proof (baseline=${base_lines} none=${none_lines} canned=${canned_lines})"
  exit 10
fi

if [[ "$base_lines" -ne "$none_lines" || "$base_lines" -ne "$canned_lines" ]]; then
  echo "A-1-a RED: length mismatch baseline=${base_lines} none=${none_lines} canned=${canned_lines}"
  exit 11
fi

if ! diff -u "$BASELINE" "$LIVE_NONE"; then
  echo "A-1-c RED: A-1-2 (none) Decide-time series differ from baseline"
  exit 12
fi

if ! diff -u "$BASELINE" "$LIVE_CANNED"; then
  echo "A-1-c RED: A-1-2b (canned) Decide-time series differ from baseline"
  exit 12
fi

# --- A-1-e: canned arm must actually Accept (series length) ---
accepted="$(grep -Eo 'accepted=[0-9]+' "$CANNED_LOG" | tail -1 | cut -d= -f2 || true)"
if [[ -z "${accepted}" ]]; then
  echo "A-1-e RED: accepted= count missing from flavor-a1-digest stderr"
  exit 13
fi
echo "A-1-e accepted=${accepted} series_len=${canned_lines}"
if [[ "$accepted" -eq 0 ]]; then
  echo "A-1-e RED: accepted=0 (canned arm produced no VerifiedFlavor)"
  exit 13
fi
if [[ "$accepted" -ne "$canned_lines" ]]; then
  echo "A-1-e RED: accepted (${accepted}) != series length (${canned_lines})"
  exit 13
fi

echo "A-1-d: frozen genesis scenario_id=7 campaign_index=0 created_date=2026-07-29 difficulty=Standard"
echo "A-1 GREEN: ${base_lines} Decide-time digests identical across A-1-1 / A-1-2 / A-1-2b (accepted=${accepted})"
echo "A-1-3 / A-1-4 (real LLM loaded): NOT RUN — remains T-8 proposition (b)"
exit 0
