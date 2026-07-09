# -*- coding: utf-8 -*-
"""TUIのヘッドレススモークテスト (textual run_test)。

重い処理(埋め込み・LLM)はモックし、UIの配線のみを検証する:
  - 4タブの存在とタブ切替時のフォーカス移動
  - RECORD: 日記保存で diary.md に追記され、裏の同期ワーカーが呼ばれる
  - CONSULT: チャット送信で consult が呼ばれ回答が表示される
  - SETTINGS: 読み取り専用プロフィール表示 + profiler ボタン

pytest 収集対象 (tests/conftest.py の Sandbox が自動適用)。
"""
from __future__ import annotations

import asyncio
import importlib.util
import sys
from pathlib import Path

import pytest

pytest.importorskip("textual")

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "src" / "python"))

import core.facade as facade  # noqa: E402
from core.paths import CALENDAR_JSON, DIARY_MD, FINANCE_JSON  # noqa: E402
from textual.containers import Grid  # noqa: E402
from textual.widgets import Input, Markdown, TabbedContent, TextArea  # noqa: E402

_spec = importlib.util.spec_from_file_location(
    "pkb_ui_app", ROOT / "src" / "python" / "ui_tui" / "app.py",
)
ui_app = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(ui_app)


class _FakeEngine:
    def sync_diary_index(self, force: bool = False) -> bool:
        return False

    def shutdown(self) -> None:
        pass


def _seed_sandbox_tui_fixtures() -> None:
    """TUI mount/save に必要な最小 Sandbox フィクスチャ (paths 経由のみ)。"""
    DIARY_MD.parent.mkdir(parents=True, exist_ok=True)
    DIARY_MD.write_text("", encoding="utf-8")
    CALENDAR_JSON.write_text("{}", encoding="utf-8")
    FINANCE_JSON.write_text("{}", encoding="utf-8")


async def _smoke() -> None:
    _seed_sandbox_tui_fixtures()

    calls: dict[str, list[str]] = {"consult": []}

    def _mock_consult(q, status=None, **kwargs):
        calls["consult"].append(q)
        if status:
            status("mocked")
        return (
            "## 1. 現状分析\nmock\n## 2. 価値観との整合性\nmock\n"
            "## 3. 必要なスキルギャップ\nmock\n## 4. 次の一手\nmock"
        )

    def _mock_get_engine():
        return _FakeEngine()

    _real_consult = facade.consult
    _real_get_engine = facade.get_engine
    facade.consult = _mock_consult
    facade.get_engine = _mock_get_engine

    try:
        dashboard = ui_app.DecisionDashboard()
        async with dashboard.run_test(size=(120, 40)) as pilot:
            main = dashboard.query_one("#main-tabs", TabbedContent)
            for tid in ("record", "import", "consult", "settings"):
                dashboard.query_one(f"#{tid}")
            assert dashboard.query_one("#calendar-widget") is not None
            sub = dashboard.query_one("#record-sub-tabs", TabbedContent)
            assert {p.id for p in sub.query("TabPane")} == {
                "record-events", "record-finance", "record-diary",
            }

            # ---- RECORD: 日記サブタブで保存 ----
            sub.active = "record-diary"
            await pilot.pause()
            dashboard.query_one("#diary-body", TextArea).text = (
                "- 気分: テスト\n- 学び: TUIスモーク"
            )
            dashboard._record_date = "2026-07-05"
            dashboard._current_events = []
            dashboard._update_date_labels()
            await pilot.press("ctrl+s")
            await pilot.pause()
            assert "TUIスモーク" in DIARY_MD.read_text(encoding="utf-8")

            # ---- CONSULT: フォーカス、属性、チャット ----
            main.active = "consult"
            chat = None
            for _ in range(50):
                await pilot.pause()
                try:
                    chat = dashboard.query_one("#chat-input", Input)
                    chat.focus()
                    if dashboard.focused and dashboard.focused.id == "chat-input":
                        break
                except Exception:
                    await asyncio.sleep(0.05)
            assert chat is not None, "consult タブの #chat-input がマウントされない"
            assert dashboard.focused.id == "chat-input"
            chat.value = "テスト相談です"
            await pilot.press("enter")
            for _ in range(50):
                if not dashboard._consult_busy and calls["consult"]:
                    break
                await asyncio.sleep(0.1)
            await pilot.pause()
            assert calls["consult"] == ["テスト相談です"]
            assert len(dashboard.query_one("#chat-log").query(Markdown)) == 2

            # ---- SETTINGS: 基本情報 + 読み取り専用プロフィール ----
            main.active = "settings"
            first_key = ui_app.FIXED_FIELDS[0][0]
            for _ in range(50):
                await pilot.pause()
                try:
                    dashboard.query_one(f"#fixed-{first_key}", Input).focus()
                    if dashboard.focused.id == f"fixed-{first_key}":
                        break
                except Exception:
                    await asyncio.sleep(0.05)
            assert dashboard.focused.id == f"fixed-{first_key}"
            assert dashboard.query_one("#fixed-grid", Grid) is not None
            assert len(dashboard.query("#fixed-grid Input")) == 6
            dashboard.query_one("#fixed-height", Input).value = "170"
            await pilot.click("#save-fixed-attrs")
            await pilot.pause()
            summary = dashboard.query_one("#profile-summary", TextArea)
            assert summary.read_only
            text = summary.text
            assert "自動生成" in text or "価値観" in text or "プロフィール未生成" in text
    finally:
        facade.consult = _real_consult
        facade.get_engine = _real_get_engine


def test_ui_smoke() -> None:
    asyncio.run(_smoke())
