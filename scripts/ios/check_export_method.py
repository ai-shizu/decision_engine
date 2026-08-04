#!/usr/bin/env python3
"""Production checker: ExportOptions.method must be app-store-connect."""
from __future__ import annotations

import plistlib
import sys
from pathlib import Path


def main() -> None:
    if len(sys.argv) != 2:
        raise SystemExit("usage: check_export_method.py <ExportOptions.plist>")
    pl = plistlib.loads(Path(sys.argv[1]).read_bytes())
    method = pl.get("method")
    if method not in ("app-store-connect", "app-store"):
        raise SystemExit(f"RED: export method={method!r} not app-store-connect")
    print(f"GREEN: ExportOptions method={method}")


if __name__ == "__main__":
    main()
