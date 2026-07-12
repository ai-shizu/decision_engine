# -*- coding: utf-8 -*-
"""Audit finding remediation — interview_report.v1 tensor_profile UI contract.

Static source-inspection tests (same pattern as
test_engine_tensor_profiling_ui_contract.py). Verifies the frontend surface
without executing React — no JS runtime is available inside pytest.
"""
from __future__ import annotations

from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
DESKTOP = ROOT / "apps" / "desktop" / "src"


def _read(rel: str) -> str:
    return (DESKTOP / rel).read_text(encoding="utf-8")


def test_interview_tab_renders_tensor_profile_panel() -> None:
    tab = _read("components/InterviewTab.tsx")
    assert "TensorProfilePanel" in tab
    assert 'import { TensorProfilePanel } from "./TensorProfilePanel"' in tab
    mission_block = tab.split(">MISSION_RESULT<", 1)[1].split(")}\n\n      <form", 1)[0]
    assert "TensorProfilePanel" in mission_block


def test_interview_and_gd_share_single_report_render_path() -> None:
    tab = _read("components/InterviewTab.tsx")
    # exactly one MISSION_RESULT block; exactly one TensorProfilePanel usage
    # inside it (no mode-specific duplication for interview_sim vs gd_sim).
    assert tab.count(">MISSION_RESULT<") == 1
    mission_block = tab.split(">MISSION_RESULT<", 1)[1].split(")}\n\n      <form", 1)[0]
    assert mission_block.count("TensorProfilePanel") == 1
    assert "mode ===" not in mission_block


def test_interview_tab_mounts_tensor_profile_from_existing_report_state() -> None:
    tab = _read("components/InterviewTab.tsx")
    assert "report.tensor_profile" in tab
    # no new state, no new fetch/IPC introduced for the panel itself
    mission_block = tab.split(">MISSION_RESULT<", 1)[1].split(")}\n\n      <form", 1)[0]
    for forbidden in ("useEffect", "pkbInvoke", "consult(", "await "):
        assert forbidden not in mission_block, f"unexpected {forbidden} in MISSION_RESULT"


def test_profile_tab_has_no_fixed_preview() -> None:
    """Finding 9 — PROFILE no longer hosts fixed 6D preview; measured path is Interview/GD."""
    tab = _read("components/ProfileTab.tsx")
    assert "TensorRadarChart" not in tab
    assert "TENSOR_PROFILE_6D" not in tab
    assert "PHASE 1 PREVIEW / NOT MEASURED" not in tab
    assert "TENSOR_RADAR_PREVIEW" not in tab
    for expected_value in ("0.72", "0.58", "0.64", "0.68", "0.76", "0.61"):
        assert expected_value not in tab
    assert "tensorProfileView" not in tab
    assert "TensorProfilePanel" not in tab
    interview = _read("components/InterviewTab.tsx")
    mission = interview.split(">MISSION_RESULT<", 1)[1].split(")}\n\n      <form", 1)[0]
    assert "TensorProfilePanel" in mission
    assert "report.tensor_profile" in mission


def test_tensor_profile_panel_does_not_render_evidence_internals() -> None:
    panel = _read("components/TensorProfilePanel.tsx")
    forbidden = (
        ".quote",
        "speaker_alias",
        "turn_id",
        "turn_index",
        "indicator_id",
        "evidence_id",
        ".evidence",
    )
    for token in forbidden:
        assert token not in panel, f"forbidden evidence-internal token in TensorProfilePanel: {token}"


def test_tensor_profile_panel_does_not_import_profile_tab_constants() -> None:
    panel = _read("components/TensorProfilePanel.tsx")
    view = _read("lib/tensorProfileView.ts")
    # Check for an actual import statement, not the bare word "ProfileTab"
    # (which may legitimately appear in explanatory comments).
    assert "ProfileTab\"" not in panel and "from \"./ProfileTab" not in panel
    assert "ProfileTab\"" not in view and "from \"./ProfileTab" not in view
    assert "TENSOR_RADAR_PREVIEW" not in panel
    assert "TENSOR_RADAR_PREVIEW" not in view


