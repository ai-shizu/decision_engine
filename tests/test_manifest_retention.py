# -*- coding: utf-8 -*-
"""Finding 11 — bounded retention for valid immutable retrieval manifests."""
from __future__ import annotations

import json
import os
import sys
import time
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
    load_latest_retrieval_manifest,
    manifest_to_dict,
    save_retrieval_manifest,
)

_EXPECTED_STATUS_WARNING = (
    "コンテキスト監査記録を保存できませんでした。相談処理は継続します。"
)
_STDERR_WARNING = (
    "[PKB] retrieval manifest persistence failed; continuing without manifest update"
)
_PERSIST_MSG = "retrieval manifest persistence failed"


def _patch_store(tmp_path, monkeypatch, limit: int = 3):
    from core import paths
    import core.retrieval_manifest as rm

    manifest_dir = tmp_path / "retrieval_manifests"
    manifest_dir.mkdir(parents=True, exist_ok=True)
    monkeypatch.setattr(paths, "RETRIEVAL_MANIFESTS_DIR", manifest_dir)
    monkeypatch.setattr(
        paths, "LATEST_RETRIEVAL_MANIFEST", manifest_dir / "latest.json",
    )
    monkeypatch.setattr(rm, "RETRIEVAL_MANIFEST_RETENTION_LIMIT", limit)
    return manifest_dir


