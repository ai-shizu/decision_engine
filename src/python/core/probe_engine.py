# -*- coding: utf-8 -*-
from __future__ import annotations

from dataclasses import asdict, dataclass, replace
import hashlib

from .canonicalization import canonical_json_bytes


@dataclass(frozen=True)
class HistoricalNode:
    id: str
    date_range: str
    fact_text: str
    source: str
    is_trusted: bool = True
    superseded_by: str | None = None
    weight: float = 1.0
    disputed_with: str | None = None

    def to_dict(self) -> dict:
        return asdict(self)

    @classmethod
    def from_dict(cls, data: dict) -> "HistoricalNode":
        return cls(**data)


def _node_id(date_range: str, fact_text: str, source: str) -> str:
    payload = b"decision-engine/historical-node/v2\0" + canonical_json_bytes({
        "date_range": date_range,
        "fact_text": fact_text,
        "source": source,
    })
    return "hn-" + hashlib.blake2b(payload, digest_size=6).hexdigest()


def create_historical_node(
    *,
    date_range: str,
    fact_text: str,
    source: str,
    is_trusted: bool = True,
    weight: float = 1.0,
    disputed_with: str | None = None,
) -> HistoricalNode:
    weight = max(0.05, min(1.0, float(weight)))
    return HistoricalNode(
        id=_node_id(date_range, fact_text, source),
        date_range=date_range,
        fact_text=fact_text,
        source=source,
        is_trusted=is_trusted,
        weight=weight,
        disputed_with=disputed_with,
    )


def supersede_historical_node(
    node: HistoricalNode,
    *,
    fact_text: str,
    source: str | None = None,
    date_range: str | None = None,
) -> tuple[HistoricalNode, HistoricalNode]:
    replacement = create_historical_node(
        date_range=date_range or node.date_range,
        fact_text=fact_text,
        source=source or node.source,
        is_trusted=node.is_trusted,
        weight=node.weight,
        disputed_with=node.disputed_with,
    )
    linked_original = replace(node, superseded_by=replacement.id)
    return linked_original, replacement
