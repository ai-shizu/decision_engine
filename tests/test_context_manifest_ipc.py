# -*- coding: utf-8 -*-
"""Phase 4-A STEP 4 — context.manifest.latest IPC contract tests."""
from __future__ import annotations

import json
import sys
from pathlib import Path

import pytest

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "src" / "python"))

import engine_stdio  # noqa: E402
from core.retrieval_manifest import (  # noqa: E402
    CandidateStatus,
    ContextLane,
    LaneUsageV1,
    ReasonCode,
    RetrievalCandidateV1,
    SourceType,
    build_bounded_context_with_manifest,
    build_retrieval_manifest,
    compute_content_hash,
    manifest_to_dict,
    save_retrieval_manifest,
)


def _minimal_candidate(**overrides) -> RetrievalCandidateV1:
    base = dict(
        candidate_id="CURRENT:abc",
        document_id="abc",
        content_hash=compute_content_hash("x"),
        source_type=SourceType.CURRENT_QUERY,
        lane=ContextLane.CURRENT,
        char_count=1,
        included_chars=1,
        status=CandidateStatus.ACCEPTED,
        reason_code=ReasonCode.ACCEPTED_REQUIRED_CURRENT,
        selection_rank=None,
        source_index=None,
        speaker_alias=None,
        memory_kind=None,
    )
    base.update(overrides)
    return RetrievalCandidateV1(**base)


def _minimal_lane(**overrides) -> LaneUsageV1:
    base = dict(
        lane=ContextLane.CURRENT,
        budget_chars=2400,
        used_chars=0,
        formatting_chars=0,
        accepted_count=0,
        rejected_count=0,
        deduplicated_count=0,
    )
    base.update(overrides)
    return LaneUsageV1(**base)


def _minimal_manifest(**overrides):
    cand = _minimal_candidate()
    lanes = (
        _minimal_lane(used_chars=1, formatting_chars=0, accepted_count=1),
        _minimal_lane(lane=ContextLane.RECENT_TRANSCRIPT, budget_chars=3600),
        _minimal_lane(lane=ContextLane.WORKING_MEMORY, budget_chars=4000),
        _minimal_lane(lane=ContextLane.RETRIEVED_EVIDENCE, budget_chars=2000),
    )
    factory_kwargs = dict(
        session_id="ipc-test-session",
        transcript_version=1,
        query_hash="a" * 32,
        context_hash="b" * 32,
        prompt_version="pv1",
        runtime_identity="ab" * 64,
        used_chars=1,
        formatting_overhead_chars=0,
        candidates=(cand,),
        lane_usage=lanes,
    )
    factory_kwargs.update(overrides)
    return build_retrieval_manifest(**factory_kwargs)


def _manifest_pair():
    cand = _minimal_candidate()
    lanes = (
        _minimal_lane(used_chars=1, formatting_chars=0, accepted_count=1),
        _minimal_lane(lane=ContextLane.RECENT_TRANSCRIPT, budget_chars=3600),
        _minimal_lane(lane=ContextLane.WORKING_MEMORY, budget_chars=4000),
        _minimal_lane(lane=ContextLane.RETRIEVED_EVIDENCE, budget_chars=2000),
    )
    manifest_a = build_retrieval_manifest(
        session_id="session-a",
        transcript_version=1,
        query_hash="a" * 32,
        context_hash="a" * 32,
        prompt_version="pv1",
        runtime_identity="ab" * 64,
        used_chars=1,
        formatting_overhead_chars=0,
        candidates=(cand,),
        lane_usage=lanes,
    )
    manifest_b = build_retrieval_manifest(
        session_id="session-b",
        transcript_version=2,
        query_hash="b" * 32,
        context_hash="c" * 32,
        prompt_version="pv1",
        runtime_identity="ab" * 64,
        used_chars=1,
        formatting_overhead_chars=0,
        candidates=(cand,),
        lane_usage=lanes,
    )
    assert manifest_a.manifest_id != manifest_b.manifest_id
    return manifest_a, manifest_b


def _patch_manifest_paths(tmp_path, monkeypatch) -> Path:
    from core import paths

    manifest_dir = tmp_path / "retrieval_manifests"
    monkeypatch.setattr(paths, "RETRIEVAL_MANIFESTS_DIR", manifest_dir)
    monkeypatch.setattr(
        paths, "LATEST_RETRIEVAL_MANIFEST", manifest_dir / "latest.json",
    )
    manifest_dir.mkdir(parents=True, exist_ok=True)
    return manifest_dir


