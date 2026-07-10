# -*- coding: utf-8 -*-
"""Phase 3-B — romance_analysis backend contract tests."""
from __future__ import annotations

import json
import sys
from pathlib import Path

import pytest

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "src" / "python"))

from core.romance_analysis import (  # noqa: E402
    ALLOWED_ACTIONS,
    ALLOWED_TENDENCIES,
    INSUFFICIENT_ACTION,
    INSUFFICIENT_TENDENCY,
    ROMANCE_ANALYSIS_SCHEMA,
    ROMANCE_SCHEMA_VERSION,
    analyze_input,
    build_llm_user_prompt,
    compute_affinity_score,
    compute_metrics,
    deterministic_action,
    deterministic_fallback,
    deterministic_tendency,
    insufficient_result,
    parse_canonical_lines,
    sanitize_input,
    validate_result,
)

_THINK_OPEN = "<" + "think" + ">"
_THINK_CLOSE = "</" + "think" + ">"


def _valid_transcript(n: int = 6) -> str:
    lines: list[str] = []
    for i in range(n):
        role = "self" if i % 2 == 0 else "contact_alias"
        lines.append(f"[{role}] message-{i}")
    return "\n".join(lines)


class StructuredCaptureBackend:
    name = "structured-capture"

    def __init__(self, response: str):
        self.response = response
        self.calls: list[tuple[str, str, dict]] = []

    def generate_structured(self, system: str, user: str, json_schema: dict, max_tokens=None):
        self.calls.append((system, user, json_schema))
        return self.response

    def generate(self, system: str, user: str, max_tokens=None, on_token=None, prefix_hash=None):
        raise AssertionError("generate() must not be used for romance structured path")


class RomanceEngineStub:
    def __init__(self, backend: StructuredCaptureBackend):
        self.backend = backend


def test_romance_schema_version_and_shape() -> None:
    assert ROMANCE_SCHEMA_VERSION == "romance_analysis.v1"
    assert ROMANCE_ANALYSIS_SCHEMA["additionalProperties"] is False
    assert "affinity_score" in ROMANCE_ANALYSIS_SCHEMA["properties"]


def test_affinity_score_strict_int_or_none() -> None:
    metrics = compute_metrics(parse_canonical_lines(sanitize_input(_valid_transcript(8))))
    tendency = deterministic_tendency(metrics)
    action = deterministic_action(metrics, 55)
    good = {
        "schema": ROMANCE_SCHEMA_VERSION,
        "affinity_score": 55,
        "interaction_tendency": tendency,
        "next_best_action": action,
    }
    assert validate_result(
        good,
        deterministic_score=55,
        expected_tendency=tendency,
        expected_action=action,
    )["affinity_score"] == 55
    for bad in (55.0, "55", True, 101, -1):
        payload = {**good, "affinity_score": bad}
        with pytest.raises(ValueError):
            validate_result(
                payload,
                deterministic_score=55,
                expected_tendency=tendency,
                expected_action=action,
            )


def test_affinity_score_deterministic_repeatable() -> None:
    text = _valid_transcript(8)
    m1 = compute_affinity_score(parse_canonical_lines(sanitize_input(text)))
    m2 = compute_affinity_score(parse_canonical_lines(sanitize_input(text)))
    assert m1 == m2
    assert isinstance(m1, int)
    assert 0 <= m1 <= 100


def test_insufficient_data_returns_null_score() -> None:
    text = "[self] only-one\n[contact_alias] two"
    result = analyze_input(RomanceEngineStub(StructuredCaptureBackend("{}")), text)
    assert result["affinity_score"] is None
    assert result["interaction_tendency"] == "判定に必要な観測量が不足しています"


def test_generate_structured_path_used_with_aggregates_only() -> None:
    text = _valid_transcript(8)
    speakers = parse_canonical_lines(sanitize_input(text))
    metrics = compute_metrics(speakers)
    score = compute_affinity_score(speakers)
    payload = json.dumps(
        {
            "schema": ROMANCE_SCHEMA_VERSION,
            "affinity_score": score,
            "interaction_tendency": deterministic_tendency(metrics),
            "next_best_action": deterministic_action(metrics, score),
        },
        ensure_ascii=False,
    )
    backend = StructuredCaptureBackend(payload)
    engine = RomanceEngineStub(backend)
    analyze_input(engine, text)
    assert len(backend.calls) == 1
    _sys, user, _schema = backend.calls[0]
    assert "message-0" not in user
    assert "田中" not in user
    assert "self_count" in user or "balance" in user


def test_llm_prompt_has_no_raw_body_or_real_names() -> None:
    raw = "[self] secret-body\n[contact_alias] reply\n" * 4
    speakers = parse_canonical_lines(sanitize_input(raw))
    score = compute_affinity_score(speakers)
    prompt = build_llm_user_prompt(compute_metrics(speakers), score)
    assert "secret-body" not in prompt
    assert "田中" not in prompt


