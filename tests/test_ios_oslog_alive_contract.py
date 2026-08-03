# -*- coding: utf-8 -*-
"""Tier 3 P0-5: cold-start instrument.alive must exist after ios_oslog::install().

Without this line, "wiring dead" and "LLM not started yet" are indistinguishable.
"""
from __future__ import annotations

from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
IOS_OSLOG_RS = ROOT / "apps" / "desktop" / "src-tauri" / "src" / "ios_oslog.rs"
IOS_OSLOG_C = ROOT / "apps" / "desktop" / "src-tauri" / "native" / "ios_oslog.c"
AI_SKILLS = ROOT / "docs" / "AI_SKILLS.md"

ALIVE_LABEL = "instrument.alive"
PHASE_LABELS = (
    "phase.baseline",
    "phase.model_loaded",
    "phase.ctx_created",
    "phase.inference",
    "phase.idle",
)


def _rs() -> str:
    return IOS_OSLOG_RS.read_text(encoding="utf-8")


def test_instrument_alive_label_constant() -> None:
    text = _rs()
    assert f'ALIVE_LABEL: &str = "{ALIVE_LABEL}"' in text or (
        f'ALIVE_LABEL = "{ALIVE_LABEL}"' in text
    ), f"ALIVE_LABEL must be exactly {ALIVE_LABEL!r}"


def test_install_calls_emit_instrument_alive() -> None:
    """D-37 target: removing this call from install() must fail this scan."""
    text = _rs()
    assert "pub fn install()" in text
    # install body must invoke survival proof (not merely define the helper).
    install_idx = text.index("pub fn install()")
    # Bound the install function roughly until the next top-level fn.
    rest = text[install_idx:]
    next_fn = rest.find("\nfn ", 1)
    if next_fn < 0:
        next_fn = rest.find("\npub fn ", 1)
    body = rest[: next_fn if next_fn > 0 else len(rest)]
    assert "emit_instrument_alive()" in body, (
        "install() must call emit_instrument_alive() (P0-5 cold-start survival)"
    )


def test_alive_uses_app_category_and_u64_path() -> None:
    text = _rs()
    assert "fn emit_instrument_alive()" in text
    alive_idx = text.index("fn emit_instrument_alive()")
    rest = text[alive_idx:]
    next_fn = rest.find("\nfn ", 1)
    if next_fn < 0:
        next_fn = rest.find("\npub fn ", 1)
    body = rest[: next_fn if next_fn > 0 else len(rest)]
    assert "CAT_APP" in body, "survival line must use category app"
    assert "ALIVE_LABEL" in body
    assert "emit_u64" in body or "pkb_oslog_u64" in body


def test_c_shim_keeps_public_llu() -> None:
    c = IOS_OSLOG_C.read_text(encoding="utf-8")
    assert "%{public}llu" in c, "numeric path must stay %{public}llu (D-36 target)"


def test_ai_skills_documents_search_strings() -> None:
    text = AI_SKILLS.read_text(encoding="utf-8")
    assert ALIVE_LABEL in text, f"AI_SKILLS must document {ALIVE_LABEL!r}"
    for label in PHASE_LABELS:
        assert label in text, f"AI_SKILLS must document search string {label!r}"
    assert "メッセージ本文" in text or "message" in text.lower()
    assert "subsystem" in text.lower()
    assert "/usr/bin/log collect" in text
    assert "/usr/bin/log show" in text
    assert 'subsystem == "com.ai-shizu.pkb"' in text
    assert "log stream" in text  # warning that stream has no device option
