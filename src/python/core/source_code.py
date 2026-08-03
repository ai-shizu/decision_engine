# -*- coding: utf-8 -*-
from __future__ import annotations

from dataclasses import asdict, dataclass, field
from datetime import date
import math
import re
import statistics
from typing import Any

from .gap_analysis import (
    GENUINE_DOC_MIN_WEIGHT,
    PRODUCTIVITY_MARKERS,
    analyze_procrastination,
    build_subjective_corpus,
)
from .line_telemetry import compute_interpersonal_axes


MAX_QUOTE = 120

SPEND_IMMEDIATE = ("外食", "娯楽", "課金", "衝動", "コンビニ")
SPEND_INVEST = ("書籍", "受講", "資格", "貯蓄", "投資")
FRICTION_MARKERS = ("衝突", "言い争い", "気まずい", "揉め", "摩擦")
ATTRIBUTION_LEXICON = {
    "internal": (
        "自分のせい", "自分が悪", "努力不足", "準備不足", "甘かった", "力不足",
        "反省", "次はこうする", "やるべきだった", "改善する", "詰めが甘",
    ),
    "external": (
        "のせい", "せいで", "環境が", "会社が", "周りが", "上司が", "理不尽",
        "仕方ない", "どうしようもない", "巻き込まれ",
    ),
    "chance": ("運が悪", "運次第", "たまたま", "偶然", "ツイてな", "巡り合わせ", "不運"),
}
NEGATORS = ("ない", "じゃない", "ではない", "わけがない", "とは思わない")


@dataclass
class EvidenceRef:
    kind: str
    date: str
    quote: str
    value: float | None = None


@dataclass
class Axis:
    score: float | None
    confidence: float
    evidence: list[EvidenceRef] = field(default_factory=list)
    updated: str = ""


@dataclass
class HumanSourceCode:
    decision_threshold: Axis
    reward_bias: Axis
    locus_of_control: Axis
    unlearning_rate: Axis
    friction_energy_ledger: Axis
    friction_response: Axis
    latency_asymmetry: Axis
    protocol_plasticity: Axis
    schema: str = "human_source_code.v1"
    updated: str = ""

    def to_dict(self) -> dict:
        data = asdict(self)
        axes = {
            name: data.pop(name)
            for name in (
                "decision_threshold",
                "reward_bias",
                "locus_of_control",
                "unlearning_rate",
                "friction_energy_ledger",
                "friction_response",
                "latency_asymmetry",
                "protocol_plasticity",
            )
        }
        data["axes"] = axes
        confidences = [float(a["confidence"]) for a in axes.values()]
        data["progress"] = round(sum(confidences) / len(confidences), 3) if confidences else 0.0
        return data


def _clamp01(value: float) -> float:
    return max(0.0, min(1.0, value))


def _round_score(value: float | None) -> float | None:
    return None if value is None else round(_clamp01(float(value)), 3)


def _updated(daily: list[dict]) -> str:
    dates = sorted(str(d.get("date", "")) for d in daily if d.get("date"))
    return dates[-1] if dates else ""


def _date(s: str) -> date | None:
    try:
        return date.fromisoformat(s)
    except (TypeError, ValueError):
        return None


def _known_names(line_telemetry: dict | None) -> dict[str, str]:
    names: dict[str, str] = {}
    if not isinstance(line_telemetry, dict):
        return names
    for dyad in line_telemetry.get("dyads", []) or []:
        name = str(dyad.get("contact_name", "") or "")
        alias = str(
            dyad.get("contact_short_id", dyad.get("contact_alias", "")) or ""
        )
        if name and alias:
            names[name] = alias
    return names


def _quote(text: str, names: dict[str, str], *, keyword: str | None = None) -> str:
    raw = str(text or "").replace("\n", " ").strip()
    if keyword and keyword in raw:
        idx = raw.find(keyword)
        raw = raw[max(0, idx - 35): idx + len(keyword) + 45].strip()
    for name, alias in names.items():
        raw = raw.replace(name, alias)
    return raw[:MAX_QUOTE]


def _evidence(kind: str, date_s: str, quote: str, names: dict[str, str],
              value: float | None = None, keyword: str | None = None) -> EvidenceRef:
    return EvidenceRef(kind=kind, date=date_s, quote=_quote(quote, names, keyword=keyword), value=value)


