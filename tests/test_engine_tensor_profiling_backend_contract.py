# -*- coding: utf-8 -*-
"""Project Calculus Phase 2 — backend contract tests."""
from __future__ import annotations

import ast
import inspect
import json
import sys
import textwrap
from pathlib import Path

import pytest

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "src" / "python"))

from core.consultation_engine import HiddenReasoningRedactor
from core.interview_report import AXIS_WHITELIST, generate_report
from core.session_memory import (
    DYNAMIC_CONTEXT_CHAR_BUDGET,
    _QUERY_TRUNCATION_MARKER,
    _TURN_TRUNCATION_MARKER,
    _truncate_query_to_budget,
    build_bounded_context,
    char_count,
    format_turns_for_evidence_prompt,
    transcript_turns_from_pairs,
)
from core.tensor_profile import (
    CALCULUS_AXIS_MAP,
    CANONICAL_DIMENSION_IDS,
    INDICATOR_IDS,
    TENSOR_PROFILE_SCHEMA,
    aggregate_profile,
    degenerate_profile,
    parse_and_validate_proposals,
    validate_quote,
)

_THINK_OPEN = "<" + "think" + ">"
_THINK_CLOSE = "</" + "think" + ">"


def _make_transcript(n_turns: int) -> list[tuple[str, str]]:
    out: list[tuple[str, str]] = []
    for i in range(n_turns):
        role = "候補者" if i % 2 else "面接官"
        out.append((role, f"turn-{i:03d}-statement about problem {i % 7} and data {i * 3}%"))
    return out


def _valid_metrics_json() -> str:
    return json.dumps(
        {
            "metrics": [
                {"axis": axis, "score": 70 + i, "evidence": f"引用-{axis}"}
                for i, axis in enumerate(AXIS_WHITELIST)
            ],
            "evidence": [],
        },
        ensure_ascii=False,
    )


class StructuredFakeBackend:
    name = "structured-fake"

    def __init__(self, responses: list[str]):
        self.responses = responses
        self.calls: list[tuple[str, str]] = []

    def generate_structured(self, system: str, user: str, json_schema: dict) -> str:
        self.calls.append((system, user))
        idx = min(len(self.calls) - 1, len(self.responses) - 1)
        return self.responses[idx]

    def generate(self, system: str, user: str, max_tokens=None, on_token=None):
        self.calls.append((system, user))
        idx = min(len(self.calls) - 1, len(self.responses) - 1)
        answer = self.responses[idx]
        if on_token:
            on_token(answer)
        return answer


class _EngineStub:
    def __init__(self, backend: StructuredFakeBackend):
        self.backend = backend


def test_canonical_six_dimensions_fixed_order() -> None:
    assert CANONICAL_DIMENSION_IDS == (
        "problem_structuring",
        "quantitative_rigor",
        "hypothesis_evidence",
        "synthesis_judgment",
        "communication",
        "collaboration_adaptability",
    )
    profile = degenerate_profile("sess-1", "hash-a", "model-x", "pv1")
    assert tuple(d.dimension_id for d in profile.dimensions) == CANONICAL_DIMENSION_IDS
    assert profile.schema == "tensor_profile.6d.v1"


def test_calculus_axis_mapping() -> None:
    expected = {
        "problem_structuring": "Structural_Decomposition",
        "quantitative_rigor": "Quantitative_Agility",
        "hypothesis_evidence": "Logical_Rigor",
        "synthesis_judgment": "Domain_Adaptability",
        "communication": "Communication_Bandwidth",
        "collaboration_adaptability": "Cognitive_Flexibility",
    }
    assert CALCULUS_AXIS_MAP == expected
    for dim_id in CANONICAL_DIMENSION_IDS:
        assert dim_id in CALCULUS_AXIS_MAP


def test_quote_exact_substring_and_length() -> None:
    turns = transcript_turns_from_pairs(
        [("候補者", "売上は前年比10%増です。仮定は市場成長3%です。")],
        "sess-q",
    )
    turn = turns[0]
    ok, _ = validate_quote("売上は前年比10%増です。", turn)
    assert ok is True
    ok_long, _ = validate_quote("x" * 121, turn)
    assert ok_long is False
    ok_fake, _ = validate_quote("存在しない引用", turn)
    assert ok_fake is False


