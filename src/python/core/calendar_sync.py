#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""
ICS カレンダー同期 (完全オフライン)
====================================
Google カレンダー等から手動エクスポートした .ics を読み込み、
calendar.json 形式へマージする。ネットワーク通信は一切行わない。

  calendar.json: {"YYYY-MM-DD": [{"time": "HH:MM", "title": "..."}, ...]}
"""

from __future__ import annotations

import shutil
from datetime import datetime
from pathlib import Path
from zoneinfo import ZoneInfo

from .calendar_manager import load_calendar, save_calendar
from .paths import CALENDAR_IMPORT_ICS

MergeMode = str  # "append" | "overwrite"
CalendarEvents = dict[str, list[dict]]


def apply_calendar_import(
    imported: CalendarEvents,
    mode: MergeMode = "append",
    *,
    source: str = "import",
) -> dict[str, int | str]:
    """抽出済み予定を calendar.json に反映する共通エントリポイント。"""
    if mode not in ("append", "overwrite"):
        raise ValueError(f"不明なマージモード: {mode}")
    if not imported:
        raise ValueError(f"{source} から予定を抽出できませんでした")

    existing = load_calendar()
    merged = merge_calendar(existing, imported, mode=mode)
    save_calendar(merged)

    imported_count = sum(len(v) for v in imported.values())
    return {
        "source": source,
        "mode": mode,
        "imported_events": imported_count,
        "imported_dates": len(imported),
        "total_events": sum(len(v) for v in merged.values()),
    }


def _unfold_ics_lines(text: str) -> list[str]:
    """RFC 5545 line folding を展開する。"""
    out: list[str] = []
    for line in text.splitlines():
        if line.startswith((" ", "\t")) and out:
            out[-1] += line[1:]
        else:
            out.append(line.rstrip("\r"))
    return out


def _parse_property(line: str) -> tuple[str, str, str] | None:
    """'NAME;PARAM=val:VALUE' → (name, params_upper, value)。"""
    if ":" not in line:
        return None
    head, value = line.split(":", 1)
    parts = head.split(";")
    name = parts[0].strip().upper()
    params = ";".join(p.strip().upper() for p in parts[1:])
    return name, params, value.strip()


def _param_dict(params: str) -> dict[str, str]:
    out: dict[str, str] = {}
    for part in params.split(";"):
        if "=" in part:
            k, v = part.split("=", 1)
            out[k.strip()] = v.strip()
    return out


def _parse_ics_datetime(value: str, params: str) -> tuple[str, str]:
    """DTSTART 値を (YYYY-MM-DD, HH:MM) に変換。"""
    p = _param_dict(params)
    is_date_only = p.get("VALUE") == "DATE" or (
        "T" not in value and len(value) >= 8 and value[:8].isdigit()
    )

    if is_date_only:
        dt = datetime.strptime(value[:8], "%Y%m%d")
        return dt.strftime("%Y-%m-%d"), "00:00"

    raw = value.rstrip("Z")
    if "T" in raw:
        date_part, time_part = raw.split("T", 1)
    else:
        date_part, time_part = raw[:8], raw[8:]

    fmt = "%Y%m%dT%H%M%S" if len(time_part) >= 6 else "%Y%m%dT%H%M"
    dt_str = f"{date_part}T{time_part[:6] if len(time_part) >= 6 else time_part}"

    if value.endswith("Z"):
        dt = datetime.strptime(dt_str, fmt).replace(tzinfo=ZoneInfo("UTC"))
        local = dt.astimezone()
    elif tzid := p.get("TZID"):
        try:
            dt = datetime.strptime(dt_str, fmt).replace(tzinfo=ZoneInfo(tzid))
            local = dt
        except Exception:
            dt = datetime.strptime(dt_str, fmt)
            local = dt
    else:
        local = datetime.strptime(dt_str, fmt)

    return local.strftime("%Y-%m-%d"), local.strftime("%H:%M")


def parse_ics(path: Path | str) -> dict[str, list[dict]]:
    """ICS ファイルから予定を calendar.json 形式で抽出する。"""
    text = Path(path).read_text(encoding="utf-8", errors="replace")
    events_by_date: dict[str, list[dict]] = {}

    in_event = False
    dtstart: tuple[str, str] | None = None
    summary = ""

    for line in _unfold_ics_lines(text):
        upper = line.upper()
        if upper == "BEGIN:VEVENT":
            in_event = True
            dtstart = None
            summary = ""
            continue
        if upper == "END:VEVENT":
            if in_event and dtstart and summary.strip():
                date_str, time_str = dtstart
                events_by_date.setdefault(date_str, []).append(
                    {"time": time_str, "title": summary.strip()}
                )
            in_event = False
            continue
        if not in_event:
            continue

        prop = _parse_property(line)
        if prop is None:
            continue
        name, params, value = prop
        if name == "DTSTART":
            try:
                dtstart = _parse_ics_datetime(value, params)
            except (ValueError, TypeError):
                dtstart = None
        elif name == "SUMMARY":
            summary = value.replace("\\n", " ").replace("\\,", ",")

    for date_str in events_by_date:
        events_by_date[date_str].sort(key=lambda e: e["time"])
    return events_by_date


def merge_calendar(
    existing: dict[str, list[dict]],
    imported: dict[str, list[dict]],
    mode: MergeMode = "append",
) -> dict[str, list[dict]]:
    """既存 calendar.json と ICS 取込データをマージする。"""
    if mode not in ("append", "overwrite"):
        raise ValueError(f"不明なマージモード: {mode}")

    merged = {d: list(evs) for d, evs in existing.items()}

    for date_str, new_events in imported.items():
        cleaned = [
            {"time": e["time"], "title": e["title"]}
            for e in new_events
            if e.get("time") and e.get("title")
        ]
        if not cleaned:
            continue
        if mode == "overwrite":
            merged[date_str] = cleaned
        else:
            existing_day = merged.get(date_str, [])
            seen = {(e["time"], e["title"]) for e in existing_day}
            for ev in cleaned:
                key = (ev["time"], ev["title"])
                if key not in seen:
                    existing_day.append(ev)
                    seen.add(key)
            existing_day.sort(key=lambda e: e["time"])
            merged[date_str] = existing_day

    return {d: evs for d, evs in merged.items() if evs}


def sync_from_ics(
    ics_path: Path | str,
    mode: MergeMode = "append",
    *,
    archive: bool = True,
) -> dict[str, int | str]:
    """ICS を calendar.json に反映する。戻り値は件数サマリ。"""
    path = Path(ics_path)
    if not path.is_file():
        raise FileNotFoundError(f"ICS ファイルが見つかりません: {path}")

    imported = parse_ics(path)
    summary = apply_calendar_import(imported, mode=mode, source="ics")
    if archive:
        CALENDAR_IMPORT_ICS.parent.mkdir(parents=True, exist_ok=True)
        shutil.copy2(path, CALENDAR_IMPORT_ICS)
    summary["archived_to"] = str(CALENDAR_IMPORT_ICS) if archive else ""
    return summary
