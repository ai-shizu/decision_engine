"""Deterministic fixed-grid Dynamic Ordinal Rasch inference."""

from __future__ import annotations

import hashlib
import json
import math
import sys
from dataclasses import dataclass
from functools import lru_cache
from pathlib import Path
from types import MappingProxyType
from typing import Any, Iterable, Mapping, Sequence

from .canonicalization import canonical_json_bytes

_ARTIFACT_PATH = (
    Path(__file__).resolve().parent
    / "artifacts"
    / "dynamic_ordinal_rasch.v1.json"
)
_SCHEMA = "dynamic_ordinal_rasch.v1"
_TOP_LEVEL_KEYS = frozenset(
    {
        "ability_grid",
        "discrimination",
        "eig_quantization",
        "items",
        "numeric_contract",
        "prior_weights",
        "schema",
        "transition",
    }
)


def _finite_float(value: Any, field: str) -> float:
    if type(value) not in {int, float}:
        raise ValueError(f"{field} must be a finite number")
    result = float(value)
    if not math.isfinite(result):
        raise ValueError(f"{field} must be a finite number")
    return result


def _positive_int(value: Any, field: str) -> int:
    if type(value) is not int or value <= 0:
        raise ValueError(f"{field} must be a positive integer")
    return value


@dataclass(frozen=True)
class OrdinalItem:
    """One fixed-discrimination Rasch item with ordered category thresholds."""

    item_id: str
    thresholds: tuple[float, ...]

    def __post_init__(self) -> None:
        if type(self.item_id) is not str or not self.item_id.strip():
            raise ValueError("item_id must be non-empty str")
        if type(self.thresholds) is not tuple or not self.thresholds:
            raise ValueError("thresholds must be a non-empty tuple")
        normalized = tuple(
            _finite_float(value, f"thresholds[{index}]")
            for index, value in enumerate(self.thresholds)
        )
        if any(left >= right for left, right in zip(normalized, normalized[1:])):
            raise ValueError("thresholds must be strictly increasing")
        object.__setattr__(self, "thresholds", normalized)


@dataclass(frozen=True)
class ItemSelection:
    item_id: str
    eig: float
    quantized_eig: int


@dataclass(frozen=True)
class _ModelArtifact:
    raw_sha256: str
    ability_grid: tuple[float, ...]
    discrimination: float
    prior: tuple[float, ...]
    transition: tuple[tuple[float, ...], ...]
    eig_quantization_factor: int
    items: tuple[OrdinalItem, ...]


def _exact_keys(value: Any, expected: frozenset[str], field: str) -> dict[str, Any]:
    if type(value) is not dict:
        raise ValueError(f"{field} must be an object")
    keys = set(value)
    missing = sorted(expected - keys)
    if missing:
        raise ValueError(f"{field}.{missing[0]} is required")
    extra = sorted(keys - expected)
    if extra:
        raise ValueError(f"{field}.{extra[0]} is not allowed")
    return value


def _normalized(values: Sequence[float], field: str) -> tuple[float, ...]:
    if not values or any(value < 0.0 or not math.isfinite(value) for value in values):
        raise ValueError(f"{field} must contain finite non-negative values")
    total = math.fsum(values)
    if total <= 0.0 or not math.isfinite(total):
        raise ValueError(f"{field} must have positive finite mass")
    return tuple(value / total for value in values)


def _transition_matrix(
    size: int,
    transition: Mapping[str, Any],
) -> tuple[tuple[float, ...], ...]:
    payload = _exact_keys(
        transition,
        frozenset(
            {
                "adjacent_probability",
                "bandwidth",
                "edge_policy",
                "stay_probability",
            }
        ),
        "transition",
    )
    if payload["bandwidth"] != 1:
        raise ValueError("transition.bandwidth must be 1 for this schema")
    if payload["edge_policy"] != "reflect":
        raise ValueError("transition.edge_policy must be reflect")
    stay = _finite_float(payload["stay_probability"], "transition.stay_probability")
    adjacent = _finite_float(
        payload["adjacent_probability"], "transition.adjacent_probability"
    )
    if stay < 0.0 or adjacent < 0.0 or stay + 2.0 * adjacent != 1.0:
        raise ValueError("transition probabilities must define exact unit mass")

    rows: list[tuple[float, ...]] = []
    for source in range(size):
        row = [0.0] * size
        row[source] = stay
        if source == 0:
            row[source] += adjacent
        else:
            row[source - 1] = adjacent
        if source == size - 1:
            row[source] += adjacent
        else:
            row[source + 1] = adjacent
        if math.fsum(row) != 1.0:
            raise ValueError("transition row must have exact unit mass")
        rows.append(tuple(row))
    return tuple(rows)


