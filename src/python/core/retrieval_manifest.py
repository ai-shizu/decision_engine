#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""RetrievalManifestV1 — deterministic context selection observability (Phase 4-A)."""
from __future__ import annotations

import hashlib
import json
import os
import re
from dataclasses import dataclass
from enum import StrEnum
from typing import Any, Literal

MANIFEST_SCHEMA = "retrieval_manifest.v1"
POLICY_VERSION = "retrieval_policy.v1"
TOTAL_BUDGET_CHARS = 12_000

_HEX32 = re.compile(r"^[0-9a-f]{32}$")
_CONTACT_ALIAS_RE = re.compile(r"^C-[0-9a-f]{8}$")

FIXED_INTERNAL_ALIASES = frozenset({"candidate", "面接官", "参加者", "メンター"})


class RetrievalManifestPersistenceError(ValueError):
    """A valid manifest could not be persisted without violating store integrity."""


MEMORY_KIND_ALLOWLIST = frozenset(
    {
        "goal",
        "claim",
        "datum",
        "assumption",
        "constraint",
        "decision",
        "open_question",
        "contradiction",
        "recommendation",
    },
)

_STATUS_REASON_ALLOWED: dict[str, frozenset[str]] = {
    "ACCEPTED": frozenset(
        {"ACCEPTED_REQUIRED_CURRENT", "ACCEPTED_WITHIN_BUDGET"},
    ),
    "ACCEPTED_TRUNCATED": frozenset({"ACCEPTED_TRUNCATED_TO_BUDGET"}),
    "REJECTED": frozenset(
        {
            "REJECTED_BUDGET_LIMIT",
            "REJECTED_TOTAL_BUDGET",
            "REJECTED_LOW_PRIORITY",
            "REJECTED_INELIGIBLE_SOURCE",
            "REJECTED_EMPTY",
            "REJECTED_SUPERSEDED",
            "REJECTED_PRIVACY_POLICY",
        },
    ),
    "DEDUPLICATED": frozenset(
        {
            "DEDUPLICATED_DOCUMENT_ID",
            "DEDUPLICATED_CONTENT_HASH",
            "DEDUPLICATED_HIGHER_LANE",
        },
    ),
}


class CandidateStatus(StrEnum):
    ACCEPTED = "ACCEPTED"
    ACCEPTED_TRUNCATED = "ACCEPTED_TRUNCATED"
    REJECTED = "REJECTED"
    DEDUPLICATED = "DEDUPLICATED"


class ContextLane(StrEnum):
    CURRENT = "CURRENT"
    RECENT_TRANSCRIPT = "RECENT_TRANSCRIPT"
    WORKING_MEMORY = "WORKING_MEMORY"
    RETRIEVED_EVIDENCE = "RETRIEVED_EVIDENCE"


LANE_FIXED_BUDGETS = {
    ContextLane.CURRENT: 2400,
    ContextLane.RECENT_TRANSCRIPT: 3600,
    ContextLane.WORKING_MEMORY: 4000,
    ContextLane.RETRIEVED_EVIDENCE: 2000,
}

LANE_FIXED_ORDER = (
    ContextLane.CURRENT,
    ContextLane.RECENT_TRANSCRIPT,
    ContextLane.WORKING_MEMORY,
    ContextLane.RETRIEVED_EVIDENCE,
)


class SourceType(StrEnum):
    CURRENT_QUERY = "CURRENT_QUERY"
    TRANSCRIPT_TURN = "TRANSCRIPT_TURN"
    MEMORY_ATOM = "MEMORY_ATOM"
    KNOWLEDGE_DOCUMENT = "KNOWLEDGE_DOCUMENT"


class ReasonCode(StrEnum):
    ACCEPTED_REQUIRED_CURRENT = "ACCEPTED_REQUIRED_CURRENT"
    ACCEPTED_WITHIN_BUDGET = "ACCEPTED_WITHIN_BUDGET"
    ACCEPTED_TRUNCATED_TO_BUDGET = "ACCEPTED_TRUNCATED_TO_BUDGET"
    REJECTED_BUDGET_LIMIT = "REJECTED_BUDGET_LIMIT"
    REJECTED_TOTAL_BUDGET = "REJECTED_TOTAL_BUDGET"
    REJECTED_LOW_PRIORITY = "REJECTED_LOW_PRIORITY"
    REJECTED_INELIGIBLE_SOURCE = "REJECTED_INELIGIBLE_SOURCE"
    REJECTED_EMPTY = "REJECTED_EMPTY"
    REJECTED_SUPERSEDED = "REJECTED_SUPERSEDED"
    REJECTED_PRIVACY_POLICY = "REJECTED_PRIVACY_POLICY"
    DEDUPLICATED_DOCUMENT_ID = "DEDUPLICATED_DOCUMENT_ID"
    DEDUPLICATED_CONTENT_HASH = "DEDUPLICATED_CONTENT_HASH"
    DEDUPLICATED_HIGHER_LANE = "DEDUPLICATED_HIGHER_LANE"


