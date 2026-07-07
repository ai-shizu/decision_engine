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

# T-24 (IMP-2): is_self 判定の失敗 (T-21) はセッションが「返信待ち」のまま
# 無限に継続する退化を招き、日次添付 (T-22) と合成して致命的な台帳肥大を
# 引き起こす (docs/AI_SKILLS.md §14)。200MB トリップワイヤより上流で、
# より具体的な診断とともに騒がしく死ぬための閾値。
_MAX_SESSION_SPAN_DAYS = 30
_MAX_SESSION_TURNS = 5000

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


def _group_chat_contacts(line_messages: list[dict]) -> set[str]:
    """T-25 (IMP-2): sender が3人以上のコンタクット = グループチャット。

    ConversationSession の awaiting_user 状態機械は「本人の返信を待つ1対1
    dyad」を前提とする。本人がほぼ発言しないグループチャットにこれを適用
    すると「返信待ち」のまま無限に蓄積し続ける (T-24 トリップワイヤが実際に
    検知した実例)。1対1分析 (dyad telemetry/coupling/gap_analysis) の設計
    意図とも整合するため、ここで自動除外する (docs/AI_SKILLS.md §14)。
    """
    per_contact: dict[str, set[str]] = {}
    for m in line_messages:
        per_contact.setdefault(m["contact"], set()).add(m["sender"])
    return {c for c, senders in per_contact.items() if len(senders) >= 3}


def extract_group_daily_logs(line_messages: list[dict]) -> dict[str, list[str]]:
    """T-25 Rev.2 (IMP-2): グループチャット (sender>=3) を「受動観測ログ」
    として状態レスに日次平坦化する (docs/AI_SKILLS.md §14)。

    ConversationSession の awaiting_user 状態機械は一切通さない —
    (contact, date) で単純に群化し時刻順に整列するだけの決定論的処理。
    増幅率は恒等的に 1 (各メッセージは自分の date キーにちょうど1回だけ
    属する) であり、鯨セッション (T-21/T-24) が構造的に発生し得ない。

    戻り値: {date: [グループ活動ブロック文字列, ...]}。self_text/
    line_self_text/has_line (dyad 意味論) には合流させない — 呼び出し側
    (load_daily_contexts) の責務として厳格に隔離すること。
    """
    group_contacts = _group_chat_contacts(line_messages)
    by_group_date: dict[tuple[str, str], list[dict]] = {}
    for m in line_messages:
        if m["contact"] not in group_contacts:
            continue
        d = normalize_date(m.get("date") or "")
        if not d or not m.get("time"):
            continue
        by_group_date.setdefault((m["contact"], d), []).append(m)

    out: dict[str, list[str]] = {}
    for (contact, d), msgs in sorted(by_group_date.items()):
        msgs = sorted(msgs, key=lambda x: x["time"])
        block = [f"[Group: {contact} | {len(msgs)}件]"]
        block.extend(f"- {m['time']} {m['sender']}: {m['text']}" for m in msgs)
        out.setdefault(d, []).append("\n".join(block))
    return out


