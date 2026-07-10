#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""Deterministic session memory compiler (SPEC_ENGINE_TENSOR_PROFILING.md §4)."""
from __future__ import annotations

import hashlib
import re
import unicodedata
from dataclasses import dataclass
from typing import Literal

MemoryKind = Literal[
    "goal",
    "claim",
    "datum",
    "assumption",
    "constraint",
    "decision",
    "open_question",
    "contradiction",
    "recommendation",
]

DYNAMIC_CONTEXT_CHAR_BUDGET = 12_000
CURRENT_TURN_CHAR_BUDGET = 2_400
EXACT_TAIL_CHAR_BUDGET = 3_600
WORKING_MEMORY_CHAR_BUDGET = 4_000
RETRIEVED_EVIDENCE_BUDGET = 2_000
MAX_ACTIVE_MEMORY_ATOMS = 24
MAX_RETRIEVED_ATOMS = 8

_KIND_PRIORITY = {
    "contradiction": 1.00,
    "open_question": 0.95,
    "constraint": 0.90,
    "decision": 0.85,
    "goal": 0.80,
    "assumption": 0.70,
    "datum": 0.65,
    "claim": 0.55,
    "recommendation": 0.50,
}

_ASSUMPTION_RE = re.compile(r"(仮定|想定|前提|もし|assuming|assume)", re.I)
_CONSTRAINT_RE = re.compile(r"(制約|条件|限界|constraint|must|cannot)", re.I)
_DECISION_RE = re.compile(r"(決定|採用|選択|結論として|decide|decision)", re.I)
_DATUM_RE = re.compile(r"(\d|%|円|万|億|kg|km|件|人|回|年|月|日|倍|割)")
_SENTENCE_SPLIT_RE = re.compile(r"(?<=[。．！？!?])\s*")

CANDIDATE_ALIASES = frozenset({"candidate", "候補者"})


def normalize_text(text: str) -> str:
    return unicodedata.normalize("NFC", text or "")


def char_count(text: str) -> int:
    return len(normalize_text(text))


def speaker_alias(role: str) -> str:
    role = normalize_text(role).strip()
    if role == "候補者":
        return "candidate"
    if role in {"面接官", "参加者", "メンター"}:
        return role
    return re.sub(r"\s+", "_", role)[:24] or "speaker"


@dataclass(frozen=True)
class TranscriptRef:
    turn_id: str
    turn_index: int
    speaker_alias: str
    quote: str


@dataclass(frozen=True)
class MemoryAtom:
    atom_id: str
    kind: MemoryKind
    canonical_text: str
    source: TranscriptRef
    subject_key: str
    status: Literal["active", "superseded"] = "active"
    superseded_by: str | None = None


@dataclass(frozen=True)
class WorkingMemoryV1:
    schema: Literal["working_memory.v1"]
    session_id: str
    transcript_version: int
    active_goal_ids: tuple[str, ...]
    constraint_ids: tuple[str, ...]
    decision_ids: tuple[str, ...]
    open_question_ids: tuple[str, ...]
    contradiction_ids: tuple[str, ...]
    supporting_atom_ids: tuple[str, ...]
    context_chars: int


def stable_turn_id(session_id: str, turn_index: int, alias: str) -> str:
    payload = f"{session_id}:{turn_index}:{alias}".encode("utf-8")
    return hashlib.blake2b(payload, digest_size=16).hexdigest()


def transcript_turns_from_pairs(
    transcript: list[tuple[str, str]],
    session_id: str,
) -> list[dict]:
    turns: list[dict] = []
    for idx, (role, text) in enumerate(transcript):
        alias = speaker_alias(role)
        turns.append(
            {
                "turn_id": stable_turn_id(session_id, idx, alias),
                "turn_index": idx,
                "speaker_alias": alias,
                "role": role,
                "text": normalize_text(text),
            }
        )
    return turns


def classify_kind(sentence: str) -> MemoryKind:
    s = sentence.strip()
    if not s:
        return "claim"
    if "?" in s or "？" in s:
        return "open_question"
    if _DATUM_RE.search(s):
        return "datum"
    if _ASSUMPTION_RE.search(s):
        return "assumption"
    if _CONSTRAINT_RE.search(s):
        return "constraint"
    if _DECISION_RE.search(s):
        return "decision"
    return "claim"


def _split_sentences(text: str) -> list[str]:
    text = normalize_text(text)
    if not text:
        return []
    parts = _SENTENCE_SPLIT_RE.split(text)
    out: list[str] = []
    for part in parts:
        part = part.strip()
        if part:
            out.append(part)
    return out or [text]


