#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""Six-dimensional tensor profile evaluator (SPEC_ENGINE_TENSOR_PROFILING.md §6)."""
from __future__ import annotations

import hashlib
import json
from dataclasses import dataclass
from decimal import Decimal, ROUND_HALF_UP
from typing import Any, Literal

DimensionId = Literal[
    "problem_structuring",
    "quantitative_rigor",
    "hypothesis_evidence",
    "synthesis_judgment",
    "communication",
    "collaboration_adaptability",
]

CANONICAL_DIMENSION_IDS: tuple[DimensionId, ...] = (
    "problem_structuring",
    "quantitative_rigor",
    "hypothesis_evidence",
    "synthesis_judgment",
    "communication",
    "collaboration_adaptability",
)

CALCULUS_AXIS_MAP: dict[str, str] = {
    "problem_structuring": "Structural_Decomposition",
    "quantitative_rigor": "Quantitative_Agility",
    "hypothesis_evidence": "Logical_Rigor",
    "synthesis_judgment": "Domain_Adaptability",
    "communication": "Communication_Bandwidth",
    "collaboration_adaptability": "Cognitive_Flexibility",
}

INDICATOR_IDS: dict[DimensionId, tuple[str, str, str]] = {
    "problem_structuring": (
        "ps_clarify_objective",
        "ps_decompose",
        "ps_prioritize",
    ),
    "quantitative_rigor": (
        "qr_units_assumptions",
        "qr_calculations",
        "qr_interpret_data",
    ),
    "hypothesis_evidence": (
        "he_form_hypothesis",
        "he_disconfirm",
        "he_update_facts",
    ),
    "synthesis_judgment": (
        "sj_implications",
        "sj_tradeoffs",
        "sj_recommendations",
    ),
    "communication": (
        "cm_signposting",
        "cm_conclusions",
        "cm_delivery",
    ),
    "collaboration_adaptability": (
        "ca_listens",
        "ca_builds_on",
        "ca_handles_challenge",
    ),
}

CANDIDATE_ALIAS = "candidate"
FIRST_FIVE_DIMENSIONS = CANONICAL_DIMENSION_IDS[:5]

TENSOR_PROFILE_SCHEMA: dict[str, Any] = {
    "type": "object",
    "additionalProperties": False,
    "required": ["evidence"],
    "properties": {
        "evidence": {
            "type": "array",
            "items": {
                "type": "object",
                "additionalProperties": False,
                "required": [
                    "dimension_id",
                    "indicator_id",
                    "level",
                    "turn_id",
                    "turn_index",
                    "speaker_alias",
                    "quote",
                ],
                "properties": {
                    "dimension_id": {"type": "string"},
                    "indicator_id": {"type": "string"},
                    "level": {"type": "integer", "minimum": 0, "maximum": 4},
                    "turn_id": {"type": "string"},
                    "turn_index": {"type": "integer", "minimum": 0},
                    "speaker_alias": {"type": "string"},
                    "quote": {"type": "string", "maxLength": 120},
                },
            },
        }
    },
}


@dataclass(frozen=True)
class TensorEvidence:
    evidence_id: str
    dimension_id: DimensionId
    indicator_id: str
    level: int
    turn_id: str
    turn_index: int
    speaker_alias: str
    quote: str


@dataclass(frozen=True)
class TensorDimension:
    dimension_id: DimensionId
    calculus_axis: str
    score: float | None
    confidence: float
    evidence: tuple[TensorEvidence, ...]


@dataclass(frozen=True)
class TensorProfile6D:
    schema: Literal["tensor_profile.6d.v1"]
    session_id: str
    transcript_hash: str
    model_hash: str
    prompt_version: str
    dimensions: tuple[TensorDimension, ...]


def _round2(value: float) -> float:
    return float(Decimal(str(value)).quantize(Decimal("0.01"), rounding=ROUND_HALF_UP))


def transcript_hash(turns: list[dict]) -> str:
    payload = json.dumps(turns, ensure_ascii=False, sort_keys=True).encode("utf-8")
    return hashlib.blake2b(payload, digest_size=16).hexdigest()


