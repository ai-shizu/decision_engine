#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""
core/interview_report.py — F4b 面接成績表・評価エンジン (interview_report.v1)
==================================================================================
docs/SPEC_FOXTROT_UI.md §7 裁定3 の実装。「LLMは定性、コードは物理量」の分離:

- **軸はホワイトリスト固定** (論理性/技術力/構成力/具体性)。LLM が軸を
  発明したり evidence を欠いたまま採点した項目はここで削る (W-37 — UI は
  検証済み構造体のみを受け取り、JSON.parse を書かない)。
- **latency はコードが実測値から合成する** — LLM の出力に latency を書かせる
  ことは絶対に許さない (憲法2: 物理量はコード、言語化のみLLM)。
- JSON 生成の信頼性は narrative_compiler.py と同一パターン (スキーマ検証→
  リトライ→上限で諦めて summary (講評テキスト) のみで返す。専用機構は
  新設しない)。

【壁A (W-38) — 変更禁止】このモジュールが `INTERVIEW_RECORDS_DIR` の
唯一の書き手/読み手であること。profiler/gap_analysis/tensor_store から
このパスを参照してはならない (`_assert_no_gap_leak` の鏡像テストで検出)。
"""

from __future__ import annotations

import json
import re
from datetime import datetime

from .paths import INTERVIEW_RECORDS_DIR

SCHEMA_VERSION = "interview_report.v1"
MAX_RETRIES = 2

# 軸ホワイトリスト固定 (SPEC 裁定3) — この4軸以外を LLM が発明したら
# パース時に削る。増減は SPEC 改訂が必要 (勝手に増やすな)。
AXIS_WHITELIST = ("論理性", "技術力", "構成力", "具体性")

_JSON_BLOCK_RE = re.compile(r"\{.*\}", re.DOTALL)


def _extract_json_block(text: str) -> dict | None:
    """LLM 出力からコードブロック等に包まれていても JSON 本体を寛容に抽出する。"""
    m = _JSON_BLOCK_RE.search(text)
    if not m:
        return None
    try:
        parsed = json.loads(m.group(0))
    except json.JSONDecodeError:
        return None
    return parsed if isinstance(parsed, dict) else None


def _parse_metrics(raw: dict | None) -> list[dict]:
    """軸ホワイトリスト・スコア clamp・evidence 必須を強制する決定論パス。

    ここを通過した dict のみが UI に届く (W-37: UI は JSON.parse を書かない)。
    証拠の無い採点・ホワイトリスト外の軸・重複軸はすべて無条件に削る。"""
    if not isinstance(raw, dict):
        return []
    items = raw.get("metrics")
    if not isinstance(items, list):
        return []
    out: list[dict] = []
    seen: set[str] = set()
    for it in items:
        if not isinstance(it, dict):
            continue
        axis = str(it.get("axis", "")).strip()
        if axis not in AXIS_WHITELIST or axis in seen:
            continue
        evidence = str(it.get("evidence", "")).strip()
        if not evidence:
            continue  # 証拠のない採点は削る (§6.2-3 反証可能性倫理の面接版)
        try:
            score = int(round(float(it.get("score", 0))))
        except (TypeError, ValueError):
            continue
        score = max(0, min(100, score))
        seen.add(axis)
        out.append({"axis": axis, "score": score, "evidence": evidence})
    return out


def synthesize_latency(latencies: list[dict]) -> dict:
    """憲法2の直接適用: 物理量はコードが集計する。LLM には一切書かせない。"""
    secs = sorted(float(l["sec"]) for l in latencies if "sec" in l)
    if not secs:
        return {"median_sec": 0.0, "max_sec": 0.0, "n": 0}
    n = len(secs)
    mid = n // 2
    median = secs[mid] if n % 2 else (secs[mid - 1] + secs[mid]) / 2
    return {"median_sec": round(median, 1), "max_sec": round(secs[-1], 1), "n": n}


def _metrics_prompt(transcript_text: str, summary: str, retry: bool) -> str:
    axis_list = "、".join(AXIS_WHITELIST)
    prompt = (
        f"# 議論トランスクリプト\n{transcript_text or '(発言なし)'}\n\n"
        f"# 講評\n{summary}\n\n"
        f"上記を踏まえ、候補者を次の{len(AXIS_WHITELIST)}軸【のみ】で評価し、"
        "JSON のみを出力せよ (前後に説明文・コードブロック記号を書かない):\n"
        f'{{"metrics": [{{"axis": "<{axis_list} のいずれか1つ>", '
        '"score": <0から100の整数>, "evidence": "<議論からの短い引用>"}, ...]}\n'
        f"{axis_list} の4軸すべてを1つずつ出力し、各軸に議論からの"
        "具体的な引用 (evidence) を必ず付けること。"
    )
    if retry:
        prompt += (
            "\n\n(前回の出力は無効でした。指定の JSON 形式のみを、"
            f"{axis_list} の4軸すべてに evidence 付きで出力し直してください)")
    return prompt


def generate_report(engine, system: str, transcript_text: str, summary: str,
                    config: dict | None, latencies: list[dict],
                    max_retries: int = MAX_RETRIES) -> dict:
    """4軸評価 JSON をスキーマ検証→リトライ→諦めのパターンで確定させ、
    interview_report.v1 を組み立てる (narrative_compiler と同一パターン)。

    engine は `.backend.generate(system, user)` を持つダックタイピング
    (ConsultationEngine 相当。テストは軽量なスクリプト付きバックエンドで代替可能)。
    metrics が最後まで揃わなくても summary/latency は必ず返す (退化フォールバック
    — SPEC: 「リトライ→上限で諦めて講評テキストのみ返す」)。"""
    metrics: list[dict] = []
    attempts = 0
    while len(metrics) < len(AXIS_WHITELIST) and attempts <= max_retries:
        prompt = _metrics_prompt(transcript_text, summary, retry=attempts > 0)
        text = engine.backend.generate(system, prompt)
        text = re.sub(r"<think>.*?</think>\s*", "", text, flags=re.DOTALL).strip()
        metrics = _parse_metrics(_extract_json_block(text))
        attempts += 1

    return {
        "schema": SCHEMA_VERSION,
        "date": datetime.now().isoformat(timespec="seconds"),
        "config": config or {},
        "metrics": metrics,
        "summary": summary,
        "latency": synthesize_latency(latencies),
        "simulated": True,
    }


def persist_report(report: dict, genre: str) -> str:
    """壁A: `data/records/interviews/` への永続化。専用インデックスは作らない
    (ファイル名 = 日時+ジャンルが台帳そのもの。IMP-1 の教訓 — 台帳の複雑化を
    避ける)。W-32: 実名・ES本文はここへ複写しない (呼び出し側の責務)。"""
    INTERVIEW_RECORDS_DIR.mkdir(parents=True, exist_ok=True)
    ts = datetime.now().strftime("%Y%m%dT%H%M%S")
    slug = re.sub(r"[^\w\-]+", "_", genre or "general").strip("_")[:30] or "general"
    path = INTERVIEW_RECORDS_DIR / f"interview_{ts}_{slug}.json"
    path.write_text(json.dumps(report, ensure_ascii=False, indent=2), encoding="utf-8")
    return str(path)
