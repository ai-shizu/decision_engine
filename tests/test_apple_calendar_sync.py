# -*- coding: utf-8 -*-
"""apple_calendar_sync.py のユニットテスト (モック SQLite / ネットワーク不使用)。"""
import json
import sqlite3
import sys
import tempfile
from datetime import datetime
from pathlib import Path
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "src" / "python"))

from core import apple_calendar_sync as acs  # noqa: E402
from core import calendar_manager as cal  # noqa: E402

CORE_OFFSET = acs.CORE_DATA_EPOCH_OFFSET


def _core_epoch(dt: datetime) -> float:
    return dt.timestamp() - CORE_OFFSET


def _make_calendar_db(path: Path, table: str = "CalendarItem") -> None:
    conn = sqlite3.connect(path)
    try:
        if table == "CalendarItem":
            conn.executescript(
                """
                CREATE TABLE CalendarItem (
                    summary TEXT,
                    start_date REAL,
                    hidden INTEGER DEFAULT 0,
                    all_day INTEGER DEFAULT 0
                );
                """
            )
            conn.execute(
                "INSERT INTO CalendarItem VALUES (?, ?, 0, 0)",
                ("朝会", _core_epoch(datetime(2026, 7, 5, 9, 0, 0))),
            )
            conn.execute(
                "INSERT INTO CalendarItem VALUES (?, ?, 0, 0)",
                ("チームMTG", _core_epoch(datetime(2026, 7, 5, 14, 0, 0))),
            )
            conn.execute(
                "INSERT INTO CalendarItem VALUES (?, ?, 0, 1)",
                ("終日イベント", _core_epoch(datetime(2026, 7, 10, 0, 0, 0))),
            )
            conn.execute(
                "INSERT INTO CalendarItem VALUES (?, ?, 1, 0)",
                ("非表示", _core_epoch(datetime(2026, 7, 5, 15, 0, 0))),
            )
        else:
            conn.executescript(
                """
                CREATE TABLE Event (
                    summary TEXT,
                    start_date REAL
                );
                """
            )
            conn.execute(
                "INSERT INTO Event VALUES (?, ?)",
                ("レガシー予定", _core_epoch(datetime(2026, 7, 6, 10, 30, 0))),
            )
        conn.commit()
    finally:
        conn.close()


def test_parse_calendar_item_db():
    with tempfile.TemporaryDirectory() as tmp:
        db = Path(tmp) / "Calendar.sqlitedb"
        _make_calendar_db(db, "CalendarItem")
        parsed = acs.parse_apple_calendar_db(db, days_back=30, days_ahead=30)
        assert "2026-07-05" in parsed
        assert len(parsed["2026-07-05"]) == 2
        assert parsed["2026-07-05"][0]["title"] == "朝会"
        assert parsed["2026-07-10"][0]["time"] == "00:00"


def test_parse_legacy_event_table():
    with tempfile.TemporaryDirectory() as tmp:
        db = Path(tmp) / "legacy.sqlite"
        _make_calendar_db(db, "Event")
        parsed = acs.parse_apple_calendar_db(db, days_back=30, days_ahead=30)
        assert parsed["2026-07-06"][0]["title"] == "レガシー予定"


def test_find_calendar_databases_custom_path():
    with tempfile.TemporaryDirectory() as tmp:
        db = Path(tmp) / "Calendar.sqlitedb"
        _make_calendar_db(db)
        found = acs.find_calendar_databases([db])
        assert found == [db.resolve()]


@patch.object(acs, "is_macos", return_value=True)
def test_sync_from_apple_calendar_roundtrip(_mock_macos):
    backup = cal.CALENDAR_JSON.read_text(encoding="utf-8") if cal.CALENDAR_JSON.exists() else None
    with tempfile.TemporaryDirectory() as tmp:
        db = Path(tmp) / "Calendar.sqlitedb"
        _make_calendar_db(db)
        try:
            summary = acs.sync_from_apple_calendar(mode="overwrite", db_path=db)
            assert summary["imported_events"] == 3
            data = json.loads(cal.CALENDAR_JSON.read_text(encoding="utf-8"))
            assert "2026-07-05" in data
            assert summary["source"].startswith("apple_calendar:")
        finally:
            if backup is None:
                cal.CALENDAR_JSON.unlink(missing_ok=True)
            else:
                cal.CALENDAR_JSON.write_text(backup, encoding="utf-8")


def test_is_macos_matches_platform():
    assert acs.is_macos() == (sys.platform == "darwin")

