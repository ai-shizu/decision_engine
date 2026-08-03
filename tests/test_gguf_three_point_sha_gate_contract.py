# -*- coding: utf-8 -*-
"""Tier 3 P0-2: three-point GGUF SHA gate contract.

Frozen expected_size / expected_sha must not drift. The gate script must name
all three points (source / stage / archive) and must not use mtime.
"""
from __future__ import annotations

import re
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
SCRIPT = ROOT / "scripts" / "gguf_three_point_sha_gate.sh"

# Auditor-frozen (TIER3 draft §9 / Tier 2 SHA). DO NOT CHANGE without commander ruling.
EXPECTED_SIZE = "1117320736"
EXPECTED_SHA = "6a1a2eb6d15622bf3c96857206351ba97e1af16c30d7a74ee38970e434e9407e"


def _read() -> str:
    return SCRIPT.read_text(encoding="utf-8")


def test_gguf_three_point_sha_gate_script_exists() -> None:
    assert SCRIPT.is_file(), "scripts/gguf_three_point_sha_gate.sh must exist (P0-2)"


def test_gguf_three_point_sha_gate_checks_all_three_points() -> None:
    text = _read()
    code = re.sub(r"#.*?$", "", text, flags=re.M)
    for needle in (
        "apps/desktop/models/pocket-brain.gguf",
        "gen/apple/assets/models/pocket-brain.gguf",
        "pkb-desktop_iOS.xcarchive/Products/Applications/Coraxis.app/assets/models/pocket-brain.gguf",
    ):
        assert needle in text, f"gate must name path fragment {needle!r}"
    assert "SOURCE" in code and "STAGE" in code and "ARCHIVE" in code
    assert "STAGE: ABSENT" in text or "STAGE_ABSENT" in text or "ABSENT" in text
    assert "ARCHIVE: ABSENT" in text or "ARCHIVE_ABSENT" in text
    # mtime must not be the freshness criterion.
    assert "mtime" not in code.lower(), "mtime must not gate GGUF freshness"


def test_gguf_three_point_sha_gate_frozen_expected_values() -> None:
    text = _read()
    m_size = re.search(r"^EXPECTED_SIZE=(\d+)\s*$", text, re.M)
    m_sha = re.search(r"^EXPECTED_SHA=([0-9a-f]{64})\s*$", text, re.M)
    assert m_size is not None, "EXPECTED_SIZE assignment missing"
    assert m_sha is not None, "EXPECTED_SHA assignment missing"
    assert m_size.group(1) == EXPECTED_SIZE, (
        f"EXPECTED_SIZE drifted: got {m_size.group(1)} want {EXPECTED_SIZE}"
    )
    assert m_sha.group(1) == EXPECTED_SHA, (
        f"EXPECTED_SHA drifted: got {m_sha.group(1)} want {EXPECTED_SHA}"
    )