def test_invalid_json_falls_back_deterministically() -> None:
    text = _valid_transcript(8)
    backend = StructuredCaptureBackend("not-json")
    result = analyze_input(RomanceEngineStub(backend), text)
    assert result["schema"] == ROMANCE_SCHEMA_VERSION
    assert isinstance(result["affinity_score"], int)


def test_redacted_thinking_stripped_before_validation() -> None:
    text = _valid_transcript(8)
    speakers = parse_canonical_lines(sanitize_input(text))
    metrics = compute_metrics(speakers)
    score = compute_affinity_score(speakers)
    wrapped = (
        _THINK_OPEN + "hidden" + _THINK_CLOSE
        + json.dumps(
            {
                "schema": ROMANCE_SCHEMA_VERSION,
                "affinity_score": score,
                "interaction_tendency": deterministic_tendency(metrics),
                "next_best_action": deterministic_action(metrics, score),
            },
            ensure_ascii=False,
        )
    )
    backend = StructuredCaptureBackend(wrapped)
    result = analyze_input(RomanceEngineStub(backend), text)
    assert result["affinity_score"] == score


def test_consult_romance_does_not_search_or_log(monkeypatch, tmp_path) -> None:
    from core.consultation_engine import ConsultationEngine, RuleBasedBackend

    eng = ConsultationEngine.__new__(ConsultationEngine)
    eng._backend = RuleBasedBackend()
    eng._last_interview_report = None
    eng._last_romance_analysis = None
    eng._search_daemon = None
    eng._slot_cache = None
    eng._interview_state = None
    eng._gd_state = None

    called = {"embed": 0, "log": 0}

    def _embed(_q):
        called["embed"] += 1
        return [0.0] * 384

    def _fake_analyze(_engine, _text):
        metrics = compute_metrics(parse_canonical_lines(sanitize_input(_valid_transcript(8))))
        score = 50
        return {
            "schema": ROMANCE_SCHEMA_VERSION,
            "affinity_score": score,
            "interaction_tendency": deterministic_tendency(metrics),
            "next_best_action": deterministic_action(metrics, score),
        }

    monkeypatch.setattr("core.consultation_engine.ConsultationEngine.embed", _embed)
    monkeypatch.setattr("core.romance_analysis.analyze_input", _fake_analyze)

    answer = eng.consult(_valid_transcript(8), mode="romance_analysis")
    assert answer == "交流パルス解析が完了しました。"
    assert called["embed"] == 0
    assert eng._last_romance_analysis is not None


def test_other_modes_clear_romance_result() -> None:
    from core.consultation_engine import ConsultationEngine, RuleBasedBackend

    eng = ConsultationEngine.__new__(ConsultationEngine)
    eng._backend = RuleBasedBackend()
    eng._last_interview_report = None
    eng._last_romance_analysis = {
        "schema": ROMANCE_SCHEMA_VERSION,
        "affinity_score": 1,
        "interaction_tendency": "x",
        "next_best_action": "y",
    }
    eng._search_daemon = None
    eng._slot_cache = None
    eng._interview_state = None
    eng._gd_state = None
    eng.embed = lambda _q: [0.0] * 384  # type: ignore[method-assign]
    eng.sync_diary_index = lambda force=False: False  # type: ignore[method-assign]
    eng.sync_knowledge_index = lambda: None  # type: ignore[method-assign]
    eng.search_daily = lambda _q, _k: []  # type: ignore[method-assign]
    eng.search_index = lambda *_a, **_k: []  # type: ignore[method-assign]
    eng.build_static_prefix = lambda: ""  # type: ignore[method-assign]
    eng.build_dynamic_suffix = lambda *_a, **_k: ""  # type: ignore[method-assign]
    eng._generate_redacted = lambda *_a, **_k: "ok"  # type: ignore[method-assign]

    eng.consult("hello", mode="consult")
    assert eng._last_romance_analysis is None


def test_stdio_includes_validated_romance_analysis(monkeypatch) -> None:
    import engine_stdio

    class _Facade:
        @staticmethod
        def consult(*_a, **_k):
            return "交流パルス解析が完了しました。"

        @staticmethod
        def last_interview_report():
            return None

        @staticmethod
        def last_romance_analysis():
            metrics = compute_metrics(parse_canonical_lines(sanitize_input(_valid_transcript(8))))
            score = 42
            return {
                "schema": ROMANCE_SCHEMA_VERSION,
                "affinity_score": score,
                "interaction_tendency": deterministic_tendency(metrics),
                "next_best_action": deterministic_action(metrics, score),
            }

    monkeypatch.setattr(engine_stdio, "_import_facade", lambda: _Facade)
    result = engine_stdio.dispatch(
        "consult",
        {"query": _valid_transcript(8), "mode": "romance_analysis"},
    )
    assert result["romance_analysis"]["affinity_score"] == 42
    assert "report" not in result