def _strict_int(value: Any, *, field: str, nonnegative: bool = False) -> int:
    if type(value) is not int or isinstance(value, bool):
        raise ValueError(f"{field} must be int")
    if nonnegative and value < 0:
        raise ValueError(f"{field} must be nonnegative")
    return value


def _strict_str(value: Any, *, field: str, allow_none: bool = False) -> str | None:
    if value is None:
        if allow_none:
            return None
        raise ValueError(f"{field} is required")
    if type(value) is not str or not value.strip():
        raise ValueError(f"{field} must be non-empty str")
    if "\n" in value or "\r" in value:
        raise ValueError(f"{field} must not contain newlines")
    return value


def _strict_hash(value: Any, *, field: str) -> str:
    text = _strict_str(value, field=field)
    assert text is not None
    if not _HEX32.fullmatch(text):
        raise ValueError(f"{field} must be 32-char lowercase hex")
    return text


def _strict_model_hash(value: Any) -> str:
    if type(value) is not str:
        raise ValueError("model_hash must be str")
    if value != "" and not _HEX32.fullmatch(value):
        raise ValueError("model_hash must be empty or 32-char lowercase hex")
    return value


def _strict_tuple(
    value: Any,
    *,
    field: str,
    element_type: type,
    element_label: str,
) -> tuple[Any, ...]:
    if type(value) is not tuple:
        raise ValueError(f"{field} must be tuple")
    for item in value:
        if type(item) is not element_type:
            raise ValueError(element_label)
    return value


def is_verified_contact_alias(alias: str) -> bool:
    return bool(_CONTACT_ALIAS_RE.fullmatch(alias))


def safe_manifest_speaker_alias(alias: str | None) -> str | None:
    if alias is None:
        return None
    if alias in FIXED_INTERNAL_ALIASES:
        return alias
    if is_verified_contact_alias(alias):
        return alias
    return None


def compute_content_hash(text: str) -> str:
    from .session_memory import normalize_text

    payload = normalize_text(text).encode("utf-8")
    return hashlib.blake2b(payload, digest_size=16).hexdigest()


def compute_context_hash(context: str) -> str:
    return hashlib.blake2b(context.encode("utf-8"), digest_size=16).hexdigest()


def compute_query_hash(query: str) -> str:
    from .session_memory import normalize_text

    payload = normalize_text(query).encode("utf-8")
    return hashlib.blake2b(payload, digest_size=16).hexdigest()


def make_candidate_id(lane: ContextLane, document_id: str) -> str:
    return f"{lane.value}:{document_id}"


def _validate_status_reason(status: CandidateStatus, reason: ReasonCode) -> None:
    allowed = _STATUS_REASON_ALLOWED.get(status.value)
    if not allowed or reason.value not in allowed:
        raise ValueError("status/reason mismatch")


