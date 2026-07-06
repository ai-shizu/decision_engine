#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""TUI 起動ラッパー (後方互換: python src/ui/app.py)。"""

import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / "src" / "python"))

from ui_tui.app import DecisionDashboard  # noqa: E402

if __name__ == "__main__":
    DecisionDashboard().run()
