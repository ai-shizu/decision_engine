#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""
core/narrative_compiler.py — NARRATIVE COMPILER (Target Delta D3)
==================================================================================
docs/SPEC_CHARLIE_DELTA.md §3.5 の実装。泥臭いプロファイル (gap_analysis が
検出した証拠付きギャップ) を、一貫した ES ドラフトと、その戦略を解説する
「Recruiter's Eye」に昇華する。

【スコープの明示的な縮小 (Architect's Override)】
SPEC 原案は HumanSourceCode (5軸) と HistoricalNode (信頼済み事実グラフ) を
入力に想定していたが、これらは Target Delta D1 (未着手) の産物であり本
セッションには存在しない。今回は既に実在するデータ — `deep_profile.json`
の `gap_analysis.gaps` (true_gakuchika を含む、証拠付きの決定論的ギャップ) —
のみを素材として使う。存在しないデータを参照する設計より、実在するデータで
確実に動く設計を選んだ。

【幻覚防止の核心】
LLM には「素材にない事実の創作禁止」「各段落末尾に [ref:N] タグ必須」を
指示する。生成後、決定論パスで各段落の参照タグを検証し、有効な参照を
持たない段落は「コンパイルエラー」として破棄する (スタイルの問題ではなく
正誤の問題として扱う)。

【Recruiter's Eye は別呼び出しで生成し、ES 本文とは別チャネルで返す】
data/es/draft_*.md に書き込むのは es_text のみ。Recruiter's Eye (メタ解説)
を同じファイルに混入すると、es_review モードの「ドキュメント単体評価」原則
(§7.1.1) をメタ解説ごと汚染する。UI 表示用のメタデータとして result dict
だけに含める。
"""

from __future__ import annotations

import hashlib
import json
import re

from .paths import DEEP_PROFILE, ES_DIR

MAX_MATERIAL_ITEMS = 5
MAX_RETRIES = 2
_REF_RE = re.compile(r"\[ref:(\d+)\]")

# true_gakuchika (真の熱量) を最優先で採用する — SPEC §3.5 の決定論パス方針。
_TYPE_PRIORITY = {
    "true_gakuchika": 0, "task_avoidance": 1, "stabilizer_effect": 2,
    "intention_gap": 3, "blind_spot": 3, "intellectualization_gap": 4,
}


def _select_material(gap_result: dict, max_items: int = MAX_MATERIAL_ITEMS) -> list[dict]:
    """証拠 (quotes または calendar/LINE evidence) を持つギャップだけを素材化する。
    証拠の無いギャップは「反証可能性」の原則 (AI_SKILLS §6.2-3) に反するため使わない。"""
    items = []
    for g in gap_result.get("gaps", []):
        evidence: list[str] = []
        for q in g.get("subjective", {}).get("quotes", []) or []:
            evidence.append(f"[{q.get('date', '?')}] {q.get('quote', '')}")
        obj = g.get("objective", {})
        obj_evidence = obj.get("evidence") if isinstance(obj, dict) else None
        if isinstance(obj_evidence, dict):
            for title in obj_evidence.get("events", [])[:3]:
                evidence.append(f"予定: {title}")
            for line_ev in obj_evidence.get("line", [])[:2]:
                evidence.append(f"[{line_ev.get('date', '?')}] LINE言及: {line_ev.get('keyword', '')}")
        if not evidence:
            continue
        items.append({"theme": g["theme"], "type": g["type"],
                     "insight": g.get("insight", ""), "evidence": evidence})
    items.sort(key=lambda it: _TYPE_PRIORITY.get(it["type"], 9))
    return items[:max_items]


def _material_block(material: list[dict]) -> str:
    lines = []
    for i, m in enumerate(material):
        lines.append(f"[{i + 1}] テーマ: {m['theme']} (種別: {m['type']})")
        lines.append(f"    洞察: {m['insight']}")
        lines.append(f"    証拠: {' / '.join(m['evidence'])}")
    return "\n".join(lines)


def _parse_claims(text: str, n_material: int) -> tuple[str, list[dict]]:
    """[ref:N] タグを持つ段落だけを採用する。タグ無し段落は幻覚扱いで破棄。"""
    paragraphs = [p.strip() for p in re.split(r"\n\s*\n", text) if p.strip()]
    claims: list[dict] = []
    kept: list[str] = []
    for p in paragraphs:
        refs = sorted({int(m) for m in _REF_RE.findall(p)})
        valid_refs = [r for r in refs if 1 <= r <= n_material]
        if not valid_refs:
            continue
        clean = _REF_RE.sub("", p).strip()
        if not clean:
            continue
        claims.append({"text": clean, "node_refs": valid_refs})
        kept.append(clean)
    return "\n\n".join(kept), claims


def _persist_draft(target_domain: str, es_text: str) -> str:
    slug = re.sub(r"[^\w\-]+", "_", target_domain).strip("_")[:30] or "draft"
    ES_DIR.mkdir(parents=True, exist_ok=True)
    path = ES_DIR / f"draft_{slug}.md"
    path.write_text(f"# {target_domain} (AI生成ドラフト)\n\n{es_text}\n", encoding="utf-8")
    return str(path)


def compile_narrative(engine, target_domain: str | None = None,
                      max_retries: int = MAX_RETRIES) -> dict:
    """gap_analysis の証拠付きギャップから ES ドラフト + Recruiter's Eye を生成する。

    engine は ConsultationEngine 相当 (.backend.generate(system, user) を持つ
    ダックタイピング。テストでは FakeBackend を積んだインスタンスで差し替え可能)。
    """
    if not DEEP_PROFILE.exists():
        return {"ok": False, "reason": "no_deep_profile", "es_text": "", "claims": [],
                "recruiters_eye": "(profiler.py 未実行のため素材がありません)",
                "compiled_from": ""}
    try:
        profile = json.loads(DEEP_PROFILE.read_text(encoding="utf-8"))
    except (json.JSONDecodeError, OSError):
        return {"ok": False, "reason": "profile_unreadable", "es_text": "", "claims": [],
                "recruiters_eye": "(deep_profile.json の読込に失敗しました)",
                "compiled_from": ""}

    gap_result = profile.get("gap_analysis", {})
    material = _select_material(gap_result)
    if not material:
        return {"ok": False, "reason": "no_material", "es_text": "", "claims": [],
                "recruiters_eye": "(証拠付きの素材が不足しています。日記・予定・LINE等の"
                                 "記録を増やしてから再実行してください)",
                "compiled_from": ""}

    if target_domain is None:
        from .es_manager import select_es
        es = select_es(None)
        target_domain = es["target_domain"] if es else "志望職種"

    block = _material_block(material)
    system = (
        "あなたはトップ層の就職エントリーシート (ES) を書くライターである。"
        "与えられた『素材』に記載された事実のみを用いて文章を構成する。"
        "素材に無い事実を創作することは絶対に禁止する。"
        "各段落の末尾に、その段落が根拠とした素材番号を [ref:N] の形式で"
        "必ず1つ以上付与すること (N は素材の番号、複数可)。"
    )
    prompt = (f"# 志望領域\n{target_domain}\n\n# 素材\n{block}\n\n"
             "上記の素材のみを用いて、ES 本文を3〜4段落で作成せよ。")

    text = _generate_clean(engine, system, prompt)
    es_text, claims = _parse_claims(text, len(material))

    attempts = 0
    while not claims and attempts < max_retries:
        attempts += 1
        retry_prompt = prompt + (
            "\n\n(前回の出力には有効な [ref:N] タグを持つ段落がありませんでした。"
            "各段落に必ず1つ以上、素材番号を付与し直してください)")
        text = _generate_clean(engine, system, retry_prompt)
        es_text, claims = _parse_claims(text, len(material))

    eye_system = ("あなたは採用戦略を解説するキャリアコンサルタントである。"
                 "候補者本人向けに、なぜこの構成・事実を選んだのかを解説する。")
    eye_prompt = (
        f"# 志望領域\n{target_domain}\n\n# 使用した素材\n{block}\n\n"
        f"# 生成した ES 本文\n{es_text or '(有効な本文なし)'}\n\n"
        "この構成判断について300字程度で解説せよ。必ず次を含めること: "
        "(1) どの弱点を先回りして隠したか (2) どの定量証拠がどの評価軸 "
        "(再現性・主体性・規模) を撃つか。")
    recruiters_eye = _generate_clean(engine, eye_system, eye_prompt)

    compiled_from = hashlib.blake2b(
        (json.dumps(material, ensure_ascii=False, sort_keys=True) + target_domain)
        .encode("utf-8"), digest_size=8).hexdigest()

    result = {
        "ok": bool(claims), "es_text": es_text, "claims": claims,
        "recruiters_eye": recruiters_eye, "compiled_from": compiled_from,
        "target_domain": target_domain,
    }
    if not claims:
        result["reason"] = "no_valid_claims"
        return result

    draft_path = _persist_draft(target_domain, es_text)
    result["draft_path"] = draft_path
    from .consultation_log import append_consultation
    append_consultation(
        f"[narrative_compile] {target_domain}",
        f"{es_text}\n\n[Recruiter's Eye]\n{recruiters_eye}", simulated=True)
    return result


def _generate_clean(engine, system: str, prompt: str) -> str:
    text = engine.backend.generate(system, prompt)
    return re.sub(r"<think>.*?</think>\s*", "", text, flags=re.DOTALL).strip()