@dataclass(frozen=True)
class RetrievalCandidateV1:
    candidate_id: str
    document_id: str
    content_hash: str
    source_type: SourceType
    lane: ContextLane
    char_count: int
    included_chars: int
    status: CandidateStatus
    reason_code: ReasonCode
    selection_rank: int | None
    source_index: int | None
    speaker_alias: str | None
    memory_kind: str | None

    def __post_init__(self) -> None:
        _strict_str(self.candidate_id, field="candidate_id")
        _strict_str(self.document_id, field="document_id")
        _strict_hash(self.content_hash, field="content_hash")
        if type(self.source_type) is not SourceType:
            raise ValueError("source_type must be SourceType")
        if type(self.lane) is not ContextLane:
            raise ValueError("lane must be ContextLane")
        _strict_int(self.char_count, field="char_count", nonnegative=True)
        _strict_int(self.included_chars, field="included_chars", nonnegative=True)
        if self.included_chars > self.char_count:
            raise ValueError("included_chars exceeds char_count")
        if self.selection_rank is not None:
            _strict_int(self.selection_rank, field="selection_rank", nonnegative=True)
        if self.source_index is not None:
            _strict_int(self.source_index, field="source_index", nonnegative=True)
        if self.speaker_alias is not None:
            if type(self.speaker_alias) is not str:
                raise ValueError("speaker_alias must be str or None")
            if safe_manifest_speaker_alias(self.speaker_alias) != self.speaker_alias:
                raise ValueError("speaker_alias not approved")
        if self.memory_kind is not None:
            if type(self.memory_kind) is not str:
                raise ValueError("memory_kind must be str or None")
            if self.memory_kind not in MEMORY_KIND_ALLOWLIST:
                raise ValueError("memory_kind not allowed")
        if type(self.status) is not CandidateStatus:
            raise ValueError("status must be CandidateStatus")
        if type(self.reason_code) is not ReasonCode:
            raise ValueError("reason_code must be ReasonCode")
        _validate_status_reason(self.status, self.reason_code)
        if self.status == CandidateStatus.ACCEPTED:
            if self.included_chars != self.char_count:
                raise ValueError("ACCEPTED included_chars mismatch")
        elif self.status == CandidateStatus.ACCEPTED_TRUNCATED:
            if not (0 < self.included_chars < self.char_count):
                raise ValueError("ACCEPTED_TRUNCATED included_chars mismatch")
        else:
            if self.included_chars != 0:
                raise ValueError("terminal reject must have included_chars=0")


@dataclass(frozen=True)
class LaneUsageV1:
    lane: ContextLane
    budget_chars: int  # candidate content budget excluding headers/separators
    used_chars: int
    formatting_chars: int
    accepted_count: int
    rejected_count: int
    deduplicated_count: int

    def __post_init__(self) -> None:
        if type(self.lane) is not ContextLane:
            raise ValueError("lane must be ContextLane")
        expected = LANE_FIXED_BUDGETS[self.lane]
        _strict_int(self.budget_chars, field="budget_chars", nonnegative=True)
        if self.budget_chars != expected:
            raise ValueError("budget_chars mismatch for lane")
        _strict_int(self.used_chars, field="used_chars", nonnegative=True)
        _strict_int(self.formatting_chars, field="formatting_chars", nonnegative=True)
        _strict_int(self.accepted_count, field="accepted_count", nonnegative=True)
        _strict_int(self.rejected_count, field="rejected_count", nonnegative=True)
        _strict_int(self.deduplicated_count, field="deduplicated_count", nonnegative=True)
        if self.formatting_chars > self.used_chars:
            raise ValueError("formatting_chars exceeds used_chars")
        content_chars = self.used_chars - self.formatting_chars
        if content_chars > self.budget_chars:
            raise ValueError("lane included content exceeds budget")


