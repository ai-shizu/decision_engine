"""FSA-2026-07-13-05: LLM output must not mutate authoritative 6D state."""

from __future__ import annotations

import ast
import json
import sys
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "src" / "python"))

from core.interview_report import AXIS_WHITELIST, generate_report
from core.session_memory import transcript_turns_from_pairs


PAIRS = [
    ("面接官", "目的を確認してください。"),
    ("候補者", "目的を明確にして、論点を構造化します。"),
    ("面接官", "数字でも検証してください。"),
    ("候補者", "売上10%と市場成長3%を前提に計算します。"),
]


class _Backend:
    name = "nondeterministic-test-backend"

    def __init__(self, response: str) -> None:
        self.response = response
        self.schemas: list[dict] = []
        self.prompts: list[str] = []

    def generate_structured(self, system: str, user: str, json_schema: dict) -> str:
        self.schemas.append(json_schema)
        self.prompts.append(user)
        return self.response


class _Engine:
    def __init__(self, response: str) -> None:
        self.backend = _Backend(response)


def _response(level_a: int, level_b: int) -> str:
    turns = transcript_turns_from_pairs(PAIRS, "fsa05-session")
    return json.dumps(
        {
            "metrics": [
                {"axis": axis, "score": 70, "evidence": "講評候補"}
                for axis in AXIS_WHITELIST
            ],
            "evidence": [
                {
                    "dimension_id": "problem_structuring",
                    "indicator_id": "ps_clarify_objective",
                    "level": level_a,
                    "turn_id": turns[1]["turn_id"],
                    "turn_index": 1,
                    "speaker_alias": "candidate",
                    "quote": "目的を明確にして",
                },
                {
                    "dimension_id": "problem_structuring",
                    "indicator_id": "ps_decompose",
                    "level": level_b,
                    "turn_id": turns[3]["turn_id"],
                    "turn_index": 3,
                    "speaker_alias": "candidate",
                    "quote": "売上10%",
                },
            ],
        },
        ensure_ascii=False,
    )


def _generate(response: str) -> tuple[dict, _Backend]:
    engine = _Engine(response)
    report = generate_report(
        engine,
        "system",
        "transcript",
        "summary",
        {},
        [],
        transcript_pairs=PAIRS,
        session_id="fsa05-session",
    )
    return report, engine.backend


def test_divergent_valid_llm_outputs_cannot_change_authoritative_tensor() -> None:
    low, _ = _generate(_response(0, 1))
    high, _ = _generate(_response(4, 4))

    assert low["tensor_profile"] == high["tensor_profile"]
    for dimension in low["tensor_profile"]["dimensions"]:
        assert dimension["score"] is None
        assert dimension["confidence"] == 0.0
        assert dimension["evidence"] == []


def test_llm_schema_and_prompt_do_not_request_authoritative_tensor_evidence() -> None:
    _, backend = _generate(_response(4, 4))
    assert backend.schemas
    for schema in backend.schemas:
        assert "evidence" not in schema.get("required", [])
        assert "evidence" not in schema.get("properties", {})
    prompt = "\n".join(backend.prompts)
    for forbidden in ("dimension_id", "indicator_id", "turn_id", "rubric level"):
        assert forbidden not in prompt


def test_interview_report_has_no_llm_to_tensor_aggregation_call_path() -> None:
    source_path = ROOT / "src" / "python" / "core" / "interview_report.py"
    tree = ast.parse(source_path.read_text(encoding="utf-8"))
    forbidden = {"aggregate_profile", "parse_and_validate_proposals"}
    used = {
        node.id
        for node in ast.walk(tree)
        if isinstance(node, ast.Name) and node.id in forbidden
    }
    imported = {
        alias.name
        for node in ast.walk(tree)
        if isinstance(node, ast.ImportFrom)
        for alias in node.names
        if alias.name in forbidden
    }
    assert not used
    assert not imported


def test_llm_metric_proposals_are_not_reused_as_future_session_facts() -> None:
    source = (ROOT / "src" / "python" / "core" / "consultation_engine.py").read_text(
        encoding="utf-8"
    )
    assert "compute_growth_context" not in source
    assert "_GROWTH_CONTEXT_TEMPLATE" not in source
    report_source = (
        ROOT / "src" / "python" / "core" / "interview_report.py"
    ).read_text(encoding="utf-8")
    assert "def compute_growth_context" not in report_source


def test_ui_labels_llm_metrics_as_non_authoritative_proposals() -> None:
    source = (
        ROOT / "apps" / "desktop" / "src" / "components" / "InterviewTab.tsx"
    ).read_text(encoding="utf-8")
    assert "AI評価候補（非測定・履歴更新に不使用）" in source
