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
