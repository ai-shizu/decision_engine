#!/usr/bin/env python3
"""Production checker: TARGETED_DEVICE_FAMILY must be 1 (not 1,2)."""
from __future__ import annotations

import re
import sys
from pathlib import Path


def main() -> None:
    if len(sys.argv) != 2:
        raise SystemExit("usage: check_device_family.py <project.pbxproj>")
    text = Path(sys.argv[1]).read_text(encoding="utf-8")
    if re.search(r'TARGETED_DEVICE_FAMILY\s*=\s*"1,2"', text):
        raise SystemExit("RED: TARGETED_DEVICE_FAMILY=1,2")
    if not re.search(r'TARGETED_DEVICE_FAMILY\s*=\s*"?1"?\s*;', text):
        raise SystemExit("RED: TARGETED_DEVICE_FAMILY=1 missing")
    print("GREEN: TARGETED_DEVICE_FAMILY=1")


if __name__ == "__main__":
    main()
