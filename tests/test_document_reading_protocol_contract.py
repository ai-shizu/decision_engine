# -*- coding: utf-8 -*-
"""Finding 7 — document reading protocol must defer to AI_SKILLS §0."""
from __future__ import annotations

import re
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
DOCS = ROOT / "docs"
AI_SKILLS = DOCS / "AI_SKILLS.md"
HANDOFF = DOCS / "HANDOFF.md"
SPEC_ECHO = DOCS / "SPEC_ECHO_GENESIS.md"
SPEC_CHARLIE = DOCS / "SPEC_CHARLIE_DELTA.md"
SPEC_FOXTROT = DOCS / "SPEC_FOXTROT_UI.md"


def _read(path: Path) -> str:
    return path.read_text(encoding="utf-8")


def _ai_skills_s0() -> str:
    text = _read(AI_SKILLS)
    m = re.search(r"^##\s+0\.\s+", text, flags=re.M)
    assert m, "AI_SKILLS §0 not found"
    nxt = re.search(r"^##\s+1\.\s+", text[m.end() :], flags=re.M)
    assert nxt, "AI_SKILLS §1 not found"
    return text[m.start() : m.end() + nxt.start()]


def _handoff_s1() -> str:
    text = _read(HANDOFF)
    m = re.search(r"^##\s+1\.\s+", text, flags=re.M)
    assert m, "HANDOFF §1 not found"
    nxt = re.search(r"^##\s+2\.\s+", text[m.end() :], flags=re.M)
    assert nxt, "HANDOFF §2 not found"
    return text[m.start() : m.end() + nxt.start()]


def _spec_reader_preamble(path: Path) -> str | None:
    """Contiguous markdown blockquote for the reader preamble, or None if absent."""
    text = _read(path)
    # Stop before first real top-level section heading (# §0 / # §1).
    # Use (?!\d) so revision comments mentioning §10/§11 do not truncate early.
    body_m = re.search(r"^#\s+§(?:0|1)(?!\d)", text, flags=re.M)
    head = text[: body_m.start()] if body_m else text
    blocks: list[str] = []
    current: list[str] = []
    for line in head.splitlines():
        if line.startswith(">"):
            current.append(line)
        else:
            if current:
                blocks.append("\n".join(current))
                current = []
    if current:
        blocks.append("\n".join(current))
    for block in blocks:
        if "読者への前提命令" in block:
            return block
    return None


def _require_spec_reader_preamble(path: Path) -> str:
    preamble = _spec_reader_preamble(path)
    assert preamble is not None, f"reader preamble missing in {path.name}"
    return preamble


def _full_read_ai_skills_command(text: str) -> bool:
    """True if text issues an unconditional full-read of AI_SKILLS."""
    patterns = (
        r"AI_SKILLS\.md[`\s]*を全文読",
        r"docs/AI_SKILLS\.md[`\s]*を全文読",
        r"`docs/AI_SKILLS\.md`を全文読",
        r"AI_SKILLS を全文読",
        r"AI_SKILLS\.md`?\s*を\s*全文読了",
    )
    return any(re.search(p, text) for p in patterns)


def test_ai_skills_s0_requires_section_1_for_all_tasks() -> None:
    s0 = _ai_skills_s0()
    assert "§1" in s0 and "全タスク" in s0 and "必読" in s0


def test_ai_skills_s0_has_task_section_table() -> None:
    s0 = _ai_skills_s0()
    assert "| タスク種別 | 必読節 |" in s0
    assert "Target Echo" in s0
    assert "横断的変更" in s0


def test_ai_skills_s0_has_cross_cutting_selection_rule() -> None:
    s0 = _ai_skills_s0()
    assert "横断的変更" in s0
    assert "関係しそうな節" in s0 or "関係しそうな" in s0


def test_ai_skills_s0_forbids_read_everything_by_default() -> None:
    s0 = _ai_skills_s0()
    assert "とりあえず全文" in s0 and "禁止" in s0


def test_handoff_s1_defers_to_ai_skills_s0_first() -> None:
    s1 = _handoff_s1()
    # Procedure 2 must point at AI_SKILLS §0 before deeper reading.
    assert re.search(r"AI_SKILLS\.md[`\s]*§0|AI_SKILLS\.md.*§0|§0.*読み込みプロトコル", s1)
    assert "§0" in s1


def test_handoff_s1_requires_s1_plus_task_sections() -> None:
    s1 = _handoff_s1()
    assert "§1" in s1
    assert re.search(r"タスク種別|対応する節|表から", s1)


def test_handoff_s1_has_no_full_read_ai_skills_command() -> None:
    s1 = _handoff_s1()
    assert not _full_read_ai_skills_command(s1)
    assert "AI_SKILLS.md`を全文読む" not in s1
    assert "AI_SKILLS.mdを全文読む" not in s1


def test_spec_reader_preambles_have_no_full_read_ai_skills_command() -> None:
    checked = 0
    for path in sorted(DOCS.glob("SPEC_*.md")):
        preamble = _spec_reader_preamble(path)
        if preamble is None:
            continue
        checked += 1
        assert not _full_read_ai_skills_command(preamble), path.name
    assert checked >= 3  # Echo, Charlie/Delta, Foxtrot at minimum


def test_echo_spec_routes_via_s0_then_s1_s12_and_spec() -> None:
    preamble = _require_spec_reader_preamble(SPEC_ECHO)
    assert "§0" in preamble
    assert "ルーティング" in preamble or "表" in preamble
    assert "§1" in preamble
    assert "§12" in preamble
    assert re.search(r"本SPEC|本書", preamble)
    assert not _full_read_ai_skills_command(preamble)


def test_charlie_delta_spec_routes_via_s0_then_selective_s9_s11() -> None:
    preamble = _require_spec_reader_preamble(SPEC_CHARLIE)
    assert "§0" in preamble
    assert "ルーティング" in preamble or "表" in preamble
    assert "§1" in preamble
    assert re.search(r"§9|§10|§11", preamble)
    assert "横断" in preamble or "複数" in preamble
    assert not _full_read_ai_skills_command(preamble)


def test_foxtrot_spec_keeps_existing_routing_order() -> None:
    preamble = _require_spec_reader_preamble(SPEC_FOXTROT)
    assert "§0" in preamble
    assert "ルーティング" in preamble or "表" in preamble
    assert "§1" in preamble
    assert not _full_read_ai_skills_command(preamble)


def test_handoff_and_specs_do_not_duplicate_task_table() -> None:
    forbidden_header = "| タスク種別 | 必読節 |"
    assert forbidden_header not in _handoff_s1()
    for path in (SPEC_ECHO, SPEC_CHARLIE, SPEC_FOXTROT):
        assert forbidden_header not in _require_spec_reader_preamble(path)
