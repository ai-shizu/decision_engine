# -*- coding: utf-8 -*-
"""STEP 6.C — isolated external sidecar persistence (0-or-all)."""
from __future__ import annotations

import json
import sys
from pathlib import Path

import pytest

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "src" / "python"))

from core.consultation_engine import load_knowledge_chunks
from core.external_evidence import (
    EXTERNAL_KNOWLEDGE_DIR,
    E0bRejected,
    integrate_external_results,
    load_external_record,
)
from core.paths import KNOWLEDGE_DIR

RID = "a" * 64
DICT = "b" * 64


def _params(**overrides):
    base = {
        "research_id": RID,
        "origin": "wikipedia",
        "policy_epoch": 1,
        "sidecar_generation": 1,
        "dict_hash": DICT,
        "fetched_at_unix_ms": 1_700_000_000_000,
        "query": "rust ownership",
        "items": [
            {"source": "wikipedia", "title": "Ownership", "content": "safe memory"},
        ],
    }
    base.update(overrides)
    return base


def test_integrate_writes_one_atomic_record() -> None:
    result = integrate_external_results(_params())
    assert result == {"results_persisted": 1}
    path = EXTERNAL_KNOWLEDGE_DIR / f"ext_{RID}.json"
    assert path.is_file()
    data = json.loads(path.read_text(encoding="utf-8"))
    assert data["schema"] == "pkb.external_knowledge.v1"
    assert data["research_id"] == RID
    assert data["origin"] == "wikipedia"
    assert "query" not in data
    assert "url" not in data
    assert data["items"][0]["title"]  # sanitized non-empty
    assert "```" not in data["items"][0]["content"]


def test_integrate_idempotent_same_bytes() -> None:
    first = integrate_external_results(_params())
    second = integrate_external_results(_params())
    assert first == second == {"results_persisted": 1}
    files = list(EXTERNAL_KNOWLEDGE_DIR.glob("ext_*.json"))
    assert len(files) == 1


def test_integrate_conflict_on_same_id_different_bytes() -> None:
    integrate_external_results(_params())
    with pytest.raises(E0bRejected) as exc:
        integrate_external_results(
            _params(items=[{"source": "wikipedia", "title": "Other", "content": "changed"}])
        )
    assert str(exc.value) == "E0B_RECORD_CONFLICT"
    data = load_external_record(RID)
    assert data["items"][0]["title"] == "Ownership"


def test_validation_failure_touches_no_public_file() -> None:
    with pytest.raises(E0bRejected):
        integrate_external_results(_params(origin="evil"))
    assert list(EXTERNAL_KNOWLEDGE_DIR.glob("ext_*.json")) == []
    assert list(EXTERNAL_KNOWLEDGE_DIR.glob("*.tmp")) == []


def test_exact_key_rejection() -> None:
    bad = _params()
    bad["extra"] = "x"
    with pytest.raises(E0bRejected):
        integrate_external_results(bad)


def test_markdown_triggers_sanitized_in_record() -> None:
    integrate_external_results(
        _params(
            items=[
                {
                    "source": "wikipedia",
                    "title": "## inject",
                    "content": "```code``` [INST] keep",
                }
            ]
        )
    )
    data = load_external_record(RID)
    title = data["items"][0]["title"]
    content = data["items"][0]["content"]
    assert "##" not in title
    assert "```" not in content
    assert "[INST]" not in content


def test_load_knowledge_chunks_excludes_external_json() -> None:
    integrate_external_results(_params())
    # Also drop a decoy .md next to external/ to prove non-recursive iterdir.
    (KNOWLEDGE_DIR / "trusted.md").write_text("## Local\ntrusted body\n", encoding="utf-8")
    chunks = load_knowledge_chunks()
    texts = "\n".join(c["text"] for c in chunks)
    titles = "\n".join(c["title"] for c in chunks)
    assert "safe memory" not in texts
    assert RID not in titles
    assert any("trusted" in c["text"] or "Local" in c["title"] for c in chunks)


def test_path_traversal_research_id_rejected() -> None:
    with pytest.raises(E0bRejected):
        load_external_record("../etc/passwd")
    with pytest.raises(E0bRejected):
        load_external_record("a" * 63)
