# -*- coding: utf-8 -*-
"""E0b STEP 2 — Intent Builder v2 + HMAC attestation (fail-closed / KAT / preflight).

Parent: docs/SPEC_E0B_STEP2_ATTESTATION.md
"""
from __future__ import annotations

import json
import sys
from pathlib import Path
from unittest.mock import patch

import pytest

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "src" / "python"))

GOLDEN_KAT = ROOT / "tests" / "golden" / "e0b_attestation_kat.json"

# Shared valid fixtures for fail-closed negative tests.
_VALID_K = "000102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f"
_VALID_SID = "0123456789abcdeffedcba9876543210"
_VALID_NONCE = "00112233445566778899aabbccddeeffffeeddccbbaa99887766554433221100"
_VALID_DH = "f3b8fd0c8070d2127fd0b3daaacd12c25ab5e819cd3c7d17d11cd9fa632d34e0"


def _passthrough_sanitizer():
    from core.privacy_search import EgressSanitizer

    return EgressSanitizer([])


def _build_kwargs(**overrides):
    base = {
        "abstract_queries": ["safe query"],
        "k_spawn": _VALID_K,
        "session_id": _VALID_SID,
        "txn_nonce": _VALID_NONCE,
        "sidecar_generation": 1,
        "policy_epoch": 1,
        "dict_hash": _VALID_DH,
        "sanitizer": _passthrough_sanitizer(),
    }
    base.update(overrides)
    return base


# ---------------------------------------------------------------------------
# STEP 2.A — Fail-closed context validation
# ---------------------------------------------------------------------------
def test_failclosed_k_spawn_rejects() -> None:
    from core.e0b_intent import attest_tag, build_attested_intent

    framing = b"x"
    bad_keys = [
        None,
        "",
        _VALID_K[:63],
        _VALID_K + "0",
        "A" * 64,
        "z" * 64,
        _VALID_K[:62],  # 31 bytes if decoded
    ]
    for bad in bad_keys:
        with pytest.raises((ValueError, TypeError)):
            attest_tag(bad, framing)  # type: ignore[arg-type]
        with pytest.raises((ValueError, TypeError)):
            build_attested_intent(**_build_kwargs(k_spawn=bad))  # type: ignore[arg-type]


def test_failclosed_session_nonce_dict_hash() -> None:
    from core.e0b_intent import build_attested_intent

    for bad in (_VALID_SID[:31], _VALID_SID + "0", "A" * 32, "z" * 32):
        with pytest.raises((ValueError, TypeError)):
            build_attested_intent(**_build_kwargs(session_id=bad))
    for bad in (_VALID_NONCE[:63], _VALID_NONCE + "0", "A" * 64, "z" * 64):
        with pytest.raises((ValueError, TypeError)):
            build_attested_intent(**_build_kwargs(txn_nonce=bad))
    for bad in (_VALID_DH[:63], _VALID_DH + "0", "A" * 64, "z" * 64):
        with pytest.raises((ValueError, TypeError)):
            build_attested_intent(**_build_kwargs(dict_hash=bad))


def test_failclosed_gen_epoch() -> None:
    from core.e0b_intent import build_attested_intent

    for bad in (-1, 2**64, True, 1.5, "1"):
        with pytest.raises((ValueError, TypeError)):
            build_attested_intent(**_build_kwargs(sidecar_generation=bad))  # type: ignore[arg-type]
        with pytest.raises((ValueError, TypeError)):
            build_attested_intent(**_build_kwargs(policy_epoch=bad))  # type: ignore[arg-type]


def test_failclosed_abstract_queries() -> None:
    from core.e0b_intent import build_attested_intent, canonicalize_outbound_query

    with pytest.raises((ValueError, TypeError)):
        build_attested_intent(**_build_kwargs(abstract_queries=[]))
    with pytest.raises((ValueError, TypeError)):
        build_attested_intent(
            **_build_kwargs(abstract_queries=["a", "b", "c", "d", "e"])
        )
    with pytest.raises((ValueError, TypeError)):
        build_attested_intent(**_build_kwargs(abstract_queries="not-a-list"))  # type: ignore[arg-type]
    with pytest.raises((ValueError, TypeError)):
        build_attested_intent(**_build_kwargs(abstract_queries=[1]))  # type: ignore[list-item]
    with pytest.raises(ValueError):
        canonicalize_outbound_query("\u200b")
    with pytest.raises(ValueError):
        build_attested_intent(**_build_kwargs(abstract_queries=["\u200b"]))
    with pytest.raises(ValueError):
        build_attested_intent(**_build_kwargs(abstract_queries=["a" * 4097]))


