#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""
家計簿データ管理
================
収支トランザクション (finance.json) の読み書きと DailyContext 用テキスト化。

  finance.json: {"YYYY-MM-DD": [{"type": "expense|income", "category": "...", "amount": N}, ...]}
"""

from __future__ import annotations

import json
import re
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
FINANCE_JSON = ROOT / "data" / "raw" / "finance.json"

_VALID_TYPES = frozenset({"expense", "income"})
_TYPE_ALIASES = {
    "expense": "expense", "income": "income",
    "支出": "expense", "収入": "income",
}
_TYPE_LABEL = {"expense": "支出", "income": "収入"}


def _ensure_raw_dir() -> None:
    FINANCE_JSON.parent.mkdir(parents=True, exist_ok=True)


def load_finance() -> dict[str, list[dict]]:
    """finance.json を読み込む。存在しなければ空 dict。"""
    _ensure_raw_dir()
    if not FINANCE_JSON.exists():
        return {}
    try:
        data = json.loads(FINANCE_JSON.read_text(encoding="utf-8"))
    except (json.JSONDecodeError, OSError):
        return {}
    if not isinstance(data, dict):
        return {}
    out: dict[str, list[dict]] = {}
    for k, v in data.items():
        if isinstance(v, list):
            out[str(k)] = [
                {
                    "type": str(e.get("type", "")),
                    "category": str(e.get("category", "")),
                    "amount": int(e.get("amount", 0)),
                }
                for e in v if isinstance(e, dict)
            ]
    return out


def save_finance(data: dict[str, list[dict]]) -> None:
    """finance.json へ書き込む。"""
    _ensure_raw_dir()
    FINANCE_JSON.write_text(
        json.dumps(data, ensure_ascii=False, indent=2), encoding="utf-8")


def get_transactions_for_date(date_str: str) -> list[dict]:
    """指定日のトランザクションリスト。"""
    return list(load_finance().get(date_str, []))


def set_transactions_for_date(date_str: str, transactions: list[dict]) -> None:
    """指定日のトランザクションを置換保存。"""
    data = load_finance()
    cleaned = []
    for t in transactions:
        ok, tx = validate_transaction(t)
        if ok:
            cleaned.append(tx)
    if cleaned:
        data[date_str] = cleaned
    elif date_str in data:
        del data[date_str]
    save_finance(data)


def validate_transaction(tx: dict) -> tuple[bool, dict | str]:
    """トランザクション1件を検証。成功時 (True, dict)、失敗時 (False, 理由)。"""
    ttype = str(tx.get("type", "")).strip().lower()
    ttype = _TYPE_ALIASES.get(str(tx.get("type", "")).strip(), ttype)
    if ttype not in _VALID_TYPES:
        return False, "区分は expense または income を指定してください"
    category = str(tx.get("category", "")).strip()
    if not category:
        return False, "カテゴリを入力してください"
    raw_amount = str(tx.get("amount", "")).strip()
    if not re.fullmatch(r"\d+", raw_amount):
        return False, "金額は正の整数で入力してください"
    amount = int(raw_amount)
    if amount <= 0:
        return False, "金額は1以上の整数で入力してください"
    return True, {"type": ttype, "category": category, "amount": amount}


def summarize_day(transactions: list[dict]) -> dict[str, int]:
    """日次の収入・支出・差引を集計。"""
    income = sum(t["amount"] for t in transactions if t.get("type") == "income")
    expense = sum(t["amount"] for t in transactions if t.get("type") == "expense")
    return {"income": income, "expense": expense, "net": income - expense}


def format_finance_text(transactions: list[dict]) -> str:
    """DailyContext ## Finance (家計簿) セクション用テキスト。"""
    if not transactions:
        return "(この日の家計簿データなし)"
    summary = summarize_day(transactions)
    lines = [
        f"[日次サマリ] 収入: {summary['income']:,}円 / 支出: {summary['expense']:,}円",
    ]
    for t in transactions:
        label = _TYPE_LABEL.get(t["type"], t["type"])
        lines.append(f"- [{label}] {t['category']}: {t['amount']:,}円")
    return "\n".join(lines)


def dates_with_finance() -> set[str]:
    """トランザクションが1件以上ある日付 (YYYY-MM-DD) の集合。"""
    return {d for d, txs in load_finance().items() if txs}
