# -*- coding: utf-8 -*-
"""F2 (SPEC_FOXTROT_UI.md §2.2.1) の回帰テスト。

facade.data_source_stats() の軽量 stat 正確性と、import_line_text/batch の
status コールバック配線 (ロジック無変更・配線のみ) を検証する。実行は
一時 PKB_PROJECT_ROOT 上で行い、実データには一切触れない。
"""
from __future__ import annotations

import json
import os
import sys
import tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "src" / "python"))

_TMP = tempfile.mkdtemp(prefix="pkb_import_stats_")
os.environ["PKB_PROJECT_ROOT"] = _TMP
os.environ.setdefault("HF_HUB_OFFLINE", "1")
os.environ.setdefault("TRANSFORMERS_OFFLINE", "1")

from core import facade  # noqa: E402
from core.paths import (  # noqa: E402
    CALENDAR_JSON, DATA_KNOWLEDGE, DIARY_MD, ES_DIR, FINANCE_JSON, LINE_HISTORY,
)


def test_data_source_stats_missing_sources() -> None:
    for p in (DIARY_MD, LINE_HISTORY, CALENDAR_JSON, FINANCE_JSON):
        p.parent.mkdir(parents=True, exist_ok=True)
        if p.exists():
            p.unlink()
    stats = facade.data_source_stats()
    for name in ("diary", "line", "calendar", "finance"):
        assert stats[name]["exists"] is False
        assert stats[name]["count"] == 0
        assert stats[name]["mtime"] is None
    print("  data_source_stats: missing sources report exists=False OK")


def test_data_source_stats_counts_real_content() -> None:
    DIARY_MD.parent.mkdir(parents=True, exist_ok=True)
    DIARY_MD.write_text("## 2026-07-01\n本文A\n\n## 2026-07-02\n本文B\n", encoding="utf-8")
    LINE_HISTORY.write_text(
        "[LINE] 友人とのトーク履歴\n2026/07/01(水)\n10:00\t友人\tやあ\n"
        "[LINE] 友人とのトーク履歴\n2026/07/02(木)\n10:00\t友人\thi\n",
        encoding="utf-8",
    )
    CALENDAR_JSON.write_text(
        json.dumps({"2026-07-01": [{"time": "09:00", "title": "MTG"}]}), encoding="utf-8",
    )
    FINANCE_JSON.write_text(
        json.dumps({"2026-07-01": [{"type": "expense", "category": "食費", "amount": 500}]}),
        encoding="utf-8",
    )
    stats = facade.data_source_stats()
    assert stats["diary"]["exists"] is True and stats["diary"]["count"] == 2
    assert stats["line"]["exists"] is True and stats["line"]["count"] == 2
    assert stats["calendar"]["exists"] is True and stats["calendar"]["count"] == 1
    assert stats["finance"]["exists"] is True and stats["finance"]["count"] == 1
    assert stats["diary"]["mtime"] is not None
    print("  data_source_stats: counts match real content OK")


def test_data_source_stats_dir_sources() -> None:
    ES_DIR.mkdir(parents=True, exist_ok=True)
    (ES_DIR / "es1.md").write_text("志望職種: テスト", encoding="utf-8")
    DATA_KNOWLEDGE.mkdir(parents=True, exist_ok=True)
    stats = facade.data_source_stats()
    assert stats["es"]["exists"] is True and stats["es"]["count"] == 1
    assert stats["knowledge"]["exists"] is True and stats["knowledge"]["count"] == 0
    print("  data_source_stats: directory sources (es/knowledge) OK")


def test_import_line_text_status_callback_order() -> None:
    calls: list[str] = []
    result = facade.import_line_text(
        "[LINE] テストとのトーク履歴\n2026/07/03(金)\n10:00\tテスト\tメッセージ\n",
        "line_export.txt",
        status=lambda msg: calls.append(msg),
    )
    assert result["imported"] is True
    assert len(calls) == 3, calls
    assert "受信" in calls[0]
    assert "profiler" in calls[1]
    assert calls[2] == "完了"
    print("  import_line_text status callback fires in order (F2) OK")


