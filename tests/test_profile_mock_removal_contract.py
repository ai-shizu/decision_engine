# -*- coding: utf-8 -*-
"""Finding 9 — PROFILE permanent mock surfaces must be removed."""
from __future__ import annotations

import re
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
DESKTOP = ROOT / "apps" / "desktop" / "src"
DOCS = ROOT / "docs"
CORE = ROOT / "src" / "python" / "core"


def _read(path: Path) -> str:
    return path.read_text(encoding="utf-8")


def _profile() -> str:
    return _read(DESKTOP / "components" / "ProfileTab.tsx")


def _chart() -> str:
    return _read(DESKTOP / "components" / "TensorRadarChart.tsx")


def _css() -> str:
    return _read(DESKTOP / "App.css")


FIXED_6D = ("0.72", "0.58", "0.64", "0.68", "0.76", "0.61")


def test_profile_has_no_tensor_radar_preview_constant() -> None:
    assert "TENSOR_RADAR_PREVIEW" not in _profile()


def test_profile_has_no_fixed_6d_preview_values() -> None:
    tab = _profile()
    for value in FIXED_6D:
        assert value not in tab, value


def test_profile_has_no_phase1_preview_label() -> None:
    assert "PHASE 1 PREVIEW / NOT MEASURED" not in _profile()


def test_profile_does_not_import_or_render_mbti() -> None:
    tab = _profile()
    assert "MbtiGradientBars" not in tab
    assert "from \"./MbtiGradientBars\"" not in tab


def test_mbti_gradient_bars_file_deleted() -> None:
    assert not (DESKTOP / "components" / "MbtiGradientBars.tsx").exists()


def test_tensor_radar_chart_has_no_preview_api() -> None:
    chart = _chart()
    assert "preview?" not in chart
    assert "preview =" not in chart
    assert "PREVIEW_LABEL" not in chart
    assert "PHASE 1 PREVIEW" not in chart
    assert "tensor-radar-preview-label" not in chart


def test_interview_mission_result_passes_report_tensor_profile() -> None:
    tab = _read(DESKTOP / "components" / "InterviewTab.tsx")
    mission = tab.split(">MISSION_RESULT<", 1)[1].split(")}\n\n      <form", 1)[0]
    assert "TensorProfilePanel" in mission
    assert "report.tensor_profile" in mission


def test_tensor_profile_panel_stays_display_only() -> None:
    panel = _read(DESKTOP / "components" / "TensorProfilePanel.tsx")
    for forbidden in ("pkbInvoke", "invoke(", "consult(", "MBTI", "mbti", ".quote", "evidence_id"):
        assert forbidden not in panel, forbidden


def test_no_new_mbti_estimation_or_ipc() -> None:
    desktop_ts = list(DESKTOP.rglob("*.ts")) + list(DESKTOP.rglob("*.tsx"))
    joined = "\n".join(_read(p) for p in desktop_ts)
    assert "MbtiGradientBars" not in joined
    assert "mbti_estimate" not in joined.lower()
    assert "save_mbti" not in joined.lower()
    # No MBTI IPC command strings in frontend engine wrappers.
    engine = _read(DESKTOP / "lib" / "engine.ts")
    assert re.search(r"\bmbti\b", engine, flags=re.I) is None
    for path in CORE.rglob("*.py"):
        assert "mbti" not in path.name.lower()
        if path.name in {"facade.py"} or path.name.endswith("stdio.py"):
            assert re.search(r"\bmbti\b", _read(path), flags=re.I) is None


def test_obsolete_preview_css_removed() -> None:
    css = _css()
    for sel in (
        ".mbti-preview-panel",
        ".mbti-preview-label",
        ".mbti-preview-list",
        ".mbti-preview-row",
        ".mbti-preview-labels",
        ".mbti-preview-bar",
        ".tensor-radar-preview-label",
        ".tensor-radar-section",
    ):
        assert sel not in css, sel


def test_measured_radar_css_retained() -> None:
    css = _css()
    for sel in (
        ".tensor-radar-panel",
        ".tensor-radar-svg",
        ".tensor-radar-legend",
        ".tensor-radar-tooltip",
        ".tensor-radar-help-trigger",
    ):
        assert sel in css, sel
    assert "focus-visible" in css


def test_docs_record_preview_retirement() -> None:
    skills = _read(DOCS / "AI_SKILLS.md")
    tensor_spec = _read(DOCS / "SPEC_ENGINE_TENSOR_PROFILING.md")
    phase3 = _read(DOCS / "SPEC_PHASE3_UX_ROMANCE.md")
    assert "Finding 9" in skills or "退役" in skills
    assert re.search(r"preview.*退役|退役.*preview|復元禁止", tensor_spec, flags=re.I)
    assert re.search(r"MBTI.*非表示|測定契約.*MBTI|MBTI.*測定契約", phase3)
    assert "復元禁止" in tensor_spec or "再導入禁止" in skills or "復元禁止" in skills
