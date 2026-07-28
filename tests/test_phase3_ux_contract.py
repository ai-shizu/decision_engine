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

MBTI_MOCK_VALUES = ("42", "58", "55", "45", "61", "39", "47", "53")
FIXED_6D_PREVIEW = ("0.72", "0.58", "0.64", "0.68", "0.76", "0.61")


def _read(rel: str) -> str:
    return (DESKTOP / rel).read_text(encoding="utf-8")




def test_narrative_draft_copy() -> None:
    tab = _read("components/InterviewTab.tsx")
    assert "NARRATIVE_DRAFT" in tab
    assert NARRATIVE_DRAFT_COPY in tab


def test_profile_has_no_mbti_or_fixed_6d_mocks() -> None:
    """Finding 9 — reintroduction guard for PROFILE permanent mocks."""
    profile = _read("components/ProfileTab.tsx")
    assert "MbtiGradientBars" not in profile
    assert not (DESKTOP / "components" / "MbtiGradientBars.tsx").exists()
    assert "TENSOR_RADAR_PREVIEW" not in profile
    assert "PHASE 1 PREVIEW / NOT MEASURED" not in profile
    assert "TENSOR_PROFILE_6D" not in profile
    for value in FIXED_6D_PREVIEW + MBTI_MOCK_VALUES:
        assert value not in profile, value


def test_tensor_radar_axis_tooltips_and_keyboard() -> None:
    chart = _read("components/TensorRadarChart.tsx")
    view = _read("lib/tensorProfileView.ts")
    parser = _read("lib/parseConsultResponse.ts")
    css = _read("App.css")
    assert 'role="tooltip"' in chart
    assert "tabIndex={0}" in chart
    assert "aria-describedby" in chart
    assert "tensor-radar-help" in chart or "tensor-radar-tooltip" in css
    for axis, desc in AXIS_DESCRIPTIONS.items():
        assert axis in parser or axis in view
        assert desc in view
    assert "[?]" in chart or "?" in chart.split("legend", 1)[-1]


def test_phase3_profile_and_radar_stay_offline() -> None:
    """PROFILE/radar must not invent client-side mock persistence or entropy."""
    surfaces = (
        _read("components/ProfileTab.tsx"),
        _read("components/TensorRadarChart.tsx"),
    )
    joined = "\n".join(surfaces)
    forbidden = (
        "fetch(",
        "localStorage",
        "sessionStorage",
        "Math.random",
        "Date.now",
    )
    for token in forbidden:
        assert token not in joined, f"forbidden on PROFILE/radar surface: {token}"
