# -*- coding: utf-8 -*-
"""Phase D2 PROBE funnel — deterministic tests (SPEC_PHASE_D2_PROBE.md §4)."""
from __future__ import annotations

import json
import sys
from pathlib import Path
from unittest.mock import Mock

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "src" / "python"))

from core.probe_engine import supersede_historical_node  # noqa: E402
from core.source_code import Axis, EvidenceRef, HumanSourceCode  # noqa: E402


def _axis(
    score: float | None = 0.5,
    confidence: float = 0.5,
    quote: str = "",
) -> Axis:
    evidence = [EvidenceRef("diary", "2026-06-01", quote)] if quote else []
    return Axis(score=score, confidence=confidence, evidence=evidence)


def _interpersonal() -> dict[str, Axis]:
    z = Axis(score=0.5, confidence=0.5, evidence=[])
    return {
        "friction_response": z,
        "latency_asymmetry": z,
        "protocol_plasticity": z,
    }


def _make_sc(**overrides: Axis) -> HumanSourceCode:
    ip = _interpersonal()
    base = {
        "decision_threshold": _axis(),
        "reward_bias": _axis(),
        "locus_of_control": _axis(confidence=0.8),
        "unlearning_rate": _axis(confidence=0.8),
        "friction_energy_ledger": _axis(confidence=0.8),
    }
    base.update(overrides)
    return HumanSourceCode(
        decision_threshold=base["decision_threshold"],
        reward_bias=base["reward_bias"],
        locus_of_control=base["locus_of_control"],
        unlearning_rate=base["unlearning_rate"],
        friction_energy_ledger=base["friction_energy_ledger"],
        friction_response=ip["friction_response"],
        latency_asymmetry=ip["latency_asymmetry"],
        protocol_plasticity=ip["protocol_plasticity"],
    )


def _candidate_dicts(candidates) -> list[dict]:
    return [c.to_dict() for c in candidates]


def test_probe_candidate_deterministic() -> None:
    from core.probe_funnel import ProbeStore, derive_probe_candidates, probe_next_question

    sc = _make_sc()
    store = ProbeStore()
    today = "2026-07-10"

    c1 = derive_probe_candidates(sc, store)
    c2 = derive_probe_candidates(sc, store)
    assert _candidate_dicts(c1) == _candidate_dicts(c2)

    q1 = probe_next_question(sc, store, today)
    q2 = probe_next_question(sc, store, today)
    for key in ("session_id", "question_id", "axis", "stage", "priority"):
        assert q1[key] == q2[key]


def test_probe_priority_boundary_and_tiebreak() -> None:
    from core.probe_funnel import ProbeStore, derive_probe_candidates, select_next_candidate

    sc = _make_sc(
        decision_threshold=_axis(score=0.5, confidence=0.5),
        reward_bias=_axis(score=0.5, confidence=0.5),
        locus_of_control=_axis(score=None, confidence=0.8),
    )
    store = ProbeStore()
    candidates = derive_probe_candidates(sc, store)
    by_axis = {c.axis: c for c in candidates}

    assert by_axis["decision_threshold"].priority == by_axis["reward_bias"].priority
    assert select_next_candidate(sc, store).axis == "decision_threshold"
    assert by_axis["locus_of_control"].score is None
    assert by_axis["locus_of_control"].priority == round(
        0.60 * (1.0 - 0.8) + 0.25 * 1.0 + 0.15 * 0.0, 3
    )


def test_probe_stage_monotonic_and_session_cap() -> None:
    from core.probe_funnel import (
        PROBE_QUESTION_BANK,
        ProbeStore,
        probe_next_question,
        record_probe_answer,
    )

    sc = _make_sc(decision_threshold=_axis(confidence=0.2))
    store = ProbeStore()
    today = "2026-07-11"

    stages_seen: list[str] = []
    payload = probe_next_question(sc, store, today)
    session_id = payload["session_id"]

    for expected_stage in ("FACT", "CONTEXT", "EMOTION", "MEANING"):
        assert payload["stage"] == expected_stage
        stages_seen.append(expected_stage)
        if expected_stage == "FACT":
            wrong = next(
                q.id
                for q in PROBE_QUESTION_BANK
                if q.axis == "decision_threshold" and q.stage == "CONTEXT"
            )
            try:
                record_probe_answer(
                    store, session_id, wrong, "skip ahead", today,
                )
                raise AssertionError("CONTEXT answer at FACT must raise ValueError")
            except ValueError:
                pass
        store = record_probe_answer(
            store,
            session_id,
            payload["question_id"],
            f"answer for {expected_stage}",
            today,
        )
        session = next(s for s in store.sessions if s.id == session_id)
        if expected_stage != "MEANING":
            payload = probe_next_question(sc, store, today)
            assert payload["session_id"] == session_id
        else:
            assert session.status == "closed"

    assert stages_seen == ["FACT", "CONTEXT", "EMOTION", "MEANING"]

    try:
        record_probe_answer(
            store,
            session_id,
            payload["question_id"],
            "sixth answer attempt",
            today,
        )
        raise AssertionError("sixth answer must raise ValueError")
    except ValueError:
        pass

    closed = next(s for s in store.sessions if s.id == session_id)
    assert closed.status == "closed"
    assert len(closed.questions_asked) == 4


