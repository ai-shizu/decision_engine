# -*- coding: utf-8 -*-
from __future__ import annotations

import dataclasses
import json
import sys
from pathlib import Path
from unittest.mock import Mock

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "src" / "python"))


def _day(
    date: str,
    diary: str = "",
    events: list[dict] | None = None,
    tx: list[dict] | None = None,
    line_self: str = "",
) -> dict:
    return {
        "date": date,
        "diary_text": diary,
        "consultations": [],
        "transactions": tx or [],
        "calendar_events": events or [],
        "line_self_text": line_self,
    }


def _daily_fixture() -> list[dict]:
    days = [
        _day(
            "2026-06-01",
            diary="山田太郎と気まずい会話をした。明日こそ面接対策をやる。準備不足を反省して改善する。",
            tx=[{"date": "2026-06-01", "category": "外食", "amount": 1200}],
        ),
        _day(
            "2026-06-02",
            diary="面接対策はまだ進まない。自分の詰めが甘かったので次はこうする。",
            tx=[{"date": "2026-06-02", "category": "書籍", "amount": 2400}],
        ),
        _day(
            "2026-06-03",
            diary="コーディングテスト対策を始めると宣言した。",
            events=[{"time": "19:00", "title": "面接対策"}],
        ),
        _day(
            "2026-06-04",
            diary="佐藤花子との衝突のあと、C++の実装が一気に進んだ。",
            line_self="面接対策を終えた",
        ),
        _day(
            "2026-06-05",
            diary="ESを書き上げた。反省を次の行動に変えた。",
            events=[{"time": "10:00", "title": "コーディングテスト対策"}],
            tx=[{"date": "2026-06-05", "category": "受講", "amount": 8000}],
        ),
        _day(
            "2026-06-06",
            diary="ES執筆をやらないといけない。準備不足を認めて改善する。",
            tx=[{"date": "2026-06-06", "category": "コンビニ", "amount": 650}],
        ),
        _day("2026-06-07", diary="ESを提出した。集中できた。"),
        _day("2026-06-08", diary="企業研究を進める。自分の力不足を反省する。"),
    ]
    days[2]["consultations"] = [
        {"query": "面接対策をやる前に相談したい", "is_simulated_persona": False},
        {"query": "面接では明日こそ面接対策をやると言う", "is_simulated_persona": True},
    ]
    return days


def _line_telemetry_fixture() -> dict:
    return {
        "dyads": [
            {
                "contact_alias": "C-a1b2c3d4",
                "contact_name": "山田太郎",
                "exchanges": 30,
                "friction_responses": {"avoid": 1, "appease": 1, "repair": 2, "escalate": 0},
                "user_reply_median_min": 25.0,
                "peer_reply_median_min": 80.0,
                "formality_index": 0.25,
            },
            {
                "contact_alias": "C-e5f6a7b8",
                "contact_name": "佐藤花子",
                "exchanges": 28,
                "friction_responses": {"avoid": 0, "appease": 0, "repair": 3, "escalate": 1},
                "user_reply_median_min": 45.0,
                "peer_reply_median_min": 30.0,
                "formality_index": 0.75,
            },
            {
                "contact_alias": "C-c9d0e1f2",
                "contact_name": "鈴木一郎",
                "exchanges": 24,
                "friction_responses": {"avoid": 1, "appease": 0, "repair": 1, "escalate": 0},
                "user_reply_median_min": 60.0,
                "peer_reply_median_min": 90.0,
                "formality_index": 0.55,
            },
        ]
    }


def _compute(backend: object | None = None):
    from core import source_code

    return source_code.compute_source_code(
        _daily_fixture(),
        probe_store={"historical_nodes": [], "bounties": []},
        prev=None,
        line_telemetry=_line_telemetry_fixture(),
        backend=backend,
    )


def _plain(obj):
    if dataclasses.is_dataclass(obj):
        return dataclasses.asdict(obj)
    if hasattr(obj, "to_dict"):
        return obj.to_dict()
    if isinstance(obj, dict):
        return obj
    if hasattr(obj, "__dict__"):
        return vars(obj)
    raise TypeError(f"unsupported object shape: {type(obj)!r}")


def _get(obj, key: str):
    if isinstance(obj, dict):
        return obj.get(key)
    return getattr(obj, key)


def _axis_items(sc) -> list[tuple[str, object]]:
    plain = _plain(sc)
    if "axes" in plain:
        return list(plain["axes"].items())

    names = (
        "decision_threshold",
        "reward_bias",
        "locus_of_control",
        "unlearning_rate",
        "friction_energy_ledger",
        "friction_response",
        "latency_asymmetry",
        "protocol_plasticity",
    )
    items = []
    for name in names:
        if isinstance(plain, dict) and name in plain:
            items.append((name, plain[name]))
    interpersonal = plain.get("interpersonal") if isinstance(plain, dict) else None
    if interpersonal:
        for name, axis in _plain(interpersonal).items():
            items.append((name, axis))
    return items


def _axis_snapshot(sc) -> dict[str, tuple[float | None, float]]:
    return {
        name: (_get(axis, "score"), _get(axis, "confidence"))
        for name, axis in _axis_items(sc)
    }


def _all_quotes(sc) -> list[str]:
    quotes: list[str] = []
    for _, axis in _axis_items(sc):
        for ev in _get(axis, "evidence") or []:
            quote = _get(ev, "quote")
            if quote is not None:
                quotes.append(str(quote))
    return quotes


def test_source_code_deterministic() -> None:
    first = _compute()
    second = _compute()

    assert _axis_snapshot(first) == _axis_snapshot(second)


def test_no_emotion_inference() -> None:
    backend = Mock()
    backend.generate = Mock(side_effect=AssertionError("D1 must not call LLM generation"))

    _compute(backend=backend)

    backend.generate.assert_not_called()


def test_every_axis_has_evidence() -> None:
    sc = _compute()

    for name, axis in _axis_items(sc):
        if _get(axis, "score") is not None:
            assert _get(axis, "evidence"), f"{name} has score but no evidence"


def test_no_third_party_realname() -> None:
    sc = _compute()
    payload = json.dumps(_plain(sc), ensure_ascii=False, sort_keys=True)

    assert "山田太郎" not in payload
    assert "佐藤花子" not in payload
    assert "鈴木一郎" not in payload
    assert "C-a1b2c3d4" in payload


def test_quote_length_cap() -> None:
    sc = _compute()

    for quote in _all_quotes(sc):
        assert len(quote) <= 120


def test_historical_node_immutable() -> None:
    from core import probe_engine

    original = probe_engine.HistoricalNode(
        id="hn-original",
        date_range="2024-04..2025-03",
        fact_text="大学2年の春に研究会へ参加した",
        source="probe",
    )

    linked_original, replacement = probe_engine.supersede_historical_node(
        original,
        fact_text="大学2年の春ではなく秋に研究会へ参加した",
        source="probe",
    )

    assert original.fact_text == "大学2年の春に研究会へ参加した"
    assert replacement.id != original.id
    assert replacement.fact_text == "大学2年の春ではなく秋に研究会へ参加した"
    assert linked_original.id == original.id
    assert linked_original.fact_text == original.fact_text
    assert linked_original.superseded_by == replacement.id
    assert replacement.superseded_by is None

    forbidden = (
        "edit_historical_node",
        "update_historical_node",
        "delete_historical_node",
        "remove_historical_node",
        "mutate_historical_node",
    )
    for name in forbidden:
        assert not hasattr(probe_engine, name), f"{name} must not exist"