def _axis(score: float | None, confidence: float, evidence: list[EvidenceRef],
          updated: str) -> Axis:
    ev = evidence[:5] if score is not None else evidence[:5]
    return Axis(_round_score(score), round(_clamp01(confidence), 3), ev, updated)


def _mean(values: list[float]) -> float | None:
    return statistics.mean(values) if values else None


def _decision_threshold(daily: list[dict], names: dict[str, str], updated: str) -> Axis:
    result = analyze_procrastination(daily)
    tasks = result.get("tasks", [])
    avoidance = [float(t.get("avoidance_index", 0.0)) for t in tasks]
    preconsult = 0
    for dc in daily:
        for c in dc.get("consultations", []) or []:
            if not c.get("is_simulated_persona"):
                preconsult += 1
    declarations = int(result.get("declarations", 0) or 0)
    evidence: list[EvidenceRef] = []
    for task in tasks:
        for rec in task.get("records", []) or []:
            evidence.append(_evidence("diary", rec.get("declared", ""), rec.get("quote", ""),
                                      names, rec.get("delay_days")))
    if not avoidance and not preconsult:
        return _axis(None, 0.0, [], updated)
    score = 0.6 * (_mean(avoidance) or 0.0) + 0.4 * min(1.0, preconsult / 5.0)
    return _axis(score, min(1.0, declarations / 8.0), evidence, updated)


def _reward_bias(daily: list[dict], names: dict[str, str], updated: str) -> Axis:
    imm, invest, n_tx = 0.0, 0.0, 0
    evidence: list[EvidenceRef] = []
    for dc in daily:
        for tx in dc.get("transactions", []) or []:
            cat = str(tx.get("category", ""))
            amount = abs(float(tx.get("amount", 0) or 0))
            if amount <= 0:
                continue
            n_tx += 1
            if any(k in cat for k in SPEND_IMMEDIATE):
                imm += amount
                evidence.append(_evidence("finance", str(tx.get("date") or dc.get("date", "")),
                                          f"{cat}: {amount:.0f}", names, amount))
            elif any(k in cat for k in SPEND_INVEST):
                invest += amount
                evidence.append(_evidence("finance", str(tx.get("date") or dc.get("date", "")),
                                          f"{cat}: {amount:.0f}", names, amount))

    procrastination = analyze_procrastination(daily)
    values = [
        float(r["discounted_value"])
        for t in procrastination.get("tasks", [])
        for r in t.get("records", [])
        if r.get("discounted_value") is not None
    ]
    if not n_tx and not values:
        return _axis(None, 0.0, [], updated)
    imm_ratio = imm / (imm + invest) if (imm + invest) else 0.0
    v_bar = statistics.median(values) if values else 0.5
    return _axis(0.5 * imm_ratio + 0.5 * (1.0 - v_bar), min(1.0, n_tx / 20.0),
                 evidence, updated)


def _negated(text: str, end: int) -> bool:
    tail = text[end:end + 8]
    return any(n in tail for n in NEGATORS)


def _locus_of_control(daily: list[dict], names: dict[str, str], updated: str) -> Axis:
    counts = {"internal": 0.0, "external": 0.0, "chance": 0.0}
    evidence: list[EvidenceRef] = []
    for doc in build_subjective_corpus(daily):
        weight = float(doc.get("weight", 1.0))
        if weight < GENUINE_DOC_MIN_WEIGHT:
            continue
        text = str(doc.get("text", ""))
        for bucket, words in ATTRIBUTION_LEXICON.items():
            for kw in words:
                for m in re.finditer(re.escape(kw), text):
                    if _negated(text, m.end()):
                        continue
                    counts[bucket] += weight
                    if len(evidence) < 5:
                        evidence.append(_evidence("diary", str(doc.get("date", "")), text,
                                                  names, weight, kw))
    total = sum(counts.values())
    if total <= 0:
        return _axis(None, 0.0, [], updated)
    score = (counts["internal"] + 0.5 * counts["chance"]) / total
    return _axis(score, min(1.0, total / 10.0), evidence, updated)


