from __future__ import annotations

import sys
from pathlib import Path

import pytest

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "src" / "python"))

from core.dynamic_ordinal_rasch import (  # noqa: E402
    DynamicOrdinalRaschFilter,
    OrdinalItem,
    artifact_sha256,
)

CONTEXT_ITEM_ID = "pq-decision_threshold-CONTEXT-01"
EMOTION_ITEM_ID = "pq-decision_threshold-EMOTION-01"
FACT_ITEM_ID = "pq-decision_threshold-FACT-01"
CONTEXT_THRESHOLDS = (-1.4, -0.4, 0.6, 1.6)
EMOTION_THRESHOLDS = (-1.3, -0.3, 0.7, 1.7)
FACT_THRESHOLDS = (-1.5, -0.5, 0.5, 1.5)
GOLDEN_POSTERIOR_HEX = (
    "0x1.994844197c935p-23",
    "0x1.035db383da7dbp-19",
    "0x1.458d4a64b7c3cp-16",
    "0x1.6d0c5f874b4c2p-13",
    "0x1.5a1c870b15388p-10",
    "0x1.02f163febe071p-7",
    "0x1.1b7001040e089p-5",
    "0x1.a7c6ba022f5f1p-4",
    "0x1.9cf81540295e3p-3",
    "0x1.02833eec7b527p-2",
    "0x1.a8a0824bf63ccp-3",
    "0x1.e2738ac8c1551p-4",
    "0x1.97fc203250895p-5",
    "0x1.154d64e142515p-6",
    "0x1.43f370aa55e74p-8",
    "0x1.5656030810228p-10",
    "0x1.51dffbbfc0148p-12",
)


def _model() -> DynamicOrdinalRaschFilter:
    return DynamicOrdinalRaschFilter(
        items=(
            OrdinalItem(
                item_id=CONTEXT_ITEM_ID,
                thresholds=CONTEXT_THRESHOLDS,
            ),
            OrdinalItem(
                item_id=EMOTION_ITEM_ID,
                thresholds=EMOTION_THRESHOLDS,
            ),
            OrdinalItem(item_id=FACT_ITEM_ID, thresholds=FACT_THRESHOLDS),
        )
    )


def test_ordered_threshold_probabilities_are_complete_and_strict() -> None:
    model = _model()
    probabilities = model.response_probabilities(item_id=FACT_ITEM_ID, ability=0.0)

    assert len(probabilities) == 5
    assert all(probability > 0.0 for probability in probabilities)
    assert sum(probabilities) == pytest.approx(
        1.0,
        rel=0.0,
        abs=8.0 * sys.float_info.epsilon,
    )
    with pytest.raises(ValueError, match="strictly increasing"):
        OrdinalItem(item_id="invalid", thresholds=(-0.5, -0.5))
    with pytest.raises(ValueError, match="not defined by the model artifact"):
        DynamicOrdinalRaschFilter(
            items=(
                OrdinalItem(
                    item_id="unversioned",
                    thresholds=FACT_THRESHOLDS,
                ),
            )
        )


def test_golden_posterior_and_eig_are_bit_exact() -> None:
    model = _model()
    posterior = model.initial_posterior()
    posterior = model.update(posterior, item_id=FACT_ITEM_ID, response=4)
    posterior = model.update(posterior, item_id=FACT_ITEM_ID, response=1)
    selection = model.select_next(
        posterior,
        excluded_item_ids=frozenset({FACT_ITEM_ID}),
    )

    assert tuple(value.hex() for value in posterior) == GOLDEN_POSTERIOR_HEX
    assert selection.item_id == EMOTION_ITEM_ID
    assert selection.eig.hex() == "0x1.b2a3b7eca9fb0p-3"
    assert selection.quantized_eig == 212226
    assert model.identity == artifact_sha256()
    assert model.identity == "fb696c4c9811f564bfd80d37a501b06e2d46bf25c82d9306c6b24988a3465ebe"
