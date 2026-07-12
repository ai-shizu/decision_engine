# -*- coding: utf-8 -*-
"""Finding 3 — Manifest persistence isolation from interview/GD/debrief turns."""
from __future__ import annotations

import ast
import json
import sys
from pathlib import Path
from unittest.mock import patch

import pytest

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "src" / "python"))

from core.consultation_engine import ConsultationEngine  # noqa: E402
from core.retrieval_manifest import (  # noqa: E402
    CandidateStatus,
    ContextLane,
    LaneUsageV1,
    ReasonCode,
    RetrievalCandidateV1,
    RetrievalManifestPersistenceError,
    SourceType,
    build_retrieval_manifest,
    compute_content_hash,
    compute_context_hash,
    load_latest_retrieval_manifest,
    manifest_from_dict,
    save_retrieval_manifest,
    validate_manifest,
)

_TRANSCRIPT = [
    ("面接官", "turn-000-statement about problem 0 and data 0%"),
    ("候補者", "turn-001-statement about problem 1 and data 3%"),
    ("面接官", "turn-002-statement about problem 2 and data 6%"),
    ("候補者", "turn-003-statement about problem 3 and data 9%"),
    ("面接官", "turn-004-statement about problem 4 and data 12%"),
    ("候補者", "turn-005-statement about problem 5 and data 15%"),
]
_QUERY = "turn-005-statement about problem 5 and data 15%"
_EXPECTED_STATUS_WARNING = (
    "コンテキスト監査記録を保存できませんでした。相談処理は継続します。"
)
_STDERR_WARNING = (
    "[PKB] retrieval manifest persistence failed; continuing without manifest update"
)


class FailIfCalledBackend:
    def generate(self, *args, **kwargs):
        raise AssertionError("backend must not be called")


def _patch_store(tmp_path, monkeypatch):
    from core import paths

    manifest_dir = tmp_path / "retrieval_manifests"
    manifest_dir.mkdir(parents=True)
    monkeypatch.setattr(paths, "RETRIEVAL_MANIFESTS_DIR", manifest_dir)
    monkeypatch.setattr(
        paths, "LATEST_RETRIEVAL_MANIFEST", manifest_dir / "latest.json",
    )
    return manifest_dir


def _plant_corrupt_immutable(manifest_dir: Path) -> tuple[Path, Path, bytes, bytes]:
    manifest_id = "c" * 32
    latest_path = manifest_dir / "latest.json"
    corrupt_path = manifest_dir / f"{manifest_id}.json"
    latest_bytes = (
        json.dumps({"manifest_id": manifest_id}, ensure_ascii=False, indent=2) + "\n"
    ).encode("utf-8")
    corrupt_bytes = b'{"broken": true}\n'
    latest_path.write_bytes(latest_bytes)
    corrupt_path.write_bytes(corrupt_bytes)
    return latest_path, corrupt_path, latest_bytes, corrupt_bytes


def _engine_state() -> dict:
    return {"transcript": list(_TRANSCRIPT), "config": {}}


def test_bounded_context_continues_when_immutable_corrupt(
    tmp_path, monkeypatch, capsys,
) -> None:
    manifest_dir = _patch_store(tmp_path, monkeypatch)
    latest_path, corrupt_path, latest_before, corrupt_before = _plant_corrupt_immutable(
        manifest_dir,
    )
    statuses: list[str] = []
    engine = ConsultationEngine()
    engine._backend = FailIfCalledBackend()
    state = _engine_state()

    context = engine._bounded_context(
        state, _QUERY, mode="interview_sim", status=statuses.append,
    )

    assert isinstance(context, str) and context
    assert state.get("working_memory") is not None
    assert statuses == [_EXPECTED_STATUS_WARNING]
    err = capsys.readouterr().err
    assert err.count(_STDERR_WARNING) == 1
    assert _QUERY not in err
    assert "broken" not in err
    assert str(manifest_dir) not in err
    assert latest_path.read_bytes() == latest_before
    assert corrupt_path.read_bytes() == corrupt_before
    assert list(manifest_dir.glob("*.tmp")) == []


def test_working_memory_updated_even_when_save_fails(
    tmp_path, monkeypatch,
) -> None:
    _patch_store(tmp_path, monkeypatch)
    _plant_corrupt_immutable(tmp_path / "retrieval_manifests")
    engine = ConsultationEngine()
    engine._backend = FailIfCalledBackend()
    state = _engine_state()
    assert "working_memory" not in state

    engine._bounded_context(state, _QUERY, mode="interview_sim")

    assert state.get("working_memory") is not None