def extract_conversation_sessions(line_messages: list[dict]) -> list[dict]:
    """全LINEメッセージから ConversationSession を状態保持型で抽出する。

    T-25 (IMP-2): グループチャット (_group_chat_contacts) はここで除外する。
    """
    group_contacts = _group_chat_contacts(line_messages)
    by_contact: dict[str, list[dict]] = {}
    for m in line_messages:
        if m["contact"] in group_contacts:
            continue
        d = normalize_date(m.get("date") or "")
        if not d or not m.get("time"):
            continue
        enriched = {**m, "date": d, "dt": _parse_dt(d, m["time"])}
        by_contact.setdefault(m["contact"], []).append(enriched)

    sessions: list[dict] = []

    def _finalize(active: dict | None, contact: str) -> None:
        if active is None or active["awaiting_user"]:
            return  # 未返信セッションは確定しない
        session = _session_from_buffer(active, contact)
        span_days = len(session["dates"])
        if span_days > _MAX_SESSION_SPAN_DAYS or session["turn_count"] > _MAX_SESSION_TURNS:
            raise RuntimeError(
                f"[{contact}] の LINE セッションが異常肥大 (span={span_days}日 / "
                f"turns={session['turn_count']}) — is_self 判定の失敗による"
                "セッション無限継続 (T-21) を疑え。詳細は docs/AI_SKILLS.md "
                "§14 (IMP-2) を参照。"
            )
        sessions.append(session)

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

    # T-22 (IMP-2): DailyContext への添付はセッション全文ではなく当日分のみに
    # 絞る (docs/AI_SKILLS.md §14)。全ターンをここで日付別に分解しておき、
    # 情報ロスなく (全ターンが必ずどこかの日付キーに1回だけ入る) 参照できる
    # ようにする。セッション自体 (text/turns/stimulus/response) は dyad 分析
    # 用の完全版として従来通り保持する。
    turns_by_date: dict[str, list[str]] = {}
    responses_by_date: dict[str, list[str]] = {}
    for m, line in zip(msgs, turn_lines):
        turns_by_date.setdefault(m["date"], []).append(line)
        if m["is_self"]:
            responses_by_date.setdefault(m["date"], []).append(m["text"])

    return {
        "contact": contact,
        "start_date": dates[0],
        "end_date": dates[-1],
        "dates": dates,
        "header": header,
        "text": header + "\n" + "\n".join(turn_lines),
        "turns": turn_lines,
        "turns_by_date": turns_by_date,
        "responses_by_date": responses_by_date,
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
                 sessions_on_day: list[dict],
                 group_lines: list[str] | None = None) -> str:
    lines = [f"# DailyContext: {date}", "## Calendar"]
    lines.append(_format_calendar_events(calendar_events))
    lines.append("## Finance (家計簿)")
    lines.append(finance_text)
    lines.append("## Diary")
    lines.append(diary_text.strip() if diary_text.strip() else "(この日の日記なし)")
    lines.append("## AI_Consultations")
    lines.append(consultation_text)
    lines.append("## LINE_ConversationSessions")
    # T-22 (IMP-2): 複数日にまたがるセッションでも、この日には当日分のターン
    # のみを載せる (docs/AI_SKILLS.md §14)。全文を毎日複製すると span(日数)
    # に比例して台帳が増幅する — 全ターンは必ずどこか1日にのみ属する。
    rendered_any = False
    for s in sessions_on_day:
        day_turns = s.get("turns_by_date", {}).get(date, [])
        if not day_turns:
            continue
        rendered_any = True
        lines.append(s["header"])
        lines.extend(day_turns)
        lines.append("")
    if not rendered_any:
        lines.append("(この日の確定セッションなし)")
    lines.append("## LINE_GroupActivity (受動観測)")
    # T-25 Rev.2 (IMP-2): グループは対話ではなく「その日に観測された環境
    # ログ」として当日分のみ平坦記録する。状態機械を通さないため増幅なし。
    if group_lines:
        for block in group_lines:
            lines.append(block)
            lines.append("")
    else:
        lines.append("(この日のグループ観測なし)")
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
    group_daily_logs = extract_group_daily_logs(line_messages)
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

    # T-25 Rev.2 (IMP-2): グループ観測日を sources に line_group を付与
    # (dyad 意味論の "line" とは独立。has_line/line_self_text には触れない)。
    for d in group_daily_logs:
        dc = day(d)
        if "line_group" not in dc["sources"]:
            dc["sources"].append("line_group")

    contexts = []
    for d in sorted(days):
        dc = days[d]
        day_sessions = [s for s in all_sessions if d in s["dates"]]
        # T-22 (IMP-2): line_text/line_self も当日分のみ (全文複製の増幅源だった)
        line_text = "\n".join(
            s["header"] + "\n" + "\n".join(s["turns_by_date"][d])
            for s in day_sessions if s["turns_by_date"].get(d)
        )
        line_self = "\n".join(
            t for s in day_sessions for t in s.get("responses_by_date", {}).get(d, [])
        )
        # T-25 Rev.2 (IMP-2): グループ観測は独立フィールド。line_self_text/
        # self_text/has_line (dyad 意味論) には一切合流させない (隔離ガード)。
        group_lines_today = group_daily_logs.get(d, [])
        group_line_text = "\n\n".join(group_lines_today)
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
                                 consult_text, day_sessions, group_lines_today),
            "calendar_text": cal_text,
            "calendar_events": cal_events,
            "finance_text": finance_text,
            "transactions": tx_entries,
            "consultation_text": consult_text,
            "consultations": consult_entries,
            "diary_text": dc["diary_text"],
            "line_text": line_text,
            "line_self_text": line_self,
            "group_line_text": group_line_text,
            "self_text": "\n".join(p for p in self_parts if p.strip()).strip(),
            "conversation_sessions": day_sessions,
            "sources": dc["sources"],
            "has_diary": "diary" in dc["sources"],
            "has_line": "line" in dc["sources"],
            "has_line_group": "line_group" in dc["sources"],
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