def _minimal_manifest(**overrides):
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
    kwargs = dict(
        session_id="s",
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
    kwargs.update(overrides)
    return build_retrieval_manifest(**kwargs)


def _owned_hex_names(manifest_dir: Path) -> set[str]:
    names: set[str] = set()
    for p in manifest_dir.iterdir():
        if not p.is_file() or p.is_symlink():
            continue
        if p.name == "latest.json" or p.suffix != ".json":
            continue
        if len(p.stem) == 32 and all(c in "0123456789abcdef" for c in p.stem):
            names.add(p.name)
    return names


def _save_unique(i: int):
    return _minimal_manifest(
        session_id=f"s{i}",
        transcript_version=i,
        query_hash=f"{i:032x}",
        context_hash=f"{(i + 7):032x}",
    )


def test_under_limit_does_not_delete(tmp_path, monkeypatch) -> None:
    manifest_dir = _patch_store(tmp_path, monkeypatch, limit=3)
    ids = []
    for i in range(3):
        m = _save_unique(i)
        save_retrieval_manifest(m)
        ids.append(m.manifest_id)
        time.sleep(0.01)
    assert _owned_hex_names(manifest_dir) == {f"{mid}.json" for mid in ids}


def test_over_limit_keeps_owned_within_cap(tmp_path, monkeypatch) -> None:
    manifest_dir = _patch_store(tmp_path, monkeypatch, limit=3)
    for i in range(5):
        save_retrieval_manifest(_save_unique(i))
        time.sleep(0.01)
    assert len(_owned_hex_names(manifest_dir)) <= 3


def test_latest_payload_always_loadable(tmp_path, monkeypatch) -> None:
    _patch_store(tmp_path, monkeypatch, limit=3)
    last = None
    for i in range(5):
        last = _save_unique(i)
        save_retrieval_manifest(last)
        time.sleep(0.01)
    loaded = load_latest_retrieval_manifest()
    assert loaded is not None
    assert loaded.manifest_id == last.manifest_id


def test_latest_protected_even_if_oldest_mtime(tmp_path, monkeypatch) -> None:
    from core import paths

    manifest_dir = _patch_store(tmp_path, monkeypatch, limit=3)
    items = [_save_unique(i) for i in range(4)]
    for m in items:
        save_retrieval_manifest(m)
        time.sleep(0.01)
    # Force deterministic mtimes; make current latest the oldest file.
    latest = items[-1]
    for i, m in enumerate(items):
        p = manifest_dir / f"{m.manifest_id}.json"
        if not p.exists():
            continue
        ns = (1, 1) if m.manifest_id == latest.manifest_id else (1000 + i, 1000 + i)
        os.utime(p, ns=ns)
    # Re-save latest to run prune while it remains latest and oldest by mtime.
    save_retrieval_manifest(latest)
    assert (manifest_dir / f"{latest.manifest_id}.json").is_file()
    assert json.loads(paths.LATEST_RETRIEVAL_MANIFEST.read_text(encoding="utf-8"))[
        "manifest_id"
    ] == latest.manifest_id
    assert len(_owned_hex_names(manifest_dir)) <= 3


def test_non_latest_kept_by_mtime_then_filename(tmp_path, monkeypatch) -> None:
    from core import paths

    manifest_dir = _patch_store(tmp_path, monkeypatch, limit=3)
    manifests = [_save_unique(i) for i in range(4)]
    for m in manifests:
        save_retrieval_manifest(m)
        time.sleep(0.01)
    latest = manifests[3]
    # Equal mtime → filename descending selects which non-latest survive.
    for m in manifests:
        p = manifest_dir / f"{m.manifest_id}.json"
        if p.exists():
            os.utime(p, ns=(100, 100))
    available_non_latest = {
        name
        for name in _owned_hex_names(manifest_dir)
        if name != f"{latest.manifest_id}.json"
    }
    paths.LATEST_RETRIEVAL_MANIFEST.write_text(
        json.dumps({"manifest_id": latest.manifest_id}, ensure_ascii=False, indent=2)
        + "\n",
        encoding="utf-8",
    )
    save_retrieval_manifest(latest)
    owned = _owned_hex_names(manifest_dir)
    assert f"{latest.manifest_id}.json" in owned
    assert len(owned) == 3
    non_latest_kept = sorted(
        (n for n in owned if n != f"{latest.manifest_id}.json"),
        reverse=True,
    )
    expected_non_latest = sorted(
        available_non_latest,
        reverse=True,
    )[:2]
    assert non_latest_kept == expected_non_latest


def test_corrupt_historical_aborts_all_deletes(tmp_path, monkeypatch) -> None:
    manifest_dir = _patch_store(tmp_path, monkeypatch, limit=3)
    for i in range(3):
        save_retrieval_manifest(_save_unique(i))
        time.sleep(0.01)
    corrupt_id = "c" * 32
    corrupt_path = manifest_dir / f"{corrupt_id}.json"
    corrupt_bytes = b'{"broken": true}\n'
    corrupt_path.write_bytes(corrupt_bytes)
    pre_owned = {
        p.name: p.read_bytes()
        for p in manifest_dir.iterdir()
        if p.is_file() and p.name != "latest.json"
    }
    with pytest.raises(RetrievalManifestPersistenceError) as ei:
        save_retrieval_manifest(_save_unique(99))
    assert str(ei.value) == _PERSIST_MSG
    assert corrupt_path.read_bytes() == corrupt_bytes
    for name, data in pre_owned.items():
        path = manifest_dir / name
        assert path.is_file()
        assert path.read_bytes() == data


def test_id_mismatch_fails_before_delete(tmp_path, monkeypatch) -> None:
    manifest_dir = _patch_store(tmp_path, monkeypatch, limit=3)
    save_retrieval_manifest(_save_unique(0))
    save_retrieval_manifest(_save_unique(1))
    good = _save_unique(2)
    bad_name = "d" * 32
    bad_path = manifest_dir / f"{bad_name}.json"
    bad_path.write_text(
        json.dumps(manifest_to_dict(good), ensure_ascii=False, indent=2) + "\n",
        encoding="utf-8",
    )
    before = {
        p.name: p.read_bytes()
        for p in manifest_dir.iterdir()
        if p.is_file() and p.name != "latest.json"
    }
    with pytest.raises(RetrievalManifestPersistenceError):
        save_retrieval_manifest(_save_unique(50))
    for name, data in before.items():
        assert (manifest_dir / name).read_bytes() == data


def test_symlink_rejected_without_follow_or_delete(tmp_path, monkeypatch) -> None:
    manifest_dir = _patch_store(tmp_path, monkeypatch, limit=3)
    real = _save_unique(0)
    save_retrieval_manifest(real)
    link_path = manifest_dir / f"{'e' * 32}.json"
    try:
        link_path.symlink_to(manifest_dir / f"{real.manifest_id}.json")
    except OSError:
        pytest.skip("symlink not permitted on this host")
    with pytest.raises(RetrievalManifestPersistenceError):
        save_retrieval_manifest(_save_unique(80))
    assert link_path.is_symlink()
    assert (manifest_dir / f"{real.manifest_id}.json").is_file()


def test_unknown_non_hex_and_tmp_not_deleted(tmp_path, monkeypatch) -> None:
    manifest_dir = _patch_store(tmp_path, monkeypatch, limit=3)
    noise_file = manifest_dir / "notes.json"
    noise_md = manifest_dir / "README.md"
    noise_tmp = manifest_dir / "abc.json.tmp"
    noise_dir = manifest_dir / "subdir"
    noise_file.write_text("{}", encoding="utf-8")
    noise_md.write_text("x", encoding="utf-8")
    noise_tmp.write_text("tmp", encoding="utf-8")
    noise_dir.mkdir()
    for i in range(5):
        save_retrieval_manifest(_save_unique(i))
        time.sleep(0.01)
    assert noise_file.is_file()
    assert noise_md.is_file()
    assert noise_tmp.is_file()
    assert noise_dir.is_dir()


def test_duplicate_save_idempotent_under_retention(tmp_path, monkeypatch) -> None:
    manifest_dir = _patch_store(tmp_path, monkeypatch, limit=3)
    m = _save_unique(0)
    save_retrieval_manifest(m)
    first = (manifest_dir / f"{m.manifest_id}.json").read_bytes()
    save_retrieval_manifest(m)
    second = (manifest_dir / f"{m.manifest_id}.json").read_bytes()
    assert first == second


def test_latest_replace_failure_skips_prune(tmp_path, monkeypatch) -> None:
    manifest_dir = _patch_store(tmp_path, monkeypatch, limit=3)
    import core.retrieval_manifest as rm
    from core import paths

    old = _save_unique(0)
    save_retrieval_manifest(old)
    old_latest = paths.LATEST_RETRIEVAL_MANIFEST.read_bytes()
    new = _save_unique(1)
    prune_calls = {"n": 0}

    def _counting_prune(**kwargs):
        prune_calls["n"] += 1

    monkeypatch.setattr(rm, "_prune_retrieval_manifests", _counting_prune)
    real_replace = os.replace
    calls = {"n": 0}

    def _replace(src, dst):
        calls["n"] += 1
        if calls["n"] >= 2:
            raise OSError("simulated latest replace failure")
        return real_replace(src, dst)

    with patch("os.replace", _replace):
        with pytest.raises(RetrievalManifestPersistenceError):
            save_retrieval_manifest(new)
    assert prune_calls["n"] == 0
    assert paths.LATEST_RETRIEVAL_MANIFEST.read_bytes() == old_latest
    assert (manifest_dir / f"{new.manifest_id}.json").is_file()
    assert list(manifest_dir.glob("*.tmp")) == []


def test_prune_only_after_latest_commit(tmp_path, monkeypatch) -> None:
    _patch_store(tmp_path, monkeypatch, limit=3)
    import core.retrieval_manifest as rm
    from core import paths

    order: list[str] = []
    real_prune = rm._prune_retrieval_manifests
    real_atomic = rm._atomic_write_text

    def _atomic(path, text):
        order.append(f"write:{Path(path).name}")
        return real_atomic(path, text)

    def _prune(**kwargs):
        order.append("prune")
        assert paths.LATEST_RETRIEVAL_MANIFEST.exists()
        ptr = json.loads(paths.LATEST_RETRIEVAL_MANIFEST.read_text(encoding="utf-8"))
        assert ptr["manifest_id"] == kwargs["latest_manifest_id"]
        return real_prune(**kwargs)

    monkeypatch.setattr(rm, "_atomic_write_text", _atomic)
    monkeypatch.setattr(rm, "_prune_retrieval_manifests", _prune)
    save_retrieval_manifest(_save_unique(0))
    assert "prune" in order
    assert order.index("write:latest.json") < order.index("prune")


def test_prune_oserror_becomes_persistence_error(tmp_path, monkeypatch) -> None:
    manifest_dir = _patch_store(tmp_path, monkeypatch, limit=3)
    for i in range(3):
        save_retrieval_manifest(_save_unique(i))
        time.sleep(0.01)
    real_unlink = Path.unlink

    def _unlink(self, *args, **kwargs):
        if (
            self.suffix == ".json"
            and self.name != "latest.json"
            and len(self.stem) == 32
            and all(c in "0123456789abcdef" for c in self.stem)
        ):
            raise OSError("simulated unlink failure")
        return real_unlink(self, *args, **kwargs)

    with patch.object(Path, "unlink", _unlink):
        with pytest.raises(RetrievalManifestPersistenceError) as ei:
            save_retrieval_manifest(_save_unique(20))
    assert str(ei.value) == _PERSIST_MSG
    assert isinstance(ei.value.__cause__, OSError)
    assert list(manifest_dir.glob("*.tmp")) == []


def test_retention_failure_isolated_in_bounded_context(
    tmp_path, monkeypatch, capsys,
) -> None:
    manifest_dir = _patch_store(tmp_path, monkeypatch, limit=3)
    save_retrieval_manifest(_save_unique(0))
    corrupt_path = manifest_dir / f"{'f' * 32}.json"
    corrupt_path.write_bytes(b'{"broken": true}\n')
    statuses: list[str] = []
    engine = ConsultationEngine()

    class FailIfCalledBackend:
        def generate(self, *args, **kwargs):
            raise AssertionError("backend must not be called")

    engine._backend = FailIfCalledBackend()
    transcript = [
        ("面接官", "turn-000-statement about problem 0 and data 0%"),
        ("候補者", "turn-001-statement about problem 1 and data 3%"),
        ("面接官", "turn-002-statement about problem 2 and data 6%"),
        ("候補者", "turn-003-statement about problem 3 and data 9%"),
        ("面接官", "turn-004-statement about problem 4 and data 12%"),
        ("候補者", "turn-005-statement about problem 5 and data 15%"),
    ]
    query = "turn-005-statement about problem 5 and data 15%"
    context = engine._bounded_context(
        {
            "transcript": transcript,
            "config": {},
            "canonical_runtime_identity": "ab" * 64,
        },
        query,
        mode="interview_sim",
        status=statuses.append,
    )
    assert isinstance(context, str) and context
    assert statuses.count(_EXPECTED_STATUS_WARNING) == 1
    err = capsys.readouterr().err
    assert err.count(_STDERR_WARNING) == 1
    assert "broken" not in err
    assert str(manifest_dir) not in err
    assert query not in err
    assert list(manifest_dir.glob("*.tmp")) == []


def test_no_tmp_residue_after_successful_prune(tmp_path, monkeypatch) -> None:
    manifest_dir = _patch_store(tmp_path, monkeypatch, limit=3)
    for i in range(5):
        save_retrieval_manifest(_save_unique(i))
        time.sleep(0.01)
    assert list(manifest_dir.glob("*.tmp")) == []
    assert len(_owned_hex_names(manifest_dir)) <= 3
