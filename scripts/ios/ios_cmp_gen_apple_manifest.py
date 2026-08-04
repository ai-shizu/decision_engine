#!/usr/bin/env python3
"""Compare two gen/apple trees (exclude build/) — any add/delete/content drift is RED."""
from __future__ import annotations

import hashlib
import sys
from pathlib import Path


def fail(msg: str) -> None:
    print(f"ios_cmp_gen_apple_manifest: RED: {msg}", file=sys.stderr)
    raise SystemExit(1)


def file_digest(path: Path) -> str:
    h = hashlib.sha256()
    with path.open("rb") as f:
        for chunk in iter(lambda: f.read(1024 * 1024), b""):
            h.update(chunk)
    return h.hexdigest()


def manifest(root: Path) -> dict[str, str]:
    out: dict[str, str] = {}
    for path in sorted(root.rglob("*")):
        if not path.is_file():
            continue
        rel = path.relative_to(root).as_posix()
        if rel == "build" or rel.startswith("build/"):
            continue
        # Per-user Xcode UI state (window layout, navigator selection). Never
        # emitted by ios_regenerate_tree.sh and untracked by git, so a fresh CI
        # checkout has none while any machine that has opened Xcode does — the
        # comparison would then pass in CI and fail locally for the same commit.
        # That bites on a schedule: the unpaid Personal Team profile expires
        # every 7 days and the re-sign (C-1) requires opening Xcode.
        if "xcuserdata" in path.relative_to(root).parts:
            continue
        out[rel] = file_digest(path)
    return out


def main() -> None:
    if len(sys.argv) != 3:
        fail("usage: ios_cmp_gen_apple_manifest.py <tree_a> <tree_b>")
    a = Path(sys.argv[1])
    b = Path(sys.argv[2])
    if not a.is_dir() or not b.is_dir():
        fail("both arguments must be directories")
    ma, mb = manifest(a), manifest(b)
    only_a = sorted(set(ma) - set(mb))
    only_b = sorted(set(mb) - set(ma))
    changed = sorted(k for k in set(ma) & set(mb) if ma[k] != mb[k])
    if only_a or only_b or changed:
        for p in only_a:
            print(f"ONLY_A: {p} sha={ma[p]}")
        for p in only_b:
            print(f"ONLY_B: {p} sha={mb[p]}")
        for p in changed:
            print(f"CHANGED: {p} a={ma[p]} b={mb[p]}")
        fail(
            f"manifest drift only_a={len(only_a)} only_b={len(only_b)} changed={len(changed)}"
        )
    print(f"ios_cmp_gen_apple_manifest: GREEN files={len(ma)} (build/ excluded)")
    for rel in sorted(ma):
        print(f"  {ma[rel]}  {rel}")


if __name__ == "__main__":
    main()