def test_empty_body_prefix_only_returns_null_score() -> None:
    text = "\n".join(
        ["[self]", "[contact_alias]", "[self]", "[contact_alias]", "[self]", "[contact_alias]"]
    )
    speakers = parse_canonical_lines(sanitize_input(text))
    assert speakers == []
    assert compute_affinity_score(speakers) is None
    result = analyze_input(RomanceEngineStub(StructuredCaptureBackend("{}")), text)
    assert result["affinity_score"] is None


def test_whitespace_only_body_not_counted() -> None:
    text = "\n".join(
        [
            "[self]    ",
            "[contact_alias]\t",
            "[self] \t ",
            "[contact_alias]  ",
            "[self]\t\t",
            "[contact_alias] \t",
        ]
    )
    assert parse_canonical_lines(sanitize_input(text)) == []
    result = analyze_input(RomanceEngineStub(StructuredCaptureBackend("{}")), text)
    assert result["affinity_score"] is None


def test_mixed_valid_and_empty_counts_valid_only() -> None:
    text = "\n".join(
        [
            "[self]",
            "[contact_alias] ok-1",
            "[self] ok-2",
            "[contact_alias]",
            "[self] ok-3",
            "[contact_alias] ok-4",
        ]
    )
    speakers = parse_canonical_lines(sanitize_input(text))
    assert speakers == ["contact_alias", "self", "self", "contact_alias"]
    assert compute_affinity_score(speakers) is None


def test_unobserved_tendency_phrases_removed_from_allowlist() -> None:
    forbidden = (
        "短い往復が続いている状態です",
        "返信の間隔にばらつきがあります",
    )
    for phrase in forbidden:
        assert phrase not in ALLOWED_TENDENCIES


def test_fallback_tendency_derived_from_metrics() -> None:
    text = _valid_transcript(8)
    speakers = parse_canonical_lines(sanitize_input(text))
    metrics = compute_metrics(speakers)
    score = compute_affinity_score(speakers)
    result = deterministic_fallback(metrics, score)
    assert result["interaction_tendency"] == deterministic_tendency(metrics)
    assert result["next_best_action"] == deterministic_action(metrics, score)


def test_validate_rejects_missing_required_key() -> None:
    metrics = compute_metrics(parse_canonical_lines(sanitize_input(_valid_transcript(8))))
    score = 50
    tendency = deterministic_tendency(metrics)
    action = deterministic_action(metrics, score)
    payload = {
        "schema": ROMANCE_SCHEMA_VERSION,
        "affinity_score": score,
        "interaction_tendency": tendency,
    }
    with pytest.raises(ValueError):
        validate_result(
            payload,
            deterministic_score=score,
            expected_tendency=tendency,
            expected_action=action,
        )


def test_validate_rejects_unknown_extra_key() -> None:
    metrics = compute_metrics(parse_canonical_lines(sanitize_input(_valid_transcript(8))))
    score = 50
    tendency = deterministic_tendency(metrics)
    action = deterministic_action(metrics, score)
    payload = {
        "schema": ROMANCE_SCHEMA_VERSION,
        "affinity_score": score,
        "interaction_tendency": tendency,
        "next_best_action": action,
        "extra": "x",
    }
    with pytest.raises(ValueError):
        validate_result(
            payload,
            deterministic_score=score,
            expected_tendency=tendency,
            expected_action=action,
        )


def test_validate_rejects_non_null_score_when_deterministic_none() -> None:
    payload = {
        "schema": ROMANCE_SCHEMA_VERSION,
        "affinity_score": 10,
        "interaction_tendency": "判定に必要な観測量が不足しています",
        "next_best_action": "会話履歴を追加して再分析する",
    }
    with pytest.raises(ValueError):
        validate_result(payload, deterministic_score=None)


def test_validate_rejects_missing_score_key_when_deterministic_none() -> None:
    payload = {
        "schema": ROMANCE_SCHEMA_VERSION,
        "interaction_tendency": "判定に必要な観測量が不足しています",
        "next_best_action": "会話履歴を追加して再分析する",
    }
    with pytest.raises(ValueError):
        validate_result(payload, deterministic_score=None)


def test_schema_enum_includes_insufficient_fixed_phrases() -> None:
    tendency_enum = ROMANCE_ANALYSIS_SCHEMA["properties"]["interaction_tendency"]["enum"]
    action_enum = ROMANCE_ANALYSIS_SCHEMA["properties"]["next_best_action"]["enum"]
    assert INSUFFICIENT_TENDENCY in tendency_enum
    assert INSUFFICIENT_ACTION in action_enum


