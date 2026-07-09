# -*- coding: utf-8 -*-
"""Phase D2 PROBE funnel — deterministic candidate/session logic (SPEC_PHASE_D2_PROBE.md)."""
from __future__ import annotations

from dataclasses import asdict, dataclass, field
import hashlib
import json
import os
from pathlib import Path
from typing import Any

from .paths import DATA_PROCESSED
from .probe_engine import HistoricalNode, create_historical_node
from .source_code import HumanSourceCode

PROBE_AXIS_ORDER = (
    "decision_threshold",
    "reward_bias",
    "locus_of_control",
    "unlearning_rate",
    "friction_energy_ledger",
)

STAGE_ORDER = ("FACT", "CONTEXT", "EMOTION", "MEANING")
SESSION_STATUS = ("active", "closed")

_PROBE_STORE_PATH = DATA_PROCESSED / "probe_store.json"


@dataclass(frozen=True)
class ProbeQuestion:
    id: str
    axis: str
    stage: str
    text: str
    tags: tuple[str, ...] = ()

    def to_dict(self) -> dict:
        return {
            "id": self.id,
            "axis": self.axis,
            "stage": self.stage,
            "text": self.text,
            "tags": list(self.tags),
        }

    @classmethod
    def from_dict(cls, data: dict) -> "ProbeQuestion":
        return cls(
            id=str(data["id"]),
            axis=str(data["axis"]),
            stage=str(data["stage"]),
            text=str(data["text"]),
            tags=tuple(data.get("tags") or ()),
        )


def _bank(axis: str, stage: str, text: str) -> ProbeQuestion:
    return ProbeQuestion(f"pq-{axis}-{stage}-01", axis, stage, text)


PROBE_QUESTION_BANK: tuple[ProbeQuestion, ...] = (
    _bank("decision_threshold", "FACT", "直近で先延ばししたタスクを一つ、事実だけで書いてください。"),
    _bank("decision_threshold", "CONTEXT", "そのタスクを始める前に、何を確認しようとしていましたか。"),
    _bank("decision_threshold", "EMOTION", "その時点で自分が書ける感覚を、本人の言葉で書いてください。"),
    _bank("decision_threshold", "MEANING", "いま振り返ると、その先延ばしは何を守ろうとしていましたか。"),
    _bank("reward_bias", "FACT", "直近の支出または報酬に関する出来事を、事実だけで一つ書いてください。"),
    _bank("reward_bias", "CONTEXT", "その選択の直前に、何と比較しようとしていましたか。"),
    _bank("reward_bias", "EMOTION", "その時点で自分が書ける感覚を、本人の言葉で書いてください。"),
    _bank("reward_bias", "MEANING", "いま振り返ると、その選択は何を優先しようとしていましたか。"),
    _bank("locus_of_control", "FACT", "直近で結果が想定とずれた出来事を、事実だけで一つ書いてください。"),
    _bank("locus_of_control", "CONTEXT", "その出来事の原因を考え始める前に、何を確認していましたか。"),
    _bank("locus_of_control", "EMOTION", "その時点で自分が書ける感覚を、本人の言葉で書いてください。"),
    _bank("locus_of_control", "MEANING", "いま振り返ると、その出来事をどう受け止めようとしていましたか。"),
    _bank("unlearning_rate", "FACT", "直近で方針や前提を変えた出来事を、事実だけで一つ書いてください。"),
    _bank("unlearning_rate", "CONTEXT", "その変更を検討し始める前に、何を確認していましたか。"),
    _bank("unlearning_rate", "EMOTION", "その時点で自分が書ける感覚を、本人の言葉で書いてください。"),
    _bank("unlearning_rate", "MEANING", "いま振り返ると、その変更は何を更新しようとしていましたか。"),
    _bank("friction_energy_ledger", "FACT", "直近で摩擦や消耗を感じた出来事を、事実だけで一つ書いてください。"),
    _bank("friction_energy_ledger", "CONTEXT", "その出来事の前後で、何を優先しようとしていましたか。"),
    _bank("friction_energy_ledger", "EMOTION", "その時点で自分が書ける感覚を、本人の言葉で書いてください。"),
    _bank("friction_energy_ledger", "MEANING", "いま振り返ると、その摩擦は何を守ろうとしていましたか。"),
)


@dataclass
class ProbeCandidate:
    axis: str
    stage: str
    priority: float
    confidence: float
    score: float | None
    node_count: int
    open_session_id: str | None = None
    reasons: list[str] = field(default_factory=list)

    def to_dict(self) -> dict:
        return asdict(self)


