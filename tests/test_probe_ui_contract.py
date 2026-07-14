# -*- coding: utf-8 -*-
"""Phase F6 PROBE UI — frontend contract tests (static read + render smoke)."""
from __future__ import annotations

import re
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
DESKTOP = ROOT / "apps" / "desktop" / "src"


def _read(rel: str) -> str:
    return (DESKTOP / rel).read_text(encoding="utf-8")


def test_probe_tab_registered() -> None:
    app = _read("App.tsx")
    types = _read("lib/types.ts")

    assert '"probe"' in types or "'probe'" in types
    assert 'id: "probe"' in app
    assert 'label: "PROBE"' in app
    assert "<ProbeTab />" in app
    assert re.search(r"\[1-6\]", app), "Alt shortcut must accept 1-6"


def test_probe_engine_wrappers() -> None:
    engine = _read("lib/engine.ts")

    for fn in ("sourceCode", "probeStatus", "probeNext", "probeAnswer"):
        assert f"export async function {fn}" in engine

    assert '"profile_source_code"' in engine
    assert '"probe_status"' in engine
    assert '"probe_next"' in engine
    assert '"probe_answer"' in engine
    assert '"profile.source_code"' not in engine
    assert '"probe.status"' not in engine

    assert "session_id: sessionId" in engine
    assert "question_id: questionId" in engine
    assert "answer," in engine
    assert "today," in engine


def test_probe_tab_privacy_and_validation() -> None:
    tab = _read("components/ProbeTab.tsx")

    assert "maxLength={120}" in tab
    assert "Ctrl+Enter" in tab or "Meta+Enter" in tab
    assert 'e.key === "Enter"' in tab

    forbidden = (
        "fetch(",
        "axios",
        "localStorage",
        "sessionStorage",
        "Math.random",
        "toISOString().slice(0, 10)",
        "EvidenceRef",
        "fact_text",
        "line_text",
    )
    for token in forbidden:
        assert token not in tab, f"forbidden token in ProbeTab: {token}"

    assert "from \"react\"" in tab
    assert "probeAnswer" in tab
    assert "probeNext" in tab
    assert "probeStatus" in tab
