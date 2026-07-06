#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""時・分を縦ロールで選ぶ時刻ピッカー (予定入力用)。"""

from __future__ import annotations

from datetime import datetime

from textual.app import ComposeResult
from textual.containers import Horizontal, Vertical
from textual.widgets import Button, Label, Static

HOURS: tuple[str, ...] = tuple(f"{h:02d}" for h in range(24))
MINUTES: tuple[str, ...] = ("00", "15", "30", "45")


def default_hour_minute() -> tuple[int, int]:
    now = datetime.now()
    return now.hour, (now.minute // 15) * 15


def snap_hour_minute(time_str: str) -> tuple[int, int]:
    try:
        h, m = map(int, time_str.strip().split(":")[:2])
        h = max(0, min(23, h))
        m_idx = min(range(len(MINUTES)), key=lambda i: abs(int(MINUTES[i]) - m))
        return h, int(MINUTES[m_idx])
    except (ValueError, IndexError):
        return default_hour_minute()


class VerticalRollColumn(Vertical):
    """▲ / 表示 / ▼ の縦ロール (時刻は矢印の間に表示)。"""

    DEFAULT_CSS = """
    VerticalRollColumn {
        width: 10;
        height: auto;
        align: center middle;
    }
    VerticalRollColumn .roll-col-label {
        width: 100%;
        content-align: center middle;
        color: $text-muted;
        height: 1;
        margin-bottom: 1;
    }
    VerticalRollColumn .roll-btn {
        width: 100%;
        height: 1;
        min-height: 1;
    }
    VerticalRollColumn .roll-display {
        width: 100%;
        height: 3;
        content-align: center middle;
        border: solid $accent;
        text-style: bold;
        background: $surface;
        color: $text;
        margin: 0;
    }
    """

    def __init__(
        self,
        values: tuple[str, ...],
        label: str,
        up_id: str,
        down_id: str,
        display_id: str,
        initial: int = 0,
        **kwargs,
    ) -> None:
        super().__init__(**kwargs)
        self._values = values
        self._index = initial % len(values)
        self._label = label
        self._up_id = up_id
        self._down_id = down_id
        self._display_id = display_id

    def compose(self) -> ComposeResult:
        yield Label(self._label, classes="roll-col-label")
        yield Button("▲", classes="roll-btn", id=self._up_id)
        yield Static(self._values[self._index], classes="roll-display", id=self._display_id)
        yield Button("▼", classes="roll-btn", id=self._down_id)

    @property
    def value(self) -> str:
        return self._values[self._index]

    def set_index(self, index: int) -> None:
        self._index = index % len(self._values)
        if self.is_mounted:
            self.query_one(f"#{self._display_id}", Static).update(self._values[self._index])

    def roll(self, delta: int) -> None:
        self.set_index(self._index + delta)


class TimePicker(Horizontal):
    """時・分を ▲/▼ の間の数字で縦ロール選択 (分は15分刻み)。"""

    DEFAULT_CSS = """
    TimePicker {
        width: auto;
        height: auto;
        align: left middle;
        margin-bottom: 1;
    }
    .time-sep {
        width: 3;
        height: auto;
        content-align: center middle;
        text-style: bold;
        padding: 2 0 0 0;
    }
    """

    def __init__(self, initial: str | None = None, **kwargs) -> None:
        super().__init__(**kwargs)
        if initial:
            h, m = snap_hour_minute(initial)
        else:
            h, m = default_hour_minute()
        self._hour_idx = h
        self._min_idx = MINUTES.index(f"{m:02d}")

    def compose(self) -> ComposeResult:
        yield VerticalRollColumn(
            HOURS, "時", "time-hour-up", "time-hour-down", "time-hour-display",
            initial=self._hour_idx, classes="time-hour-roll")
        yield Static(":", classes="time-sep")
        yield VerticalRollColumn(
            MINUTES, "分", "time-min-up", "time-min-down", "time-min-display",
            initial=self._min_idx, classes="time-min-roll")

    @property
    def value(self) -> str:
        if self.is_mounted:
            hour = self.query_one(".time-hour-roll", VerticalRollColumn).value
            minute = self.query_one(".time-min-roll", VerticalRollColumn).value
            return f"{hour}:{minute}"
        return f"{HOURS[self._hour_idx]}:{MINUTES[self._min_idx]}"

    def set_value(self, time_str: str) -> None:
        h, m = snap_hour_minute(time_str)
        self._hour_idx = h
        self._min_idx = MINUTES.index(f"{m:02d}")
        if self.is_mounted:
            self.query_one(".time-hour-roll", VerticalRollColumn).set_index(self._hour_idx)
            self.query_one(".time-min-roll", VerticalRollColumn).set_index(self._min_idx)

    def on_button_pressed(self, event: Button.Pressed) -> None:
        bid = event.button.id or ""
        hour_roll = self.query_one(".time-hour-roll", VerticalRollColumn)
        min_roll = self.query_one(".time-min-roll", VerticalRollColumn)
        if bid == "time-hour-up":
            hour_roll.roll(-1)
        elif bid == "time-hour-down":
            hour_roll.roll(1)
        elif bid == "time-min-up":
            min_roll.roll(-1)
        elif bid == "time-min-down":
            min_roll.roll(1)
