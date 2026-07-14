#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""
カレンダー・日記データ管理
==========================
予定 (calendar.json) と日記 (diary.md) を表面分離したまま、
日付 (YYYY-MM-DD) をキーに読み書きする。

  calendar.json: {"YYYY-MM-DD": [{"time": "HH:MM", "title": "..."}, ...]}
  diary.md:      ## YYYY-MM-DD 見出し + Markdown 本文 (1日1枠)
"""

from __future__ import annotations

import json
import re
from datetime import date, timedelta
from .durable_persistence import (
    PersistenceReadError,
    durable_atomic_write_text,
    read_json_file,
)
from .paths import CALENDAR_JSON, DIARY_MD, PROJECT_ROOT as ROOT

_DATE_HEADING = re.compile(r"^##\s+(\d{4}-\d{2}-\d{2})\s*$")
_TIME_PATTERN = re.compile(r"^\d{1,2}:\d{2}$")


def _ensure_raw_dir() -> None:
    CALENDAR_JSON.parent.mkdir(parents=True, exist_ok=True)


def load_calendar() -> dict[str, list[dict]]:
    """calendar.json を読み込む。存在しなければ空 dict。"""
    _ensure_raw_dir()
    try:
        data = read_json_file(CALENDAR_JSON)
    except FileNotFoundError:
        return {}
    if type(data) is not dict:
        raise PersistenceReadError("calendar root must be an object")
    out: dict[str, list[dict]] = {}
    for k, v in data.items():
        if type(v) is not list or any(type(e) is not dict for e in v):
            raise PersistenceReadError("calendar entries must be object arrays")
        out[str(k)] = [
            {"time": str(e.get("time", "")), "title": str(e.get("title", ""))}
            for e in v
        ]
    return out


def save_calendar(data: dict[str, list[dict]]) -> None:
    """calendar.json へ書き込む。"""
    _ensure_raw_dir()
    durable_atomic_write_text(
        CALENDAR_JSON,
        json.dumps(data, ensure_ascii=False, indent=2),
    )


def dates_with_events() -> set[str]:
    """予定が1件以上ある日付 (YYYY-MM-DD) の集合。"""
    return {d for d, evs in load_calendar().items() if evs}


def dates_with_diary() -> set[str]:
    """日記本文がある日付 (YYYY-MM-DD) の集合。"""
    if not DIARY_MD.exists():
        return set()
    _, sections = _parse_diary_sections(DIARY_MD.read_text(encoding="utf-8"))
    return {d for d, body in sections.items() if body.strip()}


def get_events_for_date(date_str: str) -> list[dict]:
    """指定日の予定リスト (時間順)。"""
    events = list(load_calendar().get(date_str, []))
    events.sort(key=lambda e: e.get("time", ""))
    return events


def set_events_for_date(date_str: str, events: list[dict]) -> None:
    """指定日の予定リストを置換保存。"""
    data = load_calendar()
    cleaned = [
        {"time": e["time"].strip(), "title": e["title"].strip()}
        for e in events
        if e.get("time", "").strip() and e.get("title", "").strip()
    ]
    if cleaned:
        data[date_str] = cleaned
    elif date_str in data:
        del data[date_str]
    save_calendar(data)


def validate_time(time_str: str) -> bool:
    """HH:MM 形式を検証。"""
    t = time_str.strip()
    if not _TIME_PATTERN.match(t):
        return False
    h, m = map(int, t.split(":"))
    return 0 <= h <= 23 and 0 <= m <= 59


def normalize_time(time_str: str) -> str:
    """H:MM → HH:MM に正規化。"""
    h, m = map(int, time_str.strip().split(":")[:2])
    return f"{h:02d}:{m:02d}"


def load_future_events(days_ahead: int = 30,
                       from_date: date | None = None) -> list[dict]:
    """from_date から days_ahead 日間の予定を日付順で返す。"""
    start = from_date or date.today()
    end = start + timedelta(days=days_ahead)
    cal = load_calendar()
    out: list[dict] = []
    d = start
    while d <= end:
        ds = d.isoformat()
        for ev in cal.get(ds, []):
            out.append({"date": ds, "time": ev["time"], "title": ev["title"]})
        d += timedelta(days=1)
    out.sort(key=lambda e: (e["date"], e["time"]))
    return out


def format_future_context(events: list[dict]) -> str:
    """LLM プロンプト用の Future Context テキスト。"""
    if not events:
        return "(向こう1ヶ月間の登録予定なし)"
    return "\n".join(
        f"- [{e['date']}] {e['time']} {e['title']}" for e in events)


# ---------------------------------------------------------------- 日記 (1日1枠)
def _parse_diary_sections(text: str) -> tuple[str, dict[str, str]]:
    """diary.md を (ヘッダ, {date: body}) に分解。"""
    header_lines: list[str] = []
    sections: dict[str, str] = {}
    current_date: str | None = None
    current_body: list[str] = []

    for line in text.splitlines():
        m = _DATE_HEADING.match(line)
        if m:
            if current_date is not None:
                sections[current_date] = "\n".join(current_body).strip()
            current_date = m.group(1)
            current_body = []
        elif current_date is None:
            header_lines.append(line)
        else:
            current_body.append(line)

    if current_date is not None:
        sections[current_date] = "\n".join(current_body).strip()
    return "\n".join(header_lines).strip(), sections


def load_diary_for_date(date_str: str) -> str:
    """指定日の日記本文。未登録なら空文字。"""
    if not DIARY_MD.exists():
        return ""
    _, sections = _parse_diary_sections(DIARY_MD.read_text(encoding="utf-8"))
    return sections.get(date_str, "")


def save_diary_for_date(date_str: str, body: str) -> None:
    """指定日の日記を upsert (1日1枠)。本文が空ならその日の見出しを削除。"""
    _ensure_raw_dir()
    header, sections = ("", {})
    if DIARY_MD.exists():
        header, sections = _parse_diary_sections(DIARY_MD.read_text(encoding="utf-8"))

    body = body.strip()
    if body:
        sections[date_str] = body
    elif date_str in sections:
        del sections[date_str]

    lines: list[str] = []
    if header:
        lines.append(header)
        lines.append("")
    for d in sorted(sections):
        lines.append(f"## {d}")
        lines.append(sections[d])
        lines.append("")
    durable_atomic_write_text(DIARY_MD, "\n".join(lines).rstrip() + "\n")