@dataclass(frozen=True)
class ProbeAnswerRecord:
    session_id: str
    question_id: str
    stage: str
    node_id: str
    date: str
    text_quote: str
    subjective_weight: float

    def to_dict(self) -> dict:
        return asdict(self)

    @classmethod
    def from_dict(cls, data: dict) -> "ProbeAnswerRecord":
        return cls(
            session_id=str(data["session_id"]),
            question_id=str(data["question_id"]),
            stage=str(data["stage"]),
            node_id=str(data["node_id"]),
            date=str(data["date"]),
            text_quote=str(data["text_quote"]),
            subjective_weight=float(data["subjective_weight"]),
        )


@dataclass
class ProbeSession:
    id: str
    date: str
    stage: str
    target_axis: str
    questions_asked: list[str] = field(default_factory=list)
    nodes_created: list[str] = field(default_factory=list)
    status: str = "active"

    def to_dict(self) -> dict:
        return asdict(self)

    @classmethod
    def from_dict(cls, data: dict) -> "ProbeSession":
        return cls(
            id=str(data["id"]),
            date=str(data["date"]),
            stage=str(data["stage"]),
            target_axis=str(data["target_axis"]),
            questions_asked=list(data.get("questions_asked") or []),
            nodes_created=list(data.get("nodes_created") or []),
            status=str(data.get("status") or "active"),
        )


@dataclass(frozen=True)
class ProbeInsight:
    id: str
    kind: str
    axis: str
    stage: str
    priority: float
    node_refs: list[str]
    evidence_refs: list[str]
    message_code: str

    def to_dict(self) -> dict:
        return asdict(self)

    @classmethod
    def from_dict(cls, data: dict) -> "ProbeInsight":
        return cls(
            id=str(data["id"]),
            kind=str(data["kind"]),
            axis=str(data["axis"]),
            stage=str(data["stage"]),
            priority=float(data["priority"]),
            node_refs=list(data.get("node_refs") or []),
            evidence_refs=list(data.get("evidence_refs") or []),
            message_code=str(data["message_code"]),
        )


@dataclass
class ProbeStore:
    schema: str = "probe_store.v1"
    historical_nodes: list[HistoricalNode] = field(default_factory=list)
    sessions: list[ProbeSession] = field(default_factory=list)
    answers: list[ProbeAnswerRecord] = field(default_factory=list)
    insights: list[ProbeInsight] = field(default_factory=list)

    def to_dict(self) -> dict:
        return {
            "schema": self.schema,
            "historical_nodes": [n.to_dict() for n in self.historical_nodes],
            "sessions": [s.to_dict() for s in self.sessions],
            "answers": [a.to_dict() for a in self.answers],
            "insights": [i.to_dict() for i in self.insights],
        }

    @classmethod
    def from_dict(cls, data: dict) -> "ProbeStore":
        return cls(
            schema=str(data.get("schema") or "probe_store.v1"),
            historical_nodes=[
                HistoricalNode.from_dict(n) for n in data.get("historical_nodes") or []
            ],
            sessions=[ProbeSession.from_dict(s) for s in data.get("sessions") or []],
            answers=[ProbeAnswerRecord.from_dict(a) for a in data.get("answers") or []],
            insights=[ProbeInsight.from_dict(i) for i in data.get("insights") or []],
        )


def _clamp01(value: float) -> float:
    return max(0.0, min(1.0, float(value)))


def sanitize_probe_text(text: str, third_party_aliases: dict[str, str]) -> str:
    out = text.replace("\n", " ").strip()
    for real_name, alias in sorted(third_party_aliases.items()):
        out = out.replace(real_name, alias)
    return out[:120]


def _blake6(payload: str) -> str:
    return hashlib.blake2b(payload.encode("utf-8"), digest_size=6).hexdigest()


def _session_id(today: str, axis: str, stage: str, session_count: int) -> str:
    return "ps-" + _blake6(f"{today}\0{axis}\0{stage}\0{session_count}")


def _insight_id(kind: str, axis: str, stage: str, priority: float) -> str:
    return "pi-" + _blake6(f"{kind}\0{axis}\0{stage}\0{priority}")


def _node_index(store: ProbeStore) -> dict[str, HistoricalNode]:
    return {n.id: n for n in store.historical_nodes}


def _axis_node_ids(store: ProbeStore, axis: str) -> set[str]:
    node_ids: set[str] = set()
    for session in store.sessions:
        if session.target_axis != axis:
            continue
        node_ids.update(session.nodes_created)
    return node_ids


def _trusted_active_node_count(store: ProbeStore, axis: str) -> int:
    nodes = _node_index(store)
    count = 0
    for node_id in _axis_node_ids(store, axis):
        node = nodes.get(node_id)
        if node and node.is_trusted and node.superseded_by is None:
            count += 1
    return count


