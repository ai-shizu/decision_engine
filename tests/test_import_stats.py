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


if __name__ == "__main__":
    test_data_source_stats_missing_sources()
    test_data_source_stats_counts_real_content()
    test_data_source_stats_dir_sources()
    test_import_line_text_status_callback_order()
    print("test_import_stats: ALL PASS")
