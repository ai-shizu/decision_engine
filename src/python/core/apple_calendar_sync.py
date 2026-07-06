#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""
Apple カレンダー同期 (macOS ローカル DB 解析)
==============================================
macOS Calendar.app が保持する SQLite DB を読み取り、
calendar.json 形式へマージする。外部 API / ネットワーク通信は行わない。

主な DB パス (macOS):
  ~/Library/Group Containers/group.com.apple.calendar/Calendar.sqlitedb
  ~/Library/Calendars/ 配下の *.sqlite / *.sqlitedb (レガシー)
"""

from __future__ import annotations

import sqlite3
import sys
from datetime import date, datetime, time, timedelta
from pathlib import Path

from .calendar_sync import (
    CalendarEvents,
    MergeMode,
    apply_calendar_import,
    merge_calendar,
)

CORE_DATA_EPOCH_OFFSET = 978307200  # 1970-01-01 → 2001-01-01 (秒)

_EVENT_SCHEMAS: dict[str, dict[str, tuple[str, ...]]] = {
    "CalendarItem": {
        "title": ("summary", "title"),
        "start": ("start_date", "startdate"),
        "all_day": ("all_day", "allday", "is_all_day"),
        "hidden": ("hidden",),
    },
    "Event": {
        "title": ("summary", "title", "SUMMARY"),
        "start": ("start_date", "startdate", "START_DATE"),
        "all_day": ("all_day", "allday", "is_all_day"),
        "hidden": ("hidden",),
    },
}


def is_macos() -> bool:
    return sys.platform == "darwin"


def default_calendar_db_paths() -> list[Path]:
    """macOS 上の代表的な Apple カレンダー DB 候補。"""
    home = Path.home()
    return [
        home / "Library/Group Containers/group.com.apple.calendar/Calendar.sqlitedb",
        home / "Library/Calendars/Calendar.sqlitedb",
    ]


def find_calendar_databases(extra_paths: list[Path] | None = None) -> list[Path]:
    """利用可能なカレンダー SQLite DB を列挙 (重複排除・更新日時順)。"""
    seen: set[Path] = set()
    found: list[Path] = []

    def add(path: Path) -> None:
        resolved = path.resolve()
        if resolved in seen or not resolved.is_file():
            return
        seen.add(resolved)
        found.append(resolved)

    for p in extra_paths or []:
        add(p)
    for p in default_calendar_db_paths():
        add(p)

    cal_root = Path.home() / "Library/Calendars"
    if cal_root.is_dir():
        for pattern in ("*.sqlite", "*.sqlitedb", "**/*.sqlite", "**/*.sqlitedb"):
            for p in cal_root.glob(pattern):
                add(p)

    found.sort(key=lambda p: p.stat().st_mtime, reverse=True)
    return found


def _table_columns(conn: sqlite3.Connection, table: str) -> dict[str, str]:
    cur = conn.execute(f'PRAGMA table_info("{table}")')
    return {str(row[1]).lower(): str(row[1]) for row in cur.fetchall()}


def _pick_column(columns: dict[str, str], candidates: tuple[str, ...]) -> str | None:
    for name in candidates:
        if name.lower() in columns:
            return columns[name.lower()]
    return None


def _detect_event_table(conn: sqlite3.Connection) -> tuple[str, dict[str, str]] | None:
    tables = {
        str(row[0])
        for row in conn.execute(
            "SELECT name FROM sqlite_master WHERE type='table'"
        )
    }
    for table in ("CalendarItem", "Event"):
        if table not in tables:
            continue
        mapping = _EVENT_SCHEMAS[table]
        cols = _table_columns(conn, table)
        if _pick_column(cols, mapping["title"]) and _pick_column(cols, mapping["start"]):
            return table, cols
    return None


def _datetime_to_core_epoch(dt: datetime) -> float:
    return dt.timestamp() - CORE_DATA_EPOCH_OFFSET


def _core_epoch_to_local(start_value: float) -> tuple[str, str]:
    dt = datetime.fromtimestamp(float(start_value) + CORE_DATA_EPOCH_OFFSET)
    return dt.strftime("%Y-%m-%d"), dt.strftime("%H:%M")


def _coerce_bool(value: object) -> bool:
    if value is None:
        return False
    if isinstance(value, bool):
        return value
    if isinstance(value, (int, float)):
        return value != 0
    return str(value).strip().lower() in {"1", "true", "yes"}


def parse_apple_calendar_db(
    db_path: Path | str,
    *,
    days_back: int = 365,
    days_ahead: int = 365,
    from_date: date | None = None,
) -> CalendarEvents:
    """単一 SQLite DB から予定を calendar.json 形式で抽出する。"""
    path = Path(db_path)
    if not path.is_file():
        raise FileNotFoundError(f"カレンダー DB が見つかりません: {path}")

    anchor = from_date or date.today()
    range_start = datetime.combine(anchor - timedelta(days=days_back), time.min)
    range_end = datetime.combine(anchor + timedelta(days=days_ahead), time.max)
    start_core = _datetime_to_core_epoch(range_start)
    end_core = _datetime_to_core_epoch(range_end)

    events_by_date: CalendarEvents = {}
    conn = sqlite3.connect(f"file:{path}?mode=ro", uri=True)
    conn.row_factory = sqlite3.Row
    try:
        detected = _detect_event_table(conn)
        if detected is None:
            raise ValueError(f"イベントテーブルを検出できません: {path}")

        table, columns = detected
        mapping = _EVENT_SCHEMAS[table]
        title_col = _pick_column(columns, mapping["title"])
        start_col = _pick_column(columns, mapping["start"])
        all_day_col = _pick_column(columns, mapping.get("all_day", ()))
        hidden_col = _pick_column(columns, mapping.get("hidden", ()))

        if not title_col or not start_col:
            raise ValueError(f"必要な列が見つかりません: {path}")

        where = [f'"{start_col}" IS NOT NULL', f'"{start_col}" >= ?', f'"{start_col}" <= ?']
        params: list[object] = [start_core, end_core]
        if hidden_col:
            where.append(f'("{hidden_col}" IS NULL OR "{hidden_col}" = 0)')

        sql = (
            f'SELECT "{title_col}" AS title, "{start_col}" AS start_value'
            + (f', "{all_day_col}" AS all_day' if all_day_col else "")
            + f' FROM "{table}" WHERE {" AND ".join(where)}'
            + f' ORDER BY "{start_col}" ASC'
        )
        rows = conn.execute(sql, params).fetchall()
    finally:
        conn.close()

    for row in rows:
        title = str(row["title"] or "").strip()
        if not title:
            continue
        try:
            start_value = float(row["start_value"])
        except (TypeError, ValueError):
            continue
        if all_day_col and _coerce_bool(row["all_day"] if "all_day" in row.keys() else None):
            date_str = _core_epoch_to_local(start_value)[0]
            time_str = "00:00"
        else:
            date_str, time_str = _core_epoch_to_local(start_value)
        events_by_date.setdefault(date_str, []).append(
            {"time": time_str, "title": title}
        )

    for date_str in events_by_date:
        events_by_date[date_str].sort(key=lambda e: e["time"])
    return events_by_date


def parse_apple_calendars(
    db_paths: list[Path] | None = None,
    *,
    days_back: int = 365,
    days_ahead: int = 365,
) -> CalendarEvents:
    """複数 DB 候補から予定を統合 (重複は time+title で排除)。"""
    paths = db_paths if db_paths else find_calendar_databases()
    if not paths:
        raise FileNotFoundError("Apple カレンダー DB が見つかりません")

    merged: CalendarEvents = {}
    errors: list[str] = []
    for path in paths:
        try:
            chunk = parse_apple_calendar_db(
                path, days_back=days_back, days_ahead=days_ahead,
            )
            merged = merge_calendar(merged, chunk, mode="append")
        except (OSError, sqlite3.Error, ValueError) as exc:
            errors.append(f"{path.name}: {exc}")

    if not merged:
        detail = "; ".join(errors) if errors else "予定 0 件"
        raise ValueError(f"Apple カレンダーから予定を抽出できませんでした ({detail})")
    return merged


def sync_from_apple_calendar(
    mode: MergeMode = "append",
    *,
    db_path: Path | str | None = None,
    days_back: int = 365,
    days_ahead: int = 365,
) -> dict[str, int | str]:
    """Apple カレンダー DB を calendar.json に反映する。"""
    if not is_macos():
        raise RuntimeError("Apple カレンダー同期は macOS でのみ利用できます")

    if db_path is not None:
        imported = parse_apple_calendar_db(
            db_path, days_back=days_back, days_ahead=days_ahead,
        )
        source = f"apple_calendar:{Path(db_path).name}"
        db_count = 1
    else:
        paths = find_calendar_databases()
        imported = parse_apple_calendars(
            paths, days_back=days_back, days_ahead=days_ahead,
        )
        source = "apple_calendar"
        db_count = len(paths)

    summary = apply_calendar_import(imported, mode=mode, source=source)
    summary["db_count"] = db_count
    return summary