@dataclass(frozen=True)
class RetrievalManifestV1:
    schema: Literal["retrieval_manifest.v1"]
    manifest_id: str
    session_id: str
    transcript_version: int
    query_hash: str
    context_hash: str
    policy_version: str
    prompt_version: str
    model_hash: str
    total_budget_chars: int
    used_chars: int
    formatting_overhead_chars: int
    candidates: tuple[RetrievalCandidateV1, ...]
    lane_usage: tuple[LaneUsageV1, ...]

    def __post_init__(self) -> None:
        if type(self.schema) is not str:
            raise ValueError("schema must be str")
        if self.schema != MANIFEST_SCHEMA:
            raise ValueError("invalid schema")
        _strict_hash(self.manifest_id, field="manifest_id")
        _strict_str(self.session_id, field="session_id")
        _strict_int(self.transcript_version, field="transcript_version", nonnegative=True)
        _strict_hash(self.query_hash, field="query_hash")
        _strict_hash(self.context_hash, field="context_hash")
        _strict_str(self.policy_version, field="policy_version")
        _strict_str(self.prompt_version, field="prompt_version")
        _strict_model_hash(self.model_hash)
        _strict_int(
            self.total_budget_chars,
            field="total_budget_chars",
            nonnegative=True,
        )
        if self.total_budget_chars != TOTAL_BUDGET_CHARS:
            raise ValueError("total_budget_chars must be 12000")
        _strict_int(self.used_chars, field="used_chars", nonnegative=True)
        if self.used_chars > self.total_budget_chars:
            raise ValueError("used_chars exceeds budget")
        _strict_int(
            self.formatting_overhead_chars,
            field="formatting_overhead_chars",
            nonnegative=True,
        )
        _strict_tuple(
            self.candidates,
            field="candidates",
            element_type=RetrievalCandidateV1,
            element_label="candidate element must be RetrievalCandidateV1",
        )
        _strict_tuple(
            self.lane_usage,
            field="lane_usage",
            element_type=LaneUsageV1,
            element_label="lane_usage element must be LaneUsageV1",
        )
        ids = [c.candidate_id for c in self.candidates]
        if len(ids) != len(set(ids)):
            raise ValueError("duplicate candidate_id")
        if len(self.lane_usage) != 4:
            raise ValueError("lane_usage must have 4 entries")
        lanes = [lu.lane for lu in self.lane_usage]
        if lanes != list(LANE_FIXED_ORDER):
            raise ValueError("lane_usage order mismatch")
        included_sum = sum(c.included_chars for c in self.candidates)
        if included_sum + self.formatting_overhead_chars != self.used_chars:
            raise ValueError("budget accounting mismatch")
        lane_sum = sum(l.used_chars for l in self.lane_usage)
        if lane_sum != self.used_chars:
            raise ValueError("lane usage mismatch")
        for lu in self.lane_usage:
            lane_cands = [c for c in self.candidates if c.lane == lu.lane]
            accepted = sum(
                1
                for c in lane_cands
                if c.status
                in (CandidateStatus.ACCEPTED, CandidateStatus.ACCEPTED_TRUNCATED)
                and c.included_chars > 0
            )
            rejected = sum(1 for c in lane_cands if c.status == CandidateStatus.REJECTED)
            deduped = sum(1 for c in lane_cands if c.status == CandidateStatus.DEDUPLICATED)
            if lu.accepted_count != accepted:
                raise ValueError("lane accepted_count mismatch")
            if lu.rejected_count != rejected:
                raise ValueError("lane rejected_count mismatch")
            if lu.deduplicated_count != deduped:
                raise ValueError("lane deduplicated_count mismatch")
            lane_included = sum(c.included_chars for c in lane_cands)
            if lane_included + lu.formatting_chars != lu.used_chars:
                raise ValueError("lane formatting accounting mismatch")
        expected_id = _manifest_id_from_payload(_manifest_payload_dict_from_manifest(self))
        if self.manifest_id != expected_id:
            raise ValueError("manifest_id mismatch")


def validate_manifest(manifest: RetrievalManifestV1) -> RetrievalManifestV1:
    if type(manifest) is not RetrievalManifestV1:
        raise ValueError("manifest must be RetrievalManifestV1")
    expected_id = _manifest_id_from_payload(
        _manifest_payload_dict_from_manifest(manifest),
    )
    if manifest.manifest_id != expected_id:
        raise ValueError("manifest_id mismatch")
    return manifest


def _candidate_to_dict(c: RetrievalCandidateV1) -> dict[str, Any]:
    return {
        "candidate_id": c.candidate_id,
        "document_id": c.document_id,
        "content_hash": c.content_hash,
        "source_type": c.source_type.value,
        "lane": c.lane.value,
        "char_count": c.char_count,
        "included_chars": c.included_chars,
        "status": c.status.value,
        "reason_code": c.reason_code.value,
        "selection_rank": c.selection_rank,
        "source_index": c.source_index,
        "speaker_alias": c.speaker_alias,
        "memory_kind": c.memory_kind,
    }


def _lane_usage_to_dict(l: LaneUsageV1) -> dict[str, Any]:
    return {
        "lane": l.lane.value,
        "budget_chars": l.budget_chars,
        "used_chars": l.used_chars,
        "formatting_chars": l.formatting_chars,
        "accepted_count": l.accepted_count,
        "rejected_count": l.rejected_count,
        "deduplicated_count": l.deduplicated_count,
    }


def _manifest_payload_dict(
    *,
    manifest_id: str,
    session_id: str,
    transcript_version: int,
    query_hash: str,
    context_hash: str,
    policy_version: str,
    prompt_version: str,
    model_hash: str,
    total_budget_chars: int,
    used_chars: int,
    formatting_overhead_chars: int,
    candidates: tuple[RetrievalCandidateV1, ...],
    lane_usage: tuple[LaneUsageV1, ...],
) -> dict[str, Any]:
    return {
        "schema": MANIFEST_SCHEMA,
        "manifest_id": manifest_id,
        "session_id": session_id,
        "transcript_version": transcript_version,
        "query_hash": query_hash,
        "context_hash": context_hash,
        "policy_version": policy_version,
        "prompt_version": prompt_version,
        "model_hash": model_hash,
        "total_budget_chars": total_budget_chars,
        "used_chars": used_chars,
        "formatting_overhead_chars": formatting_overhead_chars,
        "candidates": [_candidate_to_dict(c) for c in candidates],
        "lane_usage": [_lane_usage_to_dict(l) for l in lane_usage],
    }