def test_failclosed_load_spawn_key_from_env() -> None:
    from core.e0b_intent import load_spawn_key_from_env

    with pytest.raises(ValueError):
        load_spawn_key_from_env(env={})
    with pytest.raises(ValueError):
        load_spawn_key_from_env(env={"PKB_EGRESS_KEY": "not-hex"})
    assert load_spawn_key_from_env(env={"PKB_EGRESS_KEY": _VALID_K}) == _VALID_K


# ---------------------------------------------------------------------------
# STEP 2.B — Cryptographic framing + HMAC KAT
# ---------------------------------------------------------------------------
def _load_kat() -> list[dict]:
    with open(GOLDEN_KAT, encoding="utf-8") as fh:
        data = json.load(fh)
    return list(data["vectors"])


def test_framing_matches_golden() -> None:
    from core.e0b_intent import attestation_framing

    for row in _load_kat():
        got = attestation_framing(
            row["session_id"],
            row["txn_nonce"],
            row["sidecar_generation"],
            row["policy_epoch"],
            row["dict_hash"],
            row["queries"],
        )
        assert got.hex() == row["framing_hex"], (
            f"{row['name']}: framing mismatch\n"
            f"  expected {row['framing_hex']}\n"
            f"  got      {got.hex()}"
        )


def test_tag_matches_golden() -> None:
    from core.e0b_intent import attest_tag

    for row in _load_kat():
        got = attest_tag(row["k_spawn"], bytes.fromhex(row["framing_hex"]))
        assert got == row["tag_hex"], f"{row['name']}: tag mismatch {got}"


def test_tag_end_to_end() -> None:
    from core.e0b_intent import attestation_framing, attest_tag

    for row in _load_kat():
        framing = attestation_framing(
            row["session_id"],
            row["txn_nonce"],
            row["sidecar_generation"],
            row["policy_epoch"],
            row["dict_hash"],
            row["queries"],
        )
        assert attest_tag(row["k_spawn"], framing) == row["tag_hex"]


def test_query_byte_length_not_scalar() -> None:
    """CJK/emoji length prefixes must be UTF-8 byte lengths (15 / 9), not scalars."""
    rows = {r["name"]: r for r in _load_kat()}
    assert "000000000000000f" in rows["KAT-1"]["framing_hex"]  # 15 bytes for 日本の就職
    assert "0000000000000009" in rows["KAT-3"]["framing_hex"]  # 9 bytes for 😀 test
    # Scalar traps would be 5 / 6:
    assert "0000000000000005e697a5" not in rows["KAT-1"]["framing_hex"]
    assert "0000000000000006f09f9880" not in rows["KAT-3"]["framing_hex"]


def test_tag_sensitivity_one_bit() -> None:
    from core.e0b_intent import attestation_framing, attest_tag

    kat1 = next(r for r in _load_kat() if r["name"] == "KAT-1")
    good = kat1["tag_hex"]
    framing = bytes.fromhex(kat1["framing_hex"])

    tweaked_key = kat1["k_spawn"][:-1] + ("e" if kat1["k_spawn"][-1] != "e" else "f")
    assert attest_tag(tweaked_key, framing) != good

    framing_gen8 = attestation_framing(
        kat1["session_id"],
        kat1["txn_nonce"],
        8,
        kat1["policy_epoch"],
        kat1["dict_hash"],
        kat1["queries"],
    )
    assert attest_tag(kat1["k_spawn"], framing_gen8) != good

    tweaked_q = list(kat1["queries"])
    tweaked_q[0] = tweaked_q[0] + "x"
    framing_q = attestation_framing(
        kat1["session_id"],
        kat1["txn_nonce"],
        kat1["sidecar_generation"],
        kat1["policy_epoch"],
        kat1["dict_hash"],
        tweaked_q,
    )
    assert attest_tag(kat1["k_spawn"], framing_q) != good


