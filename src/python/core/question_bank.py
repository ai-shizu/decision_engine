#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""
core/question_bank.py — Puppeteer (黒幕) の無菌化質問バンク (Target Delta D3)
==================================================================================
docs/SPEC_CHARLIE_DELTA.md §3.4 の実装。

【設計の核心 — 「生成」ではなく「選択」による構成的無菌化】
QUESTION_BANK は個人情報を1文字も含まない、完全に一般的な面接質問の静的辞書。
Puppeteer (select_question) は tension の高い Bounty (矛盾) の type に一致する
バンク質問を決定論的に選ぶだけであり、Bounty の theme/insight/tension を
一切読まずに質問文を作る (=LLM によるリライト/生成) ことは絶対にしない。

面接官へ渡ってよいのは選ばれた質問の**テキストだけ**。なぜその質問が選ばれた
のか (どの Bounty を突くためか) は、講評フェーズ (gap 統合が合法な唯一の場)
まで一切開示しない (不変条件 I-11, I-14)。
"""

from __future__ import annotations

# 各質問は個人情報ゼロの一般的な面接質問。type は gap_analysis / line_telemetry
# が出す gap の "type" と対応させ、Puppeteer がどの矛盾に刺さる質問かを
# 選ぶための決定論的なタグとして使う (質問文そのものには一切影響しない)。
QUESTION_BANK: dict[str, dict] = {
    "qb-001": {"type": "task_avoidance",
              "text": "計画通りに進まなかった経験と、その時どう軌道修正したかを教えてください"},
    "qb-002": {"type": "task_avoidance",
              "text": "苦手なタスクに直面したとき、具体的にどう向き合いますか"},
    "qb-003": {"type": "true_gakuchika",
              "text": "学生時代に最も時間とエネルギーを注いだ活動について教えてください"},
    "qb-004": {"type": "true_gakuchika",
              "text": "誰に頼まれたわけでもないのに続けていたことはありますか"},
    "qb-005": {"type": "intellectualization_gap",
              "text": "考えるだけでなく実際に行動へ移せなかった経験はありますか"},
    "qb-006": {"type": "intellectualization_gap",
              "text": "抽象的な議論と具体的な実行、どちらが得意で、その理由は何ですか"},
    "qb-007": {"type": "stabilizer_effect",
              "text": "オンとオフの切り替えについて、意識していることを教えてください"},
    "qb-008": {"type": "intention_gap",
              "text": "チームの中で自分はどんな役割を担うことが多いですか"},
    "qb-009": {"type": "intention_gap",
              "text": "会話や議論で、自分が話す量と聞く量のバランスをどう捉えていますか"},
    "qb-010": {"type": "blind_spot",
              "text": "人間関係を維持するために普段どんな工夫をしていますか"},
    "qb-011": {"type": "blind_spot",
              "text": "周囲との関わりの中で、自分では気づいていない可能性のある癖はありますか"},
    "qb-012": {"type": "friction_response",
              "text": "意見が対立した相手とその後どう関係を続けましたか"},
}


def select_question(bounties: list[dict], *, k: int = 1) -> list[dict]:
    """open な Bounty を tension 降順で走査し、type が一致するバンク質問を
    決定論的に選ぶ (乱数禁止)。既出のバンク質問 (他の Bounty で既に
    bank_question_id として使われたもの) は避ける — Bounty.bank_question_id
    がそのまま「既出」の記録になっている (専用の永続化を新設しない)。

    型が一致するバンク質問が尽きた場合のみ、汎用フォールバックとして
    未使用の質問から辞書順で選ぶ (unmatched でも面接を止めないため)。

    戻り値: [{"bounty_id":..., "question_id":..., "text":...}, ...] (最大 k 件)
    """
    used = {b["bank_question_id"] for b in bounties if b.get("bank_question_id")}
    open_bounties = sorted(
        (b for b in bounties if b.get("status") == "open"),
        key=lambda b: (-b["tension"], b["id"]))   # tension 降順、同値は id で安定化

    selected: list[dict] = []
    for b in open_bounties:
        if len(selected) >= k:
            break
        matching = sorted(qid for qid, q in QUESTION_BANK.items()
                          if q["type"] == b["type"] and qid not in used)
        candidates = matching or sorted(
            qid for qid in QUESTION_BANK if qid not in used)
        if not candidates:
            continue
        qid = candidates[0]
        selected.append({"bounty_id": b["id"], "question_id": qid,
                         "text": QUESTION_BANK[qid]["text"]})
        used.add(qid)
    return selected
