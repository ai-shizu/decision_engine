"""Cross-runtime deterministic score quantization and ranking."""
from __future__ import annotations

import math
from collections.abc import Iterable

SCORE_SCALE = 1_000_000
_INT64_MIN = -(1 << 63)
_INT64_MAX = (1 << 63) - 1


class RankingAnomalyError(ValueError):
    """Raised when a score cannot participate in deterministic ranking."""


def quantize_score(score: float) -> int:
    """Quantize a finite score with floor(score * 1e6 + 0.5)."""
    value = float(score)
    if not math.isfinite(value):
        raise RankingAnomalyError("search score must be finite")
    quantized = math.floor(value * SCORE_SCALE + 0.5)
    if not _INT64_MIN <= quantized <= _INT64_MAX:
        raise RankingAnomalyError("quantized search score exceeds int64 range")
    return quantized


def score_order_key(score: float, chunk_id: int) -> tuple[int, int]:
    """Return the shared total-order key: descending score, ascending ID."""
    return (-quantize_score(score), int(chunk_id))


def rank_hits(
    hits: Iterable[tuple[int, float]],
    top_k: int,
) -> list[tuple[int, float]]:
    """Validate every score, then rank by ``(-quantized_score, chunk_id)``."""
    if top_k < 0:
        raise ValueError("top_k must be non-negative")
    ranked: list[tuple[int, int, float]] = []
    for chunk_id, score in hits:
        canonical_id = int(chunk_id)
        canonical_score = float(score)
        ranked.append((
            -quantize_score(canonical_score),
            canonical_id,
            canonical_score,
        ))
    ranked.sort(key=lambda item: (item[0], item[1]))
    return [(chunk_id, score) for _score_q, chunk_id, score in ranked[:top_k]]
