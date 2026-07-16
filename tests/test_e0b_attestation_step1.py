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


# ---------------------------------------------------------------------------
# STEP 1.C — PIISnapshot
# ---------------------------------------------------------------------------
def _load_pii_golden() -> dict[str, dict]:
    data = json.loads(GOLDEN_PII.read_text(encoding="utf-8"))
    return {row["name"]: row for row in data["vectors"]}


def test_snapshot_preimage_and_hash_golden() -> None:
    """Hand-specified preimage hex + SHA-256; never derived from implementation."""
    import hashlib

    from core.e0b_attestation import PIISnapshot, preimage

    golden = _load_pii_golden()
    for name, row in golden.items():
        terms = tuple(row["terms"])
        rev = row["revision"]
        got_pre = preimage(terms, rev)
        assert got_pre.hex() == row["preimage_hex"], (
            f"{name}: preimage mismatch\n"
            f"  expected {row['preimage_hex']}\n"
            f"  got      {got_pre.hex()}"
        )
        expected_hash = hashlib.sha256(bytes.fromhex(row["preimage_hex"])).hexdigest()
        assert expected_hash == row["hash_hex"], (
            f"{name}: golden hash_hex must equal sha256(preimage_hex)"
        )
        snap = PIISnapshot.build(list(terms), rev)
        assert snap.hash_hex == row["hash_hex"], f"{name}: snapshot.hash_hex mismatch"
        assert snap.terms == terms
        assert snap.revision == rev


def test_snapshot_injective_length_prefix() -> None:
    """ab|c vs a|bc must not collide (length-prefix injectivity)."""
    golden = _load_pii_golden()
    assert golden["ab_c"]["preimage_hex"] != golden["a_bc"]["preimage_hex"]
    assert golden["ab_c"]["hash_hex"] != golden["a_bc"]["hash_hex"]


def test_snapshot_byte_length_not_scalar() -> None:
    """CJK/emoji framing must use UTF-8 byte length (3/4), not scalar count (1/1)."""
    golden = _load_pii_golden()
    # len_be(3) || e697a5  and  len_be(4) || f09f9880 appear in hand golden
    assert "0000000000000003e697a5" in golden["cjk"]["preimage_hex"]
    assert "0000000000000004f09f9880" in golden["emoji"]["preimage_hex"]
    # Scalar-length trap would encode as len=1:
    assert "0000000000000001e697a5" not in golden["cjk"]["preimage_hex"]
    assert "0000000000000001f09f9880" not in golden["emoji"]["preimage_hex"]


def test_snapshot_order_independent() -> None:
    from core.e0b_attestation import PIISnapshot

    a = PIISnapshot.build(["c", "ab"], 1)
    b = PIISnapshot.build(["ab", "c"], 1)
    assert a.hash_hex == b.hash_hex
    assert a.terms == ("ab", "c")
    assert b.terms == ("ab", "c")


def test_snapshot_canonical_dedup_integration() -> None:
    from core.e0b_attestation import PIISnapshot

    # ZWSP between Atsuki and space-Ishizu (same as zwsp_evasion golden input family)
    with_zwsp = PIISnapshot.build(["Atsuki\u200b Ishizu"], 1)
    variants = PIISnapshot.build(["ATSUKI ISHIZU", "atsuki  ishizu"], 1)
    assert with_zwsp.hash_hex == variants.hash_hex
    assert with_zwsp.terms == ("atsuki ishizu",)
    assert variants.terms == ("atsuki ishizu",)


def test_snapshot_revision_affects_hash() -> None:
    from core.e0b_attestation import PIISnapshot

    golden = _load_pii_golden()
    rev1 = PIISnapshot.build(["a"], 1)
    rev2 = PIISnapshot.build(["a"], 2)
    assert rev2.hash_hex == golden["rev2"]["hash_hex"]
    assert rev1.hash_hex != rev2.hash_hex


def test_snapshot_empty_is_valid() -> None:
    from core.e0b_attestation import PIISnapshot

    golden = _load_pii_golden()
    snap = PIISnapshot.build([], 1)
    assert snap.hash_hex == golden["empty"]["hash_hex"]
    assert snap.terms == ()


def test_snapshot_rejects_empty_canonical_term() -> None:
    from core.e0b_attestation import PIISnapshot

    with pytest.raises(ValueError):
        PIISnapshot.build(["\u200b"], 1)


def test_snapshot_rejects_non_monotonic_revision() -> None:
    from core.e0b_attestation import PIISnapshot

    prev = PIISnapshot.build(["a"], 5)
    with pytest.raises(ValueError):
        PIISnapshot.build(["a"], 3, previous=prev)
    with pytest.raises(ValueError):
        PIISnapshot.build(["a"], 5, previous=prev)


def test_snapshot_rejects_negative_revision() -> None:
    from core.e0b_attestation import PIISnapshot

    with pytest.raises(ValueError):
        PIISnapshot.build(["a"], -1)


def test_snapshot_rejects_oversized_term() -> None:
    from core.e0b_attestation import PIISnapshot

    huge = "a" * 4097
    with pytest.raises(ValueError):
        PIISnapshot.build([huge], 1)


def test_snapshot_immutability() -> None:
    from dataclasses import FrozenInstanceError

    from core.e0b_attestation import PIISnapshot

    snap = PIISnapshot.build(["ab", "c"], 1)
    assert isinstance(snap.terms, tuple)
    with pytest.raises(FrozenInstanceError):
        snap.revision = 9  # type: ignore[misc]
    with pytest.raises(FrozenInstanceError):
        snap.terms = ("x",)  # type: ignore[misc]
    with pytest.raises(FrozenInstanceError):
        snap.hash_hex = "0" * 64  # type: ignore[misc]
