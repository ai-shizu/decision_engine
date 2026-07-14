# -*- coding: utf-8 -*-
"""FSA-2026-07-13-10 RED contracts for cross-runtime ranking determinism."""
from __future__ import annotations

import base64
import json
import math
import struct
import sys
import zlib
from pathlib import Path

import numpy as np
import pytest

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "src" / "python"))

from core import consultation_engine as ce  # noqa: E402
from core.search_daemon import (  # noqa: E402
    SearchDaemonClient,
    SearchDaemonError,
)
from core.score_ranking import rank_hits  # noqa: E402

DIM = 384
LANES = 4
BLOCK_BYTES = DIM * LANES * 4 + LANES * 4
NATIVE_SEARCH_EXE = ROOT / "build" / "search_engine.exe"

# Two float32 vectors measured to rank oppositely under the current ARM NEON
# four-accumulator reduction and NumPy matmul. The NumPy score gap is one ULP,
# 5.960464477539063e-08. Compressed only to keep the contract source readable.
_PERTURBATION_FIXTURE = (
    "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"
)


def _write_index(path: Path, vectors: np.ndarray) -> None:
    vectors = np.asarray(vectors, dtype=np.float32)
    assert vectors.ndim == 2 and vectors.shape[1] == DIM
    n_vectors = len(vectors)
    n_blocks = (n_vectors + LANES - 1) // LANES
    with path.open("wb") as handle:
        handle.write(struct.pack(
            "<8sIIIIII",
            b"PKBVEC01",
            DIM,
            LANES,
            n_vectors,
            n_blocks,
            BLOCK_BYTES,
            0,
        ))
        for block in range(n_blocks):
            lanes = [
                vectors[block * LANES + lane]
                if block * LANES + lane < n_vectors
                else np.zeros(DIM, dtype=np.float32)
                for lane in range(LANES)
            ]
            for dimension in range(DIM):
                handle.write(struct.pack(
                    "<4f",
                    *(float(lanes[lane][dimension]) for lane in range(LANES)),
                ))
            handle.write(struct.pack(
                "<4i",
                *(
                    block * LANES + lane
                    if block * LANES + lane < n_vectors
                    else -1
                    for lane in range(LANES)
                ),
            ))


def _native_search(index: Path, query: np.ndarray, top_k: int) -> list[tuple[int, float]]:
    assert NATIVE_SEARCH_EXE.is_file(), (
        f"native search engine is missing: {NATIVE_SEARCH_EXE}"
    )
    client = SearchDaemonClient(
        exe=NATIVE_SEARCH_EXE,
        scratch_path=index.with_name("scratch.bin"),
    )
    try:
        client.start()
        return client.search(index, np.asarray(query, dtype=np.float32).tobytes(), top_k)
    finally:
        client.close()


def _python_search(
    index: Path,
    query: np.ndarray,
    top_k: int,
    monkeypatch: pytest.MonkeyPatch,
) -> list[dict]:
    metadata = index.with_name("metadata.json")
    raw = index.read_bytes()
    n_vectors = struct.unpack("<8sIIIIII", raw[:32])[3]
    metadata.write_text(
        json.dumps({
            "chunks": [
                {"id": chunk_id, "text": f"chunk-{chunk_id}"}
                for chunk_id in range(n_vectors)
            ]
        }),
        encoding="utf-8",
    )
    monkeypatch.setattr(ce, "SEARCH_EXE", index.with_name("missing-search-engine"))
    engine = ce.ConsultationEngine()
    engine._search_daemon_failed = True
    return engine.search_index(index, metadata, query, top_k=top_k)


def test_equal_score_ties_have_one_chunk_id_total_order_across_engines(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    index = tmp_path / "ties.bin"
    vectors = np.zeros((64, DIM), dtype=np.float32)
    query = np.zeros(DIM, dtype=np.float32)
    _write_index(index, vectors)

    native_ids = [chunk_id for chunk_id, _score in _native_search(index, query, 64)]
    python_ids = [hit["id"] for hit in _python_search(index, query, 64, monkeypatch)]
    canonical_ids = list(range(64))

    assert native_ids == python_ids == canonical_ids, (
        "equal-score order lacks a shared chunk_id tie-break; "
        f"native={native_ids}, python={python_ids}"
    )


def test_sub_1e7_float_drift_cannot_change_top1_membership(
    tmp_path: Path,
) -> None:
    raw = zlib.decompress(base64.b64decode(_PERTURBATION_FIXTURE))
    vectors = np.frombuffer(raw, dtype="<f4").copy().reshape(2, DIM)
    query = np.ones(DIM, dtype=np.float32)
    index = tmp_path / "perturbation.bin"
    _write_index(index, vectors)

    python_scores = vectors @ query
    assert 0.0 < abs(float(python_scores[0] - python_scores[1])) <= 1e-7
    native_top = _native_search(index, query, 1)
    python_top_id = rank_hits(enumerate(python_scores), 1)[0][0]

    assert native_top[0][0] == python_top_id, (
        "sub-1e-7 accumulation drift changed Top-1 membership; "
        f"native={native_top}, python_top={python_top_id}, "
        f"scores={python_scores.tolist()}"
    )


def test_nan_and_infinity_are_rejected_before_ranking(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    vectors = np.zeros((3, DIM), dtype=np.float32)
    vectors[0, 0] = np.nan
    vectors[1, 0] = np.inf
    vectors[2, 0] = 1.0
    query = np.zeros(DIM, dtype=np.float32)
    query[0] = 1.0
    index = tmp_path / "nonfinite.bin"
    _write_index(index, vectors)

    native_rejected = False
    python_rejected = False
    native_result: object = None
    python_result: object = None
    try:
        native_result = _native_search(index, query, 3)
    except SearchDaemonError:
        native_rejected = True
    try:
        python_result = _python_search(index, query, 3, monkeypatch)
    except (FloatingPointError, ValueError, RuntimeError):
        python_rejected = True

    assert native_rejected and python_rejected, (
        "non-finite scores reached ranking instead of hard-failing; "
        f"native={native_result}, python={python_result}, "
        f"native_finite={all(math.isfinite(score) for _, score in native_result or [])}"
    )
