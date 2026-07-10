# -*- coding: utf-8 -*-
"""UI Orphan Integration — frontend contract tests (static read)."""
from __future__ import annotations

import re
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
DESKTOP = ROOT / "apps" / "desktop" / "src"


def _read(rel: str) -> str:
    return (DESKTOP / rel).read_text(encoding="utf-8")


def test_profile_tab_registered() -> None:
    app = _read("App.tsx")
    types = _read("lib/types.ts")

    assert '"profile"' in types or "'profile'" in types
    assert '{ id: "profile", label: "PROFILE" }' in app
    assert "<ProfileTab />" in app
    assert re.search(r"\[1-7\]", app), "Alt shortcut must accept 1-7"


def test_orphan_engine_wrappers() -> None:
    engine = _read("lib/engine.ts")

    assert "export async function narrativeCompile" in engine
    assert "export async function knowledgeFetchPending" in engine
    assert '"narrative.compile"' in engine
    assert '"knowledge.fetch_pending"' in engine

    for cmd in (
        '"profile.source_code"',
        '"oracle.payload"',
        '"oracle.report"',
        '"twin.forecast"',
        '"tensor.rebuild"',
    ):
        assert cmd in engine, f"missing command string: {cmd}"


def test_profile_tab_contract() -> None:
    tab = _read("components/ProfileTab.tsx")

    for fn in ("sourceCode", "oraclePayload", "oracleReport", "twinForecast", "tensorRebuild"):
        assert fn in tab, f"ProfileTab must use {fn}"

    assert "oraclePayload(" in tab
    assert "useEffect" in tab
    mount_block = tab.split("useEffect", 1)[1].split("}, [loadSterile]", 1)[0]
    assert "loadSterile" in mount_block
    sterile_block = tab.split("const loadSterile = useCallback", 1)[1].split("}, []);", 1)[0]
    assert "oraclePayload(" in sterile_block
    assert "oracleReport(" not in mount_block, "oracleReport must not run on mount"

    forbidden = (
        "fetch(",
        "axios",
        "localStorage",
        "sessionStorage",
        "Math.random",
        "Date.now",
        ".quote",
        "fact_text",
        "line_text",
    )
    for token in forbidden:
        assert token not in tab, f"forbidden token in ProfileTab: {token}"


def test_existing_tabs_receive_orphan_actions() -> None:
    import_tab = _read("components/ImportTab.tsx")
    interview_tab = _read("components/InterviewTab.tsx")

    assert "knowledgeFetchPending" in import_tab
    assert "narrativeCompile" in interview_tab

    for tab_src, name in ((import_tab, "ImportTab"), (interview_tab, "InterviewTab")):
        assert "fetch(" not in tab_src, f"{name} must not use fetch("
        assert "localStorage" not in tab_src, f"{name} must not use localStorage"
