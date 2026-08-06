#!/usr/bin/env bash
# T4-F: prove a networking feature cannot reach the release build.
#
# Measured 2026-08-06, before the fix: adding egress-live to the --features argv
# of ios_release_build.sh left every CI check green. The policy validated
# features_exact against an allowlist, and the build command carried a second,
# unrelated copy of the same list; they agreed by coincidence, not by
# construction. "Not currently present" is not "cannot be introduced".
#
# Two ways in, both must be shut:
#   A. edit the policy JSON        -> allowlist mismatch, fail-closed
#   B. edit the build argv         -> impossible: argv is derived from the policy
#
# Phase 3 restores everything and re-proves the clean state, so a drill that
# dies half-way cannot leave a mutated release script behind.
#
# Exit 0 only if every phase behaved.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
BUILD="$ROOT/scripts/ios_release_build.sh"
POLICY="$ROOT/apps/desktop/src-tauri/ios/policy/ios-release.policy.json"
LOG_DIR="${T4F_LOG_DIR:-/tmp/t4f-evidence/release-features-drill}"
mkdir -p "$LOG_DIR"

sha_of() { shasum -a 256 "$1" | awk '{print $1}'; }
say() { echo "DRILL[release_features] $*"; }

[[ -f "$BUILD" ]] || { echo "missing $BUILD" >&2; exit 1; }
[[ -f "$POLICY" ]] || { echo "missing $POLICY" >&2; exit 1; }

BUILD_BAK="$LOG_DIR/ios_release_build.sh.bak"
POLICY_BAK="$LOG_DIR/ios-release.policy.json.bak"
cp "$BUILD" "$BUILD_BAK"
cp "$POLICY" "$POLICY_BAK"
BUILD_BEFORE="$(sha_of "$BUILD")"
POLICY_BEFORE="$(sha_of "$POLICY")"
say "build_sha_before=$BUILD_BEFORE"
say "policy_sha_before=$POLICY_BEFORE"

restore() {
  cp "$BUILD_BAK" "$BUILD"
  cp "$POLICY_BAK" "$POLICY"
}
trap 'restore' EXIT

# The wrapper needs signing env it will never have here; it must fail for the
# feature reason BEFORE it reaches that check, so we look at the message rather
# than the bare exit code.
run_build() {
  set +e
  bash "$BUILD" >"$1" 2>&1
  local ec=$?
  set -e
  echo "$ec"
}

# --- Phase A: poison the policy -------------------------------------------
python3 - "$POLICY" <<'PY'
import json, sys
p = json.load(open(sys.argv[1], encoding="utf-8"))
p["features_exact"] = ["pocket-brain", "secure-vault", "flavor-live", "egress-live"]
json.dump(p, open(sys.argv[1], "w", encoding="utf-8"), indent=2, ensure_ascii=False)
PY
A_LOG="$LOG_DIR/phaseA_policy_poisoned.log"
A_EC="$(run_build "$A_LOG")"
say "phaseA exit=$A_EC (expect non-zero)"
if [[ "$A_EC" -eq 0 ]]; then
  say "FAIL: poisoned policy was accepted"
  exit 1
fi
if ! grep -q 'features_exact mismatch' "$A_LOG"; then
  say "FAIL: rejected, but not for the feature reason — see $A_LOG"
  exit 1
fi
say "phaseA OK: allowlist rejected egress-live in the policy"
cp "$POLICY_BAK" "$POLICY"

# --- Phase B: the argv cannot be poisoned independently --------------------
# Structural, not behavioural: there is no literal feature list in the argv to
# edit. If one reappears, this fails and the old hole is back.
B_LOG="$LOG_DIR/phaseB_argv_structure.log"
{
  echo "--- grep: literal feature lists outside the python allowlist ---"
  grep -n -- '--features' "$BUILD"
} >"$B_LOG" 2>&1
if grep -n -- '--features' "$BUILD" | grep -v 'VALIDATED_FEATURES' | grep -qv '^\s*#'; then
  say "FAIL: a literal --features list is present in the argv again — see $B_LOG"
  exit 1
fi
say "phaseB OK: argv carries no literal list; it derives from the validated policy"

# --- Phase C: restored state is clean --------------------------------------
restore
BUILD_AFTER="$(sha_of "$BUILD")"
POLICY_AFTER="$(sha_of "$POLICY")"
say "build_sha_after=$BUILD_AFTER"
say "policy_sha_after=$POLICY_AFTER"
if [[ "$BUILD_BEFORE" != "$BUILD_AFTER" || "$POLICY_BEFORE" != "$POLICY_AFTER" ]]; then
  say "FAIL: drill did not restore the tree"
  exit 1
fi
say "phaseC OK: tree restored byte-identical"

say "PASS"
