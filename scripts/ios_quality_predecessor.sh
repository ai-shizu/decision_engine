#!/usr/bin/env bash
# T4-D: Same-SHA quality predecessor — GitHub Checks API, fail-closed.
# Pending/missing continue polling until timeout. Terminal RED states exit immediately.
# Optional mock: QUALITY_PREDECESSOR_CHECK_RUNS_FILE points at a JSON payload file
# (or a directory of attempt-N.json files polled in order).
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
POLICY="$ROOT/apps/desktop/src-tauri/ios/policy/ios-release.policy.json"
TARGET_SHA="${1:-${TARGET_SHA:-}}"
TIMEOUT_SEC="${QUALITY_PREDECESSOR_TIMEOUT_SEC:-1800}"
POLL_SEC="${QUALITY_PREDECESSOR_POLL_SEC:-30}"
RECEIPT="${QUALITY_PREDECESSOR_RECEIPT:-/private/tmp/t4d-logs-fresh/quality_predecessor.receipt.json}"
MOCK_FILE="${QUALITY_PREDECESSOR_CHECK_RUNS_FILE:-}"
MOCK_DIR="${QUALITY_PREDECESSOR_CHECK_RUNS_DIR:-}"
MOCK_IDX=0

die() { echo "ios_quality_predecessor: RED: $*" >&2; exit 1; }

[[ "$TARGET_SHA" =~ ^[0-9a-f]{40}$ ]] || die "TARGET_SHA must be 40-hex, got '${TARGET_SHA:-}'"
command -v python3 >/dev/null 2>&1 || die "python3 required"
if [[ -z "$MOCK_FILE" && -z "$MOCK_DIR" ]]; then
  command -v gh >/dev/null 2>&1 || die "gh CLI required"
fi

REQUIRED_FILE="$(mktemp)"
REQUIRED_SLUG="$(python3 - <<'PY' "$POLICY"
import json, sys
p = json.load(open(sys.argv[1], encoding="utf-8"))
print(p["quality_predecessor_required_app_slug"])
PY
)"
python3 - <<'PY' "$POLICY" >"$REQUIRED_FILE"
import json, sys
p = json.load(open(sys.argv[1], encoding="utf-8"))
names = p["quality_predecessor_checks"]
if len(names) != 13 or len(set(names)) != 13:
    raise SystemExit("policy quality_predecessor_checks must be 13 unique names")
for name in names:
    print(name)
PY
REQUIRED_COUNT="$(wc -l <"$REQUIRED_FILE" | tr -d ' ')"
[[ "$REQUIRED_COUNT" -eq 13 ]] || die "expected 13 unique checks in policy, got $REQUIRED_COUNT"

REPO="${GITHUB_REPOSITORY:-}"
if [[ -z "$REPO" && -z "$MOCK_FILE" && -z "$MOCK_DIR" ]]; then
  REPO="$(gh repo view --json nameWithOwner -q .nameWithOwner)"
fi
[[ -n "$REPO" || -n "$MOCK_FILE" || -n "$MOCK_DIR" ]] || die "cannot resolve GITHUB_REPOSITORY"

echo "ios_quality_predecessor: repo=${REPO:-mock} sha=$TARGET_SHA timeout=${TIMEOUT_SEC}s slug=$REQUIRED_SLUG"
START_EPOCH="$(date -u +%s)"
DEADLINE=$((START_EPOCH + TIMEOUT_SEC))
mkdir -p "$(dirname "$RECEIPT")"

fetch_payload() {
  if [[ -n "$MOCK_DIR" ]]; then
    MOCK_IDX=$((MOCK_IDX + 1))
    local f
    f="$(printf '%s/attempt-%d.json' "$MOCK_DIR" "$MOCK_IDX")"
    if [[ ! -f "$f" ]]; then
      # Reuse last attempt if exhausted (steady state)
      f="$(printf '%s/attempt-%d.json' "$MOCK_DIR" "$((MOCK_IDX - 1))")"
    fi
    [[ -f "$f" ]] || die "mock attempt file missing: $f"
    cat "$f"
    return 0
  fi
  if [[ -n "$MOCK_FILE" ]]; then
    cat "$MOCK_FILE"
    return 0
  fi
  gh api "repos/${REPO}/commits/${TARGET_SHA}/check-runs" --paginate
}

