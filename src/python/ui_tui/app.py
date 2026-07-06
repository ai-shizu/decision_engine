#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""
意思決定支援ダッシュボード (Textual TUI)
==========================================
  [RECORD]   カレンダー + 予定 / 家計簿 / 日記
  [IMPORT]   履歴取り込み
  [CONSULT]  チャット相談 (フル幅)
  [SETTINGS] 基本情報入力 + 自動プロフィール + profiler 再分析

起動:  python src/ui/app.py  (または python -m ui_tui.app)
"""

from __future__ import annotations

import subprocess
import sys
from datetime import date
from pathlib import Path

ROOT = Path(__file__).resolve().parents[3]
sys.path.insert(0, str(ROOT / "src" / "python"))

from textual import events, on, work  # noqa: E402
from textual.app import App, ComposeResult  # noqa: E402
from textual.containers import Grid, Horizontal, Vertical, VerticalScroll  # noqa: E402
from textual.widgets import (  # noqa: E402
    Button, Footer, Header, Input, Label, Markdown, Select, Static, TabbedContent,
    TabPane, TextArea,
)

from core import apple_calendar_sync, facade  # noqa: E402
from core import calendar_manager as cal  # noqa: E402
from core import finance_manager as fin  # noqa: E402
from ui_tui.calendar_widget import CalendarWidget  # noqa: E402
from ui_tui.time_picker import TimePicker  # noqa: E402

DIARY_MD = facade.DIARY_MD
LINE_HISTORY = facade.LINE_HISTORY
USER_PROFILE = facade.USER_PROFILE
PROFILER_PY = facade.profiler_script_path()
FIXED_FIELDS = facade.FIXED_ATTRIBUTE_FIELDS

_WEEKDAYS_JP = "月火水木金土日"


def _clean_dropped_path(text: str) -> Path | None:
    t = text.strip().strip("&").strip().strip('"').strip("'")
    if not t or "\n" in t:
        return None
    p = Path(t)
    return p if p.is_file() else None


def _format_date_label(date_str: str) -> str:
    d = date.fromisoformat(date_str)
    wd = _WEEKDAYS_JP[d.weekday()]
    return f"{date_str} ({wd})"


class DecisionDashboard(App):
    TITLE = "PKB 意思決定支援ダッシュボード"
    SUB_TITLE = "完全オフライン / Snapdragon X NEON"

    BINDINGS = [
        ("ctrl+s", "save_record", "記録を保存"),
        ("ctrl+q", "quit", "終了"),
    ]

    CSS = """
    #record-pane { padding: 0 1; height: 1fr; layout: vertical; }
    #record-scroll { height: 1fr; min-height: 1; width: 100%; }
    #calendar-widget { width: 100%; }
    #record-date-banner {
        height: 1;
        margin: 0 0 1 0;
        color: $accent;
        text-style: bold;
    }
    #record-sub-tabs { height: auto; width: 100%; }
    TabbedContent#record-sub-tabs ContentSwitcher { height: auto; }
    TabbedContent#record-sub-tabs TabPane { height: auto; padding: 0; }
    #event-pane, #finance-pane, #diary-pane {
        height: auto;
        width: 100%;
        layout: vertical;
    }
    .pane-title { height: 1; margin-bottom: 1; }
    .record-list-scroll {
        height: auto;
        max-height: 8;
        min-height: 3;
        border: solid $secondary;
        margin: 1 0;
    }
    #event-list, #finance-list {
        height: auto;
        width: 100%;
        padding: 0 1;
    }
    #event-form {
        height: auto;
        width: 100%;
        border-top: solid $secondary;
        padding-top: 1;
        margin-top: 1;
    }
    #event-form-row {
        height: auto;
        min-height: 3;
        align: left middle;
        margin-top: 1;
    }
    #event-title { width: 1fr; }
    TimePicker { width: auto; }
    #finance-summary { height: 1; color: $text-muted; margin-bottom: 1; }
    #finance-forms {
        height: auto;
        min-height: 5;
        border-top: solid $secondary;
        padding-top: 1;
        margin-top: 1;
    }
    .finance-section { padding: 0 0 1 0; }
    .finance-section.finance-expense { border-top: solid $error; padding-top: 1; }
    .finance-section.finance-income { border-top: solid $success; padding-top: 1; }
    .finance-row { height: auto; min-height: 3; align: left middle; }
    #expense-category, #income-category { width: 1fr; }
    #expense-amount, #income-amount { width: 12; }
    #record-actions { height: auto; min-height: 3; margin-top: 1; }
    #record-status { height: 1; }
    #diary-body { height: 12; min-height: 8; border: solid $primary; }
    #import-pane { padding: 1 2; height: 1fr; }
    #line-drop-input { border: dashed $secondary; margin: 1 0; }
    #ics-sync-row { height: auto; margin: 1 0; align: left middle; }
    #ics-merge-mode { width: 1fr; max-width: 40; margin-right: 1; }
    #apple-sync-row { height: auto; margin: 1 0; align: left middle; }
    #sync-apple-calendar:disabled { opacity: 0.45; }
    .import-divider { height: 1; margin: 2 0 1 0; border-top: solid $secondary; }
    .import-hint { color: $text-muted; height: auto; margin: 1 0; }
    .field-label { width: 8; content-align: right middle; }
    .status { color: $text-muted; height: 1; }
    .hint { color: $text-muted; }
    .section-label { text-style: bold; margin-bottom: 1; height: auto; }

    #consult-col { padding: 0 1; height: 1fr; }
    #chat-log { height: 1fr; border: solid $primary; padding: 0 1; }
    #chat-input { dock: bottom; }
    .chat-user { background: $boost; margin: 1 0 0 8; padding: 0 1; }
    .chat-ai { margin: 1 8 0 0; padding: 0 1; }

    #settings-pane { padding: 1 2; height: 1fr; layout: vertical; }
    #settings-fixed-panel {
        height: auto;
        border: solid $primary;
        padding: 0 1 1 1;
        margin-bottom: 1;
    }
    #fixed-grid {
        grid-size: 2;
        grid-gutter: 0 1;
        height: auto;
    }
    #fixed-grid .field-label {
        width: 100%;
        height: 3;
        content-align: right middle;
    }
    #fixed-grid Input { width: 100%; height: 3; }
    #settings-auto-scroll {
        height: 1fr;
        min-height: 6;
        border: solid $secondary;
        padding: 0 1;
        margin-bottom: 1;
    }
    #profile-summary { height: auto; min-height: 8; }
    .profile-note { color: $text-muted; height: auto; margin: 1 0; }
    #settings-actions { height: auto; min-height: 3; }
    """

    def __init__(self):
        super().__init__()
        self.engine = facade.get_engine()
        self._consult_busy = False
        self._record_date = date.today().isoformat()
        self._current_events: list[dict] = []
        self._current_transactions: list[dict] = []

    def compose(self) -> ComposeResult:
        yield Header()
        with TabbedContent(initial="record", id="main-tabs"):
            with TabPane("RECORD (記録)", id="record"):
                with Vertical(id="record-pane"):
                    with VerticalScroll(id="record-scroll"):
                        yield CalendarWidget(selected=self._record_date, id="calendar-widget")
                        yield Static("", id="record-date-banner")
                        with TabbedContent(initial="record-events", id="record-sub-tabs"):
                            with TabPane("予定", id="record-events"):
                                with Vertical(id="event-pane"):
                                    yield Label("", id="event-day-title", classes="pane-title")
                                    with VerticalScroll(classes="record-list-scroll"):
                                        yield Static("(読み込み中…)", id="event-list")
                                    with Vertical(id="event-form"):
                                        yield TimePicker(id="event-time-picker")
                                        with Horizontal(id="event-form-row"):
                                            yield Input(
                                                placeholder="ミーティング名など",
                                                id="event-title")
                                            yield Button(
                                                "追加", id="add-event", variant="primary")
                            with TabPane("家計簿", id="record-finance"):
                                with Vertical(id="finance-pane"):
                                    yield Label("", id="finance-day-title", classes="pane-title")
                                    yield Static("", id="finance-summary")
                                    with VerticalScroll(classes="record-list-scroll"):
                                        yield Static("(読み込み中…)", id="finance-list")
                                    with Vertical(id="finance-forms"):
                                        with Vertical(classes="finance-section finance-expense"):
                                            with Horizontal(classes="finance-row"):
                                                yield Label("支出", classes="field-label")
                                                yield Input(
                                                    placeholder="食費・交通費など",
                                                    id="expense-category")
                                                yield Input(
                                                    placeholder="5000", id="expense-amount")
                                                yield Button(
                                                    "追加", id="add-expense", variant="primary")
                                        with Vertical(classes="finance-section finance-income"):
                                            with Horizontal(classes="finance-row"):
                                                yield Label("収入", classes="field-label")
                                                yield Input(
                                                    placeholder="給与・副業など",
                                                    id="income-category")
                                                yield Input(
                                                    placeholder="300000", id="income-amount")
                                                yield Button(
                                                    "追加", id="add-income", variant="primary")
                            with TabPane("日記", id="record-diary"):
                                with Vertical(id="diary-pane"):
                                    yield Label("", id="diary-day-title", classes="pane-title")
                                    yield TextArea(id="diary-body")
                    with Horizontal(id="record-actions"):
                        yield Button("保存 (Ctrl+S)", variant="primary", id="save-record")
                        yield Label("  選択中の日付を一括保存", classes="hint")
                    yield Static("", id="record-status", classes="status")

            with TabPane("IMPORT", id="import"):
                with Vertical(id="import-pane"):
                    yield Label("LINE トーク履歴の取り込み", classes="section-label")
                    yield Static(
                        "LINE公式のエクスポート (.txt) をここにドラッグ&ドロップして Enter。\n"
                        "取り込み後、裏で profiler が価値観を自動抽出します。",
                        classes="import-hint")
                    yield Input(
                        placeholder="line_history.txt をドラッグ&ドロップして Enter",
                        id="line-drop-input")
                    yield Static("", classes="import-divider")
                    yield Label("Google カレンダー (ICS) の同期", classes="section-label")
                    yield Static(
                        "Google カレンダーを ICS 形式で手動エクスポートし、"
                        "下のボタンからファイルを選んで取り込みます。\n"
                        "ネットワーク通信は行いません (ローカルファイルのみ)。",
                        classes="import-hint")
                    with Horizontal(id="ics-sync-row"):
                        yield Select(
                            (
                                ("追記 (既存予定に追加)", "append"),
                                ("上書き (同一日付を置換)", "overwrite"),
                            ),
                            value="append",
                            id="ics-merge-mode",
                            allow_blank=False)
                        yield Button(
                            "ICSファイルを同期", variant="primary", id="sync-ics")
                    yield Static("", classes="import-divider")
                    yield Label("Apple カレンダー (macOS) の同期", classes="section-label")
                    yield Static(
                        "macOS の Calendar.app が保持するローカル SQLite DB を読み取り、"
                        "予定を取り込みます (外部 API 不使用)。",
                        id="apple-sync-hint",
                        classes="import-hint")
                    with Horizontal(id="apple-sync-row"):
                        yield Button(
                            "Appleカレンダー同期",
                            variant="primary",
                            id="sync-apple-calendar")
                    yield Static("", id="import-status", classes="status")

            with TabPane("CONSULT (相談)", id="consult"):
                with Vertical(id="consult-col"):
                    yield VerticalScroll(id="chat-log")
                    yield Static("", id="consult-status", classes="status")
                    yield Input(
                        placeholder="相談を入力して Enter (自動プロフィール + 過去ログを参照)",
                        id="chat-input")

            with TabPane("SETTINGS (設定)", id="settings"):
                with Vertical(id="settings-pane"):
                    with Vertical(id="settings-fixed-panel"):
                        yield Label(
                            "基本情報 (手入力・profiler では変わりません)",
                            classes="section-label")
                        with Grid(id="fixed-grid"):
                            for key, label in FIXED_FIELDS:
                                yield Label(label, classes="field-label")
                                yield Input(id=f"fixed-{key}")
                        yield Button(
                            "基本情報を保存", variant="primary", id="save-fixed-attrs")
                        yield Static("", id="fixed-status", classes="status")
                    with VerticalScroll(id="settings-auto-scroll"):
                        yield Label(
                            "自動プロフィール (読み取り専用)",
                            classes="section-label")
                        yield Static(
                            "日記・LINE・相談・家計簿から profiler が抽象化して抽出します。",
                            classes="profile-note")
                        yield TextArea(id="profile-summary", read_only=True)
                    with Horizontal(id="settings-actions"):
                        yield Button("再分析 (profiler)", id="rerun-profiler")
                    yield Static("", id="settings-status", classes="status")
        yield Footer()

    def on_mount(self) -> None:
        self._load_fixed_attributes()
        self._load_profile_summary()
        self._update_date_labels()
        self._refresh_calendar_marks()
        self._load_record_worker(self._record_date)
        self._focus_record_subtab("record-events")
        self._configure_apple_sync_ui()
        self.call_after_refresh(self._scroll_record_top)

    def _configure_apple_sync_ui(self) -> None:
        btn = self.query_one("#sync-apple-calendar", Button)
        hint = self.query_one("#apple-sync-hint", Static)
        if apple_calendar_sync.is_macos():
            btn.disabled = False
            hint.update(
                "macOS の Calendar.app が保持するローカル SQLite DB "
                "(~/Library/Calendars/ 等) を読み取り、予定を取り込みます "
                "(外部 API 不使用)。"
            )
        else:
            btn.disabled = True
            hint.update("Apple カレンダー同期は macOS でのみ利用できます (Windows では無効)。")

    def _scroll_record_top(self) -> None:
        scroll = self.query_one("#record-scroll", VerticalScroll)
        scroll.scroll_home(animate=False)

    def _status(self, widget_id: str, msg: str) -> None:
        self.query_one(f"#{widget_id}", Static).update(msg)

    def _load_fixed_attributes(self) -> None:
        attrs = facade.load_user_profile().get("fixed_attributes", {})
        for key, _ in FIXED_FIELDS:
            self.query_one(f"#fixed-{key}", Input).value = str(attrs.get(key, ""))

    def _load_profile_summary(self) -> None:
        self.query_one("#profile-summary", TextArea).text = facade.format_user_profile_summary()

    def _update_date_labels(self) -> None:
        label = _format_date_label(self._record_date)
        self.query_one("#record-date-banner", Static).update(f"編集中の日付: {label}")
        self.query_one("#event-day-title", Label).update(f"{label} の予定")
        self.query_one("#finance-day-title", Label).update(f"{label} の収支")
        self.query_one("#diary-day-title", Label).update(f"{label} の日記")

    def _pending_event_dates(self) -> set[str]:
        if self._current_events:
            return {self._record_date}
        return set()

    def _refresh_calendar_marks(self) -> None:
        widget = self.query_one("#calendar-widget", CalendarWidget)
        widget.refresh_calendar(
            event_dates=cal.dates_with_events(),
            pending_dates=self._pending_event_dates(),
            selected=self._record_date,
        )

    def _render_event_list(self) -> None:
        if not self._current_events:
            text = "(この日の予定はまだありません)"
        else:
            lines = [f"  {e['time']}  {e['title']}" for e in self._current_events]
            text = "\n".join(lines)
        self.query_one("#event-list", Static).update(text)

    def _render_finance_list(self) -> None:
        if not self._current_transactions:
            self.query_one("#finance-list", Static).update("(この日の取引はまだありません)")
        else:
            expenses = [t for t in self._current_transactions if t["type"] == "expense"]
            incomes = [t for t in self._current_transactions if t["type"] == "income"]
            lines: list[str] = []
            if expenses:
                lines.append("【支出】")
                lines += [f"  {t['category']}: {t['amount']:,}円" for t in expenses]
            if incomes:
                if lines:
                    lines.append("")
                lines.append("【収入】")
                lines += [f"  {t['category']}: {t['amount']:,}円" for t in incomes]
            self.query_one("#finance-list", Static).update("\n".join(lines))
        summary = fin.summarize_day(self._current_transactions)
        self.query_one("#finance-summary", Static).update(
            f"収入: {summary['income']:,}円 | 支出: {summary['expense']:,}円 | "
            f"差引: {summary['net']:,}円")

    def _apply_record_data(
        self, date_str: str, events: list[dict], transactions: list[dict], diary: str,
    ) -> None:
        if date_str != self._record_date:
            return
        self._current_events = events
        self._current_transactions = transactions
        self._update_date_labels()
        self._render_event_list()
        self._render_finance_list()
        self.query_one("#diary-body", TextArea).text = diary
        self._refresh_calendar_marks()

    @work(thread=True, group="record_load")
    def _load_record_worker(self, date_str: str) -> None:
        data = facade.load_record(date_str)
        self.call_from_thread(
            self._apply_record_data,
            date_str,
            data["events"],
            data["transactions"],
            data["diary"],
        )

    def _focus_record_subtab(self, pane_id: str | None = None) -> None:
        if pane_id is None:
            pane_id = self.query_one("#record-sub-tabs", TabbedContent).active or "record-events"
        if pane_id == "record-events":
            self.query_one("#event-title", Input).focus()
        elif pane_id == "record-finance":
            self.query_one("#expense-category", Input).focus()
        elif pane_id == "record-diary":
            self.query_one("#diary-body", TextArea).focus()

    @on(TabbedContent.TabActivated, "#record-sub-tabs")
    def _on_record_subtab(self, event: TabbedContent.TabActivated) -> None:
        if event.pane.id:
            self._focus_record_subtab(event.pane.id)
            self._scroll_record_top()

    @on(TabbedContent.TabActivated, "#main-tabs")
    def _focus_mode(self, event: TabbedContent.TabActivated) -> None:
        pane = event.pane.id
        if pane == "record":
            self._focus_record_subtab()
        elif pane == "import":
            self.query_one("#sync-ics", Button).focus()
        elif pane == "consult":
            self.query_one("#chat-input", Input).focus()
        elif pane == "settings":
            self._load_fixed_attributes()
            self._load_profile_summary()
            self.query_one("#fixed-age", Input).focus()

    @on(CalendarWidget.DateSelected)
    def _on_calendar_date_selected(self, event: CalendarWidget.DateSelected) -> None:
        self._switch_record_date(event.date_str)

    def _switch_record_date(self, new_date: str) -> None:
        try:
            date.fromisoformat(new_date)
        except ValueError:
            self._status("record-status", f"日付形式が不正です: {new_date}")
            return
        if new_date == self._record_date:
            return
        self._record_date = new_date
        self._update_date_labels()
        self.query_one("#calendar-widget", CalendarWidget).refresh_calendar(
            selected=new_date)
        self._load_record_worker(new_date)

    @on(Button.Pressed, "#add-event")
    def _on_add_event(self) -> None:
        norm_time = self.query_one("#event-time-picker", TimePicker).value
        title = self.query_one("#event-title", Input).value.strip()
        if not title:
            self._status("record-status", "予定内容を入力してください")
            return
        self._current_events.append({"time": norm_time, "title": title})
        self._current_events.sort(key=lambda e: e["time"])
        self._render_event_list()
        self.query_one("#event-title", Input).value = ""
        self._refresh_calendar_marks()
        self._status(
            "record-status",
            f"{_format_date_label(self._record_date)} {norm_time} {title} を追加 — Ctrl+S で保存")

    @on(Button.Pressed, "#add-expense")
    def _on_add_expense(self) -> None:
        self._add_finance_tx(
            "expense",
            self.query_one("#expense-category", Input).value.strip(),
            self.query_one("#expense-amount", Input).value.strip(),
            "#expense-category", "#expense-amount",
        )

    @on(Button.Pressed, "#add-income")
    def _on_add_income(self) -> None:
        self._add_finance_tx(
            "income",
            self.query_one("#income-category", Input).value.strip(),
            self.query_one("#income-amount", Input).value.strip(),
            "#income-category", "#income-amount",
        )

    def _add_finance_tx(
        self, tx_type: str, category: str, amount: str,
        cat_id: str, amt_id: str,
    ) -> None:
        ok, result = fin.validate_transaction({
            "type": tx_type, "category": category, "amount": amount,
        })
        if not ok:
            self._status("record-status", str(result))
            return
        self._current_transactions.append(result)
        self._render_finance_list()
        self.query_one(cat_id, Input).value = ""
        self.query_one(amt_id, Input).value = ""
        label = "支出" if result["type"] == "expense" else "収入"
        self._status(
            "record-status",
            f"家計簿追加 [{label}] {result['category']} {result['amount']:,}円 — Ctrl+S で保存")

    def action_save_record(self) -> None:
        self._save_record()

    @on(Button.Pressed, "#save-record")
    def _on_save_button(self) -> None:
        self._save_record()

    def _save_record(self) -> None:
        d = self._record_date
        body = self.query_one("#diary-body", TextArea).text.strip()
        try:
            date.fromisoformat(d)
        except ValueError:
            self._status("record-status", f"日付形式が不正です: {d}")
            return
        if not body and not self._current_events and not self._current_transactions:
            self._status("record-status", "予定・家計簿・日記がすべて空です")
            return

        try:
            result = facade.save_record(
                d, self._current_events, self._current_transactions, body,
            )
        except ValueError as exc:
            self._status("record-status", str(exc))
            return

        self._refresh_calendar_marks()
        sync_msg = (
            "DailyContext結晶化+ベクトル同期完了"
            if result.get("index_rebuilt") else "同期不要 (最新)"
        )
        self._status("record-status", f"{d} を保存しました — {sync_msg}")

    @on(Input.Submitted, "#line-drop-input")
    def _on_line_drop_submitted(self, event: Input.Submitted) -> None:
        p = _clean_dropped_path(event.value)
        if p is None:
            self._status("import-status", "ファイルが見つかりません")
            return
        event.input.value = ""
        self._import_line_file(p)

    def on_paste(self, event: events.Paste) -> None:
        if self.query_one("#main-tabs", TabbedContent).active != "import":
            return
        p = _clean_dropped_path(event.text)
        if p is not None:
            event.stop()
            self._import_line_file(p)

    def _get_merge_mode(self) -> str:
        mode = self.query_one("#ics-merge-mode", Select).value
        return mode if mode in ("append", "overwrite") else "append"

    def _after_calendar_sync(self, summary: dict, mode: str, label: str) -> None:
        rebuilt = summary.get("index_rebuilt", False)
        sync_msg = "DailyContext結晶化+ベクトル同期完了" if rebuilt else "同期不要 (最新)"
        events = cal.get_events_for_date(self._record_date)
        self._refresh_calendar_marks()
        self._apply_record_events_only(self._record_date, events)
        self._status(
            "import-status",
            (f"{label}完了: {summary['imported_events']}件 / "
             f"{summary['imported_dates']}日 ({mode}) — {sync_msg}"),
        )

    @staticmethod
    def _is_line_export(path: Path, text_head: str) -> bool:
        return "[LINE]" in text_head or "line" in path.stem.lower()

    @staticmethod
    def _pick_ics_file() -> Path | None:
        """ネイティブファイル選択ダイアログ (オフライン・ローカルのみ)。"""
        import tkinter as tk
        from tkinter import filedialog

        root = tk.Tk()
        root.withdraw()
        root.attributes("-topmost", True)
        selected = filedialog.askopenfilename(
            title="ICS ファイルを選択",
            filetypes=[
                ("iCalendar", "*.ics"),
                ("すべてのファイル", "*.*"),
            ],
        )
        root.destroy()
        if not selected:
            return None
        return Path(selected)

    @on(Button.Pressed, "#sync-ics")
    def _on_sync_ics(self) -> None:
        mode = self._get_merge_mode()
        self._status("import-status", "ICS ファイルを選択してください…")
        self._sync_ics_worker(mode)

    @work(thread=True, group="import")
    def _sync_ics_worker(self, mode: str) -> None:
        try:
            path = self._pick_ics_file()
            if path is None:
                self.call_from_thread(
                    self._status, "import-status", "ICS 同期をキャンセルしました")
                return
            self.call_from_thread(
                self._status, "import-status",
                f"{path.name} を取り込み中 ({mode})…")
            summary = facade.sync_calendar("ics", mode, ics_path=path)
            self.call_from_thread(
                self._after_calendar_sync, summary, mode, "ICS同期")
        except Exception as e:
            self.call_from_thread(
                self._status, "import-status",
                f"ICS同期エラー: {type(e).__name__}: {e}")

    @on(Button.Pressed, "#sync-apple-calendar")
    def _on_sync_apple_calendar(self) -> None:
        if not apple_calendar_sync.is_macos():
            self._status("import-status", "Apple カレンダー同期は macOS のみ利用できます")
            return
        mode = self._get_merge_mode()
        self._status("import-status", "Apple カレンダーを読み取り中…")
        self._sync_apple_calendar_worker(mode)

    @work(thread=True, group="import")
    def _sync_apple_calendar_worker(self, mode: str) -> None:
        try:
            summary = facade.sync_calendar("apple", mode)
            self.call_from_thread(
                self._after_calendar_sync, summary, mode, "Appleカレンダー同期")
        except Exception as e:
            self.call_from_thread(
                self._status, "import-status",
                f"Appleカレンダー同期エラー: {type(e).__name__}: {e}")

    def _apply_record_events_only(self, date_str: str, events: list[dict]) -> None:
        if date_str != self._record_date:
            return
        self._current_events = events
        self._render_event_list()
        self._refresh_calendar_marks()

    @work(thread=True, group="import")
    def _import_line_file(self, path: Path) -> None:
        try:
            text = path.read_text(encoding="utf-8", errors="replace")
            if not self._is_line_export(path, text[:2000]):
                self.call_from_thread(
                    self._status, "import-status",
                    "LINE履歴 (.txt) のみ取り込めます")
                return
            with open(LINE_HISTORY, "a", encoding="utf-8") as f:
                f.write("\n" + text.strip() + "\n")
            self.call_from_thread(
                self._status, "import-status",
                f"LINE履歴を取り込みました: {path.name} — 裏で価値観を再分析中…")
            subprocess.run([sys.executable, str(PROFILER_PY)],
                           capture_output=True, timeout=300)
            self.call_from_thread(
                self._status, "import-status",
                f"分析完了: {path.name} (deep_profile / user_profile 更新)")
        except Exception as e:
            self.call_from_thread(self._status, "import-status",
                                  f"取り込みエラー: {type(e).__name__}: {e}")

    @on(Button.Pressed, "#save-fixed-attrs")
    def _save_fixed_attrs(self) -> None:
        attrs = {key: self.query_one(f"#fixed-{key}", Input).value.strip()
                 for key, _ in FIXED_FIELDS}
        facade.save_fixed_attributes(attrs)
        self._status("fixed-status", "基本情報を保存しました")
        self._status("settings-status", "user_profile.json を更新しました")

    @on(Input.Submitted, "#chat-input")
    def _on_chat_submitted(self, event: Input.Submitted) -> None:
        query = event.value.strip()
        if not query:
            return
        if self._consult_busy:
            self._status("consult-status", "前の相談を処理中です…")
            return
        event.input.value = ""
        log = self.query_one("#chat-log", VerticalScroll)
        log.mount(Markdown(f"**相談**\n\n{query}", classes="chat-user"))
        log.scroll_end(animate=False)
        self._consult_busy = True
        self._run_consult(query)

    @work(thread=True, exclusive=True, group="consult")
    def _run_consult(self, query: str) -> None:
        try:
            answer = facade.consult(
                query,
                status=lambda m: self.call_from_thread(
                    self._status, "consult-status", m))
        except Exception as e:
            answer = f"エラーが発生しました: {type(e).__name__}: {e}"
        self.call_from_thread(self._append_answer, answer)

    def _append_answer(self, answer: str) -> None:
        self._consult_busy = False
        log = self.query_one("#chat-log", VerticalScroll)
        log.mount(Markdown(answer, classes="chat-ai"))
        log.scroll_end(animate=False)
        self._status("consult-status", "")
        self.query_one("#chat-input", Input).focus()

    @on(Button.Pressed, "#rerun-profiler")
    def _rerun_profiler(self) -> None:
        self._status("settings-status", "profiler を実行中…")
        self._profiler_worker()

    @work(thread=True, exclusive=True, group="profiler")
    def _profiler_worker(self) -> None:
        out = subprocess.run([sys.executable, str(PROFILER_PY)],
                             capture_output=True, text=True, timeout=300,
                             encoding="utf-8", errors="replace")
        msg = ("再分析完了 (deep_profile.json / user_profile.json 更新)"
               if out.returncode == 0 else f"profiler失敗: {out.stderr[-200:]}")
        self.call_from_thread(self._status, "settings-status", msg)
        self.call_from_thread(self._load_profile_summary)
        self.call_from_thread(self._load_fixed_attributes)

    def on_unmount(self) -> None:
        facade.shutdown_engine()


if __name__ == "__main__":
    DecisionDashboard().run()