def test_insufficient_result_round_trips_through_validator() -> None:
    result = insufficient_result()
    validated = validate_result(result, deterministic_score=None)
    assert validated == result


def test_null_score_rejects_normal_allowlist_tendency() -> None:
    metrics = compute_metrics(parse_canonical_lines(sanitize_input(_valid_transcript(8))))
    payload = {
        "schema": ROMANCE_SCHEMA_VERSION,
        "affinity_score": None,
        "interaction_tendency": deterministic_tendency(metrics),
        "next_best_action": INSUFFICIENT_ACTION,
    }
    with pytest.raises(ValueError):
        validate_result(payload, deterministic_score=None)


def test_null_score_rejects_normal_allowlist_action() -> None:
    metrics = compute_metrics(parse_canonical_lines(sanitize_input(_valid_transcript(8))))
    payload = {
        "schema": ROMANCE_SCHEMA_VERSION,
        "affinity_score": None,
        "interaction_tendency": INSUFFICIENT_TENDENCY,
        "next_best_action": deterministic_action(metrics, 50),
    }
    with pytest.raises(ValueError):
        validate_result(payload, deterministic_score=None)


def test_null_score_rejects_mixed_insufficient_tendency_normal_action() -> None:
    metrics = compute_metrics(parse_canonical_lines(sanitize_input(_valid_transcript(8))))
    payload = {
        "schema": ROMANCE_SCHEMA_VERSION,
        "affinity_score": None,
        "interaction_tendency": INSUFFICIENT_TENDENCY,
        "next_best_action": deterministic_action(metrics, 50),
    }
    with pytest.raises(ValueError):
        validate_result(payload, deterministic_score=None)


def test_null_score_rejects_mixed_normal_tendency_insufficient_action() -> None:
    metrics = compute_metrics(parse_canonical_lines(sanitize_input(_valid_transcript(8))))
    payload = {
        "schema": ROMANCE_SCHEMA_VERSION,
        "affinity_score": None,
        "interaction_tendency": deterministic_tendency(metrics),
        "next_best_action": INSUFFICIENT_ACTION,
    }
    with pytest.raises(ValueError):
        validate_result(payload, deterministic_score=None)


def test_non_null_score_rejects_insufficient_tendency() -> None:
    metrics = compute_metrics(parse_canonical_lines(sanitize_input(_valid_transcript(8))))
    score = 50
    action = deterministic_action(metrics, score)
    payload = {
        "schema": ROMANCE_SCHEMA_VERSION,
        "affinity_score": score,
        "interaction_tendency": INSUFFICIENT_TENDENCY,
        "next_best_action": action,
    }
    with pytest.raises(ValueError):
        validate_result(
            payload,
            deterministic_score=score,
            expected_tendency=deterministic_tendency(metrics),
            expected_action=action,
        )


def test_non_null_score_rejects_insufficient_action() -> None:
    metrics = compute_metrics(parse_canonical_lines(sanitize_input(_valid_transcript(8))))
    score = 50
    tendency = deterministic_tendency(metrics)
    payload = {
        "schema": ROMANCE_SCHEMA_VERSION,
        "affinity_score": score,
        "interaction_tendency": tendency,
        "next_best_action": INSUFFICIENT_ACTION,
    }
    with pytest.raises(ValueError):
        validate_result(
            payload,
            deterministic_score=score,
            expected_tendency=tendency,
            expected_action=deterministic_action(metrics, score),
        )


def test_non_null_score_rejects_mixed_insufficient_tendency_normal_action() -> None:
    metrics = compute_metrics(parse_canonical_lines(sanitize_input(_valid_transcript(8))))
    score = 50
    action = deterministic_action(metrics, score)
    payload = {
        "schema": ROMANCE_SCHEMA_VERSION,
        "affinity_score": score,
        "interaction_tendency": INSUFFICIENT_TENDENCY,
        "next_best_action": action,
    }
    with pytest.raises(ValueError):
        validate_result(
            payload,
            deterministic_score=score,
            expected_tendency=deterministic_tendency(metrics),
            expected_action=action,
        )


def test_non_null_score_rejects_mixed_normal_tendency_insufficient_action() -> None:
    metrics = compute_metrics(parse_canonical_lines(sanitize_input(_valid_transcript(8))))
    score = 50
    tendency = deterministic_tendency(metrics)
    payload = {
        "schema": ROMANCE_SCHEMA_VERSION,
        "affinity_score": score,
        "interaction_tendency": tendency,
        "next_best_action": INSUFFICIENT_ACTION,
    }
    with pytest.raises(ValueError):
        validate_result(
            payload,
            deterministic_score=score,
            expected_tendency=tendency,
            expected_action=deterministic_action(metrics, score),
        )
