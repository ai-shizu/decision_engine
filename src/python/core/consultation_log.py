#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""
AI相談履歴の永続化
==================
ユーザー Query と AI Response のペアを日付キーで ai_consultations.json に追記保存する。
DailyContext 結晶化・ベクトル検索・profiler の入力ソースとなる。
"""

from __future__ import annotations

import json
from datetime import datetime
from .paths import AI_CONSULTATIONS_JSON, PROJECT_ROOT as ROOT


def _ensure_raw_dir() -> None:
    AI_CONSULTATIONS_JSON.parent.mkdir(parents=True, exist_ok=True)


def load_consultations() -> dict[str, list[dict]]:
    """ai_consultations.json を読み込む。存在しなければ空 dict。"""
    _ensure_raw_dir()
    if not AI_CONSULTATIONS_JSON.exists():
        return {}
    try:
        data = json.loads(AI_CONSULTATIONS_JSON.read_text(encoding="utf-8"))
    except (json.JSONDecodeError, OSError):
        return {}
    if not isinstance(data, dict):
        return {}
    out: dict[str, list[dict]] = {}
    for k, v in data.items():
        if isinstance(v, list):
            out[str(k)] = [
                {
                    "timestamp": str(e.get("timestamp", "")),
                    "query": str(e.get("query", "")),
                    "response": str(e.get("response", "")),
                }
                for e in v if isinstance(e, dict)
            ]
    return out


def save_consultations(data: dict[str, list[dict]]) -> None:
    """ai_consultations.json へ書き込む。"""
    _ensure_raw_dir()
    AI_CONSULTATIONS_JSON.write_text(
        json.dumps(data, ensure_ascii=False, indent=2), encoding="utf-8")


def append_consultation(query: str, response: str,
                        timestamp: datetime | None = None) -> dict:
    """相談ペアを当日分として追記保存し、保存したエントリを返す。"""
    ts = timestamp or datetime.now()
    date_str = ts.strftime("%Y-%m-%d")
    entry = {
        "timestamp": ts.strftime("%Y-%m-%d %H:%M:%S"),
        "query": query.strip(),
        "response": response.strip(),
    }
    data = load_consultations()
    data.setdefault(date_str, []).append(entry)
    save_consultations(data)
    return entry


def get_consultations_for_date(date_str: str) -> list[dict]:
    """指定日の相談ペアリスト (時系列順)。"""
    return list(load_consultations().get(date_str, []))


def format_consultations_text(consultations: list[dict]) -> str:
    """DailyContext ## AI_Consultations セクション用テキスト。"""
    if not consultations:
        return "(この日のAI相談なし)"
    lines: list[str] = []
    for c in consultations:
        ts = c.get("timestamp", "")
        hhmm = ts.split(" ")[1][:5] if " " in ts else "??:??"
        lines.append(f"- [{hhmm}] User: {c['query']}")
        lines.append(f"- [{hhmm}] AI: {c['response']}")
    return "\n".join(lines)


def extract_user_queries(consultations: list[dict]) -> str:
    """profiler 用: ユーザー発話 (Query) のみを連結。"""
    return "\n".join(c["query"] for c in consultations if c.get("query", "").strip())
