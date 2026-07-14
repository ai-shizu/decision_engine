# -*- coding: utf-8 -*-
"""FSA-2026-07-13-09 RED contracts for derived-store integrity."""
from __future__ import annotations

import json
import sys
from pathlib import Path

import pytest

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "src" / "python"))

from core import lsm_index, tensor_store  # noqa: E402


class _FakeSearchEngine:
    name = "fsa09-fake"

    def __init__(self) -> None:
        self.embedder = self
        self.searched_paths: list[Path] = []

    def search_index(self, bin_path, meta_path, qvec, top_k=3):
        self.searched_paths.append(Path(bin_path).resolve())
        return []


def _install_manifest(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
    segment_name: str,
) -> tuple[Path, _FakeSearchEngine]:
    processed = tmp_path / "processed"
    processed.mkdir()
    manifest_path = processed / "segments.json"
    metadata_path = processed / "metadata.json"
    engine = _FakeSearchEngine()
    manifest = {
        "format": lsm_index.MANIFEST_FORMAT,
        "embedder_id": lsm_index.embedder_id(engine.embedder),
        "next_chunk_id": 1,
        "segments": [{
            "file": segment_name,
            "live": 1,
            "dead": 0,
            "payload_hash": "blake2b-256:" + "0" * 64,
        }],
        "days": {},
    }
    manifest_path.write_text(json.dumps(manifest), encoding="utf-8")
    metadata_path.write_text("{}", encoding="utf-8")
    monkeypatch.setattr(lsm_index, "PROCESSED", processed)
    monkeypatch.setattr(lsm_index, "LSM_MANIFEST", manifest_path)
    monkeypatch.setattr(lsm_index, "DIARY_META", metadata_path)
    return processed, engine


def test_lsm_rejects_segment_path_traversal_before_external_read(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    processed, engine = _install_manifest(tmp_path, monkeypatch, "../outside.bin")
    outside = tmp_path / "outside.bin"
    outside.write_bytes(b"attacker-controlled segment")

    with pytest.raises(ValueError):
        lsm_index.search_lsm(engine, qvec=None)

    assert outside.read_bytes() == b"attacker-controlled segment"
    assert outside.resolve() not in engine.searched_paths
    assert outside.resolve() != processed.resolve()


def test_lsm_missing_declared_segment_hard_fails_instead_of_partial_results(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    _processed, engine = _install_manifest(
        tmp_path,
        monkeypatch,
        "vectors.seg-000001.bin",
    )

    with pytest.raises(ValueError):
        lsm_index.search_lsm(engine, qvec=None)

    assert engine.searched_paths == []


def test_lsm_payload_hash_mismatch_fails_before_search(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    processed = tmp_path / "processed"
    processed.mkdir()
    segment_path = processed / "vectors.seg-000001.bin"
    segment_path.write_bytes(b"original segment payload")
    manifest_path = processed / "segments.json"
    metadata_path = processed / "metadata.json"
    engine = _FakeSearchEngine()
    manifest = {
        "format": lsm_index.MANIFEST_FORMAT,
        "embedder_id": lsm_index.embedder_id(engine.embedder),
        "next_chunk_id": 1,
        "segments": [{
            "file": segment_path.name,
            "live": 1,
            "dead": 0,
            "payload_hash": lsm_index.segment_payload_hash(segment_path),
        }],
        "days": {},
    }
    manifest_path.write_text(json.dumps(manifest), encoding="utf-8")
    metadata_path.write_text("{}", encoding="utf-8")
    monkeypatch.setattr(lsm_index, "PROCESSED", processed)
    monkeypatch.setattr(lsm_index, "LSM_MANIFEST", manifest_path)
    monkeypatch.setattr(lsm_index, "DIARY_META", metadata_path)
    segment_path.write_bytes(b"original segment payloae")

    with pytest.raises(lsm_index.DerivedStoreIntegrityError):
        lsm_index.search_lsm(engine, qvec=None)

    assert engine.searched_paths == []


def test_tensor_identity_binds_dyads_and_scope(tmp_path: Path) -> None:
    daily = [{
        "date": "2026-07-14",
        "diary_text": "same canonical daily input",
        "calendar_events": [],
        "transactions": [],
        "consultations": [],
        "line_self_text": "",
        "sources": ["diary"],
    }]
    first = tensor_store.build_tensor(
        daily,
        [{"contact": "alice", "messages": 1}],
        tmp_path / "first.bin",
        scope="global",
    )
    different_dyad = tensor_store.build_tensor(
        daily,
        [{"contact": "bob", "messages": 99}],
        tmp_path / "different_dyad.bin",
        scope="global",
    )
    different_scope = tensor_store.build_tensor(
        daily,
        [{"contact": "alice", "messages": 1}],
        tmp_path / "different_scope.bin",
        scope="dyad",
        alias="alice",
        contact="alice",
    )

    stores = [
        tensor_store.TensorStore(first),
        tensor_store.TensorStore(different_dyad),
        tensor_store.TensorStore(different_scope),
    ]
    try:
        identities = {store.content_hash64 for store in stores}
        assert len(identities) == 3, (
            "content_hash64 collides when canonical dyads or scope changes: "
            f"{[store.content_hash64 for store in stores]}"
        )
    finally:
        for store in stores:
            store.close()


def test_tensor_reader_rejects_one_byte_payload_tamper(tmp_path: Path) -> None:
    daily = [{
        "date": "2026-07-14",
        "diary_text": "payload integrity",
        "calendar_events": [],
        "transactions": [],
        "consultations": [],
        "line_self_text": "",
        "sources": ["diary"],
    }]
    path = tensor_store.build_tensor(daily, None, tmp_path / "tensor.bin")
    tampered = bytearray(path.read_bytes())
    tampered[-1] ^= 0x01
    path.write_bytes(tampered)

    with pytest.raises(tensor_store.TensorStoreError, match="payload hash mismatch"):
        tensor_store.TensorStore(path)
