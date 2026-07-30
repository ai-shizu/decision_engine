#!/usr/bin/env bash
# A-1 deletability proof (SPEC_FLAVOR_LAYER.md §13 / F-3 T-6 / T-6b / T-8).
#
# Shell-driven (SPEC §9.2): never invoke cargo from inside cargo test.
# Arms compared to baseline:
#   A-1-1  blackbox-sim only
#   A-1-2  flavor-live + completion None      (mechanism runs; nothing Accepted)
#   A-1-2b flavor-live + canned + take         (flavor exists; A-1-e)
#   A-1-3  flavor-live + real model            (A-1-f attempts>0, A-1-g digest match)
#
# Criteria A-1-a..g. Exit non-zero on any failure of required arms.
# A-1-3 is skipped (exit 0 overall still) when no GGUF is found — declare NOT RUN.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
TAURI="$ROOT/apps/desktop/src-tauri"
OUT="${FLAVOR_A1_OUT:-$ROOT/target/flavor-a1}"
BASELINE="$OUT/a1_1_baseline.txt"
LIVE_NONE="$OUT/a1_2_flavor_live_none.txt"
LIVE_CANNED="$OUT/a1_2b_flavor_live_canned.txt"
LIVE_MODEL="$OUT/a1_3_flavor_live_model.txt"
MANIFEST="$TAURI/Cargo.toml"
MODEL_PATH="${FLAVOR_A1_MODEL:-$ROOT/apps/desktop/models/pocket-brain.gguf}"

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
echo "A-1 GREEN (1/2/2b): ${base_lines} Decide-time digests identical (accepted=${accepted})"

# --- A-1-3: real model (proposition b) ---
if [[ ! -f "$MODEL_PATH" ]]; then
  echo "A-1-3 NOT RUN — model not found at ${MODEL_PATH} (set FLAVOR_A1_MODEL to override)"
  echo "A-1-3 / A-1-4: remaining work only if a loadable GGUF is supplied"
  exit 0
fi

echo "=== A-1-3: flavor-live + real model (${MODEL_PATH}) ==="
LIVE_LOG="$(mktemp)"
cargo run --manifest-path "$MANIFEST" --bin flavor-a1-digest \
  --no-default-features --features flavor-live --release \
  -- --out "$LIVE_MODEL" --arm live --model "$MODEL_PATH" 2>&1 | tee "$LIVE_LOG"

live_lines="$(grep -c . "$LIVE_MODEL" || true)"
attempts="$(grep -Eo 'attempts=[0-9]+' "$LIVE_LOG" | tail -1 | cut -d= -f2 || true)"
live_accepted="$(grep -Eo 'accepted=[0-9]+' "$LIVE_LOG" | tail -1 | cut -d= -f2 || true)"

echo "A-1-3 series_len=${live_lines} attempts=${attempts:-missing} accepted=${live_accepted:-missing}"

if [[ -z "${attempts}" ]]; then
  echo "A-1-f RED: attempts= count missing from flavor-a1-digest stderr"
  exit 14
fi
if [[ "$attempts" -eq 0 ]]; then
  echo "A-1-f RED: attempts=0 (model arm produced no generation tries — silent Unavailable)"
  exit 14
fi

if [[ "$live_lines" -ne "$base_lines" ]]; then
  echo "A-1-a RED: A-1-3 length mismatch baseline=${base_lines} live=${live_lines}"
  exit 11
fi

if ! diff -u "$BASELINE" "$LIVE_MODEL"; then
  echo "A-1-g RED: A-1-3 Decide-time series differ from baseline"
  exit 15
fi

echo "A-1-f/g GREEN: attempts=${attempts} (>0); A-1-3 digests byte-identical to baseline (accepted=${live_accepted} may be 0)"
exit 0