def _atom_id(kind: MemoryKind, canonical_text: str, source: TranscriptRef) -> str:
    payload = (
        f"{kind}|{canonical_text}|{source.turn_id}|"
        f"{source.turn_index}|{source.speaker_alias}|{source.quote}"
    ).encode("utf-8")
    return hashlib.blake2b(payload, digest_size=16).hexdigest()


def _subject_key(kind: MemoryKind, canonical_text: str) -> str:
    words = re.findall(r"[\w\u3040-\u30ff\u4e00-\u9fff]+", canonical_text.lower())
    return "|".join(words[:3]) or kind


def compile_atoms_from_turn(turn: dict, session_id: str) -> list[MemoryAtom]:
    atoms: list[MemoryAtom] = []
    text = normalize_text(turn["text"])
    for sentence in _split_sentences(text):
        quote = sentence[:120]
        if not quote:
            continue
        if quote not in text:
            continue
        kind = classify_kind(sentence)
        canonical = sentence[:240]
        ref = TranscriptRef(
            turn_id=turn["turn_id"],
            turn_index=turn["turn_index"],
            speaker_alias=turn["speaker_alias"],
            quote=quote,
        )
        atom = MemoryAtom(
            atom_id=_atom_id(kind, canonical, ref),
            kind=kind,
            canonical_text=canonical,
            source=ref,
            subject_key=_subject_key(kind, canonical),
        )
        atoms.append(atom)
    atoms.sort(key=lambda a: (a.source.turn_index, a.kind, a.atom_id))
    return atoms


def compile_atoms(transcript: list[tuple[str, str]], session_id: str) -> list[MemoryAtom]:
    turns = transcript_turns_from_pairs(transcript, session_id)
    atoms: list[MemoryAtom] = []
    for turn in turns:
        atoms.extend(compile_atoms_from_turn(turn, session_id))
    atoms.sort(key=lambda a: (a.source.turn_index, a.kind, a.atom_id))
    return atoms


def _trigrams(text: str) -> set[str]:
    text = normalize_text(text)
    if len(text) < 3:
        return {text} if text else set()
    return {text[i : i + 3] for i in range(len(text) - 2)}


def _jaccard(a: set[str], b: set[str]) -> float:
    if not a and not b:
        return 0.0
    union = a | b
    if not union:
        return 0.0
    return len(a & b) / len(union)


def atom_priority(
    atom: MemoryAtom,
    query: str,
    current_turn_index: int,
    active_goal_ids: tuple[str, ...],
) -> float:
    goal_text = " ".join(active_goal_ids)
    overlap = _jaccard(
        _trigrams(query + goal_text),
        _trigrams(atom.canonical_text + atom.source.quote),
    )
    recency = 1 / (1 + max(0, current_turn_index - atom.source.turn_index))
    kind_w = _KIND_PRIORITY.get(atom.kind, 0.50)
    return 0.55 * overlap + 0.25 * recency + 0.20 * kind_w


_QUERY_TRUNCATION_MARKER = "\n[query truncated at char budget]"
_TURN_TRUNCATION_MARKER = "\n[turn truncated at char budget]"


def _truncate_body_to_budget_with_marker(body: str, body_budget: int, marker: str) -> str:
    body = normalize_text(body)
    if body_budget <= 0:
        raise ValueError("body budget too small")
    if char_count(body) <= body_budget:
        return body
    parts = _split_sentences(body)
    out: list[str] = []
    used = 0
    for part in parts:
        cost = char_count(part)
        if used + cost > body_budget:
            break
        out.append(part)
        used += cost
    if out:
        return "".join(out) + marker
    return body[:body_budget] + marker


def _truncate_query_to_budget(query: str, budget: int) -> str:
    query = normalize_text(query)
    if char_count(query) <= budget:
        return query
    marker = _QUERY_TRUNCATION_MARKER
    body_budget = budget - char_count(marker)
    if body_budget <= 0:
        raise ValueError("current query exceeds budget")
    return _truncate_body_to_budget_with_marker(query, body_budget, marker)


