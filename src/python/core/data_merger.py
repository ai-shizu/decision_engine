#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""
データ結合レイヤー: DailyContext + ConversationSession
=======================================================
日記とLINE履歴を日付 (YYYY-MM-DD) で結合し、DailyContext を生成する。
LINEは「状態保持型セッション (ConversationSession)」として非同期マルチターン
対話をモデリングする (単純な刺激→反応ペアは廃止)。

セッション抽出ルール (コンタクト別):
  a. 相手の発言でセッションを開始・追記
  b. ユーザーが最初の返信をするまで最大24時間超えてもクローズしない
  c. ユーザー返信後、発言が30分以上途切れた時点で1セッションとして確定

Response Latency = 相手最終発言時刻 → ユーザー最初の返信時刻
ヘッダ例: [Session: 21:14 - 21:21 同僚・田中 | Response Latency: 6m]
"""

from __future__ import annotations

import re
from datetime import datetime, timedelta

from .paths import DIARY_MD, LINE_HISTORY, PROJECT_ROOT as ROOT

IDLE_CLOSE = timedelta(minutes=30)

_DATE_PATTERNS = [
    re.compile(r"^(\d{4})-(\d{1,2})-(\d{1,2})"),
    re.compile(r"^(\d{4})/(\d{1,2})/(\d{1,2})"),
    re.compile(r"^(\d{4})\.(\d{1,2})\.(\d{1,2})"),
]


def normalize_date(s: str) -> str | None:
    if not s:
        return None
    s = s.strip()
    for pat in _DATE_PATTERNS:
        m = pat.match(s)
        if m:
            y, mo, d = int(m.group(1)), int(m.group(2)), int(m.group(3))
            return f"{y:04d}-{mo:02d}-{d:02d}"
    return None


def _parse_dt(date_str: str, time_str: str) -> datetime:
    h, m = map(int, time_str.strip().split(":")[:2])
    y, mo, d = map(int, date_str.split("-"))
    return datetime(y, mo, d, h, m)


def _fmt_latency(delta: timedelta) -> str:
    total_min = max(0, int(delta.total_seconds() // 60))
    h, mins = divmod(total_min, 60)
    if h and mins:
        return f"{h}h {mins}m"
    if h:
        return f"{h}h"
    return f"{mins}m"


def _fmt_session_range(start: datetime, end: datetime) -> str:
    if start.date() == end.date():
        return f"{start.strftime('%H:%M')} - {end.strftime('%H:%M')}"
    return (f"{start.strftime('%Y-%m-%d %H:%M')} - "
            f"{end.strftime('%Y-%m-%d %H:%M')}")


def extract_conversation_sessions(line_messages: list[dict]) -> list[dict]:
    """全LINEメッセージから ConversationSession を状態保持型で抽出する。"""
    by_contact: dict[str, list[dict]] = {}
    for m in line_messages:
        d = normalize_date(m.get("date") or "")
        if not d or not m.get("time"):
            continue
        enriched = {**m, "date": d, "dt": _parse_dt(d, m["time"])}
        by_contact.setdefault(m["contact"], []).append(enriched)

    sessions: list[dict] = []

    def _finalize(active: dict | None, contact: str) -> None:
        if active is None or active["awaiting_user"]:
            return  # 未返信セッションは確定しない
        sessions.append(_session_from_buffer(active, contact))

    for contact, msgs in by_contact.items():
        msgs.sort(key=lambda x: x["dt"])
        active: dict | None = None

        for msg in msgs:
            if active is None:
                if not msg["is_self"]:
                    active = {
                        "messages": [msg],
                        "awaiting_user": True,
                        "last_opponent_dt": msg["dt"],
                    }
                continue

            gap = msg["dt"] - active["messages"][-1]["dt"]

            if active["awaiting_user"]:
                if msg["is_self"]:
                    active["awaiting_user"] = False
                    active["first_user_reply_dt"] = msg["dt"]
                    active["latency"] = msg["dt"] - active["last_opponent_dt"]
                    active["messages"].append(msg)
                else:
                    active["messages"].append(msg)
                    active["last_opponent_dt"] = msg["dt"]
            elif gap >= IDLE_CLOSE:
                _finalize(active, contact)
                active = None
                if not msg["is_self"]:
                    active = {
                        "messages": [msg],
                        "awaiting_user": True,
                        "last_opponent_dt": msg["dt"],
                    }
            else:
                active["messages"].append(msg)

        _finalize(active, contact)

    sessions.sort(key=lambda s: s["start_time"])
    return sessions


def _session_from_buffer(active: dict, contact: str) -> dict:
    msgs: list[dict] = active["messages"]
    start_dt, end_dt = msgs[0]["dt"], msgs[-1]["dt"]
    latency: timedelta | None = active.get("latency")
    latency_str = _fmt_latency(latency) if latency else "N/A"
    header = (f"[Session: {_fmt_session_range(start_dt, end_dt)} {contact} | "
              f"Response Latency: {latency_str}]")

    turn_lines = [
        f"- {m['time']} {'自分' if m['is_self'] else m['sender']}: {m['text']}"
        for m in msgs
    ]
    dates = sorted({m["date"] for m in msgs})
    opponent = " / ".join(m["text"] for m in msgs if not m["is_self"])
    user = " / ".join(m["text"] for m in msgs if m["is_self"])

    return {
        "contact": contact,
        "start_date": dates[0],
        "end_date": dates[-1],
        "dates": dates,
        "header": header,
        "text": header + "\n" + "\n".join(turn_lines),
        "turns": turn_lines,
        "stimulus": opponent,
        "response": user,
        "response_latency": latency_str,
        "response_latency_seconds": int(latency.total_seconds()) if latency else None,
        "latency_minutes": int(latency.total_seconds() // 60) if latency else None,
        "start_time": start_dt.isoformat(),
        "end_time": end_dt.isoformat(),
        "turn_count": len(msgs),
    }


def _format_calendar_events(events: list[dict]) -> str:
    if not events:
        return "(この日の予定なし)"
    return "\n".join(f"- {e['time']} {e['title']}" for e in events)


def _render_text(date: str, calendar_events: list[dict], finance_text: str,
                 diary_text: str, consultation_text: str,
                 sessions_on_day: list[dict]) -> str:
    lines = [f"# DailyContext: {date}", "## Calendar"]
    lines.append(_format_calendar_events(calendar_events))
    lines.append("## Finance (家計簿)")
    lines.append(finance_text)
    lines.append("## Diary")
    lines.append(diary_text.strip() if diary_text.strip() else "(この日の日記なし)")
    lines.append("## AI_Consultations")
    lines.append(consultation_text)
    lines.append("## LINE_ConversationSessions")
    if sessions_on_day:
        for s in sessions_on_day:
            lines.append(s["header"])
            lines.extend(s["turns"])
            lines.append("")
    else:
        lines.append("(この日の確定セッションなし)")
    return "\n".join(lines).strip()


def load_daily_contexts() -> list[dict]:
    from . import profiler  # 遅延import
    from .calendar_manager import load_calendar
    from .consultation_log import (
        extract_user_queries, format_consultations_text, load_consultations,
    )
    from .finance_manager import format_finance_text, load_finance

    diary_entries = profiler.load_diary_entries()
    line_messages = profiler.load_line_messages()
    all_sessions = extract_conversation_sessions(line_messages)
    calendar = load_calendar()
    consultations_by_date = load_consultations()
    finance_by_date = load_finance()

    days: dict[str, dict] = {}

    def day(d: str) -> dict:
        return days.setdefault(d, {
            "date": d, "diary_text": "", "calendar_events": [],
            "consultations": [], "transactions": [], "sources": [],
        })

    for e in diary_entries:
        d = normalize_date(e["date"])
        if d is None:
            continue
        dc = day(d)
        dc["diary_text"] = (dc["diary_text"] + "\n" + e["text"].strip()).strip()
        if "diary" not in dc["sources"]:
            dc["sources"].append("diary")

    for d, events in calendar.items():
        nd = normalize_date(d)
        if nd is None or not events:
            continue
        dc = day(nd)
        dc["calendar_events"] = sorted(events, key=lambda e: e.get("time", ""))
        if "calendar" not in dc["sources"]:
            dc["sources"].append("calendar")

    for d, entries in consultations_by_date.items():
        nd = normalize_date(d)
        if nd is None or not entries:
            continue
        dc = day(nd)
        dc["consultations"] = entries
        if "consultation" not in dc["sources"]:
            dc["sources"].append("consultation")

    for d, txs in finance_by_date.items():
        nd = normalize_date(d)
        if nd is None or not txs:
            continue
        dc = day(nd)
        dc["transactions"] = txs
        if "finance" not in dc["sources"]:
            dc["sources"].append("finance")

    # セッションが触れる日を sources に line を付与
    for s in all_sessions:
        for d in s["dates"]:
            dc = day(d)
            if "line" not in dc["sources"]:
                dc["sources"].append("line")

    contexts = []
    for d in sorted(days):
        dc = days[d]
        day_sessions = [s for s in all_sessions if d in s["dates"]]
        line_text = "\n".join(s["text"] for s in day_sessions)
        line_self = "\n".join(s["response"] for s in day_sessions if s["response"])
        cal_events = dc["calendar_events"]
        cal_text = _format_calendar_events(cal_events)
        consult_entries = dc["consultations"]
        consult_text = format_consultations_text(consult_entries)
        consult_user = extract_user_queries(consult_entries)
        tx_entries = dc["transactions"]
        finance_text = format_finance_text(tx_entries)
        self_parts = [dc["diary_text"], line_self, consult_user]
        contexts.append({
            "title": d,
            "date": d,
            "text": _render_text(d, cal_events, finance_text, dc["diary_text"],
                                 consult_text, day_sessions),
            "calendar_text": cal_text,
            "calendar_events": cal_events,
            "finance_text": finance_text,
            "transactions": tx_entries,
            "consultation_text": consult_text,
            "consultations": consult_entries,
            "diary_text": dc["diary_text"],
            "line_text": line_text,
            "line_self_text": line_self,
            "self_text": "\n".join(p for p in self_parts if p.strip()).strip(),
            "conversation_sessions": day_sessions,
            "sources": dc["sources"],
            "has_diary": "diary" in dc["sources"],
            "has_line": "line" in dc["sources"],
            "has_calendar": "calendar" in dc["sources"],
            "has_consultation": "consultation" in dc["sources"],
            "has_finance": "finance" in dc["sources"],
        })
    return contexts


def collect_conversation_sessions(daily: list[dict] | None = None) -> list[dict]:
    """全DailyContextから ConversationSession を重複なく集約 (contact+start_time で一意)。"""
    if daily is None:
        daily = load_daily_contexts()
    seen: set[str] = set()
    out: list[dict] = []
    for dc in daily:
        for s in dc.get("conversation_sessions", []):
            key = f"{s['contact']}:{s['start_time']}"
            if key not in seen:
                seen.add(key)
                out.append(s)
    out.sort(key=lambda s: s["start_time"])
    return out


# 後方互換 (旧テスト・呼び出し)
def collect_interaction_pairs(daily: list[dict] | None = None) -> list[dict]:
    """非推奨: ConversationSession を旧ペア形式に射影。"""
    return [{
        "date": s["start_date"], "contact": s["contact"],
        "stimulus": s["stimulus"], "response": s["response"],
        "response_latency": s["response_latency"],
        "latency_minutes": s.get("latency_minutes"),
    } for s in collect_conversation_sessions(daily)]


if __name__ == "__main__":
    ctxs = load_daily_contexts()
    both = [c for c in ctxs if c["has_diary"] and c["has_line"]]
    sessions = collect_conversation_sessions(ctxs)
    print(f"DailyContext: {len(ctxs)} 日 (両方 {len(both)}日)")
    print(f"ConversationSessions: {len(sessions)} 件")
    for s in sessions[:4]:
        print(f"  {s['header']}")
