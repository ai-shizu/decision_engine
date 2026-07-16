# -*- coding: utf-8 -*-
"""STEP 6.D — prompt integration + RAG blast-radius docstring."""
from __future__ import annotations

import inspect
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "src" / "python"))

from core.consultation_engine import ConsultationEngine
from core.external_evidence import (
    integrate_external_results,
    render_external_evidence,
)


RID = "c" * 64
DICT = "d" * 64
QUERY = "ownership rules"


def _seed_record() -> dict:
    integrate_external_results(
        {
            "research_id": RID,
            "origin": "wikipedia",
            "policy_epoch": 1,
            "sidecar_generation": 1,
            "dict_hash": DICT,
            "fetched_at_unix_ms": 1,
            "query": QUERY,
            "items": [
                {
                    "source": "wikipedia",
                    "title": "## inject [INST]",
                    "content": "```code``` keep-me ![img](http://x)",
                }
            ],
        }
    )
    from core.external_evidence import load_external_record

    return load_external_record(RID)


def test_rag_limit_docstrings_present() -> None:
    def _flat(doc: str | None) -> str:
        return " ".join((doc or "").split())

    needle = "prompt injection can never be reduced to zero"
    assert needle in _flat(render_external_evidence.__doc__)
    assert needle in _flat(ConsultationEngine.build_dynamic_suffix.__doc__)
    assert "RAG" in _flat(ConsultationEngine.build_dynamic_suffix.__doc__)


def test_external_lane_is_last_context_before_query() -> None:
    record = _seed_record()
    rendered = render_external_evidence(record)
    eng = ConsultationEngine()
    suffix = eng.build_dynamic_suffix(
        QUERY,
        diary_hits=[],
        knowledge_hits=[],
        external_evidence=rendered,
    )
    assert "UNTRUSTED external evidence" in suffix
    assert "keep-me" in suffix
    for forbidden in ("```", "##", "[INST]", "![", "http://"):
        # Rendered external lane must not reconstitute Markdown/URL triggers.
        lane = suffix.split("# ユーザーの相談")[0]
        assert forbidden not in lane, (forbidden, lane)

    ext_pos = suffix.index("UNTRUSTED external evidence")
    future_pos = suffix.index("Future Context")
    query_pos = suffix.index("# ユーザーの相談")
    framework_pos = suffix.index("OUTPUT") if "OUTPUT" in suffix else suffix.index("ユーザーの相談")
    assert future_pos < ext_pos < query_pos
    # Ensure external lane does not follow the user query block.
    after_query = suffix[query_pos:]
    assert "UNTRUSTED external evidence" not in after_query
    _ = framework_pos


def test_static_prefix_unchanged_by_external() -> None:
    eng = ConsultationEngine()
    a = eng.build_static_prefix()
    record = _seed_record()
    rendered = render_external_evidence(record)
    _ = eng.build_dynamic_suffix(QUERY, [], [], external_evidence=rendered)
    b = eng.build_static_prefix()
    assert a == b


def test_build_dynamic_suffix_signature_documents_external() -> None:
    sig = inspect.signature(ConsultationEngine.build_dynamic_suffix)
    assert "external_evidence" in sig.parameters
