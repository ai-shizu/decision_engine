#!/usr/bin/env bash
# T4-E §4.3 — Forbid #[test] / #[cfg(test)] under cfg(target_os = "ios").
#
# Why: those tests compile under aarch64-apple-ios-sim cargo check, but no CI
# job executes them. sanitize_one_liner lived in that shape for months and
# asserted something false without ever failing. A gate that cannot go RED
# is not a gate — this one RED-fails on the first planted ios-cfg test.
#
# Usage:
#   bash scripts/ios_cfg_test_forbid_gate.sh [root_dir]
#
# Exit 0 = GREEN (no ios-cfg-gated tests). Exit 1 = RED.
set -euo pipefail

ROOT="${1:-$(cd "$(dirname "$0")/.." && pwd)}"
SRC="$ROOT/apps/desktop/src-tauri/src"

[[ -d "$SRC" ]] || {
  echo "ios_cfg_test_forbid_gate: RED: missing src tree: $SRC" >&2
  exit 1
}

python3 - "$SRC" <<'PY'
import re
import sys
from pathlib import Path

src = Path(sys.argv[1])
cfg_re = re.compile(r'#\[cfg\s*\(\s*target_os\s*=\s*"ios"\s*\)\s*\]')
test_attr_re = re.compile(r'#\[(?:test|cfg\s*\(\s*test\s*\))\]')

violations = []

def skip_ws_and_attrs(lines, i):
    """Advance past blank lines and additional #[] attributes after the ios cfg."""
    n = len(lines)
    while i < n:
        s = lines[i].strip()
        if s == "" or s.startswith("//"):
            i += 1
            continue
        if s.startswith("#[") and s.endswith("]"):
            i += 1
            continue
        if s.startswith("#["):
            # multi-line attribute — consume until closing ]
            while i < n and "]" not in lines[i]:
                i += 1
            i += 1
            continue
        break
    return i

def item_end(lines, start):
    """Return exclusive end index of the item starting at `start` (brace-balanced)."""
    n = len(lines)
    # Find first '{' on this or following lines; if ';' first, item is a single stmt.
    i = start
    brace = 0
    seen_brace = False
    while i < n:
        line = lines[i]
        # strip line comments for brace counting (good enough for this gate)
        code = line.split("//", 1)[0]
        if not seen_brace:
            if ";" in code and "{" not in code:
                return i + 1
            if "{" in code:
                seen_brace = True
        brace += code.count("{") - code.count("}")
        i += 1
        if seen_brace and brace <= 0:
            return i
    return n

for path in sorted(src.rglob("*.rs")):
    text = path.read_text(encoding="utf-8")
    lines = text.splitlines()
    for idx, line in enumerate(lines):
        if not cfg_re.search(line):
            continue
        # cfg applies to the next item (after other attrs).
        item_start = skip_ws_and_attrs(lines, idx + 1)
        if item_start >= len(lines):
            continue
        # Also scan attrs between cfg and item for #[test] / #[cfg(test)].
        between = "\n".join(lines[idx + 1 : item_start + 1])
        end = item_end(lines, item_start)
        body = "\n".join(lines[idx:end])
        # Match test attrs that appear under this gated item (including stacked attrs).
        for m in test_attr_re.finditer(body):
            # Locate line number of the match within `body`.
            prefix = body[: m.start()]
            rel = prefix.count("\n")
            abs_line = idx + 1 + rel
            violations.append(f"{path}:{abs_line}: ios-cfg-gated test attribute: {m.group(0)}")

if violations:
    print("ios_cfg_test_forbid_gate: RED")
    for v in violations:
        print(v)
    sys.exit(1)

print("ios_cfg_test_forbid_gate: GREEN")
print("ios_cfg_blocks_scanned_ok")
sys.exit(0)
PY