def test_insufficient_evidence_score_none_not_zero() -> None:
    turns = transcript_turns_from_pairs(
        [
            ("候補者", "構造化して考えます。"),
            ("面接官", "もう少し深掘りしてください。"),
        ],
        "sess-insuff",
    )
    proposals = [
        {
            "dimension_id": "problem_structuring",
            "indicator_id": "ps_clarify_objective",
            "level": 3,
            "turn_id": turns[0]["turn_id"],
            "turn_index": 0,
            "speaker_alias": "candidate",
            "quote": "構造化して考えます。",
        }
    ]
    profile = aggregate_profile(
        "sess-insuff", "hash", "model", "pv1", turns, proposals
    )
    dim = profile.dimensions[0]
    assert dim.score is None
    assert dim.score != 0.0


def test_score_confidence_formulas() -> None:
    turns = transcript_turns_from_pairs(
        [
            ("候補者", "目的を明確にし、売上10%と仮定3%で計算します。"),
            ("面接官", "根拠は？"),
            ("候補者", "仮説を検証し、結論として投資を推奨します。"),
        ],
        "sess-formula",
    )
    proposals = [
        {
            "dimension_id": "problem_structuring",
            "indicator_id": "ps_clarify_objective",
            "level": 4,
            "turn_id": turns[0]["turn_id"],
            "turn_index": 0,
            "speaker_alias": "candidate",
            "quote": "目的を明確にし、",
        },
        {
            "dimension_id": "problem_structuring",
            "indicator_id": "ps_decompose",
            "level": 2,
            "turn_id": turns[2]["turn_id"],
            "turn_index": 2,
            "speaker_alias": "candidate",
            "quote": "仮説を検証し、",
        },
        {
            "dimension_id": "quantitative_rigor",
            "indicator_id": "qr_calculations",
            "level": 3,
            "turn_id": turns[0]["turn_id"],
            "turn_index": 0,
            "speaker_alias": "candidate",
            "quote": "売上10%と仮定3%で計算します。",
        },
        {
            "dimension_id": "quantitative_rigor",
            "indicator_id": "qr_units_assumptions",
            "level": 3,
            "turn_id": turns[2]["turn_id"],
            "turn_index": 2,
            "speaker_alias": "candidate",
            "quote": "仮説を検証し、",
        },
    ]
    profile = aggregate_profile(
        "sess-formula", "hash", "model", "pv1", turns, proposals
    )
    ps = profile.dimensions[0]
    assert ps.score == 0.75
    assert ps.confidence > 0.0
    qr = profile.dimensions[1]
    assert qr.score == 0.75


def test_hundred_turn_determinism_and_budget() -> None:
    transcript = _make_transcript(100)
    query = "最新の候補者発言に応答せよ"
    kwargs = dict(
        session_id="sess-100",
        transcript=transcript,
        current_query=query,
        model_hash="mh1",
        prompt_version="pv1",
    )
    ctx1, wm1 = build_bounded_context(**kwargs)
    ctx2, wm2 = build_bounded_context(**kwargs)
    assert ctx1 == ctx2
    assert wm1 == wm2
    assert char_count(ctx1) <= DYNAMIC_CONTEXT_CHAR_BUDGET
    assert wm1.context_chars == char_count(ctx1)
    assert ctx1.count(query) == 1
    assert not ctx1.rstrip().endswith("turn-")
    assert not ctx1.rstrip().endswith("[")


def test_streaming_hidden_reasoning_redaction() -> None:
    redactor = HiddenReasoningRedactor()
    chunks: list[str] = []
    for piece in ("<th", "ink>secret", _THINK_CLOSE + "answer"):
        chunks.append(redactor.feed(piece))
    assert chunks == ["", "", "answer"]
    final = redactor.finalize()
    assert final == "answer"
    assert "secret" not in final

    eval_input = "prefix" + _THINK_OPEN + "hidden" + _THINK_CLOSE + "visible"
    cleaned = HiddenReasoningRedactor.redact_full(eval_input)
    assert cleaned == "prefixvisible"
    assert "hidden" not in cleaned


