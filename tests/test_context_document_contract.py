# -*- coding: utf-8 -*-
"""Finding 4 — CONTEXT.md stable architecture index contract."""
from __future__ import annotations

import re
from pathlib import Path

import pytest

ROOT = Path(__file__).resolve().parents[1]
CONTEXT = ROOT / "docs" / "CONTEXT.md"
AI_SKILLS = ROOT / "docs" / "AI_SKILLS.md"
README = ROOT / "README.md"
APP_TSX = ROOT / "apps" / "desktop" / "src" / "App.tsx"


def _read(path: Path) -> str:
    return path.read_text(encoding="utf-8")


def _extract_app_tabs(app_src: str) -> list[tuple[str, str]]:
    """Extract (id, label) pairs from the const TABS array only."""
    start = app_src.find("const TABS")
    assert start >= 0, "const TABS not found"
    eq = app_src.find("=", start)
    assert eq >= 0
    brace = app_src.find("[", eq)
    assert brace >= 0
    depth = 0
    end = None
    for i, ch in enumerate(app_src[brace:], start=brace):
        if ch == "[":
            depth += 1
        elif ch == "]":
            depth -= 1
            if depth == 0:
                end = i
                break
    assert end is not None
    block = app_src[brace : end + 1]
    return re.findall(
        r'\{\s*id:\s*"([^"]+)"\s*,\s*label:\s*"([^"]+)"\s*\}',
        block,
    )


def _extract_context_ui_tabs(context: str) -> list[tuple[str, str]]:
    """Extract (id, label) from the UI surface markdown table."""
    # Prefer an explicit UI surface section table with id | label columns.
    m = re.search(
        r"##\s+5\.\s+UI surface.*?(?=^##\s+\d|\Z)",
        context,
        flags=re.S | re.M | re.I,
    )
    section = m.group(0) if m else context
    rows: list[tuple[str, str]] = []
    for line in section.splitlines():
        if not line.strip().startswith("|"):
            continue
        cells = [c.strip().strip("`") for c in line.strip().strip("|").split("|")]
        if len(cells) < 2:
            continue
        if cells[0].lower() in {"id", "tab id", "---"} or set(cells[0]) <= {"-", ":"}:
            continue
        if re.fullmatch(r"[a-z_]+", cells[0]) and re.fullmatch(r"[A-Z_]+", cells[1]):
            rows.append((cells[0], cells[1]))
    return rows


def _repo_paths_referenced(text: str) -> list[str]:
    """Collect repo-relative path-like references from markdown."""
    found: set[str] = set()
    for m in re.finditer(r"`([^`\n]+)`", text):
        found.add(m.group(1))
    for m in re.finditer(r"\[[^\]]*\]\(([^)]+)\)", text):
        found.add(m.group(1))
    paths: list[str] = []
    for raw in found:
        p = raw.strip().strip("`").lstrip("./")
        if p.startswith(("http://", "https://", "#")):
            continue
        # Only accept paths that look like repo roots we document.
        if not re.match(
            r"^(apps/|src/|docs/|tests/|scripts/|config/)",
            p.replace("\\", "/"),
        ):
            continue
        if " " in p or "(" in p or ")" in p:
            continue
        paths.append(p.replace("\\", "/"))
    return sorted(set(paths))


def test_context_declares_role() -> None:
    text = _read(CONTEXT)
    assert "安定" in text or "索引" in text
    assert "役割" in text
    assert "本書だけを根拠" in text or "本書だけ" in text


def test_context_declares_document_priority() -> None:
    text = _read(CONTEXT)
    for name in ("AI_SKILLS", "HANDOFF", "INCIDENT_LEDGER"):
        assert name in text
    assert "実コード" in text or "ソース" in text
    assert "優先" in text


def test_context_ui_table_matches_app_tabs() -> None:
    tabs = _extract_app_tabs(_read(APP_TSX))
    assert tabs, "failed to extract App.tsx TABS"
    doc_tabs = _extract_context_ui_tabs(_read(CONTEXT))
    assert doc_tabs == tabs, f"CONTEXT UI table {doc_tabs!r} != App TABS {tabs!r}"