def validate_quote(quote: str, turn: dict) -> tuple[bool, str]:
    if type(quote) is not str:
        return False, "quote must be str"
    clean_quote = quote.strip()
    if not clean_quote:
        return False, "empty quote"
    if len(clean_quote) > 120:
        return False, "quote too long"
    text = turn.get("text", "")
    if clean_quote not in text:
        return False, "quote not exact substring"
    return True, ""


def _require_int(name: str, value: object) -> int:
    if type(value) is not int or isinstance(value, bool):
        raise ValueError(f"invalid {name}")
    return value


def _require_str(name: str, value: object) -> str:
    if type(value) is not str:
        raise ValueError(f"invalid {name}")
    return value


def _evidence_id(item: dict) -> str:
    payload = json.dumps(item, ensure_ascii=False, sort_keys=True).encode("utf-8")
    return hashlib.blake2b(payload, digest_size=16).hexdigest()


def parse_and_validate_proposals(
    raw: dict,
    turns: list[dict],
    *,
    allow_duplicate_dimensions: bool = False,
) -> list[dict]:
    if not isinstance(raw, dict):
        raise ValueError("payload must be object")
    items = raw.get("evidence")
    if not isinstance(items, list):
        raise ValueError("evidence must be array")

    turn_by_id = {t["turn_id"]: t for t in turns}
    seen_dim_indicator: set[tuple[str, str]] = set()
    seen_dim_only: set[str] = set()
    accepted: list[dict] = []

    for item in items:
        if not isinstance(item, dict):
            raise ValueError("evidence item must be object")
        dim = _require_str("dimension_id", item.get("dimension_id")).strip()
        indicator = _require_str("indicator_id", item.get("indicator_id")).strip()
        if dim not in CANONICAL_DIMENSION_IDS:
            raise ValueError(f"unknown dimension_id: {dim}")
        if indicator not in INDICATOR_IDS[dim]:  # type: ignore[index]
            raise ValueError(f"unknown indicator_id: {indicator}")
        key = (dim, indicator)
        if key in seen_dim_indicator:
            raise ValueError("duplicate dimension/indicator pair")
        seen_dim_indicator.add(key)
        if not allow_duplicate_dimensions and dim in seen_dim_only:
            raise ValueError("duplicate dimension evidence")
        seen_dim_only.add(dim)

        level = _require_int("level", item.get("level"))
        if level < 0 or level > 4:
            raise ValueError("level out of range")

        turn_id = _require_str("turn_id", item.get("turn_id")).strip()
        turn = turn_by_id.get(turn_id)
        if turn is None:
            raise ValueError("unknown turn_id")
        turn_index = _require_int("turn_index", item.get("turn_index"))
        if turn_index != turn["turn_index"]:
            raise ValueError("turn_index mismatch")

        alias = _require_str("speaker_alias", item.get("speaker_alias")).strip()
        if dim in FIRST_FIVE_DIMENSIONS and alias != CANDIDATE_ALIAS:
            raise ValueError("first five dimensions require candidate evidence")
        if alias == CANDIDATE_ALIAS and turn["speaker_alias"] != CANDIDATE_ALIAS:
            raise ValueError("candidate alias on non-candidate turn")

        raw_quote = item.get("quote")
        if type(raw_quote) is not str:
            raise ValueError("invalid quote")
        clean_quote = raw_quote.strip()
        ok, reason = validate_quote(clean_quote, turn)
        if not ok:
            raise ValueError(reason)

        accepted.append(
            {
                "dimension_id": dim,
                "indicator_id": indicator,
                "level": level,
                "turn_id": turn_id,
                "turn_index": turn_index,
                "speaker_alias": alias,
                "quote": clean_quote,
            }
        )
    return accepted