# ---------------------------------------------------------------------------
# STEP 2.C — All-or-Nothing sanitizer preflight
# ---------------------------------------------------------------------------
def test_preflight_allornothing_middle_violation_no_hmac() -> None:
    from core.privacy_search import EgressSanitizer, EgressViolationError

    sanitizer = EgressSanitizer(["himitsu"])
    with patch("core.e0b_intent.attest_tag") as mock_attest:
        with pytest.raises(EgressViolationError):
            from core.e0b_intent import build_attested_intent

            build_attested_intent(
                ["ai career", "himitsu project", "python async"],
                k_spawn=_VALID_K,
                session_id=_VALID_SID,
                txn_nonce=_VALID_NONCE,
                sidecar_generation=1,
                policy_epoch=1,
                dict_hash=_VALID_DH,
                sanitizer=sanitizer,
            )
        assert mock_attest.call_count == 0


def test_preflight_allornothing_trailing_violation_no_hmac() -> None:
    from core.privacy_search import EgressSanitizer, EgressViolationError

    sanitizer = EgressSanitizer(["himitsu"])
    with patch("core.e0b_intent.attest_tag") as mock_attest:
        with pytest.raises(EgressViolationError):
            from core.e0b_intent import build_attested_intent

            build_attested_intent(
                ["ai career", "python async", "himitsu project"],
                k_spawn=_VALID_K,
                session_id=_VALID_SID,
                txn_nonce=_VALID_NONCE,
                sidecar_generation=1,
                policy_epoch=1,
                dict_hash=_VALID_DH,
                sanitizer=sanitizer,
            )
        assert mock_attest.call_count == 0


def test_preflight_safe_queries_hmac_once() -> None:
    from core.e0b_intent import build_attested_intent
    from core.privacy_search import EgressSanitizer

    sanitizer = EgressSanitizer(["himitsu"])
    with patch("core.e0b_intent.attest_tag", return_value="ab" * 32) as mock_attest:
        result = build_attested_intent(
            ["ai career", "python async"],
            k_spawn=_VALID_K,
            session_id=_VALID_SID,
            txn_nonce=_VALID_NONCE,
            sidecar_generation=1,
            policy_epoch=1,
            dict_hash=_VALID_DH,
            sanitizer=sanitizer,
        )
        assert mock_attest.call_count == 1
        assert result["attestation"] == "ab" * 32

    # Real path: 64 lowercase hex attestation
    real = build_attested_intent(
        ["ai career", "python async"],
        k_spawn=_VALID_K,
        session_id=_VALID_SID,
        txn_nonce=_VALID_NONCE,
        sidecar_generation=1,
        policy_epoch=1,
        dict_hash=_VALID_DH,
        sanitizer=sanitizer,
    )
    assert set(real.keys()) == {"abstract_queries", "attestation"}
    assert len(real["attestation"]) == 64
    assert all(c in "0123456789abcdef" for c in real["attestation"])


def test_preflight_invisible_pii_rejected_after_canonicalize() -> None:
    from core.privacy_search import EgressSanitizer, EgressViolationError

    sanitizer = EgressSanitizer(["himitsu"])
    with patch("core.e0b_intent.attest_tag") as mock_attest:
        with pytest.raises(EgressViolationError):
            from core.e0b_intent import build_attested_intent

            # ZWSP between "himi" and "tsu" — stripped by canonicalize_outbound
            build_attested_intent(
                ["himi\u200btsu"],
                k_spawn=_VALID_K,
                session_id=_VALID_SID,
                txn_nonce=_VALID_NONCE,
                sidecar_generation=1,
                policy_epoch=1,
                dict_hash=_VALID_DH,
                sanitizer=sanitizer,
            )
        assert mock_attest.call_count == 0


def test_integration_build_matches_kat1() -> None:
    from core.e0b_intent import build_attested_intent
    from core.privacy_search import EgressSanitizer

    kat1 = next(r for r in _load_kat() if r["name"] == "KAT-1")
    result = build_attested_intent(
        list(kat1["queries"]),
        k_spawn=kat1["k_spawn"],
        session_id=kat1["session_id"],
        txn_nonce=kat1["txn_nonce"],
        sidecar_generation=kat1["sidecar_generation"],
        policy_epoch=kat1["policy_epoch"],
        dict_hash=kat1["dict_hash"],
        sanitizer=EgressSanitizer([]),
    )
    assert result["abstract_queries"] == kat1["queries"]
    assert result["attestation"] == kat1["tag_hex"]