def test_malformed_json_validation_rejection() -> None:
    turns = transcript_turns_from_pairs(
        [("候補者", "構造化して考えます。")],
        "sess-val",
    )
    with pytest.raises(ValueError):
        parse_and_validate_proposals({"evidence": "not-a-list"}, turns)
    with pytest.raises(ValueError):
        parse_and_validate_proposals(
            {
                "evidence": [
                    {
                        "dimension_id": "unknown_axis",
                        "indicator_id": "ps_clarify_objective",
                        "level": 3,
                        "turn_id": turns[0]["turn_id"],
                        "turn_index": 0,
                        "speaker_alias": "candidate",
                        "quote": "構造化して考えます。",
                    }
                ]
            },
            turns,
        )
    with pytest.raises(ValueError):
        parse_and_validate_proposals(
            {
                "evidence": [
                    {
                        "dimension_id": "problem_structuring",
                        "indicator_id": "ps_clarify_objective",
                        "level": 3,
                        "turn_id": turns[0]["turn_id"],
                        "turn_index": 0,
                        "speaker_alias": "candidate",
                        "quote": "構造化して考えます。",
                    },
                    {
                        "dimension_id": "problem_structuring",
                        "indicator_id": "ps_decompose",
                        "level": 2,
                        "turn_id": turns[0]["turn_id"],
                        "turn_index": 0,
                        "speaker_alias": "candidate",
                        "quote": "構造化して考えます。",
                    },
                ]
            },
            turns,
        )
    with pytest.raises(ValueError):
        parse_and_validate_proposals(
            {
                "evidence": [
                    {
                        "dimension_id": "problem_structuring",
                        "indicator_id": "ps_clarify_objective",
                        "level": 9,
                        "turn_id": turns[0]["turn_id"],
                        "turn_index": 0,
                        "speaker_alias": "candidate",
                        "quote": "構造化して考えます。",
                    }
                ]
            },
            turns,
        )
    with pytest.raises(ValueError):
        parse_and_validate_proposals(
            {
                "evidence": [
                    {
                        "dimension_id": "problem_structuring",
                        "indicator_id": "ps_clarify_objective",
                        "level": 3,
                        "turn_id": turns[0]["turn_id"],
                        "turn_index": 0,
                        "speaker_alias": "candidate",
                        "quote": "存在しない引用テキスト",
                    }
                ]
            },
            turns,
        )
    assert isinstance(TENSOR_PROFILE_SCHEMA, dict)
    assert TENSOR_PROFILE_SCHEMA.get("type") == "object"


def test_prompt_includes_real_turn_metadata() -> None:
    pairs = [
        ("面接官", "最初の質問です。"),
        ("候補者", "目的を明確にし構造化します。"),
        ("候補者", "売上10%を前提に計算します。"),
    ]
    turns = transcript_turns_from_pairs(pairs, "sess-meta")
    block = format_turns_for_evidence_prompt(turns)
    for turn in turns:
        if turn["speaker_alias"] == "candidate":
            assert turn["turn_id"] in block
            assert f"turn_index: {turn['turn_index']}" in block
            assert f"speaker_alias: {turn['speaker_alias']}" in block
            assert turn["text"] in block


def test_e2e_llm_evidence_is_not_authoritative_tensor_input() -> None:
    pairs = [
        ("面接官", "質問"),
        ("候補者", "目的を明確にし構造化します。"),
        ("面接官", "深掘り"),
        ("候補者", "売上10%を前提に計算します。"),
    ]
    turns = transcript_turns_from_pairs(pairs, "sess-e2e")
    valid_evidence = [
        {
            "dimension_id": "problem_structuring",
            "indicator_id": "ps_clarify_objective",
            "level": 4,
            "turn_id": turns[1]["turn_id"],
            "turn_index": 1,
            "speaker_alias": "candidate",
            "quote": "目的を明確にし",
        },
        {
            "dimension_id": "problem_structuring",
            "indicator_id": "ps_decompose",
            "level": 3,
            "turn_id": turns[3]["turn_id"],
            "turn_index": 3,
            "speaker_alias": "candidate",
            "quote": "売上10%",
        },
        {
            "dimension_id": "quantitative_rigor",
            "indicator_id": "qr_calculations",
            "level": 3,
            "turn_id": turns[3]["turn_id"],
            "turn_index": 3,
            "speaker_alias": "candidate",
            "quote": "計算します。",
        },
        {
            "dimension_id": "quantitative_rigor",
            "indicator_id": "qr_units_assumptions",
            "level": 2,
            "turn_id": turns[1]["turn_id"],
            "turn_index": 1,
            "speaker_alias": "candidate",
            "quote": "構造化します。",
        },
    ]
    good = json.dumps(
        {
            "metrics": [
                {"axis": axis, "score": 80, "evidence": "e"}
                for axis in AXIS_WHITELIST
            ],
            "evidence": valid_evidence,
        },
        ensure_ascii=False,
    )
    backend = StructuredFakeBackend([good])
    engine = _EngineStub(backend)
    report = generate_report(
        engine,
        "system",
        "transcript",
        "summary",
        {},
        [],
        transcript_pairs=pairs,
        session_id="sess-e2e",
    )
    assert turns[1]["turn_id"] not in backend.calls[0][1]
    assert "dimension_id" not in backend.calls[0][1]
    for dim in report["tensor_profile"]["dimensions"]:
        assert dim["score"] is None
        assert dim["confidence"] == 0.0
        assert dim["evidence"] == []