def test_context_manifest_latest_no_manifest() -> None:
    result = engine_stdio.dispatch("context.manifest.latest", {})
    assert result == {
        "manifest": None,
        "reason": "NO_MANIFEST",
    }
    assert set(result) == {"manifest", "reason"}


def test_context_manifest_latest_valid_manifest(tmp_path, monkeypatch) -> None:
    _patch_manifest_paths(tmp_path, monkeypatch)
    _, _, manifest = build_bounded_context_with_manifest(
        session_id="ipc-contract-session",
        transcript=[
            ("面接官", "turn-000-statement about problem 0 and data 0%"),
            ("候補者", "turn-001-statement about problem 1 and data 3%"),
        ],
        current_query="turn-001-statement about problem 1 and data 3%",
        runtime_identity="ab" * 64,
    )
    save_retrieval_manifest(manifest)

    result = engine_stdio.dispatch("context.manifest.latest", {})
    assert set(result.keys()) == {"manifest", "reason"}
    assert result["reason"] is None
    assert type(result["manifest"]) is dict
    assert result["manifest"]["schema"] == "retrieval_manifest.v1"
    assert result["manifest"]["manifest_id"] == manifest.manifest_id
    blob = json.dumps(result, ensure_ascii=False)
    for forbidden in ("canonical_text", "quote", "山田太郎", "候補者"):
        assert forbidden not in blob


def test_context_manifest_latest_rejects_invalid_params() -> None:
    with pytest.raises(ValueError):
        engine_stdio.dispatch("context.manifest.latest", {"unexpected": True})
    with pytest.raises(ValueError):
        engine_stdio.dispatch("context.manifest.latest", [])
    with pytest.raises(ValueError):
        engine_stdio.dispatch("context.manifest.latest", None)


def test_context_manifest_latest_rejects_unknown_command() -> None:
    with pytest.raises(ValueError, match="unknown command"):
        engine_stdio.dispatch("context.manifest.unknown", {})


def test_context_manifest_latest_corrupt_manifest_hard_failure(
    tmp_path, monkeypatch,
) -> None:
    manifest_dir = _patch_manifest_paths(tmp_path, monkeypatch)
    manifest_id = "c" * 32
    latest_path = manifest_dir / "latest.json"
    manifest_path = manifest_dir / f"{manifest_id}.json"
    latest_path.write_text(
        json.dumps({"manifest_id": manifest_id}),
        encoding="utf-8",
    )
    manifest_path.write_text('{"broken": true}', encoding="utf-8")
    latest_before = latest_path.read_bytes()
    manifest_before = manifest_path.read_bytes()

    with pytest.raises(ValueError):
        engine_stdio.dispatch("context.manifest.latest", {})

    assert latest_path.read_bytes() == latest_before
    assert manifest_path.read_bytes() == manifest_before
    result_probe = None
    try:
        result_probe = engine_stdio.dispatch("context.manifest.latest", {})
    except ValueError:
        pass
    assert result_probe != {"manifest": None, "reason": "NO_MANIFEST"}


def test_context_manifest_latest_pointer_payload_mismatch_propagates(
    tmp_path, monkeypatch,
) -> None:
    manifest_dir = _patch_manifest_paths(tmp_path, monkeypatch)
    manifest_a, manifest_b = _manifest_pair()
    latest_path = manifest_dir / "latest.json"
    manifest_path = manifest_dir / f"{manifest_a.manifest_id}.json"
    latest_path.write_text(
        json.dumps({"manifest_id": manifest_a.manifest_id}, ensure_ascii=False, indent=2)
        + "\n",
        encoding="utf-8",
    )
    manifest_path.write_text(
        json.dumps(manifest_to_dict(manifest_b), ensure_ascii=False, indent=2) + "\n",
        encoding="utf-8",
    )
    latest_before = latest_path.read_bytes()
    manifest_before = manifest_path.read_bytes()

    with pytest.raises(ValueError, match="latest pointer manifest_id mismatch"):
        engine_stdio.dispatch("context.manifest.latest", {})

    assert latest_path.read_bytes() == latest_before
    assert manifest_path.read_bytes() == manifest_before
    assert list(manifest_dir.glob("*.tmp")) == []


def test_context_manifest_latest_does_not_call_engine(monkeypatch) -> None:
    from core import facade

    def _fail_get_engine():
        raise AssertionError("get_engine must not be called for context.manifest.latest")

    monkeypatch.setattr(facade, "get_engine", _fail_get_engine)
    result = engine_stdio.dispatch("context.manifest.latest", {})
    assert result == {
        "manifest": None,
        "reason": "NO_MANIFEST",
    }
