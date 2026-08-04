#!/usr/bin/env python3
# DISPOSABLE mutant for F-2-e mutation drill ONLY.
#
# Reproduces the F-1 failure mode that motivated byte-scan:
# under a non-C locale, BSD grep can return 0 hits on Mach-O / binary plist
# even when the ASCII marker bytes are present. This helper:
#   - LC_ALL=C  → real `grep -aoE` count (inherits C)
#   - otherwise → returns 0 for Mach-O / Apple binary plist inputs
#                 (text inputs still use grep, matching F-1 matrix)
#
# Do not use as the production detector.
from __future__ import annotations

import os
import subprocess
import sys
from pathlib import Path


def _is_macho_or_bplist(path: str) -> bool:
    head = Path(path).read_bytes()[:8]
    # Mach-O 64-bit LE / BE, fat, or Apple binary plist
    if head.startswith(b"bplist00"):
        return True
    if head[:4] in (
        b"\xcf\xfa\xed\xfe",  # MH_MAGIC_64
        b"\xfe\xed\xfa\xcf",  # MH_CIGAM_64
        b"\xce\xfa\xed\xfe",  # MH_MAGIC
        b"\xfe\xed\xfa\xce",  # MH_CIGAM
        b"\xca\xfe\xba\xbe",  # FAT_MAGIC
        b"\xbe\xba\xfe\xca",  # FAT_CIGAM
    ):
        return True
    return False


def _grep_count(path: str, pattern: str, env: dict[str, str] | None = None) -> int:
    proc = subprocess.run(
        ["grep", "-aoE", pattern, path],
        stdout=subprocess.PIPE,
        stderr=subprocess.DEVNULL,
        check=False,
        env=env,
    )
    n = proc.stdout.count(b"\n")
    if proc.stdout and not proc.stdout.endswith(b"\n"):
        n += 1
    return n


def main() -> int:
    if len(sys.argv) < 3:
        print(
            "usage: count_lan_strings_grep_mutant.py <path> <LAN_STRING_RE> [--list]",
            file=sys.stderr,
        )
        return 2
    path, pattern = sys.argv[1], sys.argv[2]
    lc = os.environ.get("LC_ALL", "")

    # F-1 shape: non-C locale + binary input → blind (0), regardless of host grep.
    if lc != "C" and _is_macho_or_bplist(path):
        if len(sys.argv) >= 4 and sys.argv[3] == "--list":
            return 0
        print(0)
        return 0

    env = os.environ.copy()
    if lc == "C":
        env["LC_ALL"] = "C"

    if len(sys.argv) >= 4 and sys.argv[3] == "--list":
        proc = subprocess.run(
            ["grep", "-aoE", pattern, path],
            stdout=subprocess.PIPE,
            stderr=subprocess.DEVNULL,
            check=False,
            env=env,
        )
        sys.stdout.buffer.write(proc.stdout)
        return 0

    print(_grep_count(path, pattern, env=env))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
