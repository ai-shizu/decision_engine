#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""
動的 ES (エントリーシート / 企画書) 管理
==========================================
data/es/ 配下の Markdown / テキストを読み込み、本文から
「ターゲットドメイン (志望業界・職種)」を動的に抽出する。

【ドメイン非依存の原則 — 変更禁止】
テック・金融・クリエイティブ等、特定業界の if-elif をここに書いてはならない。
ドメインは常に「ES テキスト自身」から導出する:
  1. 明示フィールド (志望業界: / 志望職種: 等) があれば最優先
  2. なければ頻度ベースのキーワード抽出 (英数トークン・カタカナ語・漢字連続)
抽出結果はシミュレーター (interview_sim / gd_sim / es_review) のペルソナ生成に
使われ、面接官・添削者の専門性は ES が語る領域に自動追従する。
"""

from __future__ import annotations

import re
from pathlib import Path

from .paths import ACTIVE_ES

_EXPLICIT_DOMAIN_RE = re.compile(
    r"^(?:志望業界|志望職種|応募職種|応募先|ターゲット(?:ドメイン)?)\s*[:：]\s*(.+)$",
    re.MULTILINE,
)
# 英数トークン (C++/OpenGL/HFT 等) / カタカナ語 / 漢字連続 を語彙として拾う
_TOKEN_RE = re.compile(r"[A-Za-z][A-Za-z0-9+#.]+|[ァ-ヶー]{3,}|[一-鿿]{2,6}")

# ES 定型語 (どの業界の ES にも現れるためドメイン信号にならない)
_STOPWORDS = {
    "こと", "もの", "ため", "経験", "学生", "時代", "学生時代", "貴社", "御社",
    "活動", "自分", "自身", "目標", "課題", "結果", "成果", "取り組み", "以下",
    "エントリー", "シート", "エントリーシート", "志望", "動機", "理由", "強み",
    "について", "考え", "気持ち", "入社", "仕事",
}

MAX_KEYWORDS = 8
MAX_BODY_CHARS = 2500  # プロンプト注入時の本文上限


def _read_text_lenient(path: Path) -> str:
    """UTF-8 第一、cp932 フォールバック (consultation_engine と同方針)。"""
    raw = path.read_bytes()
    try:
        return raw.decode("utf-8")
    except UnicodeDecodeError:
        try:
            return raw.decode("cp932")
        except UnicodeDecodeError:
            return raw.decode("utf-8", errors="replace")


def extract_keywords(text: str, limit: int = MAX_KEYWORDS) -> list[str]:
    """頻度順のドメイン語彙 (同数なら初出順)。決定論的。"""
    counts: dict[str, int] = {}
    order: dict[str, int] = {}
    for i, m in enumerate(_TOKEN_RE.finditer(text)):
        tok = m.group(0)
        if tok in _STOPWORDS or len(tok) < 2:
            continue
        counts[tok] = counts.get(tok, 0) + 1
        order.setdefault(tok, i)
    ranked = sorted(counts, key=lambda t: (-counts[t], order[t]))
    return ranked[:limit]


def extract_target_domain(text: str) -> dict:
    """ES テキストからターゲットドメインを抽出する。"""
    keywords = extract_keywords(text)
    m = _EXPLICIT_DOMAIN_RE.search(text)
    if m:
        return {"domain": m.group(1).strip(), "explicit": True,
                "keywords": keywords}
    domain = "・".join(keywords[:4]) if keywords else "(ドメイン不明)"
    return {"domain": domain, "explicit": False, "keywords": keywords}


def _parse_es(path: Path) -> dict:
    text = _read_text_lenient(path).strip()
    title = path.stem
    hm = re.search(r"^#\s+(.+)$", text, re.MULTILINE)
    if hm:
        title = hm.group(1).strip()
    domain = extract_target_domain(text)
    return {
        "name": path.stem,
        "path": str(path),
        "title": title,
        "body": text,
        "target_domain": domain["domain"],
        "explicit_domain": domain["explicit"],
        "keywords": domain["keywords"],
        "mtime": path.stat().st_mtime,
    }


def load_es_documents() -> list[dict]:
    """保持する ES は常に active_es.md ただ1件 (F-16)。

    W-53: 真実の源は ACTIVE_ES ただ一つ。ES_DIR 内の他ファイル (レガシー) は
    削除されず物理的に残りうるが、この読み手からは構造的に不可視 —
    active_es.md 以外を読む経路をここに新設しないこと。
    """
    if not ACTIVE_ES.exists():
        return []
    return [_parse_es(ACTIVE_ES)]


def select_es(name: str | None = None) -> dict | None:
    """常に active_es.md を返す (無ければ None)。

    F-16 による単一化で `name` は意味を失った。呼び出し側の互換のため
    引数は残すが、無視する (W-53)。
    """
    docs = load_es_documents()
    return docs[0] if docs else None


def get_active_es() -> dict | None:
    """ImportTab の ES_ACTIVE パネル (View 専用) 向け。

    ACTIVE_ES が無ければ None。在れば `_parse_es` の全フィールドに加え
    `char_count` (本文の View 表示用) を持つ dict を返す。
    """
    if not ACTIVE_ES.exists():
        return None
    doc = _parse_es(ACTIVE_ES)
    doc["char_count"] = len(doc["body"])
    return doc


def es_body_for_prompt(es: dict) -> str:
    body = es["body"]
    if len(body) > MAX_BODY_CHARS:
        body = body[:MAX_BODY_CHARS] + "\n…(以下略)"
    return body


def build_interviewer_persona(es: dict, stance: str = "adversarial") -> str:
    """ES のドメインに追従する面接官ペルソナ (業界ハードコードなし)。

    F-18 (SPEC_FOXTROT_UI.md §10.4): stance は "adversarial" (既定) または
    "standard"。未知値は adversarial にフォールバック (既存のストレステスト
    契約を無断で弱めない)。
    """
    kw = "、".join(es["keywords"][:6]) or "(語彙抽出なし)"
    header = (
        f"あなたは「{es['target_domain']}」領域のトップ組織で採用と専門評価を"
        "長年担当してきた面接官である。この領域の専門語彙"
        f" ({kw}) を正確に扱い、候補者の主張の裏を取る。\n"
    )
    if stance == "standard":
        style = (
            "面接スタイル (Standard):\n"
            "- 提出された ES の記載内容を根拠に、深掘り質問で理解を確かめる。\n"
            "- 前提の曖昧さや数字の根拠を丁寧に問い、必要なら考える足場を与える。\n"
            "- 圧迫や誘導尋問はしない。候補者が力を出せるよう建設的に進める。\n"
            "- ただし人格攻撃はしない。攻撃対象は常に論理と事実。\n"
            "- 一度に1つの問いだけを投げる。長い講義をしない。"
        )
    else:
        style = (
            "面接スタイル (Adversarial):\n"
            "- 提出された ES の記載内容【のみ】を根拠に質問する。ES にない情報を"
            "勝手に補完して助け舟を出さない。\n"
            "- ES の矛盾・誇張・技術的/戦略的選択の妥当性を、悪意を持った"
            "圧迫質問で攻撃せよ。「本当に一人でやったのか」「その選択は"
            "他の選択肢と比較したのか」等、防御の甘い箇所を執拗に突く。\n"
            "- ただし人格攻撃はしない。攻撃対象は常に論理と事実。\n"
            "- 一度に1つの問いだけを投げる。長い講義をしない。"
        )
    return header + style


def build_reviewer_persona(es: dict) -> str:
    """ES 添削者ペルソナ。日常プロファイル (gap_insights) は使わせない前提。"""
    kw = "、".join(es["keywords"][:6]) or "(語彙抽出なし)"
    return (
        f"あなたは「{es['target_domain']}」領域のトップ組織で書類選考を担当する"
        f"採用責任者である。専門語彙: {kw}。\n"
        "添削スタイル:\n"
        "- ドキュメント単体の論理的強度のみを評価する。書き手の人格・背景・"
        "日常の行動は一切考慮しない (与えられてもいない)。\n"
        "- 論理破綻・因果の飛躍・定量的根拠の欠如・再現性の不明瞭さを"
        "容赦なく指摘する。誉め言葉で薄めない。\n"
        "- 指摘には必ず該当箇所の引用を付け、書き直し例を示す。"
    )
