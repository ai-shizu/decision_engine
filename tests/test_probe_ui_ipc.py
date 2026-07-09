# -*- coding: utf-8 -*-
"""Phase F6 PROBE UI — stdio IPC contract tests."""
from __future__ import annotations

import json
import sys
from pathlib import Path
from unittest.mock import Mock

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "src" / "python"))

import engine_stdio  # noqa: E402


def test_probe_stdio_contract() -> None:
    from core.probe_funnel import PROBE_QUESTION_BANK

    today = "2026-07-09"
    status = engine_stdio.dispatch("probe.status", {"today": today})
    assert status["schema"] == "probe_status.v1"
    assert status["today"] == today
    assert isinstance(status.get("axes"), list)
    assert len(status["axes"]) == 5

    nxt = engine_stdio.dispatch("probe.next", {"today": today})
    assert nxt["schema"] == "probe_question.v1"
    bank_ids = {q.id for q in PROBE_QUESTION_BANK}
    assert nxt["question_id"] in bank_ids
    assert nxt["question"] in {q.text for q in PROBE_QUESTION_BANK}

    ans = engine_stdio.dispatch("probe.answer", {
        "session_id": nxt["session_id"],
        "question_id": nxt["question_id"],
        "answer": "sandbox probe fact answer",
        "today": today,
    })
    assert ans["schema"] == "probe_answer_result.v1"
    assert ans["saved"] is True
    assert ans["status"]["schema"] == "probe_status.v1"

    blob = json.dumps(
        {"status": status, "next": nxt, "answer": ans},
        ensure_ascii=False,
    )
    assert "山田太郎" not in blob


def test_probe_ipc_does_not_call_llm(monkeypatch) -> None:
    from core import facade

    backend = Mock()
    backend.generate = Mock(side_effect=AssertionError("F6 must not call LLM generation"))

    orig_load = facade._load_probe_sc
    backend_mock = backend

    def _load(*, backend=None):
        return orig_load(backend=backend_mock)

    monkeypatch.setattr(facade, "_load_probe_sc", _load)

    today = "2026-07-09"
    engine_stdio.dispatch("profile.source_code", {})
    engine_stdio.dispatch("probe.status", {"today": today})
    nxt = engine_stdio.dispatch("probe.next", {"today": today})
    res = engine_stdio.dispatch("probe.answer", {
        "session_id": nxt["session_id"],
        "question_id": nxt["question_id"],
        "answer": "llm-free answer path",
        "today": today,
    })

    backend.generate.assert_not_called()

    blob = json.dumps(res, ensure_ascii=False)
    for forbidden in ("sentiment", "valence", "emotion_score", "inferred_emotion"):
        assert forbidden not in blob
