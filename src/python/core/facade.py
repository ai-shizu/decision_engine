#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""
PKB 共通サービス層
==================
TUI / デスクトップ UI から共有するビジネス操作。
UI 依存は一切含まない。
"""

from __future__ import annotations

import sys
from datetime import date
from pathlib import Path
from typing import Callable

from . import apple_calendar_sync
from . import calendar_manager as cal
from . import calendar_sync
from . import finance_manager as fin
from .consultation_engine import (
    ConsultationEngine,
    FIXED_ATTRIBUTE_FIELDS,
    LINE_HISTORY,
    USER_PROFILE,
    format_user_profile_summary,
    load_user_profile,
    save_fixed_attributes,
)
from .paths import DIARY_MD, PROJECT_ROOT, PROCESSED

StatusCallback = Callable[[str], None]

_engine: ConsultationEngine | None = None


def get_engine() -> ConsultationEngine:
    global _engine
    if _engine is None:
        _engine = ConsultationEngine()
    return _engine


def shutdown_engine() -> None:
    global _engine
    if _engine is not None:
        _engine.shutdown()
        _engine = None


def load_record(date_str: str) -> dict:
    return {
        "date": date_str,
        "events": cal.get_events_for_date(date_str),
        "transactions": fin.get_transactions_for_date(date_str),
        "diary": cal.load_diary_for_date(date_str),
    }


def save_record(
    date_str: str,
    events: list[dict],
    transactions: list[dict],
    diary: str,
) -> dict:
    date.fromisoformat(date_str)
    cal.set_events_for_date(date_str, events)
    fin.set_transactions_for_date(date_str, transactions)
    body = diary.strip()
    if body:
        cal.save_diary_for_date(date_str, body)
    elif cal.load_diary_for_date(date_str):
        cal.save_diary_for_date(date_str, "")
    rebuilt = get_engine().sync_diary_index(force=True)
    return {"date": date_str, "saved": True, "index_rebuilt": rebuilt}


def consult(query: str, status: StatusCallback | None = None) -> str:
    return get_engine().consult(query, status=status)


def sync_diary_index(force: bool = False) -> bool:
    return get_engine().sync_diary_index(force=force)


def sync_calendar(
    source: str,
    mode: str = "append",
    *,
    ics_path: str | Path | None = None,
) -> dict:
    if mode not in ("append", "overwrite"):
        raise ValueError(f"不明なマージモード: {mode}")
    if source == "ics":
        if not ics_path:
            raise ValueError("ICS 同期には ics_path が必要です")
        summary = calendar_sync.sync_from_ics(ics_path, mode=mode)
    elif source == "apple":
        summary = apple_calendar_sync.sync_from_apple_calendar(mode=mode)
    else:
        raise ValueError(f"不明な同期ソース: {source}")
    summary["index_rebuilt"] = get_engine().sync_diary_index(force=True)
    return summary


def calendar_event_dates() -> list[str]:
    return sorted(cal.dates_with_events())


def _run_profiler() -> dict:
    try:
        from core import profiler  # noqa: WPS433

        profiler.main()
        return {
            "ok": True,
            "message": "再分析完了 (deep_profile.json / user_profile.json 更新)",
        }
    except Exception as exc:  # noqa: BLE001
        import traceback

        traceback.print_exc(file=sys.stderr)
        return {"ok": False, "message": f"profiler失敗: {type(exc).__name__}: {exc}"}


def run_profiler() -> dict:
    return _run_profiler()


def _append_line_text(text: str, filename: str = "") -> None:
    head = text[:2000]
    stem = Path(filename).stem.lower() if filename else ""
    if "[LINE]" not in head and "line" not in stem and not filename.lower().endswith(".txt"):
        raise ValueError("LINE履歴 (.txt) のみ取り込めます")
    with open(LINE_HISTORY, "a", encoding="utf-8") as f:
        f.write("\n" + text.strip() + "\n")


def import_line_text(text: str, filename: str = "") -> dict:
    _append_line_text(text, filename)
    result = _run_profiler()
    msg = result["message"]
    if result["ok"]:
        label = filename or "line_history.txt"
        msg = f"{label} を取り込み — {msg}"
    return {"imported": True, "filename": filename or "line_history.txt", **result, "message": msg}


def import_line_batch(files: list[dict]) -> dict:
    names: list[str] = []
    for item in files:
        content = str(item.get("content", ""))
        name = str(item.get("filename", ""))
        _append_line_text(content, name)
        names.append(name or f"file_{len(names) + 1}.txt")
    result = _run_profiler()
    msg = result["message"]
    if result["ok"]:
        msg = f"{len(names)} 件の LINE 履歴を取り込み — {msg}"
    return {
        "imported": True,
        "count": len(names),
        "filenames": names,
        **result,
        "message": msg,
    }


def sync_calendar_ics_batch(files: list[dict], mode: str = "append") -> dict:
    if mode not in ("append", "overwrite"):
        raise ValueError(f"不明なマージモード: {mode}")
    names: list[str] = []
    last_summary: dict = {}
    for item in files:
        content = str(item.get("content", ""))
        name = str(item.get("filename", "calendar.ics"))
        last_summary = sync_calendar_ics_content(content, mode)
        names.append(name)
    message = f"{len(names)} 件の ICS を同期しました ({mode})"
    if last_summary.get("index_rebuilt"):
        message += " — DailyContext結晶化+ベクトル同期完了"
    return {**last_summary, "count": len(names), "filenames": names, "message": message}


def sync_calendar_ics_content(content: str, mode: str = "append") -> dict:
    import tempfile

    with tempfile.NamedTemporaryFile(
        mode="w", suffix=".ics", delete=False, encoding="utf-8",
    ) as tf:
        tf.write(content)
        tmp_path = tf.name
    try:
        summary = sync_calendar("ics", mode, ics_path=tmp_path)
    finally:
        Path(tmp_path).unlink(missing_ok=True)
    return summary


def get_settings() -> dict:
    from .settings_api import get_settings as _get_settings

    return _get_settings()


def import_line_history(path: str | Path) -> None:
    text = Path(path).read_text(encoding="utf-8", errors="replace")
    with open(LINE_HISTORY, "a", encoding="utf-8") as f:
        f.write("\n" + text.strip() + "\n")


def profiler_script_path() -> Path:
    return PROJECT_ROOT / "src" / "python" / "core" / "profiler.py"


__all__ = [
    "DIARY_MD",
    "FIXED_ATTRIBUTE_FIELDS",
    "LINE_HISTORY",
    "PROJECT_ROOT",
    "PROCESSED",
    "USER_PROFILE",
    "calendar_event_dates",
    "consult",
    "format_user_profile_summary",
    "get_engine",
    "get_settings",
    "import_line_batch",
    "import_line_text",
    "run_profiler",
    "sync_calendar_ics_batch",
    "sync_calendar_ics_content",
    "load_record",
    "load_user_profile",
    "profiler_script_path",
    "save_fixed_attributes",
    "save_record",
    "shutdown_engine",
    "sync_calendar",
    "sync_diary_index",
]
