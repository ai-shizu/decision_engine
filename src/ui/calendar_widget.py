#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""RECORDタブ用カレンダーグリッド (月間/週間切替 + 予定マーク)。"""

from __future__ import annotations

import calendar as cal_mod
from datetime import date, timedelta

from textual.app import ComposeResult
from textual.containers import Grid, Horizontal, HorizontalScroll, Vertical
from textual.message import Message
from textual.widgets import Button, Static

_WEEKDAYS = ("月", "火", "水", "木", "金", "土", "日")


class CalendarWidget(Vertical):
    """月間/週間トグル付きカレンダー。日付選択で DateSelected を送出する。"""

    DEFAULT_CSS = """
    CalendarWidget {
        height: auto;
        width: auto;
        min-width: 70;
        border: solid $primary;
        padding: 0 1;
        margin-bottom: 1;
    }
    #cal-scroll {
        width: 100%;
        height: auto;
        max-height: 14;
    }
    #cal-toolbar {
        height: 3;
        align: left middle;
    }
    #cal-toolbar Button {
        min-width: 5;
        margin: 0 1 0 0;
    }
    #cal-title {
        width: 1fr;
        min-width: 16;
        content-align: center middle;
        text-style: bold;
    }
    #cal-grid {
        grid-size: 7;
        grid-gutter: 1 2;
        height: auto;
        width: auto;
        min-width: 63;
        margin-top: 1;
    }
    .cal-header {
        height: 1;
        min-width: 8;
        content-align: center middle;
        color: $text-muted;
    }
    .cal-day {
        min-height: 3;
        min-width: 8;
        width: 1fr;
        height: auto;
    }
    .cal-day.cal-outside {
        opacity: 0.35;
    }
    .cal-day.cal-selected {
        background: $primary;
        color: $surface;
    }
    .cal-day.cal-today {
        border: tall $accent;
    }
    .cal-day.cal-has-events {
        text-style: bold;
        color: $warning;
    }
    .cal-day.cal-selected.cal-has-events {
        color: $surface;
    }
    """

    class DateSelected(Message):
        """ユーザーが日付セルを選択した。"""

        def __init__(self, date_str: str) -> None:
            self.date_str = date_str
            super().__init__()

    def __init__(
        self,
        selected: str | None = None,
        *,
        id: str | None = "calendar-widget",
        **kwargs,
    ) -> None:
        super().__init__(id=id, **kwargs)
        today = date.today()
        self._selected = selected or today.isoformat()
        self._view = "week"
        sel = date.fromisoformat(self._selected)
        self._anchor = date(sel.year, sel.month, 1)
        self._week_start = sel - timedelta(days=sel.weekday())
        self._event_dates: set[str] = set()
        self._pending_dates: set[str] = set()
        self._render_token = 0

    def compose(self) -> ComposeResult:
        with Horizontal(id="cal-toolbar"):
            yield Button("◀", id="cal-prev", variant="default")
            yield Static("", id="cal-title")
            yield Button("▶", id="cal-next", variant="default")
            yield Button("週", id="cal-mode-week", variant="primary")
            yield Button("月", id="cal-mode-month", variant="default")
            yield Button("今日", id="cal-today", variant="default")
        with HorizontalScroll(id="cal-scroll"):
            yield Grid(id="cal-grid")

    def refresh_calendar(
        self,
        event_dates: set[str] | None = None,
        pending_dates: set[str] | None = None,
        selected: str | None = None,
    ) -> None:
        """マーク・選択日をまとめて更新し、グリッドを1回だけ再描画する。"""
        if event_dates is not None:
            self._event_dates = set(event_dates)
        if pending_dates is not None:
            self._pending_dates = set(pending_dates or set())
        if selected is not None:
            self._selected = selected
            sel = date.fromisoformat(selected)
            if self._view == "month":
                self._anchor = date(sel.year, sel.month, 1)
            else:
                self._week_start = sel - timedelta(days=sel.weekday())
        self._render_grid()

    def set_selected(self, date_str: str) -> None:
        self.refresh_calendar(selected=date_str)

    def set_marks(self, event_dates: set[str], pending_dates: set[str] | None = None) -> None:
        self.refresh_calendar(event_dates=event_dates, pending_dates=pending_dates)

    def _all_marks(self) -> set[str]:
        return self._event_dates | self._pending_dates

    def _clear_grid(self, grid: Grid) -> None:
        """グリッド子要素を完全削除 (Textual の ID レジストリ衝突を防ぐ)。"""
        for child in list(grid.children):
            child.remove()

    def _render_grid(self) -> None:
        if not self.is_mounted:
            return
        grid = self.query_one("#cal-grid", Grid)
        self._clear_grid(grid)
        self._render_token += 1

        for wd in _WEEKDAYS:
            grid.mount(Static(wd, classes="cal-header"))

        today = date.today().isoformat()
        marks = self._all_marks()

        if self._view == "month":
            self.query_one("#cal-title", Static).update(
                f"{self._anchor.year}年 {self._anchor.month}月  [月間]")
            self.query_one("#cal-mode-month", Button).variant = "primary"
            self.query_one("#cal-mode-week", Button).variant = "default"
            month_weeks = cal_mod.Calendar(firstweekday=0).monthdatescalendar(
                self._anchor.year, self._anchor.month)
            for week in month_weeks:
                for d in week:
                    self._mount_day_button(grid, d, today, marks,
                                           outside=d.month != self._anchor.month)
        else:
            week_days = [self._week_start + timedelta(days=i) for i in range(7)]
            self.query_one("#cal-title", Static).update(
                f"{week_days[0].isoformat()} ~ {week_days[-1].isoformat()}  [週間]")
            self.query_one("#cal-mode-month", Button).variant = "default"
            self.query_one("#cal-mode-week", Button).variant = "primary"
            for d in week_days:
                self._mount_day_button(grid, d, today, marks, outside=False)

    def _mount_day_button(
        self, grid: Grid, d: date, today: str, marks: set[str], *, outside: bool,
    ) -> None:
        ds = d.isoformat()
        label = f"{d.day}*" if ds in marks else str(d.day)
        classes = ["cal-day"]
        if outside:
            classes.append("cal-outside")
        if ds == self._selected:
            classes.append("cal-selected")
        if ds == today:
            classes.append("cal-today")
        if ds in marks:
            classes.append("cal-has-events")
        # 日付ボタンは id を付けない (App 全体で ID 一意制約の衝突を回避)
        grid.mount(Button(label, name=ds, classes=" ".join(classes)))

    def _navigate(self, delta_months: int = 0, delta_weeks: int = 0) -> None:
        if self._view == "month" and delta_months:
            y, m = self._anchor.year, self._anchor.month + delta_months
            while m < 1:
                m += 12
                y -= 1
            while m > 12:
                m -= 12
                y += 1
            self._anchor = date(y, m, 1)
        elif self._view == "week" and delta_weeks:
            self._week_start += timedelta(weeks=delta_weeks)
        self._render_grid()

    def on_button_pressed(self, event: Button.Pressed) -> None:
        bid = event.button.id or ""
        if bid == "cal-prev":
            self._navigate(delta_months=-1 if self._view == "month" else 0,
                           delta_weeks=-1 if self._view == "week" else 0)
            return
        if bid == "cal-next":
            self._navigate(delta_months=1 if self._view == "month" else 0,
                           delta_weeks=1 if self._view == "week" else 0)
            return
        if bid == "cal-mode-month":
            if self._view != "month":
                self._view = "month"
                sel = date.fromisoformat(self._selected)
                self._anchor = date(sel.year, sel.month, 1)
                self._render_grid()
            return
        if bid == "cal-mode-week":
            if self._view != "week":
                self._view = "week"
                sel = date.fromisoformat(self._selected)
                self._week_start = sel - timedelta(days=sel.weekday())
                self._render_grid()
            return
        if bid == "cal-today":
            today = date.today().isoformat()
            self.set_selected(today)
            self.post_message(self.DateSelected(today))
            return
        if event.button.name and "cal-day" in event.button.classes:
            ds = event.button.name
            try:
                date.fromisoformat(ds)
            except ValueError:
                return
            self._selected = ds
            self._render_grid()
            self.post_message(self.DateSelected(ds))
