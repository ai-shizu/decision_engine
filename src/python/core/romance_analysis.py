#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""
core/romance_analysis.py — Phase 3-B 交流パルス解析 (romance_analysis.v1)
=========================================================================
観測可能な会話往復量のみを扱う。第三者の感情・好意は推定しない。
生本文・実名は LLM / 永続化 / ログへ渡さない。
"""

from __future__ import annotations

import json
import re
from decimal import Decimal, ROUND_HALF_UP
from typing import Any

from .consultation_engine import HiddenReasoningRedactor

ROMANCE_SCHEMA_VERSION = "romance_analysis.v1"
MAX_INPUT_CHARS = 12_000
MAX_LINES = 500

INSUFFICIENT_TENDENCY = "判定に必要な観測量が不足しています"
INSUFFICIENT_ACTION = "会話履歴を追加して再分析する"

ALLOWED_TENDENCIES = (
    "発話数とターン切り替えは概ね均衡しています",
    "発話数に偏りがあります",
    "ターン切り替えが少ない状態です",
    "観測範囲では中程度の往復です",
)

ALLOWED_ACTIONS = (
    "同じ形式で履歴を追加して再分析する",
    "往復のバランスを意識して記録を続ける",
    "ターン切り替えを意識して記録を続ける",
)

ROMANCE_ANALYSIS_SCHEMA: dict[str, Any] = {
    "type": "object",
    "additionalProperties": False,
    "required": [
        "schema",
        "affinity_score",
        "interaction_tendency",
        "next_best_action",
    ],
    "properties": {
        "schema": {
            "type": "string",
            "enum": [ROMANCE_SCHEMA_VERSION],
        },
        "affinity_score": {
            "type": ["integer", "null"],
            "minimum": 0,
            "maximum": 100,
        },
        "interaction_tendency": {
            "type": "string",
            "enum": [*ALLOWED_TENDENCIES, INSUFFICIENT_TENDENCY],
        },
        "next_best_action": {
            "type": "string",
            "enum": [*ALLOWED_ACTIONS, INSUFFICIENT_ACTION],
        },
    },
}

_LINE_RE = re.compile(r"^\[(self|contact_alias)\]\s*(.*)$")
_CONTROL_RE = re.compile(r"[\x00-\x08\x0b\x0c\x0e-\x1f\x7f]")
_MAX_RETRIES = 2

_ROMANCE_SYSTEM = (
    "あなたは会話の観測量から短文ラベルを選ぶアシスタントです。"
    "第三者の感情・好意・恋愛感情を推定・断定してはならない。"
    "指定 JSON スキーマのみを出力し、affinity_score / interaction_tendency / "
    "next_best_action は与えられた決定値をそのまま返すこと (再計算禁止)。"
)


def sanitize_input(raw: str) -> str:
    text = str(raw or "")
    text = text.replace("\r\n", "\n").replace("\r", "\n")
    text = _CONTROL_RE.sub("", text)
    if len(text) > MAX_INPUT_CHARS:
        text = text[:MAX_INPUT_CHARS]
    lines = text.split("\n")
    if len(lines) > MAX_LINES:
        lines = lines[:MAX_LINES]
    return "\n".join(lines)


def _valid_body(body: str) -> bool:
    stripped = body.strip()
    if not stripped:
        return False
    cleaned = _CONTROL_RE.sub("", stripped).strip()
    return bool(cleaned)


def parse_canonical_lines(sanitized: str) -> list[str]:
    speakers: list[str] = []
    for line in sanitized.split("\n"):
        stripped = line.strip()
        if not stripped:
            continue
        m = _LINE_RE.match(stripped)
        if not m:
            continue
        speaker = m.group(1)
        if speaker not in ("self", "contact_alias"):
            continue
        if not _valid_body(m.group(2)):
            continue
        speakers.append(speaker)
    return speakers


def compute_metrics(speakers: list[str]) -> dict[str, int | float]:
    return _compute_metrics(speakers)


def _compute_metrics(speakers: list[str]) -> dict[str, int | float]:
    total = len(speakers)
    self_count = sum(1 for s in speakers if s == "self")
    contact_count = sum(1 for s in speakers if s == "contact_alias")
    switches = sum(
        1 for i in range(1, total) if speakers[i] != speakers[i - 1]
    )
    self_to_contact = sum(
        1
        for i in range(total - 1)
        if speakers[i] == "self" and speakers[i + 1] == "contact_alias"
    )
    self_turns_with_successor = sum(
        1 for i in range(total - 1) if speakers[i] == "self"
    )
    balance = (
        1 - abs(self_count - contact_count) / total if total else 0.0
    )
    switch_rate = switches / (total - 1) if total > 1 else 0.0
    reply_coverage = self_to_contact / max(1, self_turns_with_successor)
    return {
        "total": total,
        "self_count": self_count,
        "contact_count": contact_count,
        "switches": switches,
        "self_to_contact": self_to_contact,
        "self_turns_with_successor": self_turns_with_successor,
        "balance": balance,
        "switch_rate": switch_rate,
        "reply_coverage": reply_coverage,
    }


def _has_sufficient_data(metrics: dict[str, int | float]) -> bool:
    return (
        int(metrics["total"]) >= 6
        and int(metrics["self_count"]) >= 2
        and int(metrics["contact_count"]) >= 2
    )


def compute_affinity_score(speakers: list[str]) -> int | None:
    metrics = _compute_metrics(speakers)
    if not _has_sufficient_data(metrics):
        return None
    raw = 100 * (
        0.40 * float(metrics["balance"])
        + 0.40 * float(metrics["switch_rate"])
        + 0.20 * float(metrics["reply_coverage"])
    )
    score = int(
        Decimal(str(raw)).quantize(Decimal("1"), rounding=ROUND_HALF_UP)
    )
    return max(0, min(100, score))


def deterministic_tendency(metrics: dict[str, int | float]) -> str:
    balance = float(metrics["balance"])
    switch_rate = float(metrics["switch_rate"])
    if balance >= 0.75 and switch_rate >= 0.5:
        return ALLOWED_TENDENCIES[0]
    if balance < 0.6:
        return ALLOWED_TENDENCIES[1]
    if switch_rate < 0.4:
        return ALLOWED_TENDENCIES[2]
    return ALLOWED_TENDENCIES[3]


def deterministic_action(metrics: dict[str, int | float], score: int) -> str:
    balance = float(metrics["balance"])
    switch_rate = float(metrics["switch_rate"])
    if balance < 0.6:
        return ALLOWED_ACTIONS[1]
    if switch_rate < 0.4:
        return ALLOWED_ACTIONS[2]
    return ALLOWED_ACTIONS[0]


def insufficient_result() -> dict[str, Any]:
    return {
        "schema": ROMANCE_SCHEMA_VERSION,
        "affinity_score": None,
        "interaction_tendency": INSUFFICIENT_TENDENCY,
        "next_best_action": INSUFFICIENT_ACTION,
    }


def _strict_affinity(value: Any) -> int | None:
    if value is None:
        return None
    if type(value) is not int or isinstance(value, bool):
        raise ValueError("affinity_score must be int or null")
    if value < 0 or value > 100:
        raise ValueError("affinity_score out of range")
    return value


def validate_result(
    raw: dict[str, Any],
    *,
    deterministic_score: int | None,
    expected_tendency: str | None = None,
    expected_action: str | None = None,
) -> dict[str, Any]:
    if not isinstance(raw, dict):
        raise ValueError("result must be object")
    required = set(ROMANCE_ANALYSIS_SCHEMA["required"])
    if set(raw.keys()) != required:
        raise ValueError("key mismatch")
    if raw.get("schema") != ROMANCE_SCHEMA_VERSION:
        raise ValueError("invalid schema")
    score = _strict_affinity(raw.get("affinity_score"))
    if score != deterministic_score:
        raise ValueError("affinity_score mismatch")
    tendency = raw.get("interaction_tendency")
    action = raw.get("next_best_action")
    tendency_enum = ROMANCE_ANALYSIS_SCHEMA["properties"]["interaction_tendency"]["enum"]
    action_enum = ROMANCE_ANALYSIS_SCHEMA["properties"]["next_best_action"]["enum"]
    if tendency not in tendency_enum:
        raise ValueError("invalid interaction_tendency")
    if action not in action_enum:
        raise ValueError("invalid next_best_action")
    if deterministic_score is None:
        if tendency != INSUFFICIENT_TENDENCY:
            raise ValueError("invalid interaction_tendency")
        if action != INSUFFICIENT_ACTION:
            raise ValueError("invalid next_best_action")
    else:
        if tendency in (INSUFFICIENT_TENDENCY,) or tendency not in ALLOWED_TENDENCIES:
            raise ValueError("invalid interaction_tendency")
        if action in (INSUFFICIENT_ACTION,) or action not in ALLOWED_ACTIONS:
            raise ValueError("invalid next_best_action")
        if expected_tendency is not None and tendency != expected_tendency:
            raise ValueError("interaction_tendency mismatch")
        if expected_action is not None and action != expected_action:
            raise ValueError("next_best_action mismatch")
    return {
        "schema": ROMANCE_SCHEMA_VERSION,
        "affinity_score": score,
        "interaction_tendency": tendency,
        "next_best_action": action,
    }


def deterministic_fallback(
    metrics: dict[str, int | float], score: int,
) -> dict[str, Any]:
    return {
        "schema": ROMANCE_SCHEMA_VERSION,
        "affinity_score": score,
        "interaction_tendency": deterministic_tendency(metrics),
        "next_best_action": deterministic_action(metrics, score),
    }


def build_llm_user_prompt(metrics: dict[str, int | float], score: int) -> str:
    tendency = deterministic_tendency(metrics)
    action = deterministic_action(metrics, score)
    return (
        "# 観測量 (集計済み物理量のみ)\n"
        f"self_count: {int(metrics['self_count'])}\n"
        f"contact_count: {int(metrics['contact_count'])}\n"
        f"total_count: {int(metrics['total'])}\n"
        f"switches: {int(metrics['switches'])}\n"
        f"balance: {float(metrics['balance']):.4f}\n"
        f"switch_rate: {float(metrics['switch_rate']):.4f}\n"
        f"reply_coverage: {float(metrics['reply_coverage']):.4f}\n"
        f"affinity_score (決定済み・変更禁止): {score}\n"
        f"interaction_tendency (決定済み・変更禁止): {tendency}\n"
        f"next_best_action (決定済み・変更禁止): {action}\n\n"
        "上記の決定済み値を JSON スキーマどおりそのまま返せ。"
        "感情・好意の断定は禁止。"
    )


def _parse_llm_json(text: str) -> dict[str, Any] | None:
    cleaned = HiddenReasoningRedactor.redact_full(text).strip()
    try:
        parsed = json.loads(cleaned)
    except json.JSONDecodeError:
        return None
    return parsed if isinstance(parsed, dict) else None


def _generate_structured_romance(engine, prompt: str) -> str:
    backend = engine.backend
    if not hasattr(backend, "generate_structured"):
        raise AttributeError("backend lacks generate_structured")
    return backend.generate_structured(
        _ROMANCE_SYSTEM,
        prompt,
        ROMANCE_ANALYSIS_SCHEMA,
    )


def analyze_input(engine, raw_input: str) -> dict[str, Any]:
    sanitized = sanitize_input(raw_input)
    speakers = parse_canonical_lines(sanitized)
    metrics = _compute_metrics(speakers)
    score = compute_affinity_score(speakers)
    if score is None:
        return insufficient_result()

    expected_tendency = deterministic_tendency(metrics)
    expected_action = deterministic_action(metrics, score)
    prompt = build_llm_user_prompt(metrics, score)
    for _ in range(_MAX_RETRIES):
        try:
            text = _generate_structured_romance(engine, prompt)
            parsed = _parse_llm_json(text)
            if parsed is None:
                continue
            return validate_result(
                parsed,
                deterministic_score=score,
                expected_tendency=expected_tendency,
                expected_action=expected_action,
            )
        except (ValueError, AttributeError, TypeError):
            continue
    return deterministic_fallback(metrics, score)
