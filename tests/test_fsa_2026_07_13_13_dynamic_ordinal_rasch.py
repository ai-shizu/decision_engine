from __future__ import annotations

import math
import struct
import sys
from pathlib import Path

import pytest

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "src" / "python"))


def _load_subject():
    try:
        from core import dynamic_ordinal_rasch
    except ImportError:
        pytest.fail(
            "Dynamic Ordinal Rasch Filter is absent; no identifiable skill model exists",
            pytrace=False,
        )
    return dynamic_ordinal_rasch


def _two_pl_probability(theta: float, discrimination: float, difficulty: float) -> float:
    logit = discrimination * (theta - difficulty)
    return 1.0 / (1.0 + math.exp(-logit))


def test_unanchored_2pl_shift_scale_invariance_is_replaced_by_fixed_rasch_scale() -> None:
    theta = 1.25
    discrimination = 1.5
    difficulty = -0.75
    scale = 2.0
    shift = 0.5

    original = _two_pl_probability(theta, discrimination, difficulty)
    transformed = _two_pl_probability(
        scale * theta + shift,
        discrimination / scale,
        scale * difficulty + shift,
    )
    assert original == transformed, "unanchored 2PL must expose scale non-identifiability"

    dor = _load_subject()
    item = dor.OrdinalItem(
        item_id="anchor-item",
        thresholds=(-1.5, -0.5, 0.5, 1.5),
    )
    model = dor.DynamicOrdinalRaschFilter(items=(item,))

    assert model.schema == "dynamic_ordinal_rasch.v1"
    assert model.discrimination == 1.0
    assert tuple(model.ability_grid) == tuple(dor.CANONICAL_ABILITY_GRID)
    assert len(model.identity) == 64
    with pytest.raises(TypeError):
        dor.OrdinalItem(
            item_id="forbidden-2pl-item",
            thresholds=(-1.5, -0.5, 0.5, 1.5),
            discrimination=2.0,
        )


def test_posterior_and_eig_policy_are_bit_reproducible_with_total_order() -> None:
    dor = _load_subject()
    thresholds = (-1.5, -0.5, 0.5, 1.5)
    model = dor.DynamicOrdinalRaschFilter(
        items=(
            dor.OrdinalItem(item_id="item-b", thresholds=thresholds),
            dor.OrdinalItem(item_id="item-a", thresholds=thresholds),
            dor.OrdinalItem(item_id="item-observed", thresholds=thresholds),
        )
    )
    observations = (("item-observed", 4), ("item-observed", 1))

    outputs: list[tuple[tuple[bytes, ...], str, bytes]] = []
    for _ in range(8):
        posterior = model.initial_posterior()
        for item_id, response in observations:
            posterior = model.update(posterior, item_id=item_id, response=response)
        selection = model.select_next(
            posterior,
            excluded_item_ids=frozenset({"item-observed"}),
        )
        outputs.append(
            (
                tuple(struct.pack(">d", probability) for probability in posterior),
                selection.item_id,
                struct.pack(">d", selection.eig),
            )
        )

    assert len(set(outputs)) == 1, "same evidence must yield bit-identical posterior and EIG"
    assert outputs[0][1] == "item-a", "equal EIG must use item_id ascending tie-break"
