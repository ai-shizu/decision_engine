#!/usr/bin/env bash
# T4-E H-4 — Mutation drill for arena.dead_reason positive-control tests.
#
# Breaks arena_terminal_codes so every state maps to (0, 0), runs the driven-
# session host tests, expects RED, restores, expects GREEN. Proves the
# positive-control tests can go red when the instrument is sabotaged.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
TARGET="$ROOT/apps/desktop/src-tauri/src/blackbox_arena/handle.rs"
LOG_DIR="${T4E_LOG_DIR:-/tmp/t4e-evidence/mutations}"
mkdir -p "$LOG_DIR"
SRC_TAURI="$ROOT/apps/desktop/src-tauri"

sha_of() { shasum -a 256 "$1" | awk '{print $1}'; }

[[ -f "$TARGET" ]] || { echo "missing $TARGET" >&2; exit 1; }

BACKUP="$LOG_DIR/handle.rs.arena_dead_reason.bak"
cp "$TARGET" "$BACKUP"
BEFORE="$(sha_of "$TARGET")"
echo "DRILL[arena_dead_reason_instrument] before_sha=$BEFORE"

python3 - "$TARGET" <<'PY'
from pathlib import Path
import sys
p = Path(sys.argv[1])
text = p.read_text(encoding="utf-8")
# Sabotage: force the instrument to always report "no death".
old = """pub(crate) const fn arena_terminal_codes(
    state: crate::blackbox_sim::fsm::SessionState,
) -> (u64, u64) {
    use crate::blackbox_sim::fsm::{FailureReason, SessionState};
    match state {
        SessionState::Genesis | SessionState::Active { .. } => (0, 0),
        SessionState::Sealed => (1, 0),
        SessionState::Dead { reason } => (
            1,
            match reason {
                FailureReason::AccountingBreach => 1,
                FailureReason::SnapshotDigestMismatch => 2,
                FailureReason::ReplayDivergence => 3,
                FailureReason::InternalInvariantBroken => 4,
            },
        ),
    }
}"""
new = """pub(crate) const fn arena_terminal_codes(
    state: crate::blackbox_sim::fsm::SessionState,
) -> (u64, u64) {
    let _ = state;
    // T4-E mutation plant: instrument permanently reports zero.
    (0, 0)
}"""
if old not in text:
    raise SystemExit("arena_terminal_codes body not found for mutation")
p.write_text(text.replace(old, new, 1), encoding="utf-8")
print("planted always-zero arena_terminal_codes")
PY

set +e
(
  cd "$SRC_TAURI"
  cargo test --features blackbox-sim --lib \
    driven_session_inventory_desync_emits_dead_reason_4 \
    -- --nocapture
) >"$LOG_DIR/arena_dead_reason.red.log" 2>&1
RED_EC=$?
set -e
echo "DRILL[arena_dead_reason_instrument] expected_exit!=0 actual_exit=$RED_EC"
# Show the assertion failures (tail keeps evidence readable).
tail -n 80 "$LOG_DIR/arena_dead_reason.red.log"

cp "$BACKUP" "$TARGET"
AFTER="$(sha_of "$TARGET")"
echo "DRILL[arena_dead_reason_instrument] after_restore_sha=$AFTER"
[[ "$BEFORE" == "$AFTER" ]] || {
  echo "DRILL[arena_dead_reason_instrument]: FAIL restore mismatch" >&2
  exit 1
}

set +e
(
  cd "$SRC_TAURI"
  cargo test --features blackbox-sim --lib \
    driven_session_inventory_desync_emits_dead_reason_4 \
    -- --nocapture
) >"$LOG_DIR/arena_dead_reason.green.log" 2>&1
GREEN_EC=$?
set -e
echo "DRILL[arena_dead_reason_instrument] restore_exit=$GREEN_EC"
tail -n 40 "$LOG_DIR/arena_dead_reason.green.log"

if [[ "$RED_EC" -ne 0 && "$GREEN_EC" -eq 0 ]]; then
  echo "DRILL[arena_dead_reason_instrument]: PASS"
  exit 0
fi
echo "DRILL[arena_dead_reason_instrument]: FAIL" >&2
exit 1