def test_status_warning_exactly_once(tmp_path, monkeypatch) -> None:
    _patch_store(tmp_path, monkeypatch)
    _plant_corrupt_immutable(tmp_path / "retrieval_manifests")
    statuses: list[str] = []
    engine = ConsultationEngine()
    engine._backend = FailIfCalledBackend()
    engine._bounded_context(
        _engine_state(), _QUERY, mode="gd_sim", status=statuses.append,
    )
    assert statuses == [_EXPECTED_STATUS_WARNING]


def test_stderr_warning_fixed_non_sensitive(tmp_path, monkeypatch, capsys) -> None:
    manifest_dir = _patch_store(tmp_path, monkeypatch)
    _plant_corrupt_immutable(manifest_dir)
    engine = ConsultationEngine()
    engine._backend = FailIfCalledBackend()
    engine._bounded_context(_engine_state(), _QUERY, mode="debrief")
    err = capsys.readouterr().err
    assert err.strip() == _STDERR_WARNING
    assert "ValueError" not in err
    assert "Traceback" not in err
    assert "retrieval_manifests" not in err


def test_no_query_manifest_path_or_exc_text_in_warnings(
    tmp_path, monkeypatch, capsys,
) -> None:
    manifest_dir = _patch_store(tmp_path, monkeypatch)
    _plant_corrupt_immutable(manifest_dir)
    statuses: list[str] = []
    engine = ConsultationEngine()
    engine._backend = FailIfCalledBackend()
    secret_query = "SECRET_QUERY_TOKEN_SHOULD_NOT_LEAK"
    engine._bounded_context(
        _engine_state(), secret_query, mode="interview_sim", status=statuses.append,
    )
    err = capsys.readouterr().err
    blob = err + "".join(statuses)
    assert secret_query not in blob
    assert str(manifest_dir) not in blob
    assert "broken" not in blob
    assert err.strip() == _STDERR_WARNING
    assert statuses == [_EXPECTED_STATUS_WARNING]


def test_corrupt_immutable_and_pointer_bytes_unchanged(tmp_path, monkeypatch) -> None:
    manifest_dir = _patch_store(tmp_path, monkeypatch)
    latest_path, corrupt_path, latest_before, corrupt_before = _plant_corrupt_immutable(
        manifest_dir,
    )
    engine = ConsultationEngine()
    engine._backend = FailIfCalledBackend()
    engine._bounded_context(_engine_state(), _QUERY, mode="interview_sim")
    assert latest_path.read_bytes() == latest_before
    assert corrupt_path.read_bytes() == corrupt_before


def test_corrupt_pointer_not_auto_repaired(tmp_path, monkeypatch) -> None:
    manifest_dir = _patch_store(tmp_path, monkeypatch)
    latest_path = manifest_dir / "latest.json"
    bad = (json.dumps({"manifest_id": "c" * 32, "extra": 1}) + "\n").encode("utf-8")
    latest_path.write_bytes(bad)
    engine = ConsultationEngine()
    engine._backend = FailIfCalledBackend()
    engine._bounded_context(_engine_state(), _QUERY, mode="interview_sim")
    assert latest_path.read_bytes() == bad
    assert not (manifest_dir / "latest.json.bak").exists()


def test_tmp_residue_absent_after_persistence_failure(tmp_path, monkeypatch) -> None:
    manifest_dir = _patch_store(tmp_path, monkeypatch)
    _plant_corrupt_immutable(manifest_dir)
    engine = ConsultationEngine()
    engine._backend = FailIfCalledBackend()
    engine._bounded_context(_engine_state(), _QUERY, mode="interview_sim")
    assert list(manifest_dir.glob("*.tmp")) == []
    assert list(manifest_dir.rglob("*.tmp")) == []


def test_validate_manifest_failure_still_propagates(tmp_path, monkeypatch) -> None:
    _patch_store(tmp_path, monkeypatch)
    engine = ConsultationEngine()
    engine._backend = FailIfCalledBackend()
    state = _engine_state()
    with patch(
        "core.retrieval_manifest.validate_manifest",
        side_effect=ValueError("manifest_id mismatch"),
    ):
        with pytest.raises(ValueError, match="manifest_id mismatch"):
            engine._bounded_context(state, _QUERY, mode="interview_sim")
    assert "working_memory" not in state


