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

THRESHOLDS = (-1.5, -0.5, 0.5, 1.5)
GOLDEN_POSTERIOR_HEX = (
    "0x1.616fdfbe7812cp-11",
    "0x1.aba7a9298c072p-10",
    "0x1.0477985fb25a3p-8",
    "0x1.2b5cbd4b5d47fp-7",
    "0x1.3d82ed653dbd0p-6",
    "0x1.2fa9171dc73c4p-5",
    "0x1.00c39eef1262dp-4",
    "0x1.7b424a7d3ca7ap-4",
    "0x1.e7c6c0bcf3a32p-4",
    "0x1.12630806eaf8bp-3",
    "0x1.10979d669be74p-3",
    "0x1.e41619f51ac79p-4",
    "0x1.85b79d4beb6fbp-4",
    "0x1.2135126f064ffp-4",
    "0x1.92bdb6caf5e04p-5",
    "0x1.0b96f45ed8532p-5",
    "0x1.56dc54ebfbd7bp-6",
)


def _model() -> DynamicOrdinalRaschFilter:
    return DynamicOrdinalRaschFilter(
        items=(
            OrdinalItem(item_id="item-a", thresholds=THRESHOLDS),
            OrdinalItem(item_id="item-b", thresholds=THRESHOLDS),
            OrdinalItem(item_id="item-observed", thresholds=THRESHOLDS),
        )
    )


def test_ordered_threshold_probabilities_are_complete_and_strict() -> None:
    model = _model()
    probabilities = model.response_probabilities(item_id="item-a", ability=0.0)

    assert len(probabilities) == 5
    assert all(probability > 0.0 for probability in probabilities)
    assert sum(probabilities) == 1.0
    with pytest.raises(ValueError, match="strictly increasing"):
        OrdinalItem(item_id="invalid", thresholds=(-0.5, -0.5))
    with pytest.raises(ValueError, match="not defined by the model artifact"):
        DynamicOrdinalRaschFilter(
            items=(OrdinalItem(item_id="unversioned", thresholds=THRESHOLDS),)
        )


def test_golden_posterior_and_eig_are_bit_exact() -> None:
    model = _model()
    posterior = model.initial_posterior()
    posterior = model.update(posterior, item_id="item-observed", response=4)
    posterior = model.update(posterior, item_id="item-observed", response=1)
    selection = model.select_next(
        posterior,
        excluded_item_ids=frozenset({"item-observed"}),
    )

    assert tuple(value.hex() for value in posterior) == GOLDEN_POSTERIOR_HEX
    assert selection.item_id == "item-a"
    assert selection.eig.hex() == "0x1.c997a336a89b8p-3"
    assert selection.quantized_eig == 223434
    assert model.identity == artifact_sha256()
    assert model.identity == "d693480e8a4c5f5a5fb5e37328e59ad2f90e1833c6ff95348a8548b5fe638593"
