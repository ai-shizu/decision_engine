# -*- coding: utf-8 -*-
"""Puppeteer (黒幕) の質問選択ロジック (Target Delta D3) の決定論的テスト。

外部依存なし・LLM 不使用・乱数不使用。select_question() は Bounty の
theme/insight を一切参照せず、id/type/tension/status/bank_question_id
のみから質問を選ぶことを検証する。
"""
from __future__ import annotations

import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "src" / "python"))

from core import question_bank as qb  # noqa: E402


def _bounty(bid: str, gtype: str, tension: float, status: str = "open",
           bank_question_id: str | None = None) -> dict:
    return {"id": bid, "theme": f"theme-of-{bid}", "type": gtype,
            "tension": tension, "status": status,
            "bank_question_id": bank_question_id}


def test_select_by_tension_descending() -> None:
    bounties = [
        _bounty("bt-low", "task_avoidance", 0.3),
        _bounty("bt-high", "task_avoidance", 0.9),
    ]
    selected = qb.select_question(bounties, k=1)
    assert len(selected) == 1
    assert selected[0]["bounty_id"] == "bt-high", "tension 最大の Bounty が優先されていない"
    assert selected[0]["text"] in [q["text"] for q in qb.QUESTION_BANK.values()]
    print("  select by tension descending OK")


def test_select_ignores_closed_bounties() -> None:
    bounties = [_bounty("bt-1", "task_avoidance", 0.9, status="resolved")]
    selected = qb.select_question(bounties, k=1)
    assert selected == [], "resolved (open でない) Bounty が選ばれている"
    print("  select ignores closed bounties OK")


def test_select_no_repeat_across_bounties() -> None:
    """同じバンク質問を2つの Bounty に重複して割り当てない。"""
    bounties = [
        _bounty("bt-a", "task_avoidance", 0.9),
        _bounty("bt-b", "task_avoidance", 0.8),
    ]
    selected = qb.select_question(bounties, k=2)
    assert len(selected) == 2
    assert selected[0]["question_id"] != selected[1]["question_id"]
    print("  select no repeat across bounties OK")


def test_select_respects_prior_bank_question_id() -> None:
    """既に bank_question_id が振られている (=既出) 質問は再利用しない。"""
    used_qid = next(qid for qid, q in qb.QUESTION_BANK.items()
                    if q["type"] == "task_avoidance")
    bounties = [
        _bounty("bt-old", "task_avoidance", 0.5, status="resolved",
               bank_question_id=used_qid),
        _bounty("bt-new", "task_avoidance", 0.9),
    ]
    selected = qb.select_question(bounties, k=1)
    assert len(selected) == 1
    assert selected[0]["question_id"] != used_qid, "既出のバンク質問を再選択している"
    print("  select respects prior bank_question_id OK")


def test_select_fallback_when_type_exhausted() -> None:
    """該当 type の質問が尽きた場合、面接を止めずに汎用質問へフォールバックする。"""
    same_type_qids = [qid for qid, q in qb.QUESTION_BANK.items()
                      if q["type"] == "friction_response"]
    bounties = [_bounty(f"bt-{i}", "friction_response", 0.9 - i * 0.01,
                        status="resolved", bank_question_id=qid)
               for i, qid in enumerate(same_type_qids)]
    bounties.append(_bounty("bt-new", "friction_response", 0.99))
    selected = qb.select_question(bounties, k=1)
    assert len(selected) == 1, "type 一致質問が尽きた際にフォールバックしていない"
    print("  select fallback when type exhausted OK")


def test_select_deterministic() -> None:
    """同一入力なら常に同一の選択結果 (乱数禁止)。"""
    bounties = [_bounty("bt-x", "true_gakuchika", 0.7),
               _bounty("bt-y", "blind_spot", 0.7)]
    r1 = qb.select_question(bounties, k=2)
    r2 = qb.select_question(bounties, k=2)
    assert r1 == r2
    print("  select deterministic OK")


if __name__ == "__main__":
    test_select_by_tension_descending()
    test_select_ignores_closed_bounties()
    test_select_no_repeat_across_bounties()
    test_select_respects_prior_bank_question_id()
    test_select_fallback_when_type_exhausted()
    test_select_deterministic()
    print("test_question_bank: ALL PASS")