def _manifest_payload_dict_from_manifest(manifest: RetrievalManifestV1) -> dict[str, Any]:
    return _manifest_payload_dict(
        manifest_id=manifest.manifest_id,
        session_id=manifest.session_id,
        transcript_version=manifest.transcript_version,
        query_hash=manifest.query_hash,
        context_hash=manifest.context_hash,
        policy_version=manifest.policy_version,
        prompt_version=manifest.prompt_version,
        model_hash=manifest.model_hash,
        total_budget_chars=manifest.total_budget_chars,
        used_chars=manifest.used_chars,
        formatting_overhead_chars=manifest.formatting_overhead_chars,
        candidates=manifest.candidates,
        lane_usage=manifest.lane_usage,
    )


def _manifest_id_from_payload(payload: dict[str, Any]) -> str:
    canonical = dict(payload)
    canonical["manifest_id"] = ""
    encoded = json.dumps(
        canonical,
        sort_keys=True,
        ensure_ascii=False,
        separators=(",", ":"),
    ).encode("utf-8")
    return hashlib.blake2b(encoded, digest_size=16).hexdigest()


def manifest_to_dict(manifest: RetrievalManifestV1) -> dict[str, Any]:
    manifest = validate_manifest(manifest)
    return _manifest_payload_dict_from_manifest(manifest)


def _require_exact_keys(data: dict[str, Any], expected: set[str], *, label: str) -> None:
    if type(data) is not dict:
        raise ValueError(f"{label} must be dict")
    if set(data.keys()) != expected:
        raise ValueError(f"{label} key mismatch")


def _validate_factory_inputs(
    *,
    session_id: str,
    transcript_version: int,
    query_hash: str,
    context_hash: str,
    prompt_version: str,
    model_hash: str,
    used_chars: int,
    formatting_overhead_chars: int,
    candidates: tuple[RetrievalCandidateV1, ...],
    lane_usage: tuple[LaneUsageV1, ...],
) -> None:
    _strict_str(session_id, field="session_id")
    _strict_int(transcript_version, field="transcript_version", nonnegative=True)
    _strict_hash(query_hash, field="query_hash")
    _strict_hash(context_hash, field="context_hash")
    _strict_str(prompt_version, field="prompt_version")
    _strict_model_hash(model_hash)
    _strict_int(used_chars, field="used_chars", nonnegative=True)
    _strict_int(
        formatting_overhead_chars,
        field="formatting_overhead_chars",
        nonnegative=True,
    )
    _strict_tuple(
        candidates,
        field="candidates",
        element_type=RetrievalCandidateV1,
        element_label="candidate element must be RetrievalCandidateV1",
    )
    _strict_tuple(
        lane_usage,
        field="lane_usage",
        element_type=LaneUsageV1,
        element_label="lane_usage element must be LaneUsageV1",
    )


def _candidate_from_dict(data: dict[str, Any]) -> RetrievalCandidateV1:
    _require_exact_keys(
        data,
        {
            "candidate_id",
            "document_id",
            "content_hash",
            "source_type",
            "lane",
            "char_count",
            "included_chars",
            "status",
            "reason_code",
            "selection_rank",
            "source_index",
            "speaker_alias",
            "memory_kind",
        },
        label="candidate",
    )
    memory_kind = data["memory_kind"]
    if memory_kind is not None:
        if type(memory_kind) is not str:
            raise ValueError("memory_kind must be str or None")
    speaker_alias = data["speaker_alias"]
    if speaker_alias is not None:
        if type(speaker_alias) is not str:
            raise ValueError("speaker_alias must be str or None")
    return RetrievalCandidateV1(
        candidate_id=_strict_str(data["candidate_id"], field="candidate_id"),
        document_id=_strict_str(data["document_id"], field="document_id"),
        content_hash=_strict_str(data["content_hash"], field="content_hash"),
        source_type=SourceType(data["source_type"]),
        lane=ContextLane(data["lane"]),
        char_count=_strict_int(data["char_count"], field="char_count", nonnegative=True),
        included_chars=_strict_int(
            data["included_chars"], field="included_chars", nonnegative=True,
        ),
        status=CandidateStatus(data["status"]),
        reason_code=ReasonCode(data["reason_code"]),
        selection_rank=(
            _strict_int(data["selection_rank"], field="selection_rank", nonnegative=True)
            if data["selection_rank"] is not None
            else None
        ),
        source_index=(
            _strict_int(data["source_index"], field="source_index", nonnegative=True)
            if data["source_index"] is not None
            else None
        ),
        speaker_alias=speaker_alias,
        memory_kind=memory_kind,
    )


