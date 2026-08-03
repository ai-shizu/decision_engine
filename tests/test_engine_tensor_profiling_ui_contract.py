# -*- coding: utf-8 -*-
"""Project Calculus Phase 1 — frontend contract tests (static + redactor mirror)."""
from __future__ import annotations

import re
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
DESKTOP = ROOT / "apps" / "desktop" / "src"
PKG = ROOT / "apps" / "desktop" / "package.json"
LOCK = ROOT / "apps" / "desktop" / "package-lock.json"

OPEN_TAG = "<think>"
CLOSE_TAG = "</think>"


def _read(rel: str) -> str:
    return (DESKTOP / rel).read_text(encoding="utf-8")


def _match_tag(s: str, pos: int, tag: str) -> int:
    remain = len(s) - pos
    if remain <= 0:
        return -1
    n = min(len(tag), remain)
    for i in range(n):
        if s[pos + i].lower() != tag[i].lower():
            return -1
    if n < len(tag):
        return 0
    return len(tag)


def _is_partial_open_prefix(s: str, pos: int) -> bool:
    fragment = s[pos:]
    if not fragment or len(fragment) >= len(OPEN_TAG):
        return False
    return fragment.lower() == OPEN_TAG[: len(fragment)].lower()


def _redact_hidden_reasoning(raw: str, streaming: bool = False) -> str:
    """Mirror of InterviewTab.redactHiddenReasoning for behavioral contract tests."""
    out: list[str] = []
    depth = 0
    i = 0
    while i < len(raw):
        open_full = _match_tag(raw, i, OPEN_TAG)
        if open_full == len(OPEN_TAG):
            depth += 1
            i += len(OPEN_TAG)
            continue
        close_full = _match_tag(raw, i, CLOSE_TAG)
        if close_full == len(CLOSE_TAG):
            if depth > 0:
                depth -= 1
            i += len(CLOSE_TAG)
            continue
        if depth == 0:
            if streaming and _is_partial_open_prefix(raw, i):
                break
            out.append(raw[i])
        i += 1
    return "".join(out)


def test_redactor_exported_and_used_before_render() -> None:
    # NOTE: the legacy per-speaker `GdThreadMessage` component (and its GD-only
    # render branch) was deleted wholesale in fcf7fed (dead-code cleanup); the
    # legacy consult()-backed gd_sim mode now renders through the *same* generic
    # ai-message branch as every other mode, keyed off `m.speaker` (set to "GD"
    # at send() time). See docs/AI_SKILLS.md for the Inner Coliseum GD Arena,
    # which owns per-speaker parsing today (lib/gdStreamParser.ts).
    tab = _read("components/InterviewTab.tsx")
    lib = _read("lib/redactHiddenReasoning.ts")
    assert "export function redactHiddenReasoning" in lib
    assert "redactHiddenReasoning(" in tab
    render_block = tab.split("messages.map", 1)[1].split("MISSION_RESULT", 1)[0]
    assert "redactHiddenReasoning" in render_block
    assert 'm.role === "user"' in render_block
    assert "m.speaker" in render_block
    assert "visibleText" in render_block or "redactHiddenReasoning(m.text" in render_block
    # Redaction must still gate speaker-name resolution/display (no path from
    # raw m.text to the DOM that skips redactHiddenReasoning first).
    assert render_block.index("redactHiddenReasoning(m.text") < render_block.index("m.speaker")


def test_redactor_behavior_table() -> None:
    cases = [
        ("", False, ""),
        ("visible only", False, "visible only"),
        ("prefix<think>secret", False, "prefix"),
        ("prefix<think>secret", True, "prefix"),
        ("a<think>x</think>b", False, "ab"),
        ("</think>tail", False, "tail"),
        ("<think>a<think>b</think></think>", False, ""),
    ]
    for raw, streaming, expected in cases:
        assert _redact_hidden_reasoning(raw, streaming) == expected

    assert _redact_hidden_reasoning("<th", True) == ""
    assert _redact_hidden_reasoning("<think>sec", True) == ""
    assert _redact_hidden_reasoning("<think>sec", False) == ""
    assert (
        _redact_hidden_reasoning("<think>secret</think>answer", False)
        == "answer"
    )

    acc = ""
    for chunk in ("<th", "ink>secret", "</think>answer"):
        acc += chunk
        streaming = chunk != "</think>answer"
        got = _redact_hidden_reasoning(acc, streaming)
        if chunk == "<th":
            assert got == ""
        elif chunk == "ink>secret":
            assert got == ""
        else:
            assert got == "answer"


def test_gd_redaction_before_parser() -> None:
    # Per-speaker GD parsing (formerly `parseGdSpeakerTurns` inside the deleted
    # `GdThreadMessage` component) now lives in the Inner Coliseum's dedicated
    # stream parser (lib/gdStreamParser.ts), decoupled from the legacy
    # consult()-backed panel in InterviewTab.tsx. Each surface is checked on
    # its own terms: the legacy panel must still redact before any render, and
    # the current speaker-turn parser must still exist with its real exports.
    tab = _read("components/InterviewTab.tsx")
    render_loop = tab.split("messages.map", 1)[1].split("MISSION_RESULT", 1)[0]
    assert "redactHiddenReasoning" in render_loop
    parser = _read("lib/gdStreamParser.ts")
    assert "export function parseGdStream" in parser
    assert "export class GdIncrementalStreamParser" in parser


