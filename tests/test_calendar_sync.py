# -*- coding: utf-8 -*-
"""calendar_sync.py のユニットテスト (ネットワーク不使用)。"""
import json
import sys
import tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "src" / "python"))

from core import calendar_manager as cal  # noqa: E402
from core import calendar_sync  # noqa: E402

SAMPLE_ICS = """\
BEGIN:VCALENDAR
VERSION:2.0
PRODID:-//PKB Test//EN
BEGIN:VEVENT
DTSTART;TZID=Asia/Tokyo:20260705T140000
SUMMARY:チームミーティング
END:VEVENT
BEGIN:VEVENT
DTSTART;TZID=Asia/Tokyo:20260705T090000
SUMMARY:朝会
END:VEVENT
BEGIN:VEVENT
DTSTART;VALUE=DATE:20260710
SUMMARY:終日イベント
END:VEVENT
END:VCALENDAR
"""


def test_parse_ics():
    with tempfile.NamedTemporaryFile(
        mode="w", suffix=".ics", delete=False, encoding="utf-8",
    ) as f:
        f.write(SAMPLE_ICS)
        path = Path(f.name)
    try:
        parsed = calendar_sync.parse_ics(path)
        assert "2026-07-05" in parsed
        assert len(parsed["2026-07-05"]) == 2
        times = [e["time"] for e in parsed["2026-07-05"]]
        assert times == sorted(times)
        assert any(e["title"] == "チームミーティング" for e in parsed["2026-07-05"])
        assert "2026-07-10" in parsed
        assert parsed["2026-07-10"][0]["time"] == "00:00"
    finally:
        path.unlink(missing_ok=True)


def test_merge_append_and_overwrite():
    existing = {
        "2026-07-05": [{"time": "08:00", "title": "既存予定"}],
        "2026-07-06": [{"time": "10:00", "title": "別日"}],
    }
    imported = {
        "2026-07-05": [
            {"time": "09:00", "title": "朝会"},
            {"time": "08:00", "title": "既存予定"},
        ],
    }
    appended = calendar_sync.merge_calendar(existing, imported, "append")
    assert len(appended["2026-07-05"]) == 2
    assert appended["2026-07-06"] == existing["2026-07-06"]

    overwritten = calendar_sync.merge_calendar(existing, imported, "overwrite")
    assert len(overwritten["2026-07-05"]) == 2
    assert overwritten["2026-07-05"][0]["title"] == "朝会"
    assert "2026-07-06" in overwritten


def test_sync_from_ics_roundtrip():
    backup = cal.CALENDAR_JSON.read_text(encoding="utf-8") if cal.CALENDAR_JSON.exists() else None
    archive = calendar_sync.CALENDAR_IMPORT_ICS
    archive_backup = archive.read_bytes() if archive.exists() else None
    try:
        with tempfile.NamedTemporaryFile(
            mode="w", suffix=".ics", delete=False, encoding="utf-8",
        ) as f:
            f.write(SAMPLE_ICS)
            path = Path(f.name)
        summary = calendar_sync.sync_from_ics(path, mode="overwrite")
        assert summary["imported_events"] == 3
        data = json.loads(cal.CALENDAR_JSON.read_text(encoding="utf-8"))
        assert "2026-07-05" in data
        assert archive.exists()
    finally:
        if backup is None:
            cal.CALENDAR_JSON.unlink(missing_ok=True)
        else:
            cal.CALENDAR_JSON.write_text(backup, encoding="utf-8")
        if archive_backup is None:
            archive.unlink(missing_ok=True)
        else:
            archive.write_bytes(archive_backup)
        path.unlink(missing_ok=True)