def _lane_usage_from_dict(data: dict[str, Any]) -> LaneUsageV1:
    _require_exact_keys(
        data,
        {
            "lane",
            "budget_chars",
            "used_chars",
            "formatting_chars",
            "accepted_count",
            "rejected_count",
            "deduplicated_count",
        },
        label="lane_usage",
    )
    return LaneUsageV1(
        lane=ContextLane(data["lane"]),
        budget_chars=_strict_int(
            data["budget_chars"], field="budget_chars", nonnegative=True,
        ),
        used_chars=_strict_int(
            data["used_chars"], field="used_chars", nonnegative=True,
        ),
        formatting_chars=_strict_int(
            data["formatting_chars"], field="formatting_chars", nonnegative=True,
        ),
        accepted_count=_strict_int(
            data["accepted_count"], field="accepted_count", nonnegative=True,
        ),
        rejected_count=_strict_int(
            data["rejected_count"], field="rejected_count", nonnegative=True,
        ),
        deduplicated_count=_strict_int(
            data["deduplicated_count"], field="deduplicated_count", nonnegative=True,
        ),
    )


def manifest_from_dict(data: dict[str, Any]) -> RetrievalManifestV1:
    _require_exact_keys(
        data,
        {
            "schema",
            "manifest_id",
            "session_id",
            "transcript_version",
            "query_hash",
            "context_hash",
            "policy_version",
            "prompt_version",
            "model_hash",
            "total_budget_chars",
            "used_chars",
            "formatting_overhead_chars",
            "candidates",
            "lane_usage",
        },
        label="manifest",
    )
    if data["schema"] != MANIFEST_SCHEMA:
        raise ValueError("invalid schema")
    raw_candidates = data["candidates"]
    if type(raw_candidates) is not list:
        raise ValueError("candidates must be list")
    raw_lane_usage = data["lane_usage"]
    if type(raw_lane_usage) is not list:
        raise ValueError("lane_usage must be list")
    candidates: list[RetrievalCandidateV1] = []
    for item in raw_candidates:
        if type(item) is not dict:
            raise ValueError("candidate item must be dict")
        candidates.append(_candidate_from_dict(item))
    lane_usage: list[LaneUsageV1] = []
    for item in raw_lane_usage:
        if type(item) is not dict:
            raise ValueError("lane_usage item must be dict")
        lane_usage.append(_lane_usage_from_dict(item))
    return RetrievalManifestV1(
        schema=MANIFEST_SCHEMA,
        manifest_id=_strict_str(data["manifest_id"], field="manifest_id"),
        session_id=_strict_str(data["session_id"], field="session_id"),
        transcript_version=_strict_int(
            data["transcript_version"], field="transcript_version", nonnegative=True,
        ),
        query_hash=_strict_str(data["query_hash"], field="query_hash"),
        context_hash=_strict_str(data["context_hash"], field="context_hash"),
        policy_version=_strict_str(data["policy_version"], field="policy_version"),
        prompt_version=_strict_str(data["prompt_version"], field="prompt_version"),
        model_hash=_strict_model_hash(data["model_hash"]),
        total_budget_chars=_strict_int(
            data["total_budget_chars"], field="total_budget_chars", nonnegative=True,
        ),
        used_chars=_strict_int(data["used_chars"], field="used_chars", nonnegative=True),
        formatting_overhead_chars=_strict_int(
            data["formatting_overhead_chars"],
            field="formatting_overhead_chars",
            nonnegative=True,
        ),
        candidates=tuple(candidates),
        lane_usage=tuple(lane_usage),
    )


def canonical_manifest_json(manifest: RetrievalManifestV1) -> str:
    manifest = validate_manifest(manifest)
    payload = _manifest_payload_dict_from_manifest(manifest)
    payload["manifest_id"] = ""
    return json.dumps(payload, sort_keys=True, ensure_ascii=False, separators=(",", ":"))


def compute_manifest_id(manifest: RetrievalManifestV1) -> str:
    manifest = validate_manifest(manifest)
    return manifest.manifest_id