def _completed_stages(store: ProbeStore, axis: str) -> set[str]:
    nodes = _node_index(store)
    sessions = {s.id: s for s in store.sessions}
    completed: set[str] = set()
    for answer in store.answers:
        session = sessions.get(answer.session_id)
        if session is None or session.target_axis != axis:
            continue
        node = nodes.get(answer.node_id)
        if node and node.is_trusted and node.superseded_by is None:
            completed.add(answer.stage)
    return completed


def _candidate_stage(store: ProbeStore, axis: str) -> str:
    completed = _completed_stages(store, axis)
    for stage in STAGE_ORDER:
        if stage not in completed:
            return stage
    return "MEANING"


def find_active_session(store: ProbeStore, axis: str) -> ProbeSession | None:
    for session in store.sessions:
        if session.target_axis == axis and session.status == "active":
            return session
    return None


def find_session_for_today(store: ProbeStore, axis: str, today: str) -> ProbeSession | None:
    for session in store.sessions:
        if session.target_axis == axis and session.date == today:
            return session
    return None


def derive_probe_candidates(
    sc: HumanSourceCode,
    store: ProbeStore,
    *,
    backend: Any | None = None,
) -> list[ProbeCandidate]:
    del backend
    candidates: list[ProbeCandidate] = []
    for axis in PROBE_AXIS_ORDER:
        axis_obj = getattr(sc, axis)
        score = axis_obj.score
        confidence = _clamp01(axis_obj.confidence)
        node_count = _trusted_active_node_count(store, axis)
        coverage = min(1.0, node_count / 4.0)
        coverage_gap = 1.0 - coverage
        extremity = 0.0 if score is None else abs(score - 0.5) * 2.0
        priority = round(
            _clamp01(
                0.60 * (1.0 - confidence)
                + 0.25 * coverage_gap
                + 0.15 * extremity
            ),
            3,
        )
        active = find_active_session(store, axis)
        candidates.append(
            ProbeCandidate(
                axis=axis,
                stage=_candidate_stage(store, axis),
                priority=priority,
                confidence=confidence,
                score=score,
                node_count=node_count,
                open_session_id=active.id if active else None,
            )
        )
    candidates.sort(
        key=lambda c: (
            -c.priority,
            PROBE_AXIS_ORDER.index(c.axis),
            STAGE_ORDER.index(c.stage),
        )
    )
    return candidates


def select_next_candidate(sc: HumanSourceCode, store: ProbeStore) -> ProbeCandidate:
    candidates = derive_probe_candidates(sc, store)
    if not candidates:
        raise ValueError("no probe candidates")
    return candidates[0]


def select_probe_question(
    candidate: ProbeCandidate,
    session: ProbeSession,
) -> ProbeQuestion:
    del candidate
    eligible = [
        q for q in PROBE_QUESTION_BANK
        if q.axis == session.target_axis and q.stage == session.stage
    ]
    if not eligible:
        raise ValueError(f"no question for axis={session.target_axis} stage={session.stage}")
    unused = [q for q in eligible if q.id not in session.questions_asked]
    return unused[0] if unused else eligible[0]


def start_session(candidate: ProbeCandidate, today: str, store: ProbeStore) -> ProbeSession:
    existing = find_session_for_today(store, candidate.axis, today)
    if existing is not None:
        return existing
    session = ProbeSession(
        id=_session_id(today, candidate.axis, candidate.stage, len(store.sessions)),
        date=today,
        stage=candidate.stage,
        target_axis=candidate.axis,
    )
    store.sessions.append(session)
    return session


def probe_next_question(
    sc: HumanSourceCode,
    store: ProbeStore,
    today: str,
    third_party_aliases: dict[str, str] | None = None,
    *,
    backend: Any | None = None,
) -> dict:
    del third_party_aliases, backend

    for axis in PROBE_AXIS_ORDER:
        active = find_active_session(store, axis)
        if active is not None:
            candidate = next(
                c for c in derive_probe_candidates(sc, store) if c.axis == axis
            )
            question = select_probe_question(candidate, active)
            return {
                "schema": "probe_question.v1",
                "session_id": active.id,
                "question_id": question.id,
                "axis": active.target_axis,
                "stage": active.stage,
                "question": question.text,
                "priority": candidate.priority,
            }

    candidate = select_next_candidate(sc, store)
    session = find_session_for_today(store, candidate.axis, today)
    if session is None:
        session = start_session(candidate, today, store)
    if session.status == "closed":
        raise ValueError("session already closed")
    question = select_probe_question(candidate, session)
    return {
        "schema": "probe_question.v1",
        "session_id": session.id,
        "question_id": question.id,
        "axis": session.target_axis,
        "stage": session.stage,
        "question": question.text,
        "priority": candidate.priority,
    }


