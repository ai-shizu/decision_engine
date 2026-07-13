#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""
外部知識キューとローカル永続化 (E0a egress lockdown)
====================================================
legacy `<fetch_query>` 文字列のローカル parse / queue / ingest のみを担う。

【E0a — 外向き通信は無条件封鎖】
  `_http_get` / `default_online_fetcher` / `process_pending` は
  `NotImplementedError("Egress blocked by E0a strict lockdown.")` を raise する
  一文 stub のみ。環境変数・mock injection・helper では解除不能。
  `online_fetch_allowed()` は常に False（環境を読まない）。

手動充足は従来どおり: ユーザーが Markdown/テキストを data/knowledge/ に置けば、
consultation_engine.sync_knowledge_index() がローカル索引へ統合する。
"""

from __future__ import annotations

import json
import re
from datetime import datetime
from pathlib import Path
from typing import Callable

from .paths import KNOWLEDGE_DIR

FETCH_TAG_RE = re.compile(r"<fetch_query>\s*(.*?)\s*</fetch_query>", re.DOTALL)
QUEUE_JSON = KNOWLEDGE_DIR / "_fetch_queue.json"

# fetcher(query) -> [{"title": str, "url": str, "text": str}, ...]
# Kept for signature compatibility; process_pending never invokes it under E0a.
Fetcher = Callable[[str], list[dict]]

MAX_RESULTS_PER_QUERY = 2
MAX_CHARS_PER_PAGE = 4000


def online_fetch_allowed() -> bool:
    return False


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


# ============================================================ E0a egress stubs
def _http_get(url: str) -> str:
    raise NotImplementedError("Egress blocked by E0a strict lockdown.")


def default_online_fetcher(query: str) -> list[dict]:
    raise NotImplementedError("Egress blocked by E0a strict lockdown.")


def process_pending(fetcher: Fetcher | None = None) -> dict:
    raise NotImplementedError("Egress blocked by E0a strict lockdown.")
