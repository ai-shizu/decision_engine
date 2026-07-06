#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""
外部知識のオンデマンド・インジェクション
==========================================
LLM が回答中に `<fetch_query>検索クエリ</fetch_query>` を出力した場合に
Python 側でフックし、外部知識の取得要求として処理するパイプライン。

【オフライン原則との関係 — 重要】
本モジュールは PKB の完全オフライン原則に対する「唯一の公認された例外」である。
ただし以下のガードレールを絶対に緩めないこと:

  1. デフォルトは完全オフライン。ネットワーク取得は環境変数
     `PKB_ALLOW_ONLINE_FETCH=1` をユーザーが明示設定した場合のみ動作する。
  2. consult 中の自動通信は行わない。<fetch_query> は「キューへの永続化」まで。
     実際の取得は facade.fetch_pending_knowledge() をユーザーが明示的に
     起動した時だけ実行される (LLM が勝手に外へ出ることはできない)。
  3. 全クエリは data/knowledge/_fetch_queue.json に平文で記録され、
     ユーザーが取得前に監査・削除できる。
  4. 未許可環境では手動充足が正: ユーザーが回答となる Markdown/テキストを
     data/knowledge/ に置けば、既存の sync_knowledge_index が自動で取り込む。

取得結果は data/knowledge/fetched_*.md として永続化され、
consultation_engine.sync_knowledge_index() が次回 consult 時に
knowledge_vectors.bin へエンベディングして検索コンテキストに統合する。
"""

from __future__ import annotations

import json
import os
import re
from datetime import datetime
from html.parser import HTMLParser
from pathlib import Path
from typing import Callable

from .paths import KNOWLEDGE_DIR

FETCH_TAG_RE = re.compile(r"<fetch_query>\s*(.*?)\s*</fetch_query>", re.DOTALL)
QUEUE_JSON = KNOWLEDGE_DIR / "_fetch_queue.json"

# fetcher(query) -> [{"title": str, "url": str, "text": str}, ...]
Fetcher = Callable[[str], list[dict]]

MAX_RESULTS_PER_QUERY = 2
MAX_CHARS_PER_PAGE = 4000
FETCH_TIMEOUT_S = 10
USER_AGENT = "Mozilla/5.0 (PKB offline-first knowledge fetcher)"


def online_fetch_allowed() -> bool:
    """ネットワーク取得の明示許可 (デフォルト False = 完全オフライン)。"""
    return os.environ.get("PKB_ALLOW_ONLINE_FETCH") == "1"


# ============================================================ タグフック
def extract_fetch_queries(text: str) -> tuple[str, list[str]]:
    """LLM 出力から <fetch_query> を除去し、(本文, クエリ一覧) を返す。"""
    queries = [q.strip() for q in FETCH_TAG_RE.findall(text) if q.strip()]
    clean = FETCH_TAG_RE.sub("", text).strip()
    # 重複除去 (順序保持)
    return clean, list(dict.fromkeys(queries))


# ============================================================ キュー永続化
def load_queue() -> list[dict]:
    if not QUEUE_JSON.exists():
        return []
    try:
        data = json.loads(QUEUE_JSON.read_text(encoding="utf-8"))
    except (json.JSONDecodeError, OSError):
        return []
    return data if isinstance(data, list) else []


def _save_queue(queue: list[dict]) -> None:
    KNOWLEDGE_DIR.mkdir(parents=True, exist_ok=True)
    QUEUE_JSON.write_text(
        json.dumps(queue, ensure_ascii=False, indent=2), encoding="utf-8")


def queue_fetch_queries(queries: list[str]) -> int:
    """クエリをキューへ追加 (既存 pending/done と重複するものは無視)。"""
    if not queries:
        return 0
    queue = load_queue()
    known = {e["query"] for e in queue}
    added = 0
    for q in queries:
        if q in known:
            continue
        queue.append({
            "query": q,
            "status": "pending",
            "requested_at": datetime.now().isoformat(timespec="seconds"),
        })
        added += 1
    if added:
        _save_queue(queue)
    return added


# ============================================================ 取得結果の永続化
def _slugify(query: str, max_len: int = 40) -> str:
    slug = re.sub(r"[^0-9A-Za-z぀-ヿ一-鿿]+", "-", query).strip("-")
    return slug[:max_len] or "query"


def ingest_results(query: str, results: list[dict]) -> Path:
    """取得テキストを data/knowledge/ 直下の Markdown として永続化する。

    load_knowledge_chunks() は KNOWLEDGE_DIR 直下の .md を非再帰で走査するため、
    サブディレクトリではなく `fetched_` プレフィックスで直置きする。
    次回 consult の sync_knowledge_index() が自動でベクトル化・統合する。"""
    KNOWLEDGE_DIR.mkdir(parents=True, exist_ok=True)
    stamp = datetime.now().strftime("%Y%m%d-%H%M%S")
    path = KNOWLEDGE_DIR / f"fetched_{stamp}_{_slugify(query)}.md"
    lines = [
        f"# 外部取得知識: {query}",
        f"(取得日時: {datetime.now().isoformat(timespec='seconds')} / "
        "knowledge_fetcher による自動取得。内容の正確性は未検証)",
        "",
    ]
    for r in results[:MAX_RESULTS_PER_QUERY]:
        title = str(r.get("title", "")).strip() or "(無題)"
        url = str(r.get("url", "")).strip()
        text = str(r.get("text", "")).strip()[:MAX_CHARS_PER_PAGE]
        lines += [f"## {title}", f"出典: {url}", "", text, ""]
    path.write_text("\n".join(lines), encoding="utf-8")
    return path


# ============================================================ オンライン取得 (明示許可時のみ)
class _TextExtractor(HTMLParser):
    """HTML から可視テキストのみを抽出する簡易パーサ (stdlib)。"""

    _SKIP = {"script", "style", "noscript", "header", "footer", "nav"}

    def __init__(self) -> None:
        super().__init__()
        self._skip_depth = 0
        self.chunks: list[str] = []

    def handle_starttag(self, tag, attrs):
        if tag in self._SKIP:
            self._skip_depth += 1

    def handle_endtag(self, tag):
        if tag in self._SKIP and self._skip_depth > 0:
            self._skip_depth -= 1

    def handle_data(self, data):
        if self._skip_depth == 0 and data.strip():
            self.chunks.append(data.strip())


class _DuckDuckGoLinks(HTMLParser):
    """DuckDuckGo HTML 版の検索結果リンク (.result__a) を抽出する。"""

    def __init__(self) -> None:
        super().__init__()
        self.links: list[dict] = []
        self._in_result_a = False
        self._current: dict = {}

    def handle_starttag(self, tag, attrs):
        if tag != "a":
            return
        a = dict(attrs)
        if "result__a" in a.get("class", "") and a.get("href"):
            self._in_result_a = True
            self._current = {"href": a["href"], "title": ""}

    def handle_data(self, data):
        if self._in_result_a:
            self._current["title"] += data

    def handle_endtag(self, tag):
        if tag == "a" and self._in_result_a:
            self._in_result_a = False
            self.links.append(self._current)


def _http_get(url: str) -> str:
    import urllib.request
    req = urllib.request.Request(url, headers={"User-Agent": USER_AGENT})
    with urllib.request.urlopen(req, timeout=FETCH_TIMEOUT_S) as r:
        raw = r.read(1_500_000)
    return raw.decode("utf-8", errors="replace")


def _resolve_ddg_url(href: str) -> str:
    """DuckDuckGo のリダイレクト URL (uddg パラメータ) を実 URL へ解決する。"""
    from urllib.parse import parse_qs, unquote, urlparse
    if href.startswith("//"):
        href = "https:" + href
    parsed = urlparse(href)
    if "duckduckgo.com" in parsed.netloc:
        uddg = parse_qs(parsed.query).get("uddg", [])
        if uddg:
            return unquote(uddg[0])
    return href


def default_online_fetcher(query: str) -> list[dict]:
    """DuckDuckGo HTML 検索 → 上位ページ本文の取得 (urllib のみ)。

    PKB_ALLOW_ONLINE_FETCH=1 でなければ RuntimeError。呼び出し元で握らないこと —
    未許可での通信は設計違反であり、静かに握り潰してはならない。"""
    if not online_fetch_allowed():
        raise RuntimeError(
            "ネットワーク取得は許可されていません (PKB_ALLOW_ONLINE_FETCH=1 が必要)")
    from urllib.parse import quote_plus

    html = _http_get(f"https://html.duckduckgo.com/html/?q={quote_plus(query)}")
    parser = _DuckDuckGoLinks()
    parser.feed(html)

    results: list[dict] = []
    for link in parser.links[:MAX_RESULTS_PER_QUERY]:
        url = _resolve_ddg_url(link["href"])
        if not url.startswith("https://"):
            continue  # 平文 HTTP は取得しない
        try:
            page = _http_get(url)
        except OSError:
            continue
        extractor = _TextExtractor()
        extractor.feed(page)
        text = re.sub(r"\s+", " ", " ".join(extractor.chunks))
        results.append({
            "title": link["title"].strip() or url,
            "url": url,
            "text": text[:MAX_CHARS_PER_PAGE],
        })
    return results


# ============================================================ キュー処理
def process_pending(fetcher: Fetcher | None = None) -> dict:
    """pending クエリを処理する。fetcher 未指定かつ未許可なら何も取得しない。

    戻り値: {"processed", "skipped_offline", "failed", "files"}"""
    queue = load_queue()
    summary = {"processed": 0, "skipped_offline": 0, "failed": 0,
               "files": []}

    if fetcher is None:
        if not online_fetch_allowed():
            summary["skipped_offline"] = sum(
                1 for e in queue if e["status"] == "pending")
            return summary
        fetcher = default_online_fetcher

    changed = False
    for entry in queue:
        if entry["status"] != "pending":
            continue
        try:
            results = fetcher(entry["query"])
        except Exception as exc:  # noqa: BLE001 — 個別クエリの失敗で全体を止めない
            entry["status"] = "failed"
            entry["error"] = f"{type(exc).__name__}: {exc}"
            summary["failed"] += 1
            changed = True
            continue
        if not results:
            entry["status"] = "failed"
            entry["error"] = "検索結果なし"
            summary["failed"] += 1
            changed = True
            continue
        path = ingest_results(entry["query"], results)
        entry["status"] = "done"
        entry["fetched_at"] = datetime.now().isoformat(timespec="seconds")
        entry["file"] = path.name
        summary["processed"] += 1
        summary["files"].append(str(path))
        changed = True

    if changed:
        _save_queue(queue)
    return summary