def test_llm_tensor_evidence_does_not_trigger_retry_or_state_update() -> None:
    pairs = [
        ("候補者", "目的を明確にし構造化します。"),
        ("面接官", "続けて"),
        ("候補者", "売上10%を前提に計算します。"),
    ]
    turns = transcript_turns_from_pairs(pairs, "sess-retry")
    bad_evidence = [
        {
            "dimension_id": "problem_structuring",
            "indicator_id": "ps_clarify_objective",
            "level": 3,
            "turn_id": "fake-turn-id",
            "turn_index": 0,
            "speaker_alias": "candidate",
            "quote": "目的を明確にし",
        }
    ]
    good_evidence = [
        {
            "dimension_id": "problem_structuring",
            "indicator_id": "ps_clarify_objective",
            "level": 4,
            "turn_id": turns[0]["turn_id"],
            "turn_index": 0,
            "speaker_alias": "candidate",
            "quote": "目的を明確にし",
        },
        {
            "dimension_id": "problem_structuring",
            "indicator_id": "ps_decompose",
            "level": 3,
            "turn_id": turns[2]["turn_id"],
            "turn_index": 2,
            "speaker_alias": "candidate",
            "quote": "売上10%",
        },
        {
            "dimension_id": "quantitative_rigor",
            "indicator_id": "qr_calculations",
            "level": 3,
            "turn_id": turns[2]["turn_id"],
            "turn_index": 2,
            "speaker_alias": "candidate",
            "quote": "計算します。",
        },
        {
            "dimension_id": "quantitative_rigor",
            "indicator_id": "qr_units_assumptions",
            "level": 2,
            "turn_id": turns[0]["turn_id"],
            "turn_index": 0,
            "speaker_alias": "candidate",
            "quote": "構造化します。",
        },
    ]
    metrics = [
        {"axis": axis, "score": 75, "evidence": "x"} for axis in AXIS_WHITELIST
    ]
    bad = json.dumps({"metrics": metrics, "evidence": bad_evidence}, ensure_ascii=False)
    good = json.dumps({"metrics": metrics, "evidence": good_evidence}, ensure_ascii=False)
    backend = StructuredFakeBackend([bad, good])
    engine = _EngineStub(backend)
    report = generate_report(
        engine,
        "system",
        "transcript",
        "summary",
        {},
        [],
        transcript_pairs=pairs,
        session_id="sess-retry",
    )
    assert len(backend.calls) == 1
    for dim in report["tensor_profile"]["dimensions"]:
        assert dim["score"] is None
        assert dim["confidence"] == 0.0
        assert dim["evidence"] == []


def test_redactor_incremental_no_full_raw_buffer() -> None:
    redactor = HiddenReasoningRedactor()
    assert not hasattr(redactor, "_raw")
    visible = "VISIBLE-" + ("x" * 5000)
    hidden = _THINK_OPEN + "secret" + _THINK_CLOSE
    for ch in visible + hidden + visible:
        redactor.feed(ch)
    assert len(redactor.pending_buffer) <= redactor.max_hold_len
    out = redactor.finalize()
    assert "secret" not in out
    assert out.count("x") == 10000

    redactor2 = HiddenReasoningRedactor()
    tag = _THINK_OPEN
    for ch in tag:
        redactor2.feed(ch)
    assert redactor2.finalize() == ""
    redactor3 = HiddenReasoningRedactor()
    for ch in _THINK_OPEN + "hide" + _THINK_CLOSE + "ok":
        redactor3.feed(ch)
    assert redactor3.finalize() == "ok"


def test_current_utterance_appears_once_in_context() -> None:
    utterance = "同一本文の候補者発言です。"
    transcript = [("面接官", "質問"), ("候補者", utterance)]
    ctx, _ = build_bounded_context(
        session_id="sess-dup",
        transcript=transcript,
        current_query=utterance,
    )
    assert ctx.count(utterance) == 1


