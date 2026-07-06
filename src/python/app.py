#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""CLI 起動ラッパー (後方互換: python src/python/app.py)。"""

import sys
from pathlib import Path

_ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(_ROOT / "src" / "python"))

from core.cli import main

if __name__ == "__main__":
    main()
