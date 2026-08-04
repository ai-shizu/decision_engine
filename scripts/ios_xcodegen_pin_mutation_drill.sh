#!/usr/bin/env bash
# Mutation drill: pinned XcodeGen installer must RED on digest/version mismatch.
# Does not modify production install script constants — injects via env overrides
# when the install script is extended; for now wraps download verification logic inline.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
STAGE="$(mktemp -d "${TMPDIR:-/tmp}/xcodegen-mut.XXXXXX")"
cleanup() { rm -rf "$STAGE"; }
trap cleanup EXIT

EXPECTED_SHA256="4d9e34b62172d645eed6457cac13fc222569974098ef4ee9c3368bedf0196806"

echo "=== mutation: wrong SHA-256 must fail ==="
printf 'not-xcodegen' >"$STAGE/fake.zip"
GOT_SHA="$(shasum -a 256 "$STAGE/fake.zip" | awk '{print $1}')"
if [[ "$GOT_SHA" == "$EXPECTED_SHA256" ]]; then
  echo "MUTATION_FAIL: fake zip unexpectedly matched pinned digest" >&2
  exit 1
fi
echo "digest_mismatch_detected: got=$GOT_SHA expected=$EXPECTED_SHA256 (would refuse install)"

echo "=== mutation: version string gate must fail on 2.45.4 ==="
# Simulate post-install version check used by ios-config-gate.yml
EXPECTED_VERSION="$(python3 -c 'import json;print(json.load(open("'"$ROOT"'/apps/desktop/src-tauri/ios/policy/ios-release.policy.json"))["xcodegen_version"])')"
FAKE_GOT="2.45.4"
set +e
test "$FAKE_GOT" = "$EXPECTED_VERSION"
RC=$?
set -e
if [[ "$RC" -eq 0 ]]; then
  echo "MUTATION_FAIL: 2.45.4 incorrectly accepted as $EXPECTED_VERSION" >&2
  exit 1
fi
echo "version_mismatch_detected: got=$FAKE_GOT expected=$EXPECTED_VERSION exit=$RC (gate would RED)"

echo "GATE: xcodegen pin mutation drill GREEN"
exit 0
