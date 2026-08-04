#!/usr/bin/env python3
"""Production checker: release/dev overlay isolation (shared by config gate + drills)."""
from __future__ import annotations

import json
import plistlib
import re
import sys
from pathlib import Path


def main() -> None:
    if len(sys.argv) != 5:
        raise SystemExit(
            "usage: check_release_dev_isolation.py <tauri.ios.conf.json> "
            "<tauri.ios.dev.conf.json> <Info.ios.plist> <Info.ios.dev.plist>"
        )
    rel = json.loads(Path(sys.argv[1]).read_text(encoding="utf-8"))
    dev = json.loads(Path(sys.argv[2]).read_text(encoding="utf-8"))
    info_rel_raw = Path(sys.argv[3]).read_bytes()
    info_dev = Path(sys.argv[4]).read_text(encoding="utf-8")
    if rel.get("build", {}).get("devUrl") is not None:
        raise SystemExit("RED: Release build.devUrl must be null")
    if rel.get("app", {}).get("security", {}).get("devCsp") is not None:
        raise SystemExit("RED: Release devCsp must be null")
    pl = plistlib.loads(re.sub(br"<!--.*?-->", b"", info_rel_raw, flags=re.S))
    for k in ("NSAppTransportSecurity", "NSLocalNetworkUsageDescription"):
        if k in pl:
            raise SystemExit(f"RED: Release Info.ios.plist has forbidden key {k}")
    if "NSAppTransportSecurity" not in info_dev or "NSLocalNetworkUsageDescription" not in info_dev:
        raise SystemExit("RED: Dev Info.ios.dev.plist missing ATS/LocalNetwork")
    if dev.get("build", {}).get("devUrl") != "http://localhost:1420":
        raise SystemExit("RED: Dev overlay missing devUrl")
    print("GREEN: release/dev overlay isolation")


if __name__ == "__main__":
    main()