def test_tensor_profile_panel_labels_and_no_severity_claims() -> None:
    panel = _read("components/TensorProfilePanel.tsx")
    assert "TENSOR_PROFILE_6D" in panel
    assert "SESSION MEASUREMENT" in panel
    forbidden_claims = ("診断", "能力証明", "科学的")
    for token in forbidden_claims:
        assert token not in panel, f"forbidden claim token in TensorProfilePanel: {token}"


def test_snapshot_specific_guards_are_absent() -> None:
    """Review remediation 3: point-in-time guards (dispatch command count,
    fixed SHA-256 of core files) do not belong in a permanent pytest module
    — they encode the state of unrelated files at authoring time, not a
    semantic contract, and go stale silently as those files evolve for
    unrelated reasons. Forbidden-surface / core non-modification is instead
    verified once, at session end, via `git diff` (see this finding's
    RESOLVED record in docs/AUDIT_FINDINGS_2026-07-11.md). This test guards
    against reintroducing that pattern into this module."""
    self_source = Path(__file__).read_text(encoding="utf-8")
    # Built by concatenation so the forbidden tokens themselves do not
    # appear as a contiguous substring in this file (which would make the
    # scan trivially self-match its own assertion text).
    forbidden_hash_import = "hash" + "lib"
    forbidden_dispatch_scan = "if cmd " + '== "'
    assert forbidden_hash_import not in self_source
    assert forbidden_dispatch_scan not in self_source


def test_tensor_panel_uses_existing_report_without_direct_ipc() -> None:
    """Semantic contract (replaces the removed dispatch-count snapshot):
    TensorProfilePanel and its data-transform module never call the IPC
    layer directly — they only consume the already-validated report the
    caller passes in as a prop."""
    panel = _read("components/TensorProfilePanel.tsx")
    view = _read("lib/tensorProfileView.ts")
    for forbidden in ("pkbInvoke", "invoke(", "consult(", "useEffect", "await "):
        assert forbidden not in panel, f"unexpected {forbidden} in TensorProfilePanel"
        assert forbidden not in view, f"unexpected {forbidden} in tensorProfileView"
    # engine.ts must not be imported by the panel or the view module.
    assert '"../lib/engine"' not in panel
    assert '"./engine"' not in view


def _css_block(css: str, selector: str) -> str:
    """Extract one CSS rule's declaration body by counting brace depth from
    the selector's opening brace — no whole-file regex scan."""
    start = css.index(selector)
    open_idx = css.index("{", start)
    depth = 1
    i = open_idx + 1
    while depth > 0:
        if css[i] == "{":
            depth += 1
        elif css[i] == "}":
            depth -= 1
        i += 1
    return css[open_idx + 1 : i - 1]


_NEW_TENSOR_PROFILE_SELECTORS = (
    ".tensor-profile-panel",
    ".tensor-profile-session-label",
    ".tensor-profile-summary-list",
    ".tensor-profile-summary-row",
    ".tensor-profile-confidence",
)


def test_tensor_profile_css_blocks_have_no_px() -> None:
    css = _read("App.css")
    for selector in _NEW_TENSOR_PROFILE_SELECTORS:
        block = _css_block(css, selector)
        assert "px" not in block, f"literal px in {selector}: {block.strip()}"


def test_tensor_profile_summary_uses_thin_border() -> None:
    css = _read("App.css")
    block = _css_block(css, ".tensor-profile-summary-row")
    assert "border-bottom: thin solid var(--border);" in block
    assert "1px" not in block


def test_tensor_radar_narrow_query_uses_rem() -> None:
    css = _read("App.css")
    assert "@media (max-width: 30rem)" in css
    assert "@media (max-width: 480px)" not in css
    block = _css_block(css, "@media (max-width: 30rem)")
    assert ".tensor-radar-legend" in block
    assert "px" not in block


def test_forbidden_tokens_on_new_frontend_surface() -> None:
    parser = _read("lib/parseConsultResponse.ts")
    view = _read("lib/tensorProfileView.ts")
    panel = _read("components/TensorProfilePanel.tsx")
    combined = parser + view + panel
    forbidden = (
        "fetch(",
        "axios",
        "localStorage",
        "sessionStorage",
        "Math.random",
        "Date.now",
        "dangerouslySetInnerHTML",
        " as any",
        "<any>",
    )
    for token in forbidden:
        assert token not in combined, f"forbidden token on new tensor-profile surface: {token}"
