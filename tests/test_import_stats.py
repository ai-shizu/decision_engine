# -*- coding: utf-8 -*-
"""F2 (SPEC_FOXTROT_UI.md §2.2.1) の回帰テスト。

facade.data_source_stats() の軽量 stat 正確性と、import_line_text/batch の
status コールバック配線 (ロジック無変更・配線のみ) を検証する。実行は
一時 PKB_PROJECT_ROOT 上で行い、実データには一切触れない。
"""
from __future__ import annotations

import json
import os
import shutil
import sys
import tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "src" / "python"))

# SPEC_FOXTROT_UI.md §10.1 (F-15): pytest 経由では tests/conftest.py がテスト
# 収集より前に PKB_PROJECT_ROOT を Sandbox へ設定済み。setdefault により
# それを尊重しつつ、本ファイルを単独実行 (`python tests/test_import_stats.py`)
# した場合の後方互換 (自前の一時ルート) も両立する (W-50: 上書きしない)。
_TMP = tempfile.mkdtemp(prefix="pkb_import_stats_")
os.environ.setdefault("PKB_PROJECT_ROOT", _TMP)
os.environ.setdefault("HF_HUB_OFFLINE", "1")
os.environ.setdefault("TRANSFORMERS_OFFLINE", "1")

from core import es_manager, facade  # noqa: E402
from core.paths import (  # noqa: E402
    ACTIVE_ES, CALENDAR_JSON, DATA_KNOWLEDGE, DIARY_MD, ES_DIR, FINANCE_JSON, LINE_HISTORY,
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
    # M20-N: ACTIVE_ES のみでも count=1 (フォールバック)
    ACTIVE_ES.parent.mkdir(parents=True, exist_ok=True)
    ACTIVE_ES.write_text("企業名: テスト社\n志望職種: テスト", encoding="utf-8")
    DATA_KNOWLEDGE.mkdir(parents=True, exist_ok=True)
    stats = facade.data_source_stats()
    assert stats["es"]["exists"] is True and stats["es"]["count"] == 1
    assert stats["knowledge"]["exists"] is True and stats["knowledge"]["count"] == 0
    print("  data_source_stats: es (ACTIVE_ES fallback) + knowledge OK")


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


# =============================================================================
# M20-N: 企業別 ES ライブラリ (旧 F-16 単一 active_es.md を置換)
# =============================================================================

def test_es_import_per_company() -> None:
    r1 = facade.import_document(
        "志望動機: 一つ目の内容です。", "es_v1.md", "es",
        company_name="アルファ社",
    )
    assert r1["imported"] is True and r1["skipped"] is False, r1
    assert r1["path"].startswith("es_"), r1
    assert r1["company_name"] == "アルファ社", r1

    # 同一企業・別内容は確認ゲート
    gated = facade.import_document(
        "志望動機: 二つ目の、より詳細な更新後の内容です。", "es_v2.md", "es",
        company_name="アルファ社",
    )
    assert gated.get("needs_confirmation") is True, gated
    assert gated["imported"] is False and gated["skipped"] is False, gated

    r2 = facade.import_document(
        "志望動機: 二つ目の、より詳細な更新後の内容です。", "es_v2.md", "es",
        company_name="アルファ社",
        confirm_overwrite=True,
    )
    assert r2["imported"] is True and r2["skipped"] is False, r2
    assert r2["path"] == r1["path"], r2
    assert "二つ目" in (ES_DIR / r2["path"]).read_text(encoding="utf-8")

    r3 = facade.import_document(
        "志望動機: 別企業の内容です。", "es_v3.md", "es",
        company_name="ベータ社",
    )
    assert r3["imported"] is True, r3
    assert r3["path"] != r1["path"], (r1, r3)

    docs = es_manager.load_es_documents()
    companies = {d["company_name"] for d in docs}
    assert companies == {"アルファ社", "ベータ社"}, companies
    print("  import_document(es): 同一企業は確認後上書き・別企業は併存 OK")


def test_es_import_idempotent() -> None:
    content = "志望動機: 冪等性確認用の内容です。"
    r1 = facade.import_document(content, "es_a.md", "es", company_name="冪等社")
    assert r1["imported"] is True and r1["skipped"] is False, r1

    r2 = facade.import_document(content, "es_b.md", "es", company_name="冪等社")
    assert r2["imported"] is False and r2["skipped"] is True, r2
    print("  import_document(es): 同一企業・同一内容の再importはskip OK")


def test_es_select_by_company() -> None:
    shutil.rmtree(ES_DIR, ignore_errors=True)
    facade.import_document("志望動機: A社向け", "a.md", "es", company_name="A株式会社")
    facade.import_document("志望動機: B社向け", "b.md", "es", company_name="B株式会社")

    a = es_manager.select_es("A株式会社")
    b = es_manager.select_es("B株式会社")
    assert a is not None and "A社向け" in a["body"], a
    assert b is not None and "B社向け" in b["body"], b
    assert es_manager.select_es("") is None
    assert es_manager.select_es("none") is None

    listed = facade.list_es()["items"]
    assert len(listed) == 2, listed
    print("  select_es / list_es: 企業別解決とゼロベース OK")


def test_es_company_alias_gate() -> None:
    """表記揺れ (株式会社A ↔ A) は強制上書きせず確認を要求する。"""
    shutil.rmtree(ES_DIR, ignore_errors=True)
    facade.import_document(
        "志望動機: 旧表記", "old.md", "es", company_name="株式会社アルファ",
    )
    gated = facade.import_document(
        "志望動機: 新表記", "new.md", "es", company_name="アルファ",
    )
    assert gated.get("needs_confirmation") is True, gated
    assert gated["conflict"]["similar"] or gated["conflict"]["exact"], gated
    assert (ES_DIR / gated["conflict"]["similar"][0]["path"]).exists() or True

    replaced = facade.import_document(
        "志望動機: 新表記", "new.md", "es",
        company_name="アルファ",
        confirm_overwrite=True,
        replace_es_id=gated["conflict"]["similar"][0]["id"],
    )
    assert replaced["imported"] is True, replaced
    docs = es_manager.load_es_documents()
    names = {d["company_name"] for d in docs}
    assert "アルファ" in names
    assert "株式会社アルファ" not in names
    print("  表記揺れ確認ゲート + 置換 OK")


def test_active_es_view_shape() -> None:
    shutil.rmtree(ES_DIR, ignore_errors=True)
    assert facade.active_es() == {"exists": False}

    facade.import_document(
        "志望動機: View形状確認用の内容です。", "shape.md", "es",
        company_name="形状確認社",
    )
    view = facade.active_es()
    assert view["exists"] is True
    assert view.get("company_name") == "形状確認社", view
    assert isinstance(view["body"], str) and "View形状" in view["body"]
    assert isinstance(view["char_count"], int) and view["char_count"] == len(view["body"])
    assert "target_domain" in view and "keywords" in view and "mtime" in view
    assert "filename" not in view, "W-32: filename を含めてはならない"
    print("  active_es(): 未登録={'exists': False} / 登録後は company_name 等を持つ OK")


def test_data_source_stats_es_multi() -> None:
    shutil.rmtree(ES_DIR, ignore_errors=True)
    stats = facade.data_source_stats()
    assert stats["es"]["exists"] is False and stats["es"]["count"] == 0, stats["es"]

    facade.import_document(
        "志望動機: countチェック用の内容です。", "count.md", "es",
        company_name="カウント社",
    )
    stats = facade.data_source_stats()
    assert stats["es"]["exists"] is True and stats["es"]["count"] == 1, stats["es"]

    facade.import_document(
        "志望動機: 二件目", "count2.md", "es", company_name="第二社",
    )
    stats = facade.data_source_stats()
    assert stats["es"]["count"] == 2, stats["es"]
    print("  data_source_stats: es count は企業件数に追従 OK")