@lru_cache(maxsize=1)
def _load_artifact() -> _ModelArtifact:
    try:
        raw = _ARTIFACT_PATH.read_bytes()
    except OSError as exc:
        raise ValueError("ordinal Rasch artifact is unavailable") from exc
    try:
        payload = json.loads(raw)
    except (UnicodeDecodeError, json.JSONDecodeError) as exc:
        raise ValueError("ordinal Rasch artifact is invalid JSON") from exc
    payload = _exact_keys(payload, _TOP_LEVEL_KEYS, "artifact")
    if raw != canonical_json_bytes(payload) + b"\n":
        raise ValueError("ordinal Rasch artifact must use canonical JSON with LF")
    if payload["schema"] != _SCHEMA:
        raise ValueError("unsupported ordinal Rasch artifact schema")
    if payload["discrimination"] != 1.0:
        raise ValueError("Rasch discrimination must be anchored at 1.0")

    numeric = _exact_keys(
        payload["numeric_contract"],
        frozenset(
            {
                "float",
                "libm",
                "reduction_order",
                "rounding_mode",
                "update",
            }
        ),
        "numeric_contract",
    )
    if numeric != {
        "float": "ieee754-binary64",
        "libm": "runtime-fingerprint-v1",
        "reduction_order": "ascending-index",
        "rounding_mode": "round-to-nearest-ties-even",
        "update": "log-sum-exp-v1",
    }:
        raise ValueError("unsupported ordinal Rasch numeric contract")
    if (
        sys.float_info.radix != 2
        or sys.float_info.mant_dig != 53
        or sys.float_info.max_exp != 1024
        or sys.float_info.rounds != 1
    ):
        raise ValueError(
            "ordinal Rasch requires IEEE-754 binary64 round-to-nearest-ties-even"
        )

    grid_value = payload["ability_grid"]
    if type(grid_value) is not list or len(grid_value) < 3:
        raise ValueError("ability_grid must contain at least three points")
    grid = tuple(
        _finite_float(value, f"ability_grid[{index}]")
        for index, value in enumerate(grid_value)
    )
    if any(left >= right for left, right in zip(grid, grid[1:])):
        raise ValueError("ability_grid must be strictly increasing")
    if 0.0 not in grid or grid[0] != -grid[-1]:
        raise ValueError("ability_grid must be fixed around the zero anchor")

    prior_value = payload["prior_weights"]
    if type(prior_value) is not list or len(prior_value) != len(grid):
        raise ValueError("prior_weights must align with ability_grid")
    prior = _normalized(
        tuple(
            _finite_float(value, f"prior_weights[{index}]")
            for index, value in enumerate(prior_value)
        ),
        "prior_weights",
    )

    quantization = _exact_keys(
        payload["eig_quantization"],
        frozenset({"factor", "rounding"}),
        "eig_quantization",
    )
    factor = _positive_int(quantization["factor"], "eig_quantization.factor")
    if quantization["rounding"] != "floor_half_up":
        raise ValueError("eig_quantization.rounding must be floor_half_up")

    item_values = payload["items"]
    if type(item_values) is not list or not item_values:
        raise ValueError("items must be a non-empty list")
    items: list[OrdinalItem] = []
    for index, item_value in enumerate(item_values):
        item_payload = _exact_keys(
            item_value,
            frozenset({"item_id", "thresholds"}),
            f"items[{index}]",
        )
        thresholds = item_payload["thresholds"]
        if type(thresholds) is not list:
            raise ValueError(f"items[{index}].thresholds must be a list")
        items.append(
            OrdinalItem(
                item_id=item_payload["item_id"],
                thresholds=tuple(thresholds),
            )
        )
    if [item.item_id for item in items] != sorted(item.item_id for item in items):
        raise ValueError("artifact items must be sorted by item_id")
    if len({item.item_id for item in items}) != len(items):
        raise ValueError("artifact item_id values must be unique")
    if len({item.thresholds for item in items}) != len(items):
        raise ValueError("artifact item thresholds must be unique")

    return _ModelArtifact(
        raw_sha256=hashlib.sha256(raw).hexdigest(),
        ability_grid=grid,
        discrimination=1.0,
        prior=prior,
        transition=_transition_matrix(len(grid), payload["transition"]),
        eig_quantization_factor=factor,
        items=tuple(items),
    )