def test_context_names_engine_stdio_as_ipc_authority() -> None:
    text = _read(CONTEXT)
    assert "engine_stdio.py" in text
    assert re.search(r"唯一|正本", text)


def test_context_has_no_fixed_stdio_command_count() -> None:
    text = _read(CONTEXT)
    assert not re.search(r"stdioコマンド\s*\d+", text)
    assert not re.search(r"\d+\s*コマンド", text)
    assert "コマンド10" not in text
    assert "10種" not in text
    assert "26コマンド" not in text


def test_context_has_no_four_tab_stale_claims() -> None:
    text = _read(CONTEXT)
    assert "4タブ" not in text
    assert "4 タブ" not in text
    assert "4 タブシェル" not in text
    assert "stdioコマンド10種" not in text


def test_context_has_no_stale_unimplemented_items() -> None:
    text = _read(CONTEXT)
    # Stale "not yet implemented" backlog phrasing from 2026-07-06 snapshot.
    assert "streaming 表示" not in text
    assert "streaming表示" not in text
    assert "status コールバック" not in text
    assert "statusコールバック" not in text
    assert "家計簿入力日・相談日のカレンダーマーク" not in text


def test_context_has_no_next_task_section() -> None:
    text = _read(CONTEXT)
    assert "次に実装すべきタスク" not in text
    assert re.search(r"^##\s+.*次に実装", text, flags=re.M) is None


def test_context_has_no_volatile_metrics() -> None:
    text = _read(CONTEXT)
    assert not re.search(r"\b[0-9a-f]{40}\b", text)  # full git sha
    assert not re.search(r"SHA-?256", text, flags=re.I)
    assert not re.search(r"\d+\s*passed", text, flags=re.I)
    assert not re.search(r"全スイート\s*\(\s*\d+\s*件", text)
    assert not re.search(r"\d+(\.\d+)?\s*GB\b", text)
    assert not re.search(r"Qwen2\.5-\d+B", text)
    assert not re.search(r"DeepSeek-R1", text)


def test_context_has_no_generated_2026_07_06() -> None:
    assert "Generated: 2026-07-06" not in _read(CONTEXT)


def test_context_does_not_own_dod_or_backlog() -> None:
    text = _read(CONTEXT)
    assert "DoD" in text or "完了の定義" in text or "バックログ" in text
    assert re.search(
        r"(DoD|完了の定義|バックログ).{0,80}(所有しない|持たない|HANDOFF|AI_SKILLS)",
        text,
        flags=re.S,
    )


def test_readme_documents_role_links() -> None:
    text = _read(README)
    assert "引継ぎ書" not in text
    assert "docs/AI_SKILLS.md" in text
    assert "docs/HANDOFF.md" in text
    assert "docs/CONTEXT.md" in text
    assert "docs/architecture/INCIDENT_LEDGER.md" in text
    assert "開発規律" in text
    assert "現在地" in text
    assert "安定アーキテクチャ" in text or "索引" in text
    assert "事故裁定" in text


def test_ai_skills_intro_declares_document_roles() -> None:
    head = "\n".join(_read(AI_SKILLS).splitlines()[:40])
    assert "AI_SKILLS" in head or "本書" in head
    assert "HANDOFF" in head
    assert "CONTEXT" in head
    assert "規律" in head
    assert "現在地" in head or "worktree" in head.lower() or "引継" in head
    assert "索引" in head or "安定" in head
    assert "§0" in head or "読み込み" in head
    # Old linear chain phrasing must be gone.
    assert "迷ったら本書 → `docs/HANDOFF.md` → `docs/CONTEXT.md` の順に参照せよ。" not in head


def test_context_referenced_repo_paths_exist() -> None:
    text = _read(CONTEXT)
    missing: list[str] = []
    for rel in _repo_paths_referenced(text):
        # Allow docs-relative links already rooted.
        candidate = ROOT / rel
        if not candidate.exists():
            missing.append(rel)
    assert missing == [], f"missing paths referenced by CONTEXT.md: {missing}"


def test_context_does_not_guide_python_tests_backslash() -> None:
    text = _read(CONTEXT)
    assert "python tests\\" not in text.lower()
    assert re.search(r"python\s+tests\\", text, flags=re.I) is None
