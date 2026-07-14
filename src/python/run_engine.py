#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""PKB エンジン起動: python src/python/run_engine.py"""

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

from core.offline_runtime import enforce_offline_environment

enforce_offline_environment()

from engine_stdio import main

if __name__ == "__main__":
    main()
