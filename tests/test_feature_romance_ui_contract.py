# -*- coding: utf-8 -*-
"""Phase 3-B — romance_analysis UI contract tests."""
from __future__ import annotations

import re
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
DESKTOP = ROOT / "apps" / "desktop" / "src"


def _read(rel: str) -> str:
    return (DESKTOP / rel).read_text(encoding="utf-8")


def test_consult_mode_dropdown_includes_romance() -> None:
    tab = _read("components/ConsultTab.tsx")
    assert "romance_analysis" in tab
    assert "Romance（交流パルス解析）" in tab


def test_consult_calls_romance_mode() -> None:
    tab = _read("components/ConsultTab.tsx")
    assert 'mode: "romance_analysis"' in tab or 'mode:"romance_analysis"' in tab


def test_no_json_parse_in_romance_surface() -> None:
    surfaces = (
        _read("components/ConsultTab.tsx"),
        _read("components/RomanceAnalysisPanel.tsx"),
    )
    joined = "\n".join(surfaces)
    assert "JSON.parse" not in joined


def test_romance_panel_meter_and_disclaimer() -> None:
    panel = _read("components/RomanceAnalysisPanel.tsx")
    css = _read("App.css")
    assert "ROMANCE / INTERACTION_PULSE" in panel
    assert "交流往復指数" in panel
    assert "N/A" in panel
    assert "好意や感情を判定するものではありません" in panel
    assert "interaction_tendency" in panel
    assert "next_best_action" in panel
    assert ".romance-pulse-meter" in css


def test_romance_ui_no_raw_input_in_panel() -> None:
    tab = _read("components/ConsultTab.tsx")
    panel = _read("components/RomanceAnalysisPanel.tsx")
    assert "会話履歴を解析しました" in tab
    assert "[self]" not in panel
    assert "contact_alias" not in panel or "交流" in panel


def test_romance_engine_types() -> None:
    engine = _read("lib/engine.ts")
    assert '"romance_analysis"' in engine
    assert "RomanceAnalysisResult" in engine
    assert "romance_analysis" in engine


def test_romance_forbidden_tokens() -> None:
    surfaces = (
        _read("components/ConsultTab.tsx"),
        _read("components/RomanceAnalysisPanel.tsx"),
        _read("lib/engine.ts"),
    )
    joined = "\n".join(surfaces)
    for token in ("fetch(", "localStorage", "Math.random", "Date.now"):
        assert token not in joined, f"forbidden: {token}"


def test_romance_css_no_literal_px_in_added_blocks() -> None:
    css = _read("App.css")
    for selector in (".romance-analysis-panel", ".romance-pulse-meter", ".romance-pulse-fill"):
        match = re.search(re.escape(selector) + r"\s*\{([^}]*)\}", css, re.DOTALL)
        assert match, f"missing {selector}"
        block = match.group(1)
        assert re.search(r"\d+px", block) is None, f"px in {selector}"
        assert "letter-spacing" not in block


ROMANCE_SUCCESS_MESSAGE = "会話履歴を解析しました"


def test_romance_submit_clears_previous_result_at_start() -> None:
    tab = _read("components/ConsultTab.tsx")
    assert "setRomanceResult(null)" in tab
    assert "stripRomanceSuccessMessages" in tab or "ROMANCE_SUCCESS_MESSAGE" in tab


def test_romance_submit_removes_previous_success_message() -> None:
    tab = _read("components/ConsultTab.tsx")
    assert "stripRomanceSuccessMessages" in tab
    assert ROMANCE_SUCCESS_MESSAGE in tab


def test_romance_requires_romance_analysis_in_response() -> None:
    tab = _read("components/ConsultTab.tsx")
    assert "if (!res.romance_analysis)" in tab
    assert "解析結果を取得できませんでした" in tab


def test_romance_failure_clears_panel_and_success_message() -> None:
    tab = _read("components/ConsultTab.tsx")
    assert "catch" in tab
    assert "setRomanceResult(null)" in tab
    assert "stripRomanceSuccessMessages" in tab


def test_spec_scope_no_deferred_phase3b() -> None:
    spec = (ROOT / "docs" / "SPEC_PHASE3_UX_ROMANCE.md").read_text(encoding="utf-8")
    scope = spec.split("## Phase 3-A deliverables")[0]
    assert "deferred" not in scope.lower()
    assert "future, not implemented" not in scope
    assert "This spec does not encode Phase 3-B logic" not in scope