def test_tensor_radar_chart_contract() -> None:
    chart = _read("components/TensorRadarChart.tsx")
    assert "export function TensorRadarChart" in chart
    assert "Math.sin" in chart and "Math.cos" in chart
    assert "<svg" in chart and "<title" in chart and "<desc" in chart
    assert "0.25" in chart and "0.50" in chart and "0.75" in chart and "1.00" in chart
    assert "toFixed(2)" in chart
    assert "role=\"img\"" in chart
    forbidden = (
        "canvas",
        "Chart.js",
        "recharts",
        "d3",
        "Math.random",
        "Date.now",
        "dangerouslySetInnerHTML",
        "fetch(",
        "localStorage",
    )
    for token in forbidden:
        assert token not in chart, f"forbidden in TensorRadarChart: {token}"


def test_profile_tab_has_no_tensor_preview() -> None:
    tab = _read("components/ProfileTab.tsx")
    assert "TensorRadarChart" not in tab
    assert "TENSOR_PROFILE_6D" not in tab
    assert "PHASE 1 PREVIEW / NOT MEASURED" not in tab
    assert "TENSOR_RADAR_PREVIEW" not in tab


def test_forbidden_tokens_on_changed_surface() -> None:
    tab = _read("components/InterviewTab.tsx")
    lib = _read("lib/redactHiddenReasoning.ts")
    redactor_slice = lib.split("export function redactHiddenReasoning", 1)[1]
    render_slice = tab.split("const visibleText = redactHiddenReasoning", 1)[1].split(
        "MISSION_RESULT", 1
    )[0]
    profile = _read("components/ProfileTab.tsx")
    chart = _read("components/TensorRadarChart.tsx")
    changed = redactor_slice + render_slice + profile + chart
    forbidden = (
        "fetch(",
        "axios",
        "localStorage",
        "sessionStorage",
        "Math.random",
        "Date.now",
        "dangerouslySetInnerHTML",
        ".quote",
        "fact_text",
        "line_text",
    )
    for token in forbidden:
        assert token not in changed, f"forbidden token in Phase 1 surface: {token}"


def test_package_manifests_unchanged() -> None:
    assert PKG.exists() and LOCK.exists()
    # Contract: Phase 1 must not edit manifests; compare absence from git diff is
    # validated in the manual gate. Here we only assert no chart dependency strings
    # were added to manifests (read-only sniff).
    pkg = PKG.read_text(encoding="utf-8")
    assert "recharts" not in pkg and "chart.js" not in pkg.lower()


def test_tensor_radar_css_classes() -> None:
    # NOTE: `.gd-thread-row` (the old GD thread bubble class) was deleted along
    # with GdThreadMessage in fcf7fed; `.vault-panel` is the block that now
    # immediately follows the tensor-radar rules and is the correct boundary.
    css = _read("App.css")
    assert ".tensor-radar-panel" in css
    assert ".tensor-radar-svg" in css
    start = css.index(".tensor-radar-panel")
    end = css.find(".vault-panel", start)
    assert end != -1, "tensor-radar CSS boundary marker (.vault-panel) not found"
    tensor_css = css[start:end]
    assert re.search(r"#[0-9a-fA-F]{3,8}", tensor_css) is None


def _css_declaration_block(css: str, selector: str) -> str:
    match = re.search(re.escape(selector) + r"\s*\{([^}]*)\}", css, re.DOTALL)
    assert match, f"missing CSS block for {selector}"
    return match.group(1)


def test_phase3_added_css_blocks_have_no_literal_px() -> None:
    css = _read("App.css")
    for selector in (
        ".tensor-radar-help-trigger:focus-visible",
        ".tensor-radar-tooltip",
    ):
        block = _css_declaration_block(css, selector)
        hits = re.findall(r"\d+px", block)
        assert hits == [], f"literal px in {selector}: {hits} in {block.strip()}"


def test_tensor_radar_legend_renders_visible_axis_name() -> None:
    chart = _read("components/TensorRadarChart.tsx")
    legend = chart.split('className="tensor-radar-legend"', 1)[1].split("</ul>", 1)[0]
    assert "d.axisName" in legend
    assert "tensor-radar-axis-name" in legend
    assert "{d.axisName}" in legend
    assert legend.index("d.axisName") < legend.index("AxisHelp")
    assert 'tabIndex={0}' in chart
    assert 'aria-describedby' in chart
    assert 'role="tooltip"' in chart
    assert "[?]" in chart
    assert "AxisHelp" in legend
    assert legend.count("d.axisName") >= 1
    desc_only = chart.split("<desc", 1)[0]
    assert "tensor-radar-axis-name" not in desc_only or "{d.axisName}" in legend
