# -*- coding: utf-8 -*-
"""E0b Intent Builder v2 — framing + HMAC-SHA256 attestation.

Byte-exact contract for Rust Dual-Run (STEP 3).
Parent: docs/SPEC_E0B_STEP2_ATTESTATION.md

Scope E cage: no subprocess/ctypes/network/dynamic exec.
"""
from __future__ import annotations

import hashlib
import hmac
import os
import unicodedata
from collections.abc import Mapping, Sequence
from typing import Any

from core.privacy_search import EgressSanitizer

MAX_FETCH_QUERIES = 4
MAX_QUERY_BYTES = 4096

_HEX_LOWER = frozenset("0123456789abcdef")
_U64_MAX = 1 << 64

# Outbound canonicalize: format/invisible only (C0/C1 remaining → ValueError).
_OUTBOUND_REMOVE: frozenset[int] = frozenset(
    {
        0x00AD,
        0x061C,
        0x180E,
        0x200B,
        0x200C,
        0x200D,
        0x200E,
        0x200F,
        0x202A,
        0x202B,
        0x202C,
        0x202D,
        0x202E,
        0x2060,
        0x2061,
        0x2062,
        0x2063,
        0x2064,
        0x2066,
        0x2067,
        0x2068,
        0x2069,
        0xFEFF,
    }
)
_C0_C1: frozenset[int] = frozenset({*range(0x00, 0x20), *range(0x7F, 0xA0)})
_SURROGATE_LO = 0xD800
_SURROGATE_HI = 0xDFFF


def _require_lowercase_hex(value: object, *, field: str, length: int) -> str:
    if type(value) is not str:
        raise TypeError(f"{field} must be str")
    if len(value) != length:
        raise ValueError(f"{field} must be exactly {length} lowercase hex chars")
    if not all(c in _HEX_LOWER for c in value):
        raise ValueError(f"{field} must be lowercase hex")
    return value


def _require_u64(value: object, *, field: str) -> int:
    if type(value) is not int or isinstance(value, bool):
        raise TypeError(f"{field} must be int")
    if value < 0 or value >= _U64_MAX:
        raise ValueError(f"{field} must be in [0, 2**64)")
    return value


def canonicalize_outbound_query(q: str) -> str:
    """Outbound query shaping for sign+send (NFC; no casefold)."""
    if type(q) is not str:
        raise TypeError("query must be str")
    for ch in q:
        cp = ord(ch)
        if _SURROGATE_LO <= cp <= _SURROGATE_HI:
            raise ValueError("lone surrogate rejected")

    stripped = "".join(ch for ch in q if ord(ch) not in _OUTBOUND_REMOVE)
    for ch in stripped:
        if ord(ch) in _C0_C1:
            raise ValueError("control characters rejected in outbound query")

    normalized = unicodedata.normalize("NFC", stripped).strip()
    if not normalized:
        raise ValueError("outbound query must not be empty after canonicalize")
    if len(normalized.encode("utf-8")) > MAX_QUERY_BYTES:
        raise ValueError("outbound query exceeds MAX_QUERY_BYTES")
    return normalized


def load_spawn_key_from_env(env: Mapping[str, str] | None = None) -> str:
    """Read PKB_EGRESS_KEY; absent/invalid → ValueError (fail-closed)."""
    source: Mapping[str, str] = os.environ if env is None else env
    raw = source.get("PKB_EGRESS_KEY")
    if raw is None:
        raise ValueError("PKB_EGRESS_KEY is required")
    return _require_lowercase_hex(raw, field="PKB_EGRESS_KEY", length=64)


def attestation_framing(
    session_id: str,
    txn_nonce: str,
    sidecar_generation: int,
    policy_epoch: int,
    dict_hash: str,
    queries: Sequence[str],
) -> bytes:
    """Length-prefixed injective framing (§1.4). Crypto-free pure function."""
    sid = _require_lowercase_hex(session_id, field="session_id", length=32)
    nonce = _require_lowercase_hex(txn_nonce, field="txn_nonce", length=64)
    gen = _require_u64(sidecar_generation, field="sidecar_generation")
    epoch = _require_u64(policy_epoch, field="policy_epoch")
    dhash = _require_lowercase_hex(dict_hash, field="dict_hash", length=64)
    if type(queries) is not list and not isinstance(queries, tuple):
        raise TypeError("queries must be a sequence of str")

    out = bytearray()
    out.extend(sid.encode("utf-8"))
    out.extend(nonce.encode("utf-8"))
    out.extend(gen.to_bytes(8, "big"))
    out.extend(epoch.to_bytes(8, "big"))
    out.extend(dhash.encode("utf-8"))
    for q in queries:
        if type(q) is not str:
            raise TypeError("each query must be str")
        raw = q.encode("utf-8")
        out.extend(len(raw).to_bytes(8, "big"))
        out.extend(raw)
    return bytes(out)


def attest_tag(k_spawn_hex: str, framing: bytes) -> str:
    """HMAC-SHA256(key=bytes.fromhex(k_spawn), msg=framing) → 64 lowercase hex."""
    key_hex = _require_lowercase_hex(k_spawn_hex, field="k_spawn", length=64)
    if type(framing) is not bytes:
        raise TypeError("framing must be bytes")
    key = bytes.fromhex(key_hex)
    return hmac.new(key, framing, hashlib.sha256).hexdigest()


def build_attested_intent(
    abstract_queries: Any,
    *,
    k_spawn: str,
    session_id: str,
    txn_nonce: str,
    sidecar_generation: int,
    policy_epoch: int,
    dict_hash: str,
    sanitizer: EgressSanitizer,
) -> dict[str, Any]:
    """Validate → canonicalize → All-or-Nothing preflight → frame → HMAC."""
    if not isinstance(sanitizer, EgressSanitizer):
        raise TypeError("sanitizer must be an EgressSanitizer")

    key = _require_lowercase_hex(k_spawn, field="k_spawn", length=64)
    sid = _require_lowercase_hex(session_id, field="session_id", length=32)
    nonce = _require_lowercase_hex(txn_nonce, field="txn_nonce", length=64)
    gen = _require_u64(sidecar_generation, field="sidecar_generation")
    epoch = _require_u64(policy_epoch, field="policy_epoch")
    dhash = _require_lowercase_hex(dict_hash, field="dict_hash", length=64)

    if type(abstract_queries) is not list:
        raise TypeError("abstract_queries must be a list")
    if not abstract_queries:
        raise ValueError("abstract_queries must not be empty")
    if len(abstract_queries) > MAX_FETCH_QUERIES:
        raise ValueError("abstract_queries exceeds MAX_FETCH_QUERIES")

    q_star = [canonicalize_outbound_query(q) for q in abstract_queries]

    # All-or-Nothing: no framing/HMAC until every query clears the sanitizer.
    for q in q_star:
        sanitizer.validate_query(q)

    framing = attestation_framing(sid, nonce, gen, epoch, dhash, q_star)
    tag = attest_tag(key, framing)
    return {"abstract_queries": list(q_star), "attestation": tag}