def artifact_sha256() -> str:
    """Return the hash that must be bound into CanonicalRuntimeIdentity."""

    return _load_artifact().raw_sha256


CANONICAL_ABILITY_GRID = _load_artifact().ability_grid


def _log_sum_exp(values: Iterable[float]) -> float:
    ordered = tuple(values)
    if not ordered:
        raise ValueError("log-sum-exp requires at least one value")
    maximum = max(ordered)
    if maximum == -math.inf:
        return -math.inf
    return maximum + math.log(math.fsum(math.exp(value - maximum) for value in ordered))


class DynamicOrdinalRaschFilter:
    """Exact deterministic Bayesian filtering on an anchored ability grid."""

    schema = _SCHEMA

    def __init__(self, *, items: Sequence[OrdinalItem] | None = None) -> None:
        artifact = _load_artifact()
        authoritative_items = {item.item_id: item for item in artifact.items}
        selected = tuple(artifact.items if items is None else items)
        if not selected:
            raise ValueError("items must not be empty")
        for item in selected:
            if type(item) is not OrdinalItem:
                raise ValueError("items must contain OrdinalItem values")
            if authoritative_items.get(item.item_id) != item:
                raise ValueError(
                    f"item {item.item_id!r} is not defined by the model artifact"
                )
        if len({item.item_id for item in selected}) != len(selected):
            raise ValueError("item_id values must be unique")

        self.ability_grid = artifact.ability_grid
        self.discrimination = artifact.discrimination
        self.identity = artifact.raw_sha256
        self.items = tuple(sorted(selected, key=lambda item: item.item_id))
        self._prior = artifact.prior
        self._transition = artifact.transition
        self._quantization_factor = artifact.eig_quantization_factor
        self._items_by_id = MappingProxyType(
            {item.item_id: item for item in self.items}
        )

    def initial_posterior(self) -> tuple[float, ...]:
        return self._prior

    def response_probabilities(
        self,
        *,
        item_id: str,
        ability: float,
    ) -> tuple[float, ...]:
        item = self._item(item_id)
        theta = _finite_float(ability, "ability")

        # Partial Credit Model / adjacent-category Rasch logits.  Category zero
        # is the reference category.  Building each cumulative threshold sum in
        # ascending index order is part of the versioned numeric contract.
        logits = [0.0]
        cumulative_threshold = 0.0
        for category, threshold in enumerate(item.thresholds, start=1):
            cumulative_threshold += threshold
            logits.append(category * theta - cumulative_threshold)
        if any(not math.isfinite(logit) for logit in logits):
            raise ArithmeticError("ordinal likelihood logits are not finite")

        log_normalizer = _log_sum_exp(logits)
        probabilities = tuple(
            math.exp(logit - log_normalizer) for logit in logits
        )
        if any(value <= 0.0 or not math.isfinite(value) for value in probabilities):
            raise ArithmeticError("ordinal likelihood is not finite positive mass")
        if not math.isclose(
            math.fsum(probabilities),
            1.0,
            rel_tol=0.0,
            abs_tol=8.0 * sys.float_info.epsilon,
        ):
            raise ArithmeticError("ordinal likelihood does not have unit mass")
        return probabilities

    def update(
        self,
        posterior: Sequence[float],
        *,
        item_id: str,
        response: int,
    ) -> tuple[float, ...]:
        belief = self._validate_belief(posterior)
        item = self._item(item_id)
        if type(response) is not int or not 0 <= response <= len(item.thresholds):
            raise ValueError(
                f"response must be an integer from 0 to {len(item.thresholds)}"
            )

        log_belief = tuple(math.log(value) if value > 0.0 else -math.inf for value in belief)
        log_prediction: list[float] = []
        for target in range(len(self.ability_grid)):
            terms = (
                log_belief[source] + math.log(self._transition[source][target])
                for source in range(len(self.ability_grid))
                if self._transition[source][target] > 0.0
            )
            log_prediction.append(_log_sum_exp(terms))

        log_joint = tuple(
            log_prediction[index]
            + math.log(
                self.response_probabilities(
                    item_id=item_id,
                    ability=ability,
                )[response]
            )
            for index, ability in enumerate(self.ability_grid)
        )
        log_evidence = _log_sum_exp(log_joint)
        if not math.isfinite(log_evidence):
            raise ArithmeticError("ordinal response has zero or invalid evidence")
        return tuple(math.exp(value - log_evidence) for value in log_joint)

    def expected_information_gain(
        self,
        posterior: Sequence[float],
        *,
        item_id: str,
    ) -> float:
        belief = self._validate_belief(posterior)
        item = self._item(item_id)
        category_count = len(item.thresholds) + 1
        conditional = tuple(
            self.response_probabilities(item_id=item_id, ability=ability)
            for ability in self.ability_grid
        )
        marginal = tuple(
            math.fsum(
                belief[index] * conditional[index][category]
                for index in range(len(belief))
            )
            for category in range(category_count)
        )
        marginal_entropy = self._entropy(marginal)
        conditional_entropy = math.fsum(
            belief[index] * self._entropy(conditional[index])
            for index in range(len(belief))
        )
        eig = marginal_entropy - conditional_entropy
        if eig < 0.0 and eig > -1e-15:
            eig = 0.0
        if eig < 0.0 or not math.isfinite(eig):
            raise ArithmeticError("expected information gain must be finite non-negative")
        return eig

    def select_next(
        self,
        posterior: Sequence[float],
        *,
        excluded_item_ids: frozenset[str] = frozenset(),
    ) -> ItemSelection:
        if type(excluded_item_ids) is not frozenset or any(
            type(item_id) is not str for item_id in excluded_item_ids
        ):
            raise ValueError("excluded_item_ids must be a frozenset of strings")
        candidates: list[ItemSelection] = []
        for item in self.items:
            if item.item_id in excluded_item_ids:
                continue
            eig = self.expected_information_gain(posterior, item_id=item.item_id)
            quantized = math.floor(eig * self._quantization_factor + 0.5)
            candidates.append(
                ItemSelection(
                    item_id=item.item_id,
                    eig=eig,
                    quantized_eig=quantized,
                )
            )
        if not candidates:
            raise ValueError("no candidate items remain")
        return min(
            candidates,
            key=lambda candidate: (-candidate.quantized_eig, candidate.item_id),
        )

    def _item(self, item_id: str) -> OrdinalItem:
        if type(item_id) is not str or item_id not in self._items_by_id:
            raise ValueError(f"unknown item_id: {item_id!r}")
        return self._items_by_id[item_id]

    def _validate_belief(self, posterior: Sequence[float]) -> tuple[float, ...]:
        if type(posterior) not in {tuple, list} or len(posterior) != len(
            self.ability_grid
        ):
            raise ValueError("posterior must align with ability_grid")
        belief = tuple(
            _finite_float(value, f"posterior[{index}]")
            for index, value in enumerate(posterior)
        )
        if any(value < 0.0 for value in belief):
            raise ValueError("posterior must contain non-negative mass")
        total = math.fsum(belief)
        if not math.isclose(total, 1.0, rel_tol=0.0, abs_tol=1e-12):
            raise ValueError("posterior must have unit mass")
        return belief

    @staticmethod
    def _entropy(probabilities: Sequence[float]) -> float:
        return -math.fsum(
            probability * math.log(probability)
            for probability in probabilities
            if probability > 0.0
        )
