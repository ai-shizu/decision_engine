# -*- coding: utf-8 -*-
"""Finding 8 — HANDOFF §10 owns gate ownership pointers, not volatile snapshots."""
from __future__ import annotations

import re
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
HANDOFF = ROOT / "docs" / "HANDOFF.md"


def _read(path: Path) -> str:
    return path.read_text(encoding="utf-8")


def _handoff_s10() -> str:
    text = _read(HANDOFF)
    m = re.search(r"^##\s+10\.\s+", text, flags=re.M)
    assert m, "HANDOFF §10 not found"
    nxt = re.search(r"^##\s+11\.\s+", text[m.end() :], flags=re.M)
    assert nxt, "HANDOFF §11 not found"
    return text[m.start() : m.end() + nxt.start()]


def test_s10_heading_has_no_saishin() -> None:
    first = _handoff_s10().splitlines()[0]
    assert "最新" not in first


def test_s10_has_no_snapshot_declarations() -> None:
    s10 = _handoff_s10()
    assert "最新再検証" not in s10
    assert "実環境で再実行" not in s10
    assert "最高アーキテクト補佐" not in s10


def test_s10_names_ai_skills_35_as_sole_dod_canon() -> None:
    s10 = _handoff_s10()
    assert "AI_SKILLS.md" in s10
    assert "§3.5" in s10 or "3.5" in s10
    assert re.search(r"唯一|正本", s10)


def test_s10_does_not_duplicate_full_dod_command_block() -> None:
    s10 = _handoff_s10()
    # Full DoD lives in AI_SKILLS §3.5; §10 must not re-own the whole suite.
    assert "cargo test" not in s10
    assert "cargo check" not in s10
    assert "npm.cmd run build" not in s10
    assert "npx.cmd tsc --noEmit" not in s10
    assert not re.search(r"pytest\s+tests/\s+-q", s10)


def test_s10_keeps_formal_boundary_entry() -> None:
    s10 = _handoff_s10()
    assert "npm.cmd run test:boundary" in s10 or "npm run test:boundary" in s10


def test_s10_boundary_cwd_is_apps_desktop() -> None:
    s10 = _handoff_s10()
    assert re.search(r"cd\s+apps[/\\]desktop", s10, flags=re.I)
    assert "apps/desktop" in s10 or r"apps\desktop" in s10


def test_s10_notes_tsc_noemit_is_not_boundary_substitute() -> None:
    s10 = _handoff_s10()
    assert "tests-runtime" in s10
    assert re.search(r"tsc\s+--noEmit|tsc --noEmit", s10)
    assert re.search(r"代替|対象にしない", s10)


def test_s10_keeps_pytest_sandbox_discipline() -> None:
    s10 = _handoff_s10()
    assert "python -m pytest" in s10
    assert "conftest.py" in s10
    assert "Sandbox" in s10 or "sandbox" in s10.lower()
    assert re.search(r"実\s*`?data/`?|実data", s10) or "data/" in s10


def test_s10_has_no_user_specific_absolute_path() -> None:
    s10 = _handoff_s10()
    assert not re.search(r"C:\\Users\\", s10, flags=re.I)
    assert "badger" not in s10.lower()


def test_s10_has_no_measured_numeric_snapshots() -> None:
    s10 = _handoff_s10()
    assert not re.search(r"\d+\s+passed\b", s10, flags=re.I)
    assert not re.search(r"\d+\s+modules?\b", s10, flags=re.I)
    assert not re.search(r"\d+\s*ms\b", s10, flags=re.I)
    assert not re.search(r"\d+\.\d+\s*s\b", s10, flags=re.I)


def test_s10_has_no_fixed_per_file_count_list() -> None:
    s10 = _handoff_s10()
    assert "test_retrieval_manifest.py" not in s10
    assert "test_integration.py" not in s10
    assert not re.search(r"tests/test_\w+\.py`?:\s*\d+", s10)


def test_s10_does_not_record_persistent_current_results() -> None:
    s10 = _handoff_s10()
    # Snapshot-style result lines (not disciplinary "do not claim GREEN").
    assert not re.search(r"(?m)^\s*-\s*TypeScript:\s*PASS\b", s10)
    assert not re.search(r"(?m)^\s*-\s*.*:\s*PASS\b", s10)
    assert not re.search(r"Vite build:\s*PASS", s10)
    assert not re.search(r"git status.*clean|:\s*clean\b", s10, flags=re.I)
    assert "最新GREEN" not in s10


def test_s10_points_measurements_to_audit_ledger_or_findings() -> None:
    s10 = _handoff_s10()
    assert re.search(
        r"AUDIT_FINDINGS|監査台帳|finding",
        s10,
        flags=re.I,
    )


def test_finding5_handoff_boundary_entry_still_holds() -> None:
    """Do not regress Finding 5's formal boundary entry contract."""
    from test_dod_boundary_contract import test_handoff_s10_has_formal_boundary_entry

    test_handoff_s10_has_formal_boundary_entry()