def test_budget_drops_whole_units_not_mid_slice() -> None:
    long_tail = "A" * 3500
    transcript = [("面接官", long_tail), ("候補者", "短い発言")]
    ctx, wm = build_bounded_context(
        session_id="sess-budget",
        transcript=transcript,
        current_query="query-" + ("Q" * 3000),
    )
    assert char_count(ctx) <= DYNAMIC_CONTEXT_CHAR_BUDGET
    assert wm.context_chars == char_count(ctx)
    assert not any(line.endswith("AA") and len(line) < 10 for line in ctx.splitlines())


def test_strict_type_rejection() -> None:
    turns = transcript_turns_from_pairs([("候補者", "構造化して考えます。")], "sess-types")
    base = {
        "dimension_id": "problem_structuring",
        "indicator_id": "ps_clarify_objective",
        "turn_id": turns[0]["turn_id"],
        "turn_index": 0,
        "speaker_alias": "candidate",
        "quote": "構造化して考えます。",
    }
    for bad_level in (3.0, "3", True):
        with pytest.raises(ValueError):
            parse_and_validate_proposals(
                {"evidence": [{**base, "level": bad_level}]}, turns
            )
    for bad_index in ("0", 0.0, True):
        with pytest.raises(ValueError):
            parse_and_validate_proposals(
                {"evidence": [{**base, "level": 3, "turn_index": bad_index}]}, turns
            )


def test_clean_quote_strip_and_store() -> None:
    turns = transcript_turns_from_pairs(
        [("候補者", "  構造化して考えます。  ")],
        "sess-quote",
    )
    accepted = parse_and_validate_proposals(
        {
            "evidence": [
                {
                    "dimension_id": "problem_structuring",
                    "indicator_id": "ps_clarify_objective",
                    "level": 3,
                    "turn_id": turns[0]["turn_id"],
                    "turn_index": 0,
                    "speaker_alias": "candidate",
                    "quote": "  構造化して考えます。  ",
                }
            ]
        },
        turns,
    )
    assert accepted[0]["quote"] == "構造化して考えます。"
    profile = aggregate_profile("sess-quote", "h", "m", "pv1", turns, accepted)
    assert profile.dimensions[0].evidence[0].quote == "構造化して考えます。"


def test_redactor_nested_hidden_block_full() -> None:
    nested = (
        _THINK_OPEN + "a" + _THINK_OPEN + "b" + _THINK_CLOSE
        + "LEAK" + _THINK_CLOSE + "OK"
    )
    assert HiddenReasoningRedactor.redact_full(nested) == "OK"


def test_redactor_nested_hidden_block_char_chunks() -> None:
    nested = (
        _THINK_OPEN + "a" + _THINK_OPEN + "b" + _THINK_CLOSE
        + "LEAK" + _THINK_CLOSE + "OK"
    )
    redactor = HiddenReasoningRedactor()
    for ch in nested:
        redactor.feed(ch)
    out = redactor.finalize()
    assert out == "OK"
    assert "LEAK" not in out
    assert "a" not in out
    assert "b" not in out


def test_redactor_nested_case_insensitive_visible() -> None:
    raw = "A" + _THINK_OPEN + "x" + _THINK_OPEN + "y" + _THINK_CLOSE + "z" + _THINK_CLOSE + "B"
    assert HiddenReasoningRedactor.redact_full(raw) == "AB"
    redactor = HiddenReasoningRedactor()
    for ch in raw:
        redactor.feed(ch)
    assert redactor.finalize() == "AB"


def test_redactor_no_full_rejoin_helpers() -> None:
    redactor = HiddenReasoningRedactor()
    assert not hasattr(redactor, "_raw")
    assert not hasattr(redactor, "_visible_len")
    assert not hasattr(redactor, "_visible_slice")


def test_redactor_feed_returns_only_current_delta() -> None:
    redactor = HiddenReasoningRedactor()
    d1 = redactor.feed("hello")
    assert d1 == "hello"
    d2 = redactor.feed(" world")
    assert d2 == " world"
    assert d2 != "hello world"
    assert redactor.finalize() == "hello world"