def _truncate_turn_block_to_budget(turn: dict, budget: int) -> str:
    header = (
        f"[turn_id: {turn['turn_id']} turn_index: {turn['turn_index']} "
        f"speaker_alias: {turn['speaker_alias']}]"
    )
    marker = _TURN_TRUNCATION_MARKER
    header_cost = char_count(header) + 1
    if budget <= header_cost + char_count(marker):
        raise ValueError("turn block budget too small")
    body_budget = budget - header_cost - char_count(marker)
    text = normalize_text(turn["text"])
    if char_count(text) <= body_budget:
        return f"{header}\n{text}"
    body = _truncate_body_to_budget_with_marker(text, body_budget, marker)
    return f"{header}\n{body}"


def _build_authoritative_current_unit(
    query: str,
    turns: list[dict],
    transcript: list[tuple[str, str]],
    budget: int,
) -> tuple[str, str, int | None]:
    if transcript and turns:
        last_role, last_text = transcript[-1]
        if last_role == "候補者" and normalize_text(last_text) == query:
            turn = turns[-1]
            return (
                "# Current turn",
                _truncate_turn_block_to_budget(turn, budget),
                turn["turn_index"],
            )
    return "# Current query", _truncate_query_to_budget(query, budget), None


def format_turn_block(turn: dict) -> str:
    return (
        f"[turn_id: {turn['turn_id']} turn_index: {turn['turn_index']} "
        f"speaker_alias: {turn['speaker_alias']}]\n{turn['text']}"
    )


def format_turns_for_evidence_prompt(
    turns: list[dict],
    char_budget: int = EXACT_TAIL_CHAR_BUDGET,
) -> str:
    """Evidence-eligible turns with stable metadata for structured evaluation."""
    allowed = {"candidate", "面接官", "参加者"}
    eligible = [t for t in turns if t["speaker_alias"] in allowed]
    if not eligible:
        return ""

    latest_candidate: dict | None = None
    for turn in reversed(eligible):
        if turn["speaker_alias"] == "candidate":
            latest_candidate = turn
            break

    selected: dict[int, str] = {}
    used = 0

    def _try_add(turn: dict, *, allow_truncated: bool) -> bool:
        nonlocal used
        if turn["turn_index"] in selected:
            return False
        block = format_turn_block(turn)
        cost = char_count(block) + (2 if selected else 0)
        if used + cost <= char_budget:
            selected[turn["turn_index"]] = block
            used += cost
            return True
        if not allow_truncated:
            return False
        remaining = char_budget - used - (2 if selected else 0)
        if remaining <= 0:
            return False
        trunc = _truncate_turn_block_to_budget(turn, remaining)
        trunc_cost = char_count(trunc) + (2 if selected else 0)
        if trunc_cost <= char_budget - used and char_count(trunc) > 0:
            selected[turn["turn_index"]] = trunc
            used += trunc_cost
            return True
        return False

    if latest_candidate is not None:
        _try_add(latest_candidate, allow_truncated=True)

    for turn in reversed(eligible):
        _try_add(turn, allow_truncated=False)

    ordered = [selected[idx] for idx in sorted(selected)]
    return "\n\n".join(ordered)


def _format_atom_line(atom: MemoryAtom) -> str:
    return f"- ({atom.kind}) {atom.canonical_text} [{atom.source.quote}]"


def _select_tail_turn_blocks(
    transcript: list[tuple[str, str]],
    session_id: str,
    char_budget: int,
    *,
    exclude_turn_index: int | None = None,
) -> list[str]:
    turns = transcript_turns_from_pairs(transcript, session_id)
    blocks: list[str] = []
    used = 0
    for turn in reversed(turns):
        if exclude_turn_index is not None and turn["turn_index"] == exclude_turn_index:
            continue
        block = format_turn_block(turn)
        cost = char_count(block) + 2
        if used + cost > char_budget:
            continue
        blocks.append(block)
        used += cost
    return list(reversed(blocks))


def _select_atom_blocks(
    atoms: list[MemoryAtom],
    char_budget: int,
) -> list[str]:
    lines: list[str] = []
    used = 0
    for atom in atoms:
        line = _format_atom_line(atom)
        cost = char_count(line) + 1
        if used + cost > char_budget:
            break
        lines.append(line)
        used += cost
    return lines


def _assemble_context_units(units: list[tuple[str, str]], total_budget: int) -> str:
    selected: list[str] = []
    used = 0
    for header, body in units:
        if not body:
            continue
        block = f"{header}\n{body}" if header else body
        cost = char_count(block) + (2 if selected else 0)
        if used + cost > total_budget:
            continue
        selected.append(block)
        used += cost
    return "\n\n".join(selected)


