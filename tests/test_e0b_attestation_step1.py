# -*- coding: utf-8 -*-
"""E0b STEP 1 — Attestation v2: canonicalize_for_match + PIISnapshot (byte-exact).

Parent: docs/SPEC_E0B_STEP1_ATTESTATION.md
Golden expectations are hand-specified hex; never derived from the implementation.
"""
from __future__ import annotations

import json
import sys
from pathlib import Path

import pytest

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "src" / "python"))
GOLDEN_CANON = ROOT / "tests" / "golden" / "e0b_canonicalize_golden.jsonl"
GOLDEN_PII = ROOT / "tests" / "golden" / "e0b_pii_snapshot_golden.json"


def _load_canonicalize_golden() -> list[dict]:
    rows: list[dict] = []
    for line in GOLDEN_CANON.read_text(encoding="utf-8").splitlines():
        line = line.strip()
        if not line:
            continue
        rows.append(json.loads(line))
    return rows


# ---------------------------------------------------------------------------
# STEP 1.B — canonicalize_for_match
# ---------------------------------------------------------------------------
def test_canonicalize_golden_byte_exact() -> None:
    from core.e0b_attestation import canonicalize_for_match

    for row in _load_canonicalize_golden():
        raw = bytes.fromhex(row["input_hex"]).decode("utf-8")
        got = canonicalize_for_match(raw).encode("utf-8").hex()
        assert got == row["expected_hex"], (
            f"{row['name']}: expected {row['expected_hex']!r}, got {got!r}"
        )


def test_canonicalize_surrogate_fail_closed() -> None:
    from core.e0b_attestation import canonicalize_for_match

    with pytest.raises(ValueError):
        canonicalize_for_match("\ud800")


def test_canonicalize_idempotent() -> None:
    from core.e0b_attestation import canonicalize_for_match

    for row in _load_canonicalize_golden():
        raw = bytes.fromhex(row["input_hex"]).decode("utf-8")
        once = canonicalize_for_match(raw)
        assert canonicalize_for_match(once) == once, f"idempotent fail on input {row['name']}"
        expected = bytes.fromhex(row["expected_hex"]).decode("utf-8")
        assert canonicalize_for_match(expected) == expected, (
            f"idempotent fail on expected {row['name']}"
        )


def test_canonicalize_negative_ascii_passthrough() -> None:
    """Negative canary: already-canonical ASCII must be unchanged (no over-strip)."""
    from core.e0b_attestation import canonicalize_for_match

    assert canonicalize_for_match("john smith") == "john smith"