def test_unknown_runtime_error_propagates(tmp_path, monkeypatch) -> None:
    _patch_store(tmp_path, monkeypatch)
    engine = ConsultationEngine()
    engine._backend = FailIfCalledBackend()
    state = _engine_state()

    def _raise_runtime(manifest):
        raise RuntimeError("unexpected disk subsystem fault")

    with patch(
        "core.retrieval_manifest.save_retrieval_manifest",
        _raise_runtime,
    ):
        with pytest.raises(RuntimeError, match="unexpected disk subsystem fault"):
            engine._bounded_context(state, _QUERY, mode="interview_sim")


def test_successful_save_no_warnings_updates_latest(
    tmp_path, monkeypatch, capsys,
) -> None:
    manifest_dir = _patch_store(tmp_path, monkeypatch)
    statuses: list[str] = []
    engine = ConsultationEngine()
    engine._backend = FailIfCalledBackend()
    state = _engine_state()
    context = engine._bounded_context(
        state, _QUERY, mode="interview_sim", status=statuses.append,
    )
    assert statuses == []
    assert capsys.readouterr().err == ""
    loaded = load_latest_retrieval_manifest()
    assert loaded is not None
    assert loaded.context_hash == compute_context_hash(context)
    assert list(manifest_dir.glob("*.tmp")) == []


def test_all_bounded_context_call_sites_pass_status() -> None:
    src = (
        Path(__file__).resolve().parents[1]
        / "src"
        / "python"
        / "core"
        / "consultation_engine.py"
    ).read_text(encoding="utf-8")
    tree = ast.parse(src)
    calls: list[ast.Call] = []

    class Visitor(ast.NodeVisitor):
        def visit_Call(self, node: ast.Call) -> None:
            func = node.func
            if isinstance(func, ast.Attribute) and func.attr == "_bounded_context":
                calls.append(node)
            elif isinstance(func, ast.Name) and func.id == "_bounded_context":
                calls.append(node)
            self.generic_visit(node)

    Visitor().visit(tree)
    assert len(calls) == 5, f"expected 5 call sites, found {len(calls)}"
    for call in calls:
        kw = {k.arg for k in call.keywords if k.arg}
        assert "status" in kw, ast.dump(call, include_attributes=False)


def test_context_manifest_latest_corrupt_still_hard_fails(
    tmp_path, monkeypatch,
) -> None:
    import engine_stdio

    manifest_dir = _patch_store(tmp_path, monkeypatch)
    latest_path, corrupt_path, latest_before, corrupt_before = _plant_corrupt_immutable(
        manifest_dir,
    )
    with pytest.raises(ValueError):
        engine_stdio.dispatch("context.manifest.latest", {})
    assert latest_path.read_bytes() == latest_before
    assert corrupt_path.read_bytes() == corrupt_before
    probe = None
    try:
        probe = engine_stdio.dispatch("context.manifest.latest", {})
    except ValueError:
        pass
    assert probe != {"manifest": None, "reason": "NO_MANIFEST"}


