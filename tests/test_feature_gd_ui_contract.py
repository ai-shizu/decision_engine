# -*- coding: utf-8 -*-
"""SPEC_FEATURE_GD_UI — frontend contract tests (static read)."""
from __future__ import annotations

import re
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
DESKTOP = ROOT / "apps" / "desktop" / "src"


def _read(rel: str) -> str:
    return (DESKTOP / rel).read_text(encoding="utf-8")


def test_parse_gd_speaker_turns_exported() -> None:
    tab = _read("components/InterviewTab.tsx")
    types = _read("lib/types.ts")

    assert "export function parseGdSpeakerTurns" in tab
    assert "GdSpeakerTurn" in types
    assert "GD_SPEAKER_HEADER_RE" in tab
    assert r":\s*(.*)$" in tab or "]:\\s*" in tab


def test_gd_parser_uses_line_start_colon_regex() -> None:
    tab = _read("components/InterviewTab.tsx")

    assert "GD_SPEAKER_HEADER_RE" in tab
    assert "]:\\s*" in tab
    assert "splitSpeakers" not in tab
    assert r"[^\][\n]{1,24})\]\s*" not in tab


def test_gd_thread_render_gating() -> None:
    tab = _read("components/InterviewTab.tsx")

    assert 'renderAs: "gd_thread"' in tab
    assert 'mode === "gd_sim"' in tab
    assert 'phase === "debrief"' in tab
    assert 'm.renderAs === "gd_thread"' in tab

    send_block = tab.split("async function send", 1)[1].split("async function handleStart", 1)[0]
    assert 'mode === "gd_sim"' in send_block
    assert 'renderAs: "gd_thread"' in send_block

    mentor_lines = [ln for ln in tab.splitlines() if "メンター" in ln and "res.answer" in ln]
    assert mentor_lines, "debrief mentor response line missing"
    assert "gd_thread" not in mentor_lines[0]


def test_gd_thread_forbidden_tokens() -> None:
    tab = _read("components/InterviewTab.tsx")

    parser_slice = tab.split("export function parseGdSpeakerTurns", 1)[1].split("function GdThreadMessage", 1)[0]
    for token in ("fetch(", "localStorage", "sessionStorage", "Math.random"):
        assert token not in parser_slice, f"forbidden token in parser: {token}"

    assert "from \"react\"" in tab
    assert "gd-thread" in tab
    assert "gd-turn" in tab
