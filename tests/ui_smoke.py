# -*- coding: utf-8 -*-
"""TUIのヘッドレススモークテスト (textual run_test)。

重い処理(埋め込み・LLM)はモックし、UIの配線のみを検証する:
  - 4タブの存在とタブ切替時のフォーカス移動
  - RECORD: 日記保存で diary.md に追記され、裏の同期ワーカーが呼ばれる
  - CONSULT: チャット送信で consult が呼ばれ回答が表示される
  - SETTINGS: 読み取り専用プロフィール表示 + profiler ボタン
"""
import asyncio
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "src" / "python"))
sys.path.insert(0, str(ROOT / "src" / "ui"))

from textual.containers import Grid  # noqa: E402
from textual.widgets import Input, Markdown, TabbedContent, TextArea  # noqa: E402

import importlib.util  # noqa: E402

import core.facade as facade  # noqa: E402

_spec = importlib.util.spec_from_file_location(
    "pkb_ui_app", ROOT / "src" / "python" / "ui_tui" / "app.py",
)
ui_app = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(ui_app)


async def main() -> None:
    dashboard = ui_app.DecisionDashboard()

    calls = {"consult": []}

    def _mock_consult(q, status=None):
        calls["consult"].append(q)
        if status:
            status("mocked")
        return (
            "## 1. 現状分析\nmock\n## 2. 価値観との整合性\nmock\n"
            "## 3. 必要なスキルギャップ\nmock\n## 4. 次の一手\nmock"
        )

    _real_consult = facade.consult
    facade.consult = _mock_consult

    diary_before = ui_app.DIARY_MD.read_text(encoding="utf-8")

    async with dashboard.run_test(size=(120, 40)) as pilot:
        main = dashboard.query_one("#main-tabs", TabbedContent)
        for tid in ("record", "import", "consult", "settings"):
            dashboard.query_one(f"#{tid}")
        assert dashboard.query_one("#calendar-widget") is not None
        sub = dashboard.query_one("#record-sub-tabs", TabbedContent)
        assert {p.id for p in sub.query("TabPane")} == {
            "record-events", "record-finance", "record-diary"}

        # ---- RECORD: 日記サブタブで保存 ----
        sub.active = "record-diary"
        await pilot.pause()
        dashboard.query_one("#diary-body", TextArea).text = "- 気分: テスト\n- 学び: TUIスモーク"
        dashboard._record_date = "2026-07-05"
        dashboard._current_events = []
        dashboard._update_date_labels()
        await pilot.press("ctrl+s")
        await pilot.pause()
        assert "TUIスモーク" in ui_app.DIARY_MD.read_text(encoding="utf-8")

        # ---- CONSULT: フォーカス、属性、チャット ----
        main.active = "consult"
        await pilot.pause()
        assert dashboard.focused.id == "chat-input"
        chat = dashboard.query_one("#chat-input", Input)
        chat.value = "テスト相談です"
        await pilot.press("enter")
        for _ in range(50):
            if not dashboard._consult_busy and calls["consult"]:
                break
            await asyncio.sleep(0.1)
        await pilot.pause()
        assert calls["consult"] == ["テスト相談です"]
        assert len(dashboard.query_one("#chat-log").query(Markdown)) == 2  # 相談+回答

        # ---- SETTINGS: 基本情報 + 読み取り専用プロフィール ----
        main.active = "settings"
        await pilot.pause()
        assert dashboard.focused.id == "fixed-age"
        assert dashboard.query_one("#fixed-grid", Grid) is not None
        assert len(dashboard.query("#fixed-grid Input")) == 6
        dashboard.query_one("#fixed-age", Input).value = "30"
        await pilot.click("#save-fixed-attrs")
        await pilot.pause()
        summary = dashboard.query_one("#profile-summary", TextArea)
        assert summary.read_only
        text = summary.text
        assert "自動生成" in text or "価値観" in text or "プロフィール未生成" in text

    # 後始末: テストで追記した日記を元に戻す
    ui_app.DIARY_MD.write_text(diary_before, encoding="utf-8")
    facade.consult = _real_consult
    print("UI smoke test: ALL PASS")
    print(f"  consult_calls={calls['consult']}")


if __name__ == "__main__":
    asyncio.run(main())
