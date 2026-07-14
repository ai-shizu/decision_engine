# -*- coding: utf-8 -*-
"""FSA-2026-07-13-07 RED contracts for authenticated state chaining."""
from __future__ import annotations

import hashlib
import inspect
import json
import sys
from pathlib import Path

import pytest

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "src" / "python"))

from core.retrieval_manifest import (  # noqa: E402
    CandidateStatus,
    ContextLane,
    LaneUsageV1,
    ReasonCode,
    RetrievalCandidateV1,
    SourceType,
    build_retrieval_manifest,
    compute_content_hash,
    load_latest_retrieval_manifest,
    manifest_to_dict,
    save_retrieval_manifest,
)
from core.state_chain import genesis_parent_hash  # noqa: E402

_RUNTIME_IDENTITY = "ab" * 64
_SESSION_A_HEAD = hashlib.sha512(b"session-a").hexdigest()
_SESSION_B_HEAD = hashlib.sha512(b"session-b").hexdigest()


def _patch_store(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> Path:
    from core import paths

    manifest_dir = tmp_path / "retrieval_manifests"
    monkeypatch.setattr(paths, "RETRIEVAL_MANIFESTS_DIR", manifest_dir)
    monkeypatch.setattr(
        paths,
        "LATEST_RETRIEVAL_MANIFEST",
        manifest_dir / "latest.json",
    )
    return manifest_dir


def _manifest(
    *,
    session_id: str,
    session_genesis_id: str,
    sequence_number: int,
    parent_hash: str,
    transcript_version: int,
    marker: str,
):
    candidate = RetrievalCandidateV1(
        candidate_id=f"CURRENT:{marker}",
        document_id=marker,
        content_hash=compute_content_hash(marker),
        source_type=SourceType.CURRENT_QUERY,
        lane=ContextLane.CURRENT,
        char_count=len(marker),
        included_chars=len(marker),
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
            used_chars=len(marker),
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
    return build_retrieval_manifest(
        parent_hash=parent_hash,
        sequence_number=sequence_number,
        session_genesis_id=session_genesis_id,
        session_id=session_id,
        transcript_version=transcript_version,
        query_hash=hashlib.blake2b(marker.encode("utf-8"), digest_size=16).hexdigest(),
        context_hash=hashlib.blake2b(
            f"context:{marker}".encode("utf-8"), digest_size=16
        ).hexdigest(),
        prompt_version="fsa07-red",
        runtime_identity=_RUNTIME_IDENTITY,
        used_chars=len(marker),
        formatting_overhead_chars=0,
        candidates=(candidate,),
        lane_usage=lanes,
    )


def _write_latest(
    manifest_dir: Path,
    *,
    manifest_id: str,
    session_genesis_id: str,
    sequence_number: int,
) -> None:
    (manifest_dir / "latest.json").write_text(
        json.dumps(
            {
                "manifest_id": manifest_id,
                "session_genesis_id": session_genesis_id,
                "sequence_number": sequence_number,
            },
            ensure_ascii=False,
            indent=2,
        )
        + "\n",
        encoding="utf-8",
    )


def _unkeyed_manifest_id(payload: dict) -> str:
    canonical = dict(payload)
    canonical["manifest_id"] = ""
    encoded = json.dumps(
        canonical,
        sort_keys=True,
        ensure_ascii=False,
        separators=(",", ":"),
    ).encode("utf-8")
    return hashlib.sha256(encoded).hexdigest()


def _strict_load_or_expose_acceptance(
    *,
    expected_session_head: str,
    expected_sequence_number: int,
    replayed_manifest_id: str,
    attack: str,
):
    parameters = inspect.signature(load_latest_retrieval_manifest).parameters
    required = {"expected_session_head", "expected_sequence_number"}
    if not required.issubset(parameters):
        accepted = load_latest_retrieval_manifest()
        assert accepted is not None
        assert accepted.manifest_id == replayed_manifest_id
        pytest.fail(
            f"{attack} accepted: loader has no expected session head/sequence boundary"
        )
    return load_latest_retrieval_manifest(
        expected_session_head=expected_session_head,
        expected_sequence_number=expected_sequence_number,
    )


def test_forged_payload_with_recomputed_unkeyed_id_is_rejected(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    manifest_dir = _patch_store(tmp_path, monkeypatch)
    original = _manifest(
        session_id="session-a",
        session_genesis_id=_SESSION_A_HEAD,
        sequence_number=1,
        parent_hash=genesis_parent_hash(_SESSION_A_HEAD),
        transcript_version=1,
        marker="alpha",
    )
    save_retrieval_manifest(original)

    forged = manifest_to_dict(original)
    forged["prompt_version"] = "attacker-controlled-prompt"
    forged_id = _unkeyed_manifest_id(forged)
    forged["manifest_id"] = forged_id
    (manifest_dir / f"{forged_id}.json").write_text(
        json.dumps(forged, ensure_ascii=False, indent=2) + "\n",
        encoding="utf-8",
    )
    _write_latest(
        manifest_dir,
        manifest_id=forged_id,
        session_genesis_id=_SESSION_A_HEAD,
        sequence_number=1,
    )

    with pytest.raises(ValueError, match=r"(?i)(mac|authentic|forg|signature)"):
        load_latest_retrieval_manifest(
            expected_session_head=_SESSION_A_HEAD,
            expected_sequence_number=1,
        )


def test_rollback_to_old_valid_manifest_is_rejected(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    manifest_dir = _patch_store(tmp_path, monkeypatch)
    old = _manifest(
        session_id="session-a",
        session_genesis_id=_SESSION_A_HEAD,
        sequence_number=1,
        parent_hash=genesis_parent_hash(_SESSION_A_HEAD),
        transcript_version=1,
        marker="old",
    )
    current = _manifest(
        session_id="session-a",
        session_genesis_id=_SESSION_A_HEAD,
        sequence_number=2,
        parent_hash=old.manifest_id,
        transcript_version=5,
        marker="current",
    )
    save_retrieval_manifest(old)
    save_retrieval_manifest(current)
    _write_latest(
        manifest_dir,
        manifest_id=old.manifest_id,
        session_genesis_id=_SESSION_A_HEAD,
        sequence_number=1,
    )

    with pytest.raises(ValueError, match=r"(?i)(rollback|sequence|stale)"):
        _strict_load_or_expose_acceptance(
            expected_session_head=_SESSION_A_HEAD,
            expected_sequence_number=2,
            replayed_manifest_id=old.manifest_id,
            attack="state replay",
        )


def test_valid_manifest_from_another_session_is_rejected(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    manifest_dir = _patch_store(tmp_path, monkeypatch)
    session_a = _manifest(
        session_id="session-a",
        session_genesis_id=_SESSION_A_HEAD,
        sequence_number=1,
        parent_hash=genesis_parent_hash(_SESSION_A_HEAD),
        transcript_version=1,
        marker="a",
    )
    session_b = _manifest(
        session_id="session-b",
        session_genesis_id=_SESSION_B_HEAD,
        sequence_number=1,
        parent_hash=genesis_parent_hash(_SESSION_B_HEAD),
        transcript_version=1,
        marker="b",
    )
    save_retrieval_manifest(session_a)
    save_retrieval_manifest(session_b)
    _write_latest(
        manifest_dir,
        manifest_id=session_a.manifest_id,
        session_genesis_id=_SESSION_A_HEAD,
        sequence_number=1,
    )

    with pytest.raises(ValueError, match=r"(?i)(cross.session|session head|session)"):
        _strict_load_or_expose_acceptance(
            expected_session_head=_SESSION_B_HEAD,
            expected_sequence_number=1,
            replayed_manifest_id=session_a.manifest_id,
            attack="cross-session replay",
        )
