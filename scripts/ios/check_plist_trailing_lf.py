#!/usr/bin/env python3
"""Production checker: plist/XML must end with a trailing LF byte."""
from __future__ import annotations

import sys
from pathlib import Path


def main() -> None:
    if len(sys.argv) != 2:
        raise SystemExit("usage: check_plist_trailing_lf.py <file>")
    path = Path(sys.argv[1])
    data = path.read_bytes()
    if not data:
        raise SystemExit("RED: empty file")
    if not data.endswith(b"\n"):
        raise SystemExit("RED: missing trailing LF")
    print("GREEN: trailing LF present")


if __name__ == "__main__":
    main()