def _unlearning_rate(daily: list[dict], names: dict[str, str], updated: str) -> Axis:
    result = analyze_procrastination(daily)
    rates: list[float] = []
    evidence: list[EvidenceRef] = []
    for task in result.get("tasks", []) or []:
        for rec in task.get("records", []) or []:
            start = _date(rec.get("declared", ""))
            end = _date(rec.get("executed", ""))
            if start is None:
                continue
            weeks = ((end - start).days / 7.0) if end is not None else 8.0
            weeks = min(8.0, max(0.0, weeks))
            rates.append(1.0 / (1.0 + weeks))
            evidence.append(_evidence("diary", rec.get("declared", ""), rec.get("quote", ""),
                                      names, weeks))
    if not rates:
        return _axis(None, 0.0, [], updated)
    return _axis(statistics.mean(rates), min(1.0, len(rates) / 5.0), evidence, updated)


def _productivity_hits(dc: dict) -> int:
    blob = str(dc.get("diary_text", "")) + "\n" + str(dc.get("line_self_text", ""))
    return sum(blob.count(kw) for kw in PRODUCTIVITY_MARKERS)


def _friction_energy_ledger(daily: list[dict], names: dict[str, str], updated: str) -> Axis:
    by_date = {str(dc.get("date")): dc for dc in daily if dc.get("date")}
    scores: list[float] = []
    evidence: list[EvidenceRef] = []
    for dc in daily:
        text = str(dc.get("diary_text", ""))
        marker = next((m for m in FRICTION_MARKERS if m in text), None)
        d = _date(str(dc.get("date", "")))
        if marker is None or d is None:
            continue
        before = after = 0
        for delta in (1, 2):
            b = (d.fromordinal(d.toordinal() - delta)).isoformat()
            a = (d.fromordinal(d.toordinal() + delta)).isoformat()
            if b in by_date:
                before += _productivity_hits(by_date[b])
            if a in by_date:
                after += _productivity_hits(by_date[a])
        diff = after - before
        scores.append(0.5 + 0.5 * math.tanh(diff / 2.0))
        evidence.append(_evidence("diary", str(dc.get("date", "")), text, names, diff, marker))
    if not scores:
        return _axis(None, 0.0, [], updated)
    return _axis(statistics.mean(scores), min(1.0, len(scores) / 5.0), evidence, updated)


def _interpersonal_axes(line_telemetry: dict | None, names: dict[str, str], updated: str) -> dict[str, Axis]:
    dyads = []
    if isinstance(line_telemetry, dict):
        dyads = list(line_telemetry.get("dyads", []) or [])
        raw_axes = line_telemetry.get("axes")
    else:
        raw_axes = None
    axes = raw_axes if isinstance(raw_axes, dict) else compute_interpersonal_axes(dyads)
    aliases = [
        str(d.get("contact_short_id", d.get("contact_alias", "")))
        for d in dyads
        if d.get("contact_short_id") or d.get("contact_alias")
    ]

    out: dict[str, Axis] = {}
    for name in ("friction_response", "latency_asymmetry", "protocol_plasticity"):
        raw = axes.get(name, {}) if isinstance(axes, dict) else {}
        score = raw.get("score")
        confidence = float(raw.get("confidence", 0.0) or 0.0)
        evidence: list[EvidenceRef] = []
        if score is not None:
            for alias in aliases[:5]:
                evidence.append(EvidenceRef("line", updated, f"contact_short_id={alias}", None))
        out[name] = _axis(score, confidence, evidence, updated)
    return out


def compute_source_code(
    daily: list[dict],
    probe_store: dict | None = None,
    prev: HumanSourceCode | None = None,
    line_telemetry: dict | None = None,
    backend: Any | None = None,
) -> HumanSourceCode:
    del probe_store, prev, backend
    updated = _updated(daily)
    names = _known_names(line_telemetry)
    interpersonal = _interpersonal_axes(line_telemetry, names, updated)
    return HumanSourceCode(
        decision_threshold=_decision_threshold(daily, names, updated),
        reward_bias=_reward_bias(daily, names, updated),
        locus_of_control=_locus_of_control(daily, names, updated),
        unlearning_rate=_unlearning_rate(daily, names, updated),
        friction_energy_ledger=_friction_energy_ledger(daily, names, updated),
        friction_response=interpersonal["friction_response"],
        latency_asymmetry=interpersonal["latency_asymmetry"],
        protocol_plasticity=interpersonal["protocol_plasticity"],
        updated=updated,
    )
