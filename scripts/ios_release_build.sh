#!/usr/bin/env bash
# T4-D: Release-only iOS build entry. Arbitrary argv passthrough is forbidden.
#
# Allowed shape (exact):
#   env -u CARGO_TARGET_DIR npx --no-install tauri ios build \
#     --ci --target aarch64 \
#     --features "$VALIDATED_FEATURES" \
#     --build-number "$VALIDATED_BUILD_NUMBER" \
#     --export-method app-store-connect
#
# Manual signing env (Tauri official names only; no automatic mixing):
#   IOS_CERTIFICATE / IOS_CERTIFICATE_PASSWORD / IOS_MOBILE_PROVISION /
#   APPLE_DEVELOPMENT_TEAM
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
POLICY="$ROOT/apps/desktop/src-tauri/ios/policy/ios-release.policy.json"
DESKTOP="$ROOT/apps/desktop"
ELIG="$ROOT/apps/desktop/src-tauri/ios/decisions/eligibility.decision.json"
EXPORT="$ROOT/apps/desktop/src-tauri/ios/decisions/export-compliance.decision.json"

die() { echo "ios_release_build: RED: $*" >&2; exit 1; }

# Refuse any caller-supplied args — the allowed command is fixed.
if [[ $# -gt 0 ]]; then
  die "arbitrary arguments are forbidden; got: $*"
fi

command -v python3 >/dev/null 2>&1 || die "python3 required"
command -v npx >/dev/null 2>&1 || die "npx required"

# --- policy / decision fail-closed ---
POLICY_OUT="$(python3 - <<'PY' "$POLICY" "$ELIG" "$EXPORT"
import json, sys
policy_path, elig_path, export_path = sys.argv[1:4]
p = json.load(open(policy_path, encoding="utf-8"))
e = json.load(open(elig_path, encoding="utf-8"))
x = json.load(open(export_path, encoding="utf-8"))

def fail(msg):
    print(msg, file=sys.stderr)
    sys.exit(2)

if p.get("export_method") != "app-store-connect":
    fail("policy export_method must be app-store-connect")
if p.get("signing_mode") != "manual":
    fail("policy signing_mode must be manual")
# The allowlist is the contract; the policy is data checked against it. Any
# feature that can open a socket must never reach a release build — egress-live
# pulls in reqwest/rustls/hickory and would make the shipped binary capable of
# network I/O, which §15.2 clause 1 forbids outright.
ALLOWED_FEATURES = ["pocket-brain", "secure-vault", "flavor-live"]
feats = p.get("features_exact")
if feats != ALLOWED_FEATURES:
    fail(f"features_exact mismatch: {feats}")
bn = p.get("build_number")
if not isinstance(bn, int) or bn < 1:
    fail(f"build_number must be positive int, got {bn!r}")

if e.get("status") != "APPROVED":
    fail("RELEASE_ELIGIBILITY: RED (decision status != APPROVED)")
if e.get("option") not in ("A", "B"):
    fail("RELEASE_ELIGIBILITY: RED (option not A/B)")
if e.get("decision_id") in (None, "") or e.get("approver") in (None, ""):
    fail("RELEASE_ELIGIBILITY: RED (missing decision_id/approver)")

if x.get("status") != "APPROVED":
    fail("EXPORT_COMPLIANCE: RED (decision status != APPROVED)")
for k in ("decision_id", "approver", "jurisdiction", "crypto_inventory_digest"):
    if x.get(k) in (None, ""):
        fail(f"EXPORT_COMPLIANCE: RED (missing {k})")
if x.get("its_app_uses_non_exempt_encryption") is None:
    fail("EXPORT_COMPLIANCE: RED (its_app_uses_non_exempt_encryption is null)")

# Plist must match approved decision (no silent ratification).
import pathlib, plistlib, re
plist_path = pathlib.Path(policy_path).resolve().parents[2] / "Info.ios.plist"
raw = plist_path.read_bytes()
# Strip XML comments for plistlib
raw = re.sub(br"<!--.*?-->", b"", raw, flags=re.S)
pl = plistlib.loads(raw)
plist_val = pl.get("ITSAppUsesNonExemptEncryption")
if plist_val != x["its_app_uses_non_exempt_encryption"]:
    fail(
        "EXPORT_COMPLIANCE: RED (plist ITSAppUsesNonExemptEncryption="
        f"{plist_val!r} != decision {x['its_app_uses_non_exempt_encryption']!r})"
    )

# Emit both, so the build argv is derived from the same validated policy that
# was just checked — not from a second copy of the list living in the argv.
print(bn)
print(",".join(feats))
PY
)" || die "policy/decision gate failed (eligibility/export compliance)"

# --- forbidden env / overlay ---
if [[ -n "${TAURI_CONFIG:-}" ]]; then
  die "TAURI_CONFIG overlay is forbidden on release"
fi

# Require manual signing secrets to be present (values never printed).
for v in IOS_CERTIFICATE IOS_CERTIFICATE_PASSWORD IOS_MOBILE_PROVISION APPLE_DEVELOPMENT_TEAM; do
  if [[ -z "${!v:-}" ]]; then
    die "missing required signing env: $v (BLOCKED_EXTERNAL_PREREQUISITE)"
  fi
done
# Automatic-signing API keys must not be mixed in.
for v in APPLE_API_ISSUER APPLE_API_KEY APPLE_API_KEY_PATH; do
  if [[ -n "${!v:-}" ]]; then
    die "automatic signing env $v is set; mixing with manual signing is forbidden"
  fi
done

# Placeholder GGUF detection (NOTGGUF / tiny stub).
STAGE_GGUF="$ROOT/apps/desktop/src-tauri/gen/apple/assets/models/pocket-brain.gguf"
SRC_GGUF="$ROOT/apps/desktop/models/pocket-brain.gguf"
EXPECTED_SIZE=1117320736
EXPECTED_SHA=6a1a2eb6d15622bf3c96857206351ba97e1af16c30d7a74ee38970e434e9407e

check_gguf() {
  local path="$1" label="$2"
  [[ -f "$path" ]] || die "GGUF $label ABSENT path=$path"
  local size sha
  size="$(wc -c <"$path" | tr -d ' ')"
  if command -v shasum >/dev/null 2>&1; then
    sha="$(shasum -a 256 "$path" | awk '{print $1}')"
  else
    sha="$(sha256sum "$path" | awk '{print $1}')"
  fi
  if [[ "$size" != "$EXPECTED_SIZE" || "$sha" != "$EXPECTED_SHA" ]]; then
    die "GGUF $label MISMATCH size=$size sha=$sha (placeholder/stub forbidden)"
  fi
  # Explicit NOTGGUF magic reject
  if head -c 7 "$path" | grep -q 'NOTGGUF'; then
    die "GGUF $label is NOTGGUF placeholder"
  fi
  echo "GGUF $label OK size=$size sha=$sha"
}

check_gguf "$SRC_GGUF" SOURCE

cd "$DESKTOP"

# Split what the policy gate emitted. Both values come from the JSON that was
# just validated, so the argv cannot drift from the policy the way a second
# hardcoded copy could — measured 2026-08-06: adding egress-live to the argv
# alone left every CI check green, because the policy check and the build
# command were two unrelated strings that merely happened to agree.
VALIDATED_BUILD_NUMBER="$(printf '%s\n' "$POLICY_OUT" | sed -n '1p')"
VALIDATED_FEATURES="$(printf '%s\n' "$POLICY_OUT" | sed -n '2p')"
export VALIDATED_BUILD_NUMBER
[[ -n "$VALIDATED_FEATURES" ]] || die "policy gate emitted no feature list"
echo "ios_release_build: invoking fixed release command build_number=$VALIDATED_BUILD_NUMBER features=$VALIDATED_FEATURES"

# Exact allowed command — no --debug/--no-sign/--open/--ignore-version-mismatches/--config.
env -u CARGO_TARGET_DIR npx --no-install tauri ios build \
  --ci \
  --target aarch64 \
  --features "$VALIDATED_FEATURES" \
  --build-number "$VALIDATED_BUILD_NUMBER" \
  --export-method app-store-connect
