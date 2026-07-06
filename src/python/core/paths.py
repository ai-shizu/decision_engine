#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""プロジェクトルートとデータパス (core / ui_tui 共通)。"""

from __future__ import annotations

import os
from pathlib import Path

# core/paths.py -> core -> python -> src -> project_root
_PROJECT_ROOT = Path(__file__).resolve().parents[3]
PROJECT_ROOT = Path(os.environ.get("PKB_PROJECT_ROOT", _PROJECT_ROOT))

DATA_RAW = PROJECT_ROOT / "data" / "raw"
DATA_PROCESSED = PROJECT_ROOT / "data" / "processed"
DATA_KNOWLEDGE = PROJECT_ROOT / "data" / "knowledge"
BUILD_DIR = PROJECT_ROOT / "build"
PROCESSED = DATA_PROCESSED
KNOWLEDGE_DIR = DATA_KNOWLEDGE
MODELS_DIR = Path(os.environ.get("PKB_MODELS_DIR", PROJECT_ROOT / "models"))
LLAMA_DIR = Path(os.environ.get("PKB_LLAMA_DIR", PROJECT_ROOT / "tools" / "llama-arm64"))

DIARY_MD = DATA_RAW / "diary.md"
LINE_HISTORY = DATA_RAW / "line_history.txt"
CALENDAR_JSON = DATA_RAW / "calendar.json"
FINANCE_JSON = DATA_RAW / "finance.json"
AI_CONSULTATIONS_JSON = DATA_RAW / "ai_consultations.json"

DIARY_BIN = DATA_PROCESSED / "vectors.bin"
DIARY_META = DATA_PROCESSED / "metadata.json"
KNOWLEDGE_BIN = DATA_PROCESSED / "knowledge_vectors.bin"
KNOWLEDGE_META = DATA_PROCESSED / "knowledge_metadata.json"
DEEP_PROFILE = DATA_PROCESSED / "deep_profile.json"
USER_PROFILE = DATA_PROCESSED / "user_profile.json"
SEARCH_EXE = BUILD_DIR / "search_engine.exe"
CALENDAR_IMPORT_ICS = DATA_RAW / "calendar_import.ics"
QUERY_BIN = DATA_PROCESSED / "query.bin"
RAW_DIARY = DIARY_MD
OUT_DIR = DATA_PROCESSED
VECTORS_BIN = DIARY_BIN
METADATA_JSON = DIARY_META