def test_orphan_immutable_kept_when_latest_replace_fails(
    tmp_path, monkeypatch,
) -> None:
    from core import paths

    manifest_dir = _patch_store(tmp_path, monkeypatch)
    cand = RetrievalCandidateV1(
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
    lanes = (
        LaneUsageV1(
            lane=ContextLane.CURRENT,
            budget_chars=2400,
            used_chars=1,
            formatting_chars=0,
            accepted_count=1,
            rejected_count=0,
            deduplicated_count=0,
        ),
        LaneUsageV1(
            lane=ContextLane.RECENT_TRANSCRIPT,
            budget_chars=3600,
            used_chars=0,
            formatting_chars=0,
            accepted_count=0,
            rejected_count=0,
            deduplicated_count=0,
        ),
        LaneUsageV1(
            lane=ContextLane.WORKING_MEMORY,
            budget_chars=4000,
            used_chars=0,
            formatting_chars=0,
            accepted_count=0,
            rejected_count=0,
            deduplicated_count=0,
        ),
        LaneUsageV1(
            lane=ContextLane.RETRIEVED_EVIDENCE,
            budget_chars=2000,
            used_chars=0,
            formatting_chars=0,
            accepted_count=0,
            rejected_count=0,
            deduplicated_count=0,
        ),
    )
    old = build_retrieval_manifest(
        session_id="old",
        transcript_version=1,
        query_hash="a" * 32,
        context_hash="b" * 32,
        prompt_version="pv1",
        model_hash="",
        used_chars=1,
        formatting_overhead_chars=0,
        candidates=(cand,),
        lane_usage=lanes,
    )
    save_retrieval_manifest(old)
    old_latest = paths.LATEST_RETRIEVAL_MANIFEST.read_bytes()
    old_id = old.manifest_id

    new = build_retrieval_manifest(
        session_id="new",
        transcript_version=2,
        query_hash="c" * 32,
        context_hash="d" * 32,
        prompt_version="pv1",
        model_hash="",
        used_chars=1,
        formatting_overhead_chars=0,
        candidates=(cand,),
        lane_usage=lanes,
    )
    assert new.manifest_id != old_id

    import os

    real_replace = os.replace
    calls = {"n": 0}

    def _replace(src, dst):
        calls["n"] += 1
        if calls["n"] >= 2:
            raise OSError("simulated latest replace failure")
        return real_replace(src, dst)

    with patch("os.replace", _replace):
        with pytest.raises(RetrievalManifestPersistenceError) as ei:
            save_retrieval_manifest(new)
    assert str(ei.value) == "retrieval manifest persistence failed"
    assert isinstance(ei.value.__cause__, OSError)

    new_path = manifest_dir / f"{new.manifest_id}.json"
    assert new_path.is_file()
    data = json.loads(new_path.read_text(encoding="utf-8"))
    validate_manifest(manifest_from_dict(data))
    assert paths.LATEST_RETRIEVAL_MANIFEST.read_bytes() == old_latest
    assert json.loads(old_latest.decode("utf-8"))["manifest_id"] == old_id
    assert list(manifest_dir.glob("*.tmp")) == []


def test_retention_corrupt_historical_isolated_without_repair(
    tmp_path, monkeypatch, capsys,
) -> None:
    """Finding 11: prune preflight failure is typed persistence; corrupt bytes stay."""
    manifest_dir = _patch_store(tmp_path, monkeypatch)
    import core.retrieval_manifest as rm

    monkeypatch.setattr(rm, "RETRIEVAL_MANIFEST_RETENTION_LIMIT", 3)
    # Seed one valid latest, then plant corrupt owned historical.
    from core.retrieval_manifest import (
        CandidateStatus,
        ContextLane,
        LaneUsageV1,
        ReasonCode,
        RetrievalCandidateV1,
        SourceType,
        build_retrieval_manifest,
        compute_content_hash,
        save_retrieval_manifest,
    )

    cand = RetrievalCandidateV1(
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
    lane = LaneUsageV1(
        lane=ContextLane.CURRENT,
        budget_chars=2400,
        used_chars=1,
        formatting_chars=0,
        accepted_count=1,
        rejected_count=0,
        deduplicated_count=0,
    )
    lanes = (
        lane,
        LaneUsageV1(
            lane=ContextLane.RECENT_TRANSCRIPT,
            budget_chars=3600,
            used_chars=0,
            formatting_chars=0,
            accepted_count=0,
            rejected_count=0,
            deduplicated_count=0,
        ),
        LaneUsageV1(
            lane=ContextLane.WORKING_MEMORY,
            budget_chars=4000,
            used_chars=0,
            formatting_chars=0,
            accepted_count=0,
            rejected_count=0,
            deduplicated_count=0,
        ),
        LaneUsageV1(
            lane=ContextLane.RETRIEVED_EVIDENCE,
            budget_chars=2000,
            used_chars=0,
            formatting_chars=0,
            accepted_count=0,
            rejected_count=0,
            deduplicated_count=0,
        ),
    )
    seed = build_retrieval_manifest(
        session_id="seed",
        transcript_version=1,
        query_hash="a" * 32,
        context_hash="b" * 32,
        prompt_version="pv1",
        model_hash="",
        used_chars=1,
        formatting_overhead_chars=0,
        candidates=(cand,),
        lane_usage=lanes,
    )
    save_retrieval_manifest(seed)
    corrupt_path = manifest_dir / f"{'a' * 32}.json"
    corrupt_bytes = b'{"broken": true}\n'
    corrupt_path.write_bytes(corrupt_bytes)

    statuses: list[str] = []
    engine = ConsultationEngine()
    engine._backend = FailIfCalledBackend()
    context = engine._bounded_context(
        _engine_state(), _QUERY, mode="interview_sim", status=statuses.append,
    )
    assert isinstance(context, str) and context
    assert statuses.count(_EXPECTED_STATUS_WARNING) == 1
    err = capsys.readouterr().err
    assert err.count(_STDERR_WARNING) == 1
    assert corrupt_path.read_bytes() == corrupt_bytes
    assert "broken" not in err
    assert str(corrupt_path) not in err
