# -*- coding: utf-8 -*-
"""Project Calculus Phase 3-A — frontend UX contract tests."""
from __future__ import annotations

import re
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
DESKTOP = ROOT / "apps" / "desktop" / "src"

NARRATIVE_DRAFT_COPY = (
    "現在のスキルと目指す姿のギャップを分析し、"
    "自己PR・ESの草案を自動生成します（※模擬面接マウント中は実行不可）"
)

AXIS_DESCRIPTIONS: dict[str, str] = {
    "Logical_Rigor": "前提と根拠を結び、筋道立てて検証する力",
    "Quantitative_Agility": "数量や概算を正確かつ素早く扱う力",
    "Structural_Decomposition": "複雑な課題を漏れなく分解する力",
    "Domain_Adaptability": "未知の業界やテーマへ知識を適用する力",
    "Communication_Bandwidth": "考えを簡潔かつ明確に伝える力",
    "Cognitive_Flexibility": "反証や相手の意見を受けて考えを更新する力",
}

MBTI_MOCK = (
    ("E", 42, "I", 58),
    ("S", 55, "N", 45),
    ("T", 61, "F", 39),
    ("J", 47, "P", 53),
)


def _read(rel: str) -> str:
    return (DESKTOP / rel).read_text(encoding="utf-8")


def test_custom_theme_textarea_rows_and_class() -> None:
    tab = _read("components/InterviewTab.tsx")
    block = tab.split("持ち込みお題", 1)[1].split("maxLength={CUSTOM_THEME_MAX_CHARS}", 1)[0]
    assert "rows={6}" in block
    assert 'className="custom-theme-textarea"' in block
    assert "rows={2}" not in block


def test_custom_theme_textarea_resize_vertical_css() -> None:
    css = _read("App.css")
    assert ".custom-theme-textarea" in css
    snippet = css.split(".custom-theme-textarea", 1)[1].split("}", 1)[0]
    assert "resize:" in snippet and "vertical" in snippet
    assert "width:" in snippet and "100%" in snippet
    assert "min-height" in snippet


def test_narrative_draft_copy() -> None:
    tab = _read("components/InterviewTab.tsx")
    assert "NARRATIVE_DRAFT" in tab
    assert NARRATIVE_DRAFT_COPY in tab


def test_mbti_gradient_bars_mock_axes() -> None:
    mbti = _read("components/MbtiGradientBars.tsx")
    profile = _read("components/ProfileTab.tsx")
    assert "MBTI_PREFERENCE_PREVIEW" in mbti
    assert "PREVIEW / NOT MEASURED" in mbti
    assert "MbtiGradientBars" in profile
    assert profile.index("MbtiGradientBars") < profile.index("TENSOR_PROFILE_6D")
    for left, left_pct, right, right_pct in MBTI_MOCK:
        assert str(left_pct) in mbti
        assert str(right_pct) in mbti
        assert left in mbti and right in mbti
    assert "linear-gradient" in mbti


def test_tensor_radar_axis_tooltips_and_keyboard() -> None:
    chart = _read("components/TensorRadarChart.tsx")
    profile = _read("components/ProfileTab.tsx")
    css = _read("App.css")
    assert 'role="tooltip"' in chart
    assert "tabIndex={0}" in chart
    assert "aria-describedby" in chart
    assert "tensor-radar-help" in chart or "tensor-radar-tooltip" in css
    for axis, desc in AXIS_DESCRIPTIONS.items():
        assert axis in profile or axis in chart
        assert desc in chart or desc in profile
    assert "[?]" in chart or "?" in chart.split("legend", 1)[-1]


def test_phase3_preview_no_external_or_ipc() -> None:
    surfaces = (
        _read("components/InterviewTab.tsx"),
        _read("components/ProfileTab.tsx"),
        _read("components/TensorRadarChart.tsx"),
        _read("components/MbtiGradientBars.tsx"),
    )
    joined = "\n".join(surfaces)
    assert "PREVIEW / NOT MEASURED" in joined or "PHASE 1 PREVIEW / NOT MEASURED" in joined
    forbidden = (
        "fetch(",
        "localStorage",
        "sessionStorage",
        "Math.random",
        "Date.now",
        "pkb_invoke",
        "consult(",
        "narrativeCompile(",
    )
    mbti = _read("components/MbtiGradientBars.tsx")
    for token in forbidden:
        assert token not in mbti, f"forbidden in MbtiGradientBars: {token}"
    assert "oraclePayload" not in mbti
    assert "tensorRebuild" not in mbti