def test_probe_answer_appends_historical_node_only() -> None:
    from core.probe_funnel import ProbeStore, probe_next_question, record_probe_answer

    sc = _make_sc(decision_threshold=_axis(confidence=0.2))
    store = ProbeStore()
    today = "2026-07-12"
    payload = probe_next_question(sc, store, today)
    store = record_probe_answer(
        store,
        payload["session_id"],
        payload["question_id"],
        "original probe fact",
        today,
    )
    original = store.historical_nodes[0]
    original_text = original.fact_text

    linked, replacement = supersede_historical_node(
        original,
        fact_text="corrected probe fact",
    )
    store.historical_nodes[0] = linked
    store.historical_nodes.append(replacement)

    assert store.historical_nodes[0].fact_text == original_text
    assert replacement.id != original.id
    assert store.historical_nodes[0].superseded_by == replacement.id
    assert len(store.historical_nodes) == 2

    import core.probe_funnel as pf

    forbidden = (
        "edit_historical_node",
        "update_historical_node",
        "delete_historical_node",
        "remove_historical_node",
    )
    for name in forbidden:
        assert not hasattr(pf, name), f"forbidden API exists: {name}"


def test_probe_no_llm_or_emotion_inference() -> None:
    from core.probe_funnel import (
        ProbeStore,
        derive_probe_candidates,
        derive_probe_insights,
        probe_next_question,
        record_probe_answer,
    )

    backend = Mock()
    backend.generate = Mock(side_effect=AssertionError("D2 must not call LLM generation"))

    sc = _make_sc(decision_threshold=_axis(confidence=0.2))
    store = ProbeStore()
    today = "2026-07-13"

    derive_probe_candidates(sc, store, backend=backend)
    payload = probe_next_question(sc, store, today, backend=backend)
    session_id = payload["session_id"]

    for stage in ("FACT", "CONTEXT", "EMOTION", "MEANING"):
        payload = probe_next_question(sc, store, today, backend=backend)
        text = (
            "自分の言葉で書いた感覚の記述"
            if stage == "EMOTION"
            else f"answer for {stage}"
        )
        store = record_probe_answer(
            store,
            session_id,
            payload["question_id"],
            text,
            today,
            backend=backend,
        )

    derive_probe_insights(sc, store, backend=backend)
    backend.generate.assert_not_called()

    blob = json.dumps(store.to_dict(), ensure_ascii=False)
    for forbidden in ("emotion_score", "sentiment", "valence", "inferred_emotion"):
        assert forbidden not in blob

    emotion_answer = next(a for a in store.answers if a.stage == "EMOTION")
    assert emotion_answer.text_quote == "自分の言葉で書いた感覚の記述"


def test_probe_privacy_and_question_bank_whitelist() -> None:
    from core.probe_funnel import (
        PROBE_QUESTION_BANK,
        ProbeStore,
        derive_probe_insights,
        probe_next_question,
        record_probe_answer,
    )

    secret = "秘密の個人フレーズXYZ"
    sc = _make_sc(
        decision_threshold=_axis(
            confidence=0.2,
            quote=secret,
        ),
    )
    store = ProbeStore()
    today = "2026-07-14"
    aliases = {"山田太郎": "C-a1b2c3d4"}

    payload = probe_next_question(sc, store, today, third_party_aliases=aliases)
    assert payload["question"] in {q.text for q in PROBE_QUESTION_BANK}
    assert secret not in payload["question"]

    store = record_probe_answer(
        store,
        payload["session_id"],
        payload["question_id"],
        "山田太郎と話した後の記録",
        today,
        third_party_aliases=aliases,
    )
    insights = derive_probe_insights(sc, store)
    blob = json.dumps(
        {"store": store.to_dict(), "insights": [i.to_dict() for i in insights]},
        ensure_ascii=False,
    )
    assert "山田太郎" not in blob
    assert "C-a1b2c3d4" in blob
    for ans in store.answers:
        assert len(ans.text_quote) <= 120
