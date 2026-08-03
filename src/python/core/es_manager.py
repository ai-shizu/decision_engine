#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""
動的 ES (エントリーシート / 企画書) 管理
==========================================
data/es/ 配下の Markdown を企業名付きで複数保持する (M20-N)。

【ドメイン非依存の原則 — 変更禁止】
テック・金融・クリエイティブ等、特定業界の if-elif をここに書いてはならない。
ドメインは常に「ES テキスト自身」から導出する:
  1. 明示フィールド (志望業界: / 志望職種: 等) があれば最優先
  2. なければ頻度ベースのキーワード抽出 (英数トークン・カタカナ語・漢字連続)
抽出結果はシミュレーター (interview_sim / gd_sim / es_review) のペルソナ生成に
使われ、面接官・添削者の専門性は ES が語る領域に自動追従する。

【企業別保持 (M20-N)】
各ファイル先頭の「企業名: …」が会社キー。draft_*.md は一覧から除外。
select_es(id) は stem / 企業名で解決。id が空/none なら None (ゼロベース面接)。
"""

from __future__ import annotations

import re
from pathlib import Path

from .paths import ACTIVE_ES, ES_DIR

_EXPLICIT_DOMAIN_RE = re.compile(
    r"^(?:志望業界|志望職種|応募職種|応募先|ターゲット(?:ドメイン)?)\s*[:：]\s*(.+)$",
    re.MULTILINE,
)
_COMPANY_RE = re.compile(r"^企業名\s*[:：]\s*(.+)$", re.MULTILINE)
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
_NONE_IDS = frozenset({"", "none", "__none__", "null", "zero", "ゼロベース"})


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


def extract_company_name(text: str, fallback: str = "") -> str:
    m = _COMPANY_RE.search(text)
    if m:
        return m.group(1).strip()
    return (fallback or "").strip()


def company_slug(company: str) -> str:
    raw = (company or "").strip() or "company"
    slug = re.sub(r"[^\w\-ぁ-んァ-ヶ一-鿿]+", "_", raw).strip("_")[:48]
    return slug or "company"


# 法人格・括弧表記の揺れを落とす (業界依存の if-elif は禁止 — 形式だけ)
_CORP_SUFFIX_RE = re.compile(
    r"(株式会社|有限会社|合同会社|合名会社|合資会社|"
    r"\(株\)|（株）|\(有\)|（有）|㈱|㈲|"
    r"Inc\.?|Corp\.?|Ltd\.?|LLC|Co\.,?\s*Ltd\.?)",
    re.IGNORECASE,
)
_SIMILARITY_THRESHOLD = 0.82


def normalize_company_key(name: str) -> str:
    """表記揺れ比較用の正規化キー (保存名そのものは変えない)。"""
    import unicodedata

    text = unicodedata.normalize("NFKC", (name or "").strip())
    text = _CORP_SUFFIX_RE.sub("", text)
    text = re.sub(r"[\s　・･./／\-ー—_]+", "", text)
    return text.casefold()


def company_name_similarity(a: str, b: str) -> float:
    """0..1。正規化キーの一致 / 包含 / SequenceMatcher。"""
    from difflib import SequenceMatcher

    ka, kb = normalize_company_key(a), normalize_company_key(b)
    if not ka or not kb:
        return 0.0
    if ka == kb:
        return 1.0
    if ka in kb or kb in ka:
        shorter, longer = (ka, kb) if len(ka) <= len(kb) else (kb, ka)
        if len(shorter) >= 2:
            return max(0.88, len(shorter) / max(len(longer), 1))
    return SequenceMatcher(None, ka, kb).ratio()


def find_similar_companies(
    company: str,
    *,
    threshold: float = _SIMILARITY_THRESHOLD,
    exclude_exact_slug: bool = False,
) -> list[dict]:
    """既存 ES から表記揺れ候補を返す (スコア降順)。"""
    target_slug = company_slug(company)
    hits: list[dict] = []
    for doc in load_es_documents():
        if exclude_exact_slug and doc["id"] == f"es_{target_slug}":
            continue
        if doc["id"] == f"es_{target_slug}" or doc.get("company_name") == company:
            # 完全一致は別経路 (exact) で扱う
            continue
        score = company_name_similarity(company, doc.get("company_name") or doc.get("title") or "")
        if score < threshold:
            # slug 同士の近さも見る
            score = max(
                score,
                company_name_similarity(company, doc["id"].removeprefix("es_").replace("_", "")),
            )
        if score >= threshold:
            hits.append({
                "id": doc["id"],
                "company_name": doc["company_name"],
                "title": doc["title"],
                "score": round(float(score), 3),
                "path": Path(doc["path"]).name,
            })
    hits.sort(key=lambda h: (-float(h["score"]), h["company_name"]))
    return hits


def ensure_company_header(content: str, company: str) -> str:
    """本文先頭に企業名フィールドを付与 / 置換する。"""
    company = company.strip()
    if not company:
        return content
    if _COMPANY_RE.search(content):
        return _COMPANY_RE.sub(f"企業名: {company}", content, count=1)
    return f"企業名: {company}\n\n{content.lstrip()}"


def es_path_for_company(company: str) -> Path:
    return ES_DIR / f"es_{company_slug(company)}.md"


def _is_es_library_file(path: Path) -> bool:
    if not path.is_file() or path.suffix.lower() not in {".md", ".txt"}:
        return False
    name = path.name
    if name.startswith("draft_"):
        return False
    if name.startswith("."):
        return False
    # active_es.md は企業別ファイルへのミラー (一覧に二重計上しない)
    if name == ACTIVE_ES.name:
        return False
    return True


def _parse_es(path: Path) -> dict:
    text = _read_text_lenient(path).strip()
    title = path.stem
    hm = re.search(r"^#\s+(.+)$", text, re.MULTILINE)
    if hm:
        title = hm.group(1).strip()
    domain = extract_target_domain(text)
    company = extract_company_name(text, fallback=title)
    return {
        "id": path.stem,
        "name": path.stem,
        "path": str(path),
        "title": title,
        "company_name": company or title,
        "body": text,
        "target_domain": domain["domain"],
        "explicit_domain": domain["explicit"],
        "keywords": domain["keywords"],
        "mtime": path.stat().st_mtime,
        "char_count": len(text),
    }


def load_es_documents() -> list[dict]:
    """企業別 ES ライブラリ (draft_* / active_es.md ミラー除外)。mtime 降順。"""
    if not ES_DIR.is_dir():
        if ACTIVE_ES.exists():
            return [_parse_es(ACTIVE_ES)]
        return []
    docs: list[dict] = []
    for path in ES_DIR.iterdir():
        if _is_es_library_file(path):
            try:
                docs.append(_parse_es(path))
            except (OSError, UnicodeDecodeError):
                continue
    # 企業別ファイルが無い旧環境: active_es.md のみをライブラリとして扱う
    if not docs and ACTIVE_ES.exists():
        try:
            docs.append(_parse_es(ACTIVE_ES))
        except (OSError, UnicodeDecodeError):
            pass
    docs.sort(key=lambda d: (-float(d["mtime"]), d["id"]))
    return docs


def select_es(name: str | None = None) -> dict | None:
    """id / stem / 企業名で ES を解決。空・none 系はゼロベース (None)。

    name 省略時は最新1件 (es_review 等の後方互換)。明示ゼロベースは
    Interview 側が esId="" を渡す。
    """
    docs = load_es_documents()
    if not docs:
        return None
    if name is None:
        return docs[0]
    key = str(name).strip()
    if key.lower() in _NONE_IDS or key == "ゼロベース（ESなし）":
        return None
    for doc in docs:
        if doc["id"] == key or doc["name"] == key:
            return doc
        if doc["company_name"] == key:
            return doc
    # スラッグ一致
    slug = company_slug(key)
    for doc in docs:
        if doc["id"] == f"es_{slug}" or doc["id"] == slug:
            return doc
    return None


def get_active_es() -> dict | None:
    """最新 ES 1件 (Import 要約・後方互換)。"""
    docs = load_es_documents()
    return docs[0] if docs else None


def list_es_summaries() -> list[dict]:
    """UI 向け軽量一覧 (本文なし)。"""
    out = []
    for doc in load_es_documents():
        out.append({
            "id": doc["id"],
            "company_name": doc["company_name"],
            "title": doc["title"],
            "target_domain": doc["target_domain"],
            "char_count": doc["char_count"],
            "mtime": doc["mtime"],
        })
    return out


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
    company = es.get("company_name") or es.get("target_domain") or "志望先"
    header = (
        f"あなたは「{company}」向け選考で「{es['target_domain']}」領域の"
        "トップ組織で採用と専門評価を長年担当してきた面接官である。この領域の専門語彙"
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