def test_redactor_ten_thousand_char_chunks() -> None:
    visible = "V" + ("x" * 9998) + "W"
    hidden = _THINK_OPEN + "secret" + _THINK_CLOSE
    redactor = HiddenReasoningRedactor()
    for ch in visible[:5000] + hidden + visible[5000:]:
        redactor.feed(ch)
    out = redactor.finalize()
    assert out == visible
    assert "secret" not in out
    assert len(redactor.pending_buffer) <= redactor.max_hold_len


def test_redactor_single_huge_chunk_feed() -> None:
    payload = "<x" * 5000
    redactor = HiddenReasoningRedactor()
    delta = redactor.feed(payload)
    assert delta == payload
    assert redactor.finalize() == payload
    assert len(redactor.pending_buffer) <= redactor.max_hold_len


def _suffix_slice_violations_in_process() -> list[str]:
    source = textwrap.dedent(inspect.getsource(HiddenReasoningRedactor._process))
    tree = ast.parse(source)
    violations: list[str] = []

    class _Visitor(ast.NodeVisitor):
        def visit_Subscript(self, node: ast.Subscript) -> None:
            if isinstance(node.slice, ast.Slice):
                lower = getattr(node.slice, "lower", None) or getattr(
                    node.slice, "start", None
                )
                upper = getattr(node.slice, "upper", None)
                if getattr(node.slice, "stop", None) is not None:
                    upper = node.slice.stop
                if lower is not None and upper is None:
                    violations.append(
                        ast.get_source_segment(source, node) or "suffix-slice"
                    )
            self.generic_visit(node)

    _Visitor().visit(tree)
    if "data[i:]" in source:
        violations.append("data[i:]")
    return violations


def test_redactor_process_has_no_suffix_slice() -> None:
    violations = _suffix_slice_violations_in_process()
    assert violations == [], f"suffix slices found: {violations}"


def test_long_duplicate_candidate_context_not_empty() -> None:
    query = "Q" * 4000
    transcript = [("候補者", query)]
    ctx, wm = build_bounded_context(
        session_id="sess-long-dup",
        transcript=transcript,
        current_query=query,
    )
    assert ctx != ""
    assert wm.context_chars > 0
    assert wm.context_chars == char_count(ctx)
    assert char_count(ctx) <= DYNAMIC_CONTEXT_CHAR_BUDGET
    assert _TURN_TRUNCATION_MARKER in ctx
    assert ctx.count(query) == 0
    assert query[:100] in ctx
    assert ctx.count("# Current turn") == 1


def test_short_duplicate_candidate_appears_once() -> None:
    utterance = "同一本文の候補者発言です。"
    transcript = [("面接官", "質問"), ("候補者", utterance)]
    ctx, _ = build_bounded_context(
        session_id="sess-short-dup",
        transcript=transcript,
        current_query=utterance,
    )
    assert ctx.count(utterance) == 1
    assert "# Current turn" in ctx


def test_evidence_prompt_includes_latest_candidate() -> None:
    pairs: list[tuple[str, str]] = []
    for i in range(100):
        role = "面接官" if i % 2 == 0 else "候補者"
        pairs.append((role, f"turn-{i:03d}-statement about topic {i % 11}"))
    turns = transcript_turns_from_pairs(pairs, "sess-latest-ev")
    candidates = [t for t in turns if t["speaker_alias"] == "candidate"]
    latest = candidates[-1]
    block = format_turns_for_evidence_prompt(turns)
    assert latest["turn_id"] in block
    assert latest["text"] in block or _TURN_TRUNCATION_MARKER in block
    ordered_indices = [
        int(line.split("turn_index: ")[1].split()[0])
        for line in block.splitlines()
        if line.startswith("[turn_id:")
    ]
    assert ordered_indices == sorted(ordered_indices)
    assert max(ordered_indices) == latest["turn_index"]
    assert len(ordered_indices) > 1


def test_truncation_marker_on_all_shortened_paths() -> None:
    long_body = "第一句です。" * 10
    truncated = _truncate_query_to_budget(long_body, 50)
    assert _QUERY_TRUNCATION_MARKER in truncated
    assert char_count(truncated) <= 50
    mono = "Q" * 500
    truncated_mono = _truncate_query_to_budget(mono, 80)
    assert _QUERY_TRUNCATION_MARKER in truncated_mono
    assert char_count(truncated_mono) <= 80
    assert _truncate_query_to_budget("短い。", 80) == "短い。"
    assert _truncate_query_to_budget("短い。", 80) == _truncate_query_to_budget("短い。", 80)
