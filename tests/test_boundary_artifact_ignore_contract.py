# -*- coding: utf-8 -*-
"""Finding 16 — boundary compile outDir must be ignored; tests-runtime must not."""
from __future__ import annotations

import re
import subprocess
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
GITIGNORE = ROOT / ".gitignore"

OUT_DIR = ".boundary-tests-" + "out"
ROOT_RULE = f"/apps/desktop/{OUT_DIR}/"

ARTIFACT_PROBE = f"apps/desktop/{OUT_DIR}/tests-runtime/probe.test.js"
SOURCE_ASSET = "apps/desktop/tests-runtime/consult_tensor_boundary.test.ts"
NEAR_MISS = f"apps/desktop/{OUT_DIR}-backup/probe.js"


def _active_gitignore_lines() -> list[str]:
    lines: list[str] = []
    for raw in GITIGNORE.read_text(encoding="utf-8").splitlines():
        stripped = raw.strip()
        if not stripped or stripped.startswith("#"):
            continue
        lines.append(stripped)
    return lines


def _check_ignore(path: str) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        ["git", "check-ignore", "-v", "--no-index", path],
        cwd=ROOT,
        capture_output=True,
        text=True,
        encoding="utf-8",
        errors="replace",
    )


def test_exact_rooted_ignore_rule_present_once() -> None:
    active = _active_gitignore_lines()
    matches = [line for line in active if line == ROOT_RULE]
    assert matches, f"exact rooted rule missing: {ROOT_RULE!r}"
    assert len(matches) == 1, f"rooted rule duplicated: {matches}"


def test_artifact_path_ignored_by_repo_gitignore() -> None:
    proc = _check_ignore(ARTIFACT_PROBE)
    assert proc.returncode == 0, (
        f"artifact must be ignored; rc={proc.returncode} "
        f"stdout={proc.stdout!r} stderr={proc.stderr!r}"
    )
    out = proc.stdout
    assert ".gitignore" in out, f"must come from repo .gitignore, got: {out!r}"
    assert ROOT_RULE in out, f"exact rooted rule missing from check-ignore: {out!r}"
    assert ".git/info/exclude" not in out
    # Reject global/excludesfile style sources (path after source:line:rule).
    source = out.split(":", 1)[0].replace("\\", "/")
    assert source.endswith(".gitignore") or source == ".gitignore", (
        f"unexpected ignore source: {source!r} full={out!r}"
    )


def test_tests_runtime_source_not_ignored() -> None:
    proc = _check_ignore(SOURCE_ASSET)
    assert proc.returncode != 0, (
        f"tests-runtime must remain visible; got ignore: {proc.stdout!r}"
    )


def test_near_miss_out_backup_not_ignored() -> None:
    proc = _check_ignore(NEAR_MISS)
    assert proc.returncode != 0, (
        f"near-miss backup dir must remain visible; got ignore: {proc.stdout!r}"
    )
