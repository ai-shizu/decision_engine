#!/usr/bin/env python3
# Byte-scan LAN/CIDR/dev-URL detector used by ios_archive_scan.sh count_lan_strings.
# Pattern is passed in (from LAN_STRING_RE in ios_archive_scan.sh — single definition).
# Semantics: count of non-overlapping ERE matches (same as `grep -aoE | wc -l`).
# Reads as raw bytes via mmap; never decodes as text (invalid UTF-8 must not throw).
from __future__ import annotations

import mmap
import re
import sys


def count_matches(path: str, pattern: str) -> int:
    # Translate grep -E pattern to Python bytes regex. Pattern is ASCII-only ERE.
    cre = re.compile(pattern.encode("ascii"))
    with open(path, "rb") as f:
        try:
            size = f.seek(0, 2)
            f.seek(0)
        except OSError:
            size = 0
        if size == 0:
            data = f.read()
            return len(cre.findall(data))
        with mmap.mmap(f.fileno(), 0, access=mmap.ACCESS_READ) as mm:
            return len(cre.findall(mm))


def list_matches(path: str, pattern: str, limit: int = 20) -> list[bytes]:
    cre = re.compile(pattern.encode("ascii"))
    out: list[bytes] = []
    with open(path, "rb") as f:
        try:
            size = f.seek(0, 2)
            f.seek(0)
        except OSError:
            size = 0
        if size == 0:
            data = f.read()
            for m in cre.finditer(data):
                out.append(m.group(0))
                if len(out) >= limit:
                    break
            return out
        with mmap.mmap(f.fileno(), 0, access=mmap.ACCESS_READ) as mm:
            for m in cre.finditer(mm):
                out.append(m.group(0))
                if len(out) >= limit:
                    break
    return out


def main() -> int:
    if len(sys.argv) < 3:
        print("usage: count_lan_strings.py <path> <LAN_STRING_RE> [--list]", file=sys.stderr)
        return 2
    path, pattern = sys.argv[1], sys.argv[2]
    if len(sys.argv) >= 4 and sys.argv[3] == "--list":
        for m in list_matches(path, pattern):
            # Never decode — emit latin-1 so arbitrary bytes print
            sys.stdout.buffer.write(m + b"\n")
        return 0
    print(count_matches(path, pattern))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