def _next_stage(stage: str) -> str | None:
    idx = STAGE_ORDER.index(stage)
    if idx + 1 >= len(STAGE_ORDER):
        return None
    return STAGE_ORDER[idx + 1]


def _subjective_weight(stage: str) -> float:
    return 1.0 if stage in ("EMOTION", "MEANING") else 0.0


def record_probe_answer(
    store: ProbeStore,
    session_id: str,
    question_id: str,
    answer_text: str,
    today: str,
    date_range: str | None = None,
    third_party_aliases: dict[str, str] | None = None,
    *,
    backend: Any | None = None,
) -> ProbeStore:
    del backend
    session = next((s for s in store.sessions if s.id == session_id), None)
    if session is None:
        raise ValueError("unknown session")
    if session.status != "active":
        raise ValueError("session not active")
    if len(session.questions_asked) >= 5:
        raise ValueError("session question cap reached")

    candidate = ProbeCandidate(
        axis=session.target_axis,
        stage=session.stage,
        priority=0.0,
        confidence=0.0,
        score=None,
        node_count=0,
    )
    expected = select_probe_question(candidate, session)
    if question_id != expected.id:
        raise ValueError("question_id does not match next selected question")
    if expected.stage != session.stage:
        raise ValueError("stage mismatch")

    sanitized = sanitize_probe_text(answer_text, third_party_aliases or {})
    node = create_historical_node(
        date_range=date_range or today,
        fact_text=sanitized,
        source="probe",
    )
    store.historical_nodes.append(node)
    store.answers.append(
        ProbeAnswerRecord(
            session_id=session.id,
            question_id=question_id,
            stage=session.stage,
            node_id=node.id,
            date=today,
            text_quote=sanitized,
            subjective_weight=_subjective_weight(session.stage),
        )
    )
    session.nodes_created.append(node.id)
    session.questions_asked.append(question_id)

    nxt = _next_stage(session.stage)
    if nxt is None:
        session.status = "closed"
    else:
        session.stage = nxt
    return store


def derive_probe_insights(
    sc: HumanSourceCode,
    store: ProbeStore,
    *,
    backend: Any | None = None,
) -> list[ProbeInsight]:
    del backend
    insights: list[ProbeInsight] = []
    candidates = {c.axis: c for c in derive_probe_candidates(sc, store)}
    for axis in PROBE_AXIS_ORDER:
        candidate = candidates[axis]
        node_refs = sorted(_axis_node_ids(store, axis))
        evidence_refs = list(node_refs)
        completed = _completed_stages(store, axis)
        stage = _candidate_stage(store, axis)
        coverage = min(1.0, candidate.node_count / 4.0)

        if candidate.confidence < 0.5:
            insights.append(
                ProbeInsight(
                    id=_insight_id("low_confidence", axis, stage, candidate.priority),
                    kind="low_confidence",
                    axis=axis,
                    stage=stage,
                    priority=candidate.priority,
                    node_refs=node_refs,
                    evidence_refs=evidence_refs,
                    message_code="probe.low_confidence",
                )
            )
        if coverage < 0.5:
            insights.append(
                ProbeInsight(
                    id=_insight_id("under_probed", axis, stage, candidate.priority),
                    kind="under_probed",
                    axis=axis,
                    stage=stage,
                    priority=candidate.priority,
                    node_refs=node_refs,
                    evidence_refs=evidence_refs,
                    message_code="probe.under_probed",
                )
            )
        if all(s in completed for s in STAGE_ORDER):
            insights.append(
                ProbeInsight(
                    id=_insight_id("stage_complete", axis, "MEANING", candidate.priority),
                    kind="stage_complete",
                    axis=axis,
                    stage="MEANING",
                    priority=candidate.priority,
                    node_refs=node_refs,
                    evidence_refs=evidence_refs,
                    message_code="probe.stage_complete",
                )
            )
    return insights


def load_probe_store(path: Path | None = None) -> ProbeStore:
    store_path = path if path is not None else _PROBE_STORE_PATH
    if not store_path.exists():
        return ProbeStore()
    data = json.loads(store_path.read_text(encoding="utf-8"))
    return ProbeStore.from_dict(data)


def save_probe_store(store: ProbeStore, path: Path | None = None) -> None:
    store_path = path if path is not None else _PROBE_STORE_PATH
    store_path.parent.mkdir(parents=True, exist_ok=True)
    payload = json.dumps(store.to_dict(), ensure_ascii=False, indent=2)
    tmp = store_path.with_suffix(".tmp")
    tmp.write_text(payload, encoding="utf-8")
    os.replace(tmp, store_path)