def build_retrieval_manifest(
    *,
    session_id: str,
    transcript_version: int,
    query_hash: str,
    context_hash: str,
    prompt_version: str,
    model_hash: str,
    used_chars: int,
    formatting_overhead_chars: int,
    candidates: tuple[RetrievalCandidateV1, ...],
    lane_usage: tuple[LaneUsageV1, ...],
) -> RetrievalManifestV1:
    """Build a complete manifest with manifest_id computed before construction."""
    _validate_factory_inputs(
        session_id=session_id,
        transcript_version=transcript_version,
        query_hash=query_hash,
        context_hash=context_hash,
        prompt_version=prompt_version,
        model_hash=model_hash,
        used_chars=used_chars,
        formatting_overhead_chars=formatting_overhead_chars,
        candidates=candidates,
        lane_usage=lane_usage,
    )
    payload = _manifest_payload_dict(
        manifest_id="",
        session_id=session_id,
        transcript_version=transcript_version,
        query_hash=query_hash,
        context_hash=context_hash,
        policy_version=POLICY_VERSION,
        prompt_version=prompt_version,
        model_hash=model_hash,
        total_budget_chars=TOTAL_BUDGET_CHARS,
        used_chars=used_chars,
        formatting_overhead_chars=formatting_overhead_chars,
        candidates=candidates,
        lane_usage=lane_usage,
    )
    manifest_id = _manifest_id_from_payload(payload)
    return RetrievalManifestV1(
        schema=MANIFEST_SCHEMA,
        manifest_id=manifest_id,
        session_id=session_id,
        transcript_version=transcript_version,
        query_hash=query_hash,
        context_hash=context_hash,
        policy_version=POLICY_VERSION,
        prompt_version=prompt_version,
        model_hash=model_hash,
        total_budget_chars=TOTAL_BUDGET_CHARS,
        used_chars=used_chars,
        formatting_overhead_chars=formatting_overhead_chars,
        candidates=candidates,
        lane_usage=lane_usage,
    )


def _validate_latest_pointer(latest: Any) -> str:
    if not isinstance(latest, dict) or set(latest.keys()) != {"manifest_id"}:
        raise ValueError("latest pointer key mismatch")
    manifest_id = latest["manifest_id"]
    if type(manifest_id) is not str:
        raise ValueError("manifest_id must be str")
    if not _HEX32.fullmatch(manifest_id):
        raise ValueError("manifest_id must be 32-char lowercase hex")
    if "/" in manifest_id or "\\" in manifest_id or "." in manifest_id:
        raise ValueError("manifest_id must not contain path separators")
    return manifest_id


_PERSISTENCE_FAILED_MSG = "retrieval manifest persistence failed"
RETRIEVAL_MANIFEST_RETENTION_LIMIT = 256
_OWNED_MANIFEST_FILENAME = re.compile(r"^[0-9a-f]{32}\.json$")


def _atomic_write_text(path: Any, text: str) -> None:
    """Write UTF-8 text via same-dir .tmp then os.replace; best-effort tmp cleanup."""
    from pathlib import Path

    target = Path(path)
    tmp = target.with_suffix(".json.tmp")
    try:
        tmp.write_text(text, encoding="utf-8")
        os.replace(tmp, target)
    except Exception:
        try:
            tmp.unlink(missing_ok=True)
        except OSError:
            pass
        raise


def _assert_existing_latest_integrity() -> None:
    """Strict-validate latest pointer + payload before any write. No repair."""
    from . import paths

    if not paths.LATEST_RETRIEVAL_MANIFEST.exists():
        return
    latest = json.loads(
        paths.LATEST_RETRIEVAL_MANIFEST.read_text(encoding="utf-8"),
    )
    manifest_id = _validate_latest_pointer(latest)
    manifest_path = paths.RETRIEVAL_MANIFESTS_DIR / f"{manifest_id}.json"
    if not manifest_path.exists():
        raise ValueError("latest manifest file missing")
    data = json.loads(manifest_path.read_text(encoding="utf-8"))
    existing = validate_manifest(manifest_from_dict(data))
    if existing.manifest_id != manifest_id:
        raise ValueError("latest pointer manifest_id mismatch")


