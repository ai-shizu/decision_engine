# -*- coding: utf-8 -*-
"""Finding 6 — Target Echo document status must match E0–E4 as-built / E5 gated."""
from __future__ import annotations

import ast
import re
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
AI_SKILLS = ROOT / "docs" / "AI_SKILLS.md"
SPEC = ROOT / "docs" / "SPEC_ECHO_GENESIS.md"
CORE = ROOT / "src" / "python" / "core"


def _read(path: Path) -> str:
    return path.read_text(encoding="utf-8")


def _ai_skills_s12() -> str:
    text = _read(AI_SKILLS)
    m = re.search(r"^##\s+12\.\s+", text, flags=re.M)
    assert m, "AI_SKILLS §12 not found"
    start = m.start()
    nxt = re.search(r"^##\s+13\.\s+", text[m.end() :], flags=re.M)
    assert nxt, "AI_SKILLS §13 not found"
    return text[start : m.end() + nxt.start()]


def _ai_skills_s12_status_surface() -> str:
    """§12 from heading until first E0/E1 as-built heading (exclusive)."""
    s12 = _ai_skills_s12()
    m = re.search(r"^###\s+E0/E1\s+完遂", s12, flags=re.M)
    assert m, "E0/E1 as-built heading missing"
    return s12[: m.start()]


def _spec_status_surface() -> str:
    text = _read(SPEC)
    m = re.search(r"^#\s+§1", text, flags=re.M)
    assert m, "SPEC §1 not found"
    return text[: m.start()]


def _module_defines(path: Path, names: set[str]) -> set[str]:
    tree = ast.parse(_read(path), filename=str(path))
    found: set[str] = set()
    for node in tree.body:
        if isinstance(node, (ast.FunctionDef, ast.AsyncFunctionDef, ast.ClassDef)):
            if node.name in names:
                found.add(node.name)
    return found


def test_ai_skills_s12_heading_distinguishes_e0_e4_done_and_e5_blocked() -> None:
    first = _ai_skills_s12().splitlines()[0]
    assert "E0" in first and "E4" in first
    assert "完遂" in first or "完了" in first
    assert "E5" in first
    assert "未着手" in first
    assert "実装は全て未着手" not in first


def test_ai_skills_s12_status_surface_has_no_all_unimplemented_claim() -> None:
    surface = _ai_skills_s12_status_surface()
    assert "実装は全て未着手" not in surface
    assert "全て未実装" not in surface


def test_ai_skills_s12_has_e0_to_e4_asbuilt_headings() -> None:
    s12 = _ai_skills_s12()
    assert re.search(r"^###\s+E0/E1\s+完遂", s12, flags=re.M)
    assert re.search(r"^###\s+.*E2\s+完遂", s12, flags=re.M)
    assert re.search(r"^###\s+E3\s+完遂", s12, flags=re.M)
    assert re.search(r"^###\s+E4\s+完遂", s12, flags=re.M)


def test_ai_skills_s12_e5_blocked_by_3000ms_gate_not_oversight() -> None:
    surface = _ai_skills_s12_status_surface()
    assert "3000ms" in surface or "3000 ms" in surface
    assert "E5" in surface
    assert re.search(r"着手禁止|計測ゲート", surface)
    assert "実装漏れ" not in surface


def test_spec_status_surface_has_no_all_phases_unimplemented() -> None:
    surface = _spec_status_surface()
    assert "全て未実装" not in surface
    assert "E0〜E5" not in surface or "未実装" not in surface.split("E0〜E5", 1)[-1][:40]
    # Stronger: the exact stale declaration must be gone.
    assert "全て未実装" not in surface
    assert not re.search(r"E0.?E5.*全て未実装|全て未実装.*E0.?E5", surface)


def test_spec_and_ai_skills_status_agree_e0_e4_done_e5_blocked() -> None:
    ai = _ai_skills_s12_status_surface()
    spec = _spec_status_surface()
    for doc in (ai, spec):
        assert re.search(r"E0.*完遂|E0.*完了", doc) or "E0" in doc and "完遂" in doc
        assert "E4" in doc and ("完遂" in doc or "実装・検証済み" in doc or "実装済" in doc)
        assert "E5" in doc and ("未着手" in doc or "未実装" in doc)
        assert "着手禁止" in doc or "計測ゲート" in doc


def test_echo_implementation_anchors_exist_via_ast() -> None:
    checks = {
        CORE / "tensor_store.py": {"TensorStore", "build_tensor"},
        CORE / "coupling.py": {"coupling_matrix"},
        CORE / "digital_twin.py": {"TwinParams", "fit_twin", "simulate"},
        CORE / "oracle.py": {"_assert_sterile", "build_oracle_payload"},
    }
    for path, names in checks.items():
        assert path.is_file(), path
        found = _module_defines(path, names)
        missing = names - found
        assert not missing, f"{path.name} missing {missing}"


def test_echo_phase_test_files_exist() -> None:
    for name in (
        "test_tensor_store.py",
        "test_coupling.py",
        "test_digital_twin.py",
        "test_oracle.py",
    ):
        assert (ROOT / "tests" / name).is_file(), name


def test_status_surfaces_do_not_claim_full_feature_completion() -> None:
    combined = _ai_skills_s12_status_surface() + "\n" + _spec_status_surface()
    assert "全機能完成" not in combined
    assert "全サブ機能が完成" not in combined
    # Declared partial constraints must still be acknowledged somewhere in §12
    # (not necessarily only in status surface — as-built may hold them).
    s12 = _ai_skills_s12()
    assert "dyad" in s12.lower() or "unratable" in s12.lower()
    assert "mask=0" in s12 or "mask = 0" in s12
    assert "E5" in s12 and ("未着手" in s12 or "着手禁止" in s12)