def _aggregate_dimension(
    dim_id: DimensionId,
    evidence: list[TensorEvidence],
) -> TensorDimension:
    by_indicator: dict[str, list[int]] = {}
    candidate_turns: set[int] = set()
    for ev in evidence:
        by_indicator.setdefault(ev.indicator_id, []).append(ev.level)
        if ev.speaker_alias == CANDIDATE_ALIAS:
            candidate_turns.add(ev.turn_index)

    def _median_int(values: list[int]) -> float:
        s = sorted(values)
        n = len(s)
        mid = n // 2
        if n % 2:
            return float(s[mid])
        return (s[mid - 1] + s[mid]) / 2.0

    indicator_scores: list[float] = []
    for levels in by_indicator.values():
        indicator_scores.append(_median_int(levels) / 4.0)

    observed_indicators = len(by_indicator)
    observed_turns = len(candidate_turns)
    accepted_count = len(evidence)

    score: float | None
    if observed_indicators >= 2 and observed_turns >= 2:
        score = _round2(sum(indicator_scores) / len(indicator_scores))
    else:
        score = None

    indicator_coverage = observed_indicators / 3
    turn_coverage = min(observed_turns, 4) / 4
    evidence_coverage = min(accepted_count, 6) / 6
    confidence = _round2(
        min(
            1.0,
            0.40 * indicator_coverage
            + 0.35 * turn_coverage
            + 0.25 * evidence_coverage,
        )
    )

    return TensorDimension(
        dimension_id=dim_id,
        calculus_axis=CALCULUS_AXIS_MAP[dim_id],
        score=score,
        confidence=confidence,
        evidence=tuple(evidence),
    )


def aggregate_profile(
    session_id: str,
    transcript_hash_value: str,
    model_hash: str,
    prompt_version: str,
    turns: list[dict],
    proposals: list[dict],
) -> TensorProfile6D:
    validated = parse_and_validate_proposals(
        {"evidence": proposals},
        turns,
        allow_duplicate_dimensions=True,
    )
    ev_by_dim: dict[DimensionId, list[TensorEvidence]] = {
        d: [] for d in CANONICAL_DIMENSION_IDS
    }
    for item in validated:
        dim = item["dimension_id"]  # type: ignore[assignment]
        ev = TensorEvidence(
            evidence_id=_evidence_id(item),
            dimension_id=dim,
            indicator_id=item["indicator_id"],
            level=item["level"],
            turn_id=item["turn_id"],
            turn_index=item["turn_index"],
            speaker_alias=item["speaker_alias"],
            quote=item["quote"],
        )
        ev_by_dim[dim].append(ev)

    dimensions = tuple(
        _aggregate_dimension(dim_id, ev_by_dim[dim_id])
        for dim_id in CANONICAL_DIMENSION_IDS
    )
    return TensorProfile6D(
        schema="tensor_profile.6d.v1",
        session_id=session_id,
        transcript_hash=transcript_hash_value,
        model_hash=model_hash,
        prompt_version=prompt_version,
        dimensions=dimensions,
    )


def degenerate_profile(
    session_id: str,
    transcript_hash_value: str,
    model_hash: str,
    prompt_version: str,
) -> TensorProfile6D:
    dimensions = tuple(
        TensorDimension(
            dimension_id=dim_id,
            calculus_axis=CALCULUS_AXIS_MAP[dim_id],
            score=None,
            confidence=0.0,
            evidence=(),
        )
        for dim_id in CANONICAL_DIMENSION_IDS
    )
    return TensorProfile6D(
        schema="tensor_profile.6d.v1",
        session_id=session_id,
        transcript_hash=transcript_hash_value,
        model_hash=model_hash,
        prompt_version=prompt_version,
        dimensions=dimensions,
    )


def profile_to_dict(profile: TensorProfile6D) -> dict:
    return {
        "schema": profile.schema,
        "session_id": profile.session_id,
        "transcript_hash": profile.transcript_hash,
        "model_hash": profile.model_hash,
        "prompt_version": profile.prompt_version,
        "dimensions": [
            {
                "dimension_id": d.dimension_id,
                "calculus_axis": d.calculus_axis,
                "score": d.score,
                "confidence": d.confidence,
                "evidence": [
                    {
                        "evidence_id": e.evidence_id,
                        "dimension_id": e.dimension_id,
                        "indicator_id": e.indicator_id,
                        "level": e.level,
                        "turn_id": e.turn_id,
                        "turn_index": e.turn_index,
                        "speaker_alias": e.speaker_alias,
                        "quote": e.quote,
                    }
                    for e in d.evidence
                ],
            }
            for d in profile.dimensions
        ],
    }


def report_tensor_field(profile: TensorProfile6D) -> dict:
    return {
        "schema": profile.schema,
        "dimensions": profile_to_dict(profile)["dimensions"],
    }
