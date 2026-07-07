#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""user_profile.json の読み書き (SETTINGS / 相談エンジン共通)。"""

from __future__ import annotations

import json
import re

from .paths import USER_PROFILE
from .text_utils import sanitize_obj, sanitize_text

FIXED_ATTRIBUTES = {
    "birthday": "",
    "gender": "",
    "height": "",
    "weight": "",
    "address": "",
    "occupation": "",
    # T-21 (IMP-2): is_self 判定 tier1 の明示上書き用。SETTINGS UI には未露出
    # (Foxtrot 凍結中) — 直接 JSON 編集 or 将来の API 経由でのみ設定可能。
    "line_self_name": "",
}

FIXED_ATTRIBUTE_LABELS = {
    "birthday": "誕生日",
    "gender": "性別",
    "height": "身長 (cm)",
    "weight": "体重 (kg)",
    "address": "住所",
    "occupation": "勤務先/学校",
}

FIXED_ATTRIBUTE_FIELDS = list(FIXED_ATTRIBUTE_LABELS.items())

_DATE_RE = re.compile(r"^\d{4}-\d{2}-\d{2}$")


def _normalize_fixed(raw: dict) -> dict:
    fixed = dict(FIXED_ATTRIBUTES)
    for key in FIXED_ATTRIBUTES:
        if key in raw:
            fixed[key] = sanitize_text(str(raw.get(key, "")).strip())
    legacy_age = sanitize_text(str(raw.get("age", "")).strip())
    if legacy_age and not fixed["birthday"] and _DATE_RE.match(legacy_age):
        fixed["birthday"] = legacy_age
    return fixed


def load_user_profile() -> dict:
    empty = {
        "schema": "user_profile.v2",
        "fixed_attributes": dict(FIXED_ATTRIBUTES),
        "inferred_profile": {},
        "auto_extracted": {},
    }
    if not USER_PROFILE.exists():
        return empty
    try:
        prev = json.loads(USER_PROFILE.read_text(encoding="utf-8"))
    except (json.JSONDecodeError, OSError):
        return empty

    merged = dict(prev.get("fixed_attributes", {}))
    merged.update(prev.get("attributes", {}))
    fixed = _normalize_fixed(merged)

    if prev.get("schema") == "user_profile.v2":
        return sanitize_obj({
            "schema": "user_profile.v2",
            "fixed_attributes": fixed,
            "inferred_profile": prev.get("inferred_profile", {}),
            "auto_extracted": prev.get("auto_extracted", {}),
        })
    return sanitize_obj({
        "schema": "user_profile.v2",
        "fixed_attributes": fixed,
        "inferred_profile": {},
        "auto_extracted": prev.get("auto_extracted", {}),
    })


def save_fixed_attributes(attrs: dict) -> None:
    profile = load_user_profile()
    cleaned = {
        k: sanitize_text(str(v).strip())
        for k, v in attrs.items()
        if k in FIXED_ATTRIBUTES
    }
    if cleaned.get("birthday") and not _DATE_RE.match(cleaned["birthday"]):
        raise ValueError("誕生日は YYYY-MM-DD 形式で入力してください")
    profile["fixed_attributes"].update(cleaned)
    USER_PROFILE.parent.mkdir(parents=True, exist_ok=True)
    USER_PROFILE.write_text(
        json.dumps(sanitize_obj(profile), ensure_ascii=False, indent=2),
        encoding="utf-8",
    )


def format_user_profile_summary() -> str:
    p = load_user_profile()
    inferred = p.get("inferred_profile", {})
    auto = p.get("auto_extracted", {})
    lines: list[str] = []

    updated = inferred.get("updated_at") or auto.get("updated_at")
    if updated:
        lines.append(f"最終更新: {sanitize_text(str(updated))}")
    lines.append("※ 日記・LINE・相談・家計簿から自動生成 (手入力不可)")
    lines.append("")

    narrative = inferred.get("meta_narrative", "")
    if narrative:
        lines += ["【自己モデル (抽象)】", sanitize_text(str(narrative)), ""]

    factual = inferred.get("factual_signals", {})
    cats = factual.get("categories", {})
    if cats:
        lines.append("【推定コンテキスト】")
        for cat, labels in cats.items():
            lines.append(f"  {sanitize_text(str(cat))}: {', '.join(sanitize_text(str(x)) for x in labels)}")
        goals = factual.get("stated_goals", [])
        if goals:
            lines.append(f"  言及された目標: {' / '.join(sanitize_text(str(g)) for g in goals)}")
        lines.append("")

    abstract = inferred.get("abstract_identity", {})
    drives = abstract.get("core_drives", [])
    if drives:
        lines.append("【コアドライブ】")
        for d in drives:
            w = d.get("weight", 0)
            lines.append(
                f"  - {sanitize_text(str(d.get('drive', '?')))} ({w:.0%}): "
                f"{sanitize_text(str(d.get('meaning', '')))}",
            )
        lines.append("")

    style = abstract.get("cognitive_style", {})
    if style.get("label"):
        lines += ["【認知スタイル】", f"  {sanitize_text(str(style['label']))}", ""]
        for b in style.get("biases", [])[:3]:
            lines.append(
                f"  · {sanitize_text(str(b.get('name')))}: "
                f"{sanitize_text(str(b.get('abstract_pattern', '')))}",
            )

    tensions = abstract.get("internal_tensions") or auto.get("internal_tensions", [])
    if tensions:
        lines.append("")
        lines.append("【内的葛藤】")
        for t in tensions[:3]:
            if isinstance(t, dict):
                lines.append(f"  - {sanitize_text(str(t.get('tension', t)))}")
            else:
                lines.append(f"  - {sanitize_text(str(t))}")

    abs_rules = auto.get("abstract_heuristics") or [
        h.get("abstract_rule") for h in abstract.get("decision_heuristics", [])
    ]
    if abs_rules:
        lines += ["", "【抽象化された意思決定ヒューリスティック】"]
        for r in abs_rules[:5]:
            if r:
                lines.append(f"  - {sanitize_text(str(r))}")

    values = auto.get("value_hierarchy", [])
    if values:
        lines += ["", "【価値観 (重み順)】"]
        for v in values[:5]:
            lines.append(
                f"  {v.get('rank', '?')}. {sanitize_text(str(v.get('value')))} → "
                f"{sanitize_text(str(v.get('root_need')))} ({v.get('weight', 0):.0%})",
            )

    biases = auto.get("dominant_biases", [])
    if biases:
        lines += ["", "【認知バイアス】"]
        for b in biases:
            lines.append(
                f"  - {sanitize_text(str(b.get('bias')))} "
                f"(強度 {b.get('intensity', 0):.0%})",
            )

    llm = sanitize_text(str(inferred.get("llm_deep_synthesis", "")).strip())
    if llm:
        lines += ["", "【LLM深層分析 (抜粋)】", llm[:1200]]
        if len(llm) > 1200:
            lines.append("… (続きは deep_profile.json を参照)")

    if len(lines) <= 3:
        lines += [
            "",
            "(プロフィール未生成)",
            "RECORD / IMPORT でデータを蓄積後、「再分析 (profiler)」を実行してください。",
        ]
    return sanitize_text("\n".join(lines))