def build_bounded_context(
    *,
    session_id: str,
    transcript: list[tuple[str, str]],
    current_query: str,
    model_hash: str = "",
    prompt_version: str = "pv1",
    current_turn_role: str | None = None,
    current_turn_text: str | None = None,
) -> tuple[str, WorkingMemoryV1]:
    """Select prompt context under fixed budgets. Raw transcript is never edited."""
    del model_hash, prompt_version
    del current_turn_role, current_turn_text
    query = normalize_text(current_query)
    turns = transcript_turns_from_pairs(transcript, session_id)
    current_turn_index = len(turns)

    exclude_turn_index: int | None = None
    units: list[tuple[str, str]] = []
    if query:
        header, body, exclude_turn_index = _build_authoritative_current_unit(
            query, turns, transcript, CURRENT_TURN_CHAR_BUDGET
        )
        units.append((header, body))

    tail_blocks = _select_tail_turn_blocks(
        transcript,
        session_id,
        EXACT_TAIL_CHAR_BUDGET,
        exclude_turn_index=exclude_turn_index,
    )
    if tail_blocks:
        units.append(("# Recent transcript", "\n\n".join(tail_blocks)))

    atoms = compile_atoms(transcript, session_id)
    if exclude_turn_index is not None:
        atoms = [a for a in atoms if a.source.turn_index != exclude_turn_index]
    active_goal_ids = tuple(
        a.atom_id for a in atoms if a.kind == "goal" and a.status == "active"
    )
    wm_atoms = _select_atoms(
        atoms,
        query,
        current_turn_index,
        active_goal_ids,
        MAX_ACTIVE_MEMORY_ATOMS,
        WORKING_MEMORY_CHAR_BUDGET,
    )
    wm_lines = _select_atom_blocks(wm_atoms, WORKING_MEMORY_CHAR_BUDGET)
    if wm_lines:
        units.append(("# Working memory", "\n".join(wm_lines)))

    retrieved = _select_atoms(
        atoms,
        query,
        current_turn_index,
        active_goal_ids,
        MAX_RETRIEVED_ATOMS,
        RETRIEVED_EVIDENCE_BUDGET,
    )
    retrieved_ids = {a.atom_id for a in wm_atoms}
    retrieved = [a for a in retrieved if a.atom_id not in retrieved_ids]
    rv_lines = _select_atom_blocks(retrieved, RETRIEVED_EVIDENCE_BUDGET)
    if rv_lines:
        units.append(("# Retrieved evidence", "\n".join(rv_lines)))

    context = _assemble_context_units(units, DYNAMIC_CONTEXT_CHAR_BUDGET)
    included_atoms = [
        a for a in wm_atoms + retrieved
        if _format_atom_line(a) in context
    ]
    wm = _working_memory_from_atoms(
        included_atoms,
        char_count(context),
        session_id,
        len(transcript),
    )
    return context, wm


def _select_atoms(
    atoms: list[MemoryAtom],
    query: str,
    current_turn_index: int,
    active_goal_ids: tuple[str, ...],
    max_atoms: int,
    char_budget: int,
) -> list[MemoryAtom]:
    active = [a for a in atoms if a.status == "active"]
    ranked = sorted(
        active,
        key=lambda a: (
            -atom_priority(a, query, current_turn_index, active_goal_ids),
            -a.source.turn_index,
            a.atom_id,
        ),
    )
    selected: list[MemoryAtom] = []
    used = 0
    for atom in ranked:
        if len(selected) >= max_atoms:
            break
        line = _format_atom_line(atom)
        cost = char_count(line) + 1
        if used + cost > char_budget:
            continue
        selected.append(atom)
        used += cost
    return selected


def _working_memory_from_atoms(
    atoms: list[MemoryAtom],
    context_chars: int,
    session_id: str,
    transcript_version: int,
) -> WorkingMemoryV1:
    def ids_for(kind: MemoryKind) -> tuple[str, ...]:
        return tuple(
            a.atom_id
            for a in atoms
            if a.kind == kind and a.status == "active"
        )

    supporting = tuple(a.atom_id for a in atoms if a.status == "active")
    return WorkingMemoryV1(
        schema="working_memory.v1",
        session_id=session_id,
        transcript_version=transcript_version,
        active_goal_ids=ids_for("goal"),
        constraint_ids=ids_for("constraint"),
        decision_ids=ids_for("decision"),
        open_question_ids=ids_for("open_question"),
        contradiction_ids=ids_for("contradiction"),
        supporting_atom_ids=supporting,
        context_chars=context_chars,
    )
