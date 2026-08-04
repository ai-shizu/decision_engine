#!/usr/bin/env python3
"""Production checker: PrivacyInfo API inventory vs policy."""
from __future__ import annotations

import json
import plistlib
import re
import sys
from pathlib import Path


def main() -> None:
    if len(sys.argv) != 3:
        raise SystemExit("usage: check_privacy_inventory.py <PrivacyInfo.xcprivacy> <policy.json>")
    raw = re.sub(br"<!--.*?-->", b"", Path(sys.argv[1]).read_bytes(), flags=re.S)
    pl = plistlib.loads(raw)
    pol = json.loads(Path(sys.argv[2]).read_text(encoding="utf-8"))
    apis = pl.get("NSPrivacyAccessedAPITypes") or []
    got = {
        (a.get("NSPrivacyAccessedAPIType"), tuple(a.get("NSPrivacyAccessedAPITypeReasons") or []))
        for a in apis
    }
    exp = {(e["type"], tuple(e["reasons"])) for e in pol["privacy_manifest"]["accessed_api_types"]}
    if got != exp:
        raise SystemExit(f"RED: Privacy inventory mismatch got={got} exp={exp}")
    print("GREEN: PrivacyInfo inventory matches policy")


if __name__ == "__main__":
    main()