def _prune_retrieval_manifests(*, latest_manifest_id: str) -> None:
    """Delete oldest owned valid immutables beyond the retention limit.

    latest_manifest_id's file is always retained. Selection among the rest uses
    (st_mtime_ns, filename) descending only — mtime is never an integrity signal.
    Corrupt / id-mismatched / symlink owned paths hard-fail with zero deletes.
    Unknown non-hex names are ignored and never deleted.
    """
    from pathlib import Path

    from . import paths

    limit = RETRIEVAL_MANIFEST_RETENTION_LIMIT
    if type(limit) is not int or limit < 1:
        raise ValueError("invalid retention limit")
    if type(latest_manifest_id) is not str or not _HEX32.fullmatch(latest_manifest_id):
        raise ValueError("invalid latest_manifest_id")

    directory = Path(paths.RETRIEVAL_MANIFESTS_DIR)
    if not directory.is_dir():
        return

    latest_name = f"{latest_manifest_id}.json"
    # Preflight: collect and strict-validate every owned path before any unlink.
    owned: list[tuple[int, str, Path]] = []
    latest_seen = False
    for entry in directory.iterdir():
        name = entry.name
        if name == "latest.json" or name.endswith(".tmp"):
            continue
        if not _OWNED_MANIFEST_FILENAME.fullmatch(name):
            continue
        if entry.is_symlink():
            raise ValueError("symlink owned manifest path")
        if not entry.is_file():
            raise ValueError("owned manifest path is not a regular file")
        data = json.loads(entry.read_text(encoding="utf-8"))
        payload = validate_manifest(manifest_from_dict(data))
        if payload.manifest_id != entry.stem:
            raise ValueError("owned manifest_id mismatch")
        mtime_ns = entry.stat().st_mtime_ns
        owned.append((mtime_ns, name, entry))
        if name == latest_name:
            latest_seen = True

    if not latest_seen:
        raise ValueError("latest manifest missing from owned set")

    if len(owned) <= limit:
        return

    non_latest = [(mt, name, path) for mt, name, path in owned if name != latest_name]
    # Newest first; ties broken by filename descending (deterministic).
    non_latest.sort(key=lambda item: (item[0], item[1]), reverse=True)
    keep_non_latest = {name for _, name, _ in non_latest[: max(limit - 1, 0)]}
    to_delete = [
        path
        for _, name, path in non_latest
        if name not in keep_non_latest
    ]
    for path in to_delete:
        path.unlink()


def save_retrieval_manifest(manifest: RetrievalManifestV1) -> None:
    from . import paths

    manifest = validate_manifest(manifest)
    try:
        paths.RETRIEVAL_MANIFESTS_DIR.mkdir(parents=True, exist_ok=True)
        _assert_existing_latest_integrity()
        path = paths.RETRIEVAL_MANIFESTS_DIR / f"{manifest.manifest_id}.json"
        if path.exists():
            existing_data = json.loads(path.read_text(encoding="utf-8"))
            existing_manifest = validate_manifest(manifest_from_dict(existing_data))
            if existing_manifest.manifest_id != manifest.manifest_id:
                raise ValueError("existing manifest_id mismatch")
        else:
            payload = json.dumps(
                manifest_to_dict(manifest),
                ensure_ascii=False,
                indent=2,
            )
            _atomic_write_text(path, payload + "\n")
        latest_payload = json.dumps(
            {"manifest_id": manifest.manifest_id},
            ensure_ascii=False,
            indent=2,
        )
        _atomic_write_text(
            paths.LATEST_RETRIEVAL_MANIFEST,
            latest_payload + "\n",
        )
        _prune_retrieval_manifests(latest_manifest_id=manifest.manifest_id)
    except (OSError, ValueError) as exc:
        raise RetrievalManifestPersistenceError(_PERSISTENCE_FAILED_MSG) from exc


def load_latest_retrieval_manifest() -> RetrievalManifestV1 | None:
    from . import paths

    if not paths.LATEST_RETRIEVAL_MANIFEST.exists():
        return None
    latest = json.loads(
        paths.LATEST_RETRIEVAL_MANIFEST.read_text(encoding="utf-8"),
    )
    manifest_id = _validate_latest_pointer(latest)
    manifest_path = paths.RETRIEVAL_MANIFESTS_DIR / f"{manifest_id}.json"
    if not manifest_path.exists():
        raise ValueError("latest manifest file missing")
    data = json.loads(manifest_path.read_text(encoding="utf-8"))
    manifest = manifest_from_dict(data)
    manifest = validate_manifest(manifest)
    if manifest.manifest_id != manifest_id:
        raise ValueError("latest pointer manifest_id mismatch")
    return manifest


def build_bounded_context_with_manifest(*args, **kwargs):
    """Re-export from session_memory (instrumentation owner)."""
    from .session_memory import build_bounded_context_with_manifest as _fn

    return _fn(*args, **kwargs)