while true; do
  NOW="$(date -u +%s)"
  if [[ "$NOW" -ge "$DEADLINE" ]]; then
    die "timeout after ${TIMEOUT_SEC}s — refusing to start Archive"
  fi

  set +e
  fetch_payload >"$RECEIPT.tmp.json" 2>"$RECEIPT.fetch.err"
  FETCH_EC=$?
  set -e
  if [[ "$FETCH_EC" -ne 0 ]]; then
    echo "ios_quality_predecessor: fetch failed exit=$FETCH_EC (will retry)"
    cat "$RECEIPT.fetch.err" || true
    sleep "$POLL_SEC"
    continue
  fi

  set +e
  python3 - <<'PY' "$RECEIPT.tmp.json" "$TARGET_SHA" "$REQUIRED_FILE" "$REQUIRED_SLUG" >"$RECEIPT.tmp"
import json, sys
from collections import defaultdict

payload = json.loads(open(sys.argv[1], encoding="utf-8").read())
sha = sys.argv[2]
required = [ln.strip() for ln in open(sys.argv[3], encoding="utf-8") if ln.strip()]
required_slug = sys.argv[4]
if len(required) != 13 or len(set(required)) != 13:
    raise SystemExit("internal: required list must be 13 unique")

runs = payload.get("check_runs") or []
if isinstance(payload, list):
    runs = []
    for page in payload:
        runs.extend(page.get("check_runs") or [])

by_name = defaultdict(list)
for r in runs:
    by_name[r.get("name") or ""].append(r)

report = {"target_sha": sha, "checks": {}, "status": "PENDING", "required_app_slug": required_slug}
missing = []
pending = []
bad = []

for name in required:
    # Consider ALL runs with this name on any SHA for duplicate/wrong-sha detection
    all_named = by_name.get(name, [])
    items = [r for r in all_named if (r.get("head_sha") or "") == sha]
    wrong_sha = [r for r in all_named if (r.get("head_sha") or "") and (r.get("head_sha") or "") != sha]

    if not items:
        if wrong_sha:
            bad.append("%s: wrong_sha" % name)
            report["checks"][name] = {
                "state": "wrong_sha",
                "observed_shas": sorted({r.get("head_sha") for r in wrong_sha}),
            }
        else:
            missing.append(name)
            report["checks"][name] = {"state": "missing"}
        continue

    if len(items) != 1:
        bad.append("%s: duplicate_count=%d" % (name, len(items)))
        report["checks"][name] = {
            "state": "duplicate",
            "ids": [i.get("id") for i in items],
        }
        continue

    r = items[0]
    st = r.get("status")
    concl = r.get("conclusion")
    app_slug = ((r.get("app") or {}).get("slug")) or ""
    entry = {
        "state": "%s/%s" % (st, concl),
        "check_run_id": r.get("id"),
        "html_url": r.get("html_url"),
        "head_sha": r.get("head_sha"),
        "app": app_slug,
    }
    report["checks"][name] = entry

    if app_slug != required_slug:
        bad.append("%s: wrong_app_slug=%s" % (name, app_slug))
        entry["state"] = "wrong_app"
        continue

    if st != "completed":
        pending.append(name)
        continue

    if concl != "success":
        # completed failure/cancelled/skipped/neutral/timed_out => terminal RED
        bad.append("%s: completed/%s" % (name, concl))
        continue

if bad:
    report["status"] = "RED"
    report["missing"] = missing
    report["pending"] = pending
    report["bad"] = bad
    print(json.dumps(report, indent=2, ensure_ascii=False))
    raise SystemExit(3)  # terminal RED

if missing or pending:
    report["status"] = "PENDING"
    report["missing"] = missing
    report["pending"] = pending
    print(json.dumps(report, indent=2, ensure_ascii=False))
    raise SystemExit(2)  # keep polling

report["status"] = "GREEN"
print(json.dumps(report, indent=2, ensure_ascii=False))
PY
  EC=$?
  set -e

  if [[ "$EC" -eq 0 ]]; then
    mv "$RECEIPT.tmp" "$RECEIPT"
    echo "ios_quality_predecessor: ALL 13 CHECKS GREEN on $TARGET_SHA"
    cat "$RECEIPT"
    exit 0
  fi
  if [[ "$EC" -eq 3 ]]; then
    mv "$RECEIPT.tmp" "$RECEIPT"
    cat "$RECEIPT"
    die "terminal RED (failure/cancelled/skipped/duplicate/wrong sha/app)"
  fi
  if [[ "$EC" -eq 2 ]]; then
    echo "ios_quality_predecessor: pending/missing — poll ($(date -u +%Y-%m-%dT%H:%M:%SZ))"
    sleep "$POLL_SEC"
    continue
  fi
  echo "ios_quality_predecessor: verifier exit=$EC — retry"
  sleep "$POLL_SEC"
done