# ---------------------------------------------------------------- (a) 分類優先順位 (先勝ち)
def test_classify_document_line_wins_over_es_vocabulary() -> None:
    # [LINE] ヘッダと ES 語彙 (志望動機) の両方を含むが、順序固定・先勝ちで line が勝つ。
    content = "[LINE] 友人とのトーク履歴\n志望動機について相談した\n10:00\t友人\tやあ\n"
    result = facade.classify_document(content, "mixed.txt")
    assert result["type"] == "line", result
    print("  classify_document: [LINE] header wins over ES vocabulary (priority) OK")


# ---------------------------------------------------------------- (b) 拒絶ゲート
def test_classify_document_rejects() -> None:
    r1 = facade.classify_document("plain text", "photo.png")
    assert r1["type"] == "reject" and "拡張子" in r1["reasons"][0], r1

    r2 = facade.classify_document("head\x00null byte contamination", "binary.txt")
    assert r2["type"] == "reject" and "NUL" in r2["reasons"][0], r2

    huge = "a" * (10 * 1024 * 1024 + 1)
    r3 = facade.classify_document(huge, "huge.txt")
    assert r3["type"] == "reject" and "サイズ" in r3["reasons"][0], r3
    print("  classify_document: extension/NUL/size rejection gates OK")


# ---------------------------------------------------------------- (c) 冪等性 (同一内容2回→skip)
def test_import_document_idempotent_skip() -> None:
    content = "自己PR: これはテスト用の文書です。\n"
    r1 = facade.import_document(content, "note1.md", "knowledge")
    assert r1["imported"] is True and r1["skipped"] is False

    r2 = facade.import_document(content, "note2.md", "knowledge")  # 別名・同一内容
    assert r2["imported"] is False and r2["skipped"] is True, r2
    print("  import_document: identical content on 2nd import -> skip (idempotent) OK")


# ---------------------------------------------------------------- (d) 名前衝突 -> 別名保存 (上書きなし)
def test_import_document_name_collision_renames() -> None:
    DATA_KNOWLEDGE.mkdir(parents=True, exist_ok=True)
    (DATA_KNOWLEDGE / "dup.md").write_text("既存の内容", encoding="utf-8")
    r = facade.import_document("全く別の新しい内容です", "dup.md", "knowledge")
    assert r["imported"] is True
    assert r["path"] != "dup.md", r
    assert (DATA_KNOWLEDGE / "dup.md").read_text(encoding="utf-8") == "既存の内容", \
        "既存ファイルが上書きされた"
    print("  import_document: name collision renames instead of overwriting OK")


# ---------------------------------------------------------------- (e) dest ホワイトリスト外 -> 例外
def test_import_document_dest_whitelist() -> None:
    try:
        facade.import_document("本文", "note.txt", "diary")
        raise AssertionError("diary への書き込みが例外を投げなかった")
    except ValueError as e:
        assert "dest" in str(e)
    print("  import_document: dest whitelist rejects 'diary' etc. OK")


# ---------------------------------------------------------------- (f) knowledge 取込 -> index 同期が呼ばれる
def test_import_document_knowledge_triggers_index_sync() -> None:
    result = facade.import_document(
        "ES/企画書には該当しない一般的な知識文書です。", "general_doc.md", "knowledge",
    )
    assert result["imported"] is True
    assert "index_rebuilt" in result, result
    print("  import_document: knowledge dest triggers sync_knowledge_index OK")


if __name__ == "__main__":
    test_data_source_stats_missing_sources()
    test_data_source_stats_counts_real_content()
    test_data_source_stats_dir_sources()
    test_import_line_text_status_callback_order()
    test_classify_document_line_wins_over_es_vocabulary()
    test_classify_document_rejects()
    test_import_document_idempotent_skip()
    test_import_document_name_collision_renames()
    test_import_document_dest_whitelist()
    test_import_document_knowledge_triggers_index_sync()
    print("test_import_stats: ALL PASS")
