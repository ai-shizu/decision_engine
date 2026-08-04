#!/usr/bin/env bash
# T4-E H-7 — Mutation drill for ios_cfg_test_forbid_gate.sh.
#
# Plants one #[test] under an existing cfg(target_os = "ios") block, expects
# the production gate to RED, restores the file byte-identically, then expects
# GREEN again. A gate that stays green after the plant is not a gate.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
GATE="$ROOT/scripts/ios_cfg_test_forbid_gate.sh"
TARGET="$ROOT/apps/desktop/src-tauri/src/blackbox_arena/handle.rs"
LOG_DIR="${T4E_LOG_DIR:-/tmp/t4e-evidence/mutations}"
mkdir -p "$LOG_DIR"

sha_of() { shasum -a 256 "$1" | awk '{print $1}'; }

[[ -f "$GATE" ]] || { echo "missing gate: $GATE" >&2; exit 1; }
[[ -f "$TARGET" ]] || { echo "missing target: $TARGET" >&2; exit 1; }

BACKUP="$LOG_DIR/handle.rs.ios_cfg_forbid.bak"
cp "$TARGET" "$BACKUP"
BEFORE="$(sha_of "$TARGET")"
echo "DRILL[ios_cfg_test_forbid] before_sha=$BEFORE"

# Insert a disposable #[test] immediately inside the log_arena_terminal_ios body
# (that function is cfg(target_os = "ios")-gated). Use a unique marker.
python3 - "$TARGET" <<'PY'
from pathlib import Path
import sys
p = Path(sys.argv[1])
text = p.read_text(encoding="utf-8")
needle = "fn log_arena_terminal_ios(state: crate::blackbox_sim::fsm::SessionState, turns: u32) {\n"
insert = (
    needle
    + "    #[test]\n"
    + "    fn t4e_mutation_plant_must_not_remain() { assert!(false); }\n"
)
if needle not in text:
    raise SystemExit("anchor not found for mutation plant")
if "t4e_mutation_plant_must_not_remain" in text:
    raise SystemExit("mutation plant already present")
p.write_text(text.replace(needle, insert, 1), encoding="utf-8")
print("planted #[test] under cfg(target_os = \"ios\") log_arena_terminal_ios")
PY

set +e
bash "$GATE" "$ROOT" >"$LOG_DIR/ios_cfg_test_forbid.red.log" 2>&1
RED_EC=$?
set -e
echo "DRILL[ios_cfg_test_forbid] expected_exit!=0 actual_exit=$RED_EC"
cat "$LOG_DIR/ios_cfg_test_forbid.red.log"

# Restore from pre-plant backup (works on dirty trees; do not git checkout).
cp "$BACKUP" "$TARGET"
AFTER="$(sha_of "$TARGET")"
echo "DRILL[ios_cfg_test_forbid] after_restore_sha=$AFTER"
[[ "$BEFORE" == "$AFTER" ]] || {
  echo "DRILL[ios_cfg_test_forbid]: FAIL restore mismatch" >&2
  exit 1
}

set +e
bash "$GATE" "$ROOT" >"$LOG_DIR/ios_cfg_test_forbid.green.log" 2>&1
GREEN_EC=$?
set -e
echo "DRILL[ios_cfg_test_forbid] restore_exit=$GREEN_EC"
cat "$LOG_DIR/ios_cfg_test_forbid.green.log"

if [[ "$RED_EC" -ne 0 && "$GREEN_EC" -eq 0 ]]; then
  echo "DRILL[ios_cfg_test_forbid]: PASS"
  exit 0
fi
echo "DRILL[ios_cfg_test_forbid]: FAIL" >&2
exit 1
