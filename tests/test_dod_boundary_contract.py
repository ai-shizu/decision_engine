# -*- coding: utf-8 -*-
"""Finding 5 — DoD boundary gate / runner contract."""
from __future__ import annotations

import json
import re
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
DESKTOP = ROOT / "apps" / "desktop"
PACKAGE_JSON = DESKTOP / "package.json"
RUNNER = DESKTOP / "scripts" / "run-boundary-tests.ps1"
TSCONFIG_BOUNDARY = DESKTOP / "tsconfig.boundary.json"
AI_SKILLS = ROOT / "docs" / "AI_SKILLS.md"
HANDOFF = ROOT / "docs" / "HANDOFF.md"
TESTS_RUNTIME = DESKTOP / "tests-runtime"


def _section(text: str, heading_pat: str) -> str:
    m = re.search(heading_pat, text, flags=re.M)
    assert m, f"section not found: {heading_pat}"
    start = m.start()
    rest = text[m.end() :]
    nxt = re.search(r"^#{1,3}\s+\d", rest, flags=re.M)
    end = m.end() + (nxt.start() if nxt else len(rest))
    return text[start:end]


def _dod_section() -> str:
    return _section(_read(AI_SKILLS), r"^###\s+3\.5\s+完了の定義")


def _handoff_s10() -> str:
    return _section(_read(HANDOFF), r"^##\s+10\.\s+")


def _read(path: Path) -> str:
    return path.read_text(encoding="utf-8")


def test_package_json_has_test_boundary_script() -> None:
    pkg = json.loads(_read(PACKAGE_JSON))
    scripts = pkg.get("scripts") or {}
    assert "test:boundary" in scripts
    cmd = scripts["test:boundary"]
    assert "run-boundary-tests.ps1" in cmd
    assert "powershell" in cmd.lower()
    # Must invoke the runner only (no inline tsc/node suite names).
    assert "manifest_boundary" not in cmd
    assert "consult_tensor" not in cmd


def test_ai_skills_dod_includes_npm_test_boundary_with_cwd() -> None:
    dod = _dod_section()
    assert "npm.cmd run test:boundary" in dod or "npm run test:boundary" in dod
    assert re.search(r"apps[/\\]desktop", dod)
    assert "test:boundary" in dod


def test_ai_skills_dod_has_no_fixed_snapshots() -> None:
    dod = _dod_section()
    assert "161" not in dod
    assert not re.search(r"\d+\s*件", dod)
    assert not re.search(r"\d+\s*modules?", dod, flags=re.I)
    assert not re.search(r"\d+\s*ms\b", dod, flags=re.I)
    assert not re.search(r"\d+\s*passed", dod, flags=re.I)


def test_handoff_s10_has_formal_boundary_entry() -> None:
    s10 = _handoff_s10()
    assert "test:boundary" in s10 or "run-boundary-tests.ps1" in s10
    assert "npm.cmd run test:boundary" in s10 or "npm run test:boundary" in s10


def test_runner_has_no_user_specific_absolute_path() -> None:
    text = _read(RUNNER)
    assert not re.search(r"C:\\Users\\", text, flags=re.I)
    assert "badger" not in text.lower()


def test_runner_accepts_python_param_or_pkb_python() -> None:
    text = _read(RUNNER)
    assert re.search(r"\[string\]\s*\$Python", text) or "-Python" in text
    assert "PKB_PYTHON" in text
    assert "LOCALAPPDATA" in text
    assert "Get-Command" in text and "python" in text


def test_runner_uses_ensure_node_path() -> None:
    text = _read(RUNNER)
    assert "ensure-node-path.ps1" in text


def test_runner_finally_deletes_boundary_out() -> None:
    text = _read(RUNNER)
    assert "finally" in text
    assert ".boundary-tests-out" in text
    assert "Remove-Item" in text


def test_tsconfig_boundary_includes_glob() -> None:
    cfg = json.loads(_read(TSCONFIG_BOUNDARY))
    include = cfg.get("include") or []
    assert "tests-runtime/**/*.test.ts" in include
    # No hand-written per-suite roots required.
    assert not any(
        p.endswith("manifest_boundary.test.ts")
        or p.endswith("consult_tensor_boundary.test.ts")
        for p in include
    )


def test_runner_dynamically_enumerates_compiled_suites() -> None:
    text = _read(RUNNER)
    assert "*.test.js" in text or ".test.js" in text
    assert "Get-ChildItem" in text or "Get-ChildItem" in text.replace("`", "")
    # Hard-fail on zero suites.
    assert re.search(r"0|Count\s*-eq\s*0|\.Count\s*-eq\s*0", text)
    # Must not hardcode only the two current suite basenames as the sole runners.
    assert not re.search(
        r'node\s+"\$OutDir/tests-runtime/manifest_boundary\.test\.js"',
        text,
    )


def test_all_existing_runtime_suites_are_covered_by_glob() -> None:
    suites = sorted(TESTS_RUNTIME.glob("*.test.ts"))
    assert suites, "expected at least one tests-runtime/*.test.ts"
    cfg = json.loads(_read(TSCONFIG_BOUNDARY))
    include = cfg.get("include") or []
    assert "tests-runtime/**/*.test.ts" in include
    # Runner dynamic enumeration must not filter to a fixed allowlist of names.
    runner = _read(RUNNER)
    for suite in suites:
        # Presence of basename as the *only* execution target is forbidden;
        # glob coverage is enough. Ensure we don't exclude via hardcoded allowlist.
        assert "allowlist" not in runner.lower()
        assert suite.name.replace(".ts", ".js")  # sanity
    assert len(suites) >= 2
