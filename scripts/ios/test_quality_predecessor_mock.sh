#!/usr/bin/env bash
# T4-D: Mock Checks API scenarios for ios_quality_predecessor.sh
# Covers: pending→success, terminal failure, timeout, duplicate, wrong SHA/app.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
PRED="$ROOT/scripts/ios_quality_predecessor.sh"
POLICY="$ROOT/apps/desktop/src-tauri/ios/policy/ios-release.policy.json"
SHA=aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa
WORK="$(mktemp -d "${TMPDIR:-/tmp}/t4d-qp-mock.XXXXXX")"
LOG_DIR="${T4D_LOG_DIR:-/private/tmp/t4d-logs-fresh/qp-mock}"
mkdir -p "$LOG_DIR" "$WORK"

NAMES=()
while IFS= read -r line; do
  NAMES+=("$line")
done < <(python3 -c 'import json;print("\n".join(json.load(open("'"$POLICY"'"))["quality_predecessor_checks"]))')
[[ "${#NAMES[@]}" -eq 13 ]]

make_runs() {
  # args: mode
  local mode="$1"
  python3 - <<'PY' "$mode" "$SHA" "${NAMES[@]}"
import json, sys
mode = sys.argv[1]
sha = sys.argv[2]
names = sys.argv[3:]
runs = []
for i, name in enumerate(names):
    st, concl, head, slug = "completed", "success", sha, "github-actions"
    if mode == "pending":
        st, concl = "in_progress", None
    elif mode == "success":
        pass
    elif mode == "failure":
        if i == 0:
            concl = "failure"
    elif mode == "duplicate":
        # two success runs same name/sha for first check
        pass
    elif mode == "wrong_sha":
        head = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"
    elif mode == "wrong_app":
        slug = "not-github-actions"
    run = {
        "id": 1000 + i,
        "name": name,
        "head_sha": head,
        "status": st,
        "conclusion": concl,
        "html_url": "https://example.invalid/%d" % (1000 + i),
        "app": {"slug": slug},
    }
    runs.append(run)
    if mode == "duplicate" and i == 0:
        runs.append(dict(run, id=2000))
print(json.dumps({"check_runs": runs}))
PY
}

pass_count=0
fail_count=0
record() {
  local n="$1" s="$2"
  echo "QP_MOCK[$n]: $s"
  if [[ "$s" == PASS ]]; then pass_count=$((pass_count+1)); else fail_count=$((fail_count+1)); fi
}

# 1) pending → success
DIR1="$WORK/pending_success"
mkdir -p "$DIR1"
make_runs pending >"$DIR1/attempt-1.json"
make_runs success >"$DIR1/attempt-2.json"
set +e
QUALITY_PREDECESSOR_CHECK_RUNS_DIR="$DIR1" \
QUALITY_PREDECESSOR_TIMEOUT_SEC=10 \
QUALITY_PREDECESSOR_POLL_SEC=0 \
QUALITY_PREDECESSOR_RECEIPT="$LOG_DIR/pending_success.receipt.json" \
bash "$PRED" "$SHA" >"$LOG_DIR/pending_success.log" 2>&1
EC=$?
set -e
echo "pending→success exit=$EC"
[[ "$EC" -eq 0 ]] && record pending_success PASS || record pending_success FAIL

# 2) terminal failure
DIR2="$WORK/failure"
mkdir -p "$DIR2"
make_runs failure >"$DIR2/attempt-1.json"
set +e
QUALITY_PREDECESSOR_CHECK_RUNS_DIR="$DIR2" \
QUALITY_PREDECESSOR_TIMEOUT_SEC=5 \
QUALITY_PREDECESSOR_POLL_SEC=0 \
QUALITY_PREDECESSOR_RECEIPT="$LOG_DIR/failure.receipt.json" \
bash "$PRED" "$SHA" >"$LOG_DIR/failure.log" 2>&1
EC=$?
set -e
echo "failure exit=$EC"
[[ "$EC" -ne 0 ]] && grep -q 'terminal RED\|completed/failure\|RED' "$LOG_DIR/failure.log" \
  && record terminal_failure PASS || record terminal_failure FAIL

# 3) timeout (always pending)
DIR3="$WORK/timeout"
mkdir -p "$DIR3"
make_runs pending >"$DIR3/attempt-1.json"
set +e
QUALITY_PREDECESSOR_CHECK_RUNS_DIR="$DIR3" \
QUALITY_PREDECESSOR_TIMEOUT_SEC=1 \
QUALITY_PREDECESSOR_POLL_SEC=1 \
QUALITY_PREDECESSOR_RECEIPT="$LOG_DIR/timeout.receipt.json" \
bash "$PRED" "$SHA" >"$LOG_DIR/timeout.log" 2>&1
EC=$?
set -e
echo "timeout exit=$EC"
[[ "$EC" -ne 0 ]] && grep -q 'timeout' "$LOG_DIR/timeout.log" \
  && record timeout PASS || record timeout FAIL

# 4) duplicate
DIR4="$WORK/dup"
mkdir -p "$DIR4"
make_runs duplicate >"$DIR4/attempt-1.json"
set +e
QUALITY_PREDECESSOR_CHECK_RUNS_DIR="$DIR4" \
QUALITY_PREDECESSOR_TIMEOUT_SEC=5 \
QUALITY_PREDECESSOR_POLL_SEC=0 \
QUALITY_PREDECESSOR_RECEIPT="$LOG_DIR/dup.receipt.json" \
bash "$PRED" "$SHA" >"$LOG_DIR/dup.log" 2>&1
EC=$?
set -e
[[ "$EC" -ne 0 ]] && grep -q 'duplicate' "$LOG_DIR/dup.log" \
  && record duplicate PASS || record duplicate FAIL

# 5) wrong SHA
DIR5="$WORK/wrong_sha"
mkdir -p "$DIR5"
make_runs wrong_sha >"$DIR5/attempt-1.json"
set +e
QUALITY_PREDECESSOR_CHECK_RUNS_DIR="$DIR5" \
QUALITY_PREDECESSOR_TIMEOUT_SEC=5 \
QUALITY_PREDECESSOR_POLL_SEC=0 \
QUALITY_PREDECESSOR_RECEIPT="$LOG_DIR/wrong_sha.receipt.json" \
bash "$PRED" "$SHA" >"$LOG_DIR/wrong_sha.log" 2>&1
EC=$?
set -e
[[ "$EC" -ne 0 ]] && grep -q 'wrong_sha\|wrong sha\|wrong_sha' "$LOG_DIR/wrong_sha.log" \
  && record wrong_sha PASS || record wrong_sha FAIL

# 6) wrong app slug
DIR6="$WORK/wrong_app"
mkdir -p "$DIR6"
make_runs wrong_app >"$DIR6/attempt-1.json"
set +e
QUALITY_PREDECESSOR_CHECK_RUNS_DIR="$DIR6" \
QUALITY_PREDECESSOR_TIMEOUT_SEC=5 \
QUALITY_PREDECESSOR_POLL_SEC=0 \
QUALITY_PREDECESSOR_RECEIPT="$LOG_DIR/wrong_app.receipt.json" \
bash "$PRED" "$SHA" >"$LOG_DIR/wrong_app.log" 2>&1
EC=$?
set -e
[[ "$EC" -ne 0 ]] && grep -q 'wrong_app\|wrong_app_slug\|wrong app' "$LOG_DIR/wrong_app.log" \
  && record wrong_app PASS || record wrong_app FAIL

echo "=== QP mock summary pass=$pass_count fail=$fail_count ==="
[[ "$fail_count" -eq 0 ]]
