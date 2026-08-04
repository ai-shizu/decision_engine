#!/usr/bin/env python3
"""Production checker: get-task-allow must be false or absent."""
from __future__ import annotations

import plistlib
import sys
from pathlib import Path


def main() -> None:
    if len(sys.argv) != 2:
        raise SystemExit("usage: check_get_task_allow.py <entitlements.plist>")
    pl = plistlib.loads(Path(sys.argv[1]).read_bytes())
    if pl.get("get-task-allow") is True:
        raise SystemExit("RED: get-task-allow=true")
    print("GREEN: get-task-allow false/absent")


if __name__ == "__main__":
    main()
