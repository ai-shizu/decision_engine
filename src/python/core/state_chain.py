#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""Process-local keyed authentication primitives for persisted state chains."""
from __future__ import annotations

import hashlib
import hmac
import re
import secrets
from typing import Any

from .canonicalization import canonical_json_bytes
from .runtime_identity import validate_runtime_digest

STATE_MAC_HEX_LENGTH = 64
_HEX64 = re.compile(r"^[0-9a-f]{64}$")
_PROCESS_ROOT_KEY = secrets.token_bytes(32)


def validate_state_mac(value: Any, *, field: str) -> str:
    if type(value) is not str or not _HEX64.fullmatch(value):
        raise ValueError(f"{field} must be 64-char lowercase hex")
    return value


def validate_sequence_number(value: Any) -> int:
    if type(value) is not int or isinstance(value, bool) or value < 1:
        raise ValueError("sequence_number must be positive int")
    return value


def _session_key(session_genesis_id: str) -> bytes:
    genesis = validate_runtime_digest(session_genesis_id)
    return hmac.digest(
        _PROCESS_ROOT_KEY,
        b"decision-engine/state-chain/session/v1\x00" + genesis.encode("ascii"),
        "sha256",
    )


def genesis_parent_hash(session_genesis_id: str) -> str:
    genesis = validate_runtime_digest(session_genesis_id)
    return hmac.new(
        _session_key(genesis),
        b"decision-engine/state-chain/genesis-parent/v1\x00"
        + genesis.encode("ascii"),
        hashlib.sha256,
    ).hexdigest()


def state_payload_mac(payload: dict[str, Any], session_genesis_id: str) -> str:
    if type(payload) is not dict:
        raise ValueError("state payload must be dict")
    genesis = validate_runtime_digest(session_genesis_id)
    canonical = dict(payload)
    canonical["manifest_id"] = ""
    try:
        encoded = canonical_json_bytes(canonical)
    except (TypeError, ValueError) as exc:
        raise ValueError("state payload must be canonical JSON") from exc
    return hmac.new(
        _session_key(genesis),
        b"decision-engine/state-chain/manifest/v1\x00" + encoded,
        hashlib.sha256,
    ).hexdigest()


def verify_state_payload_mac(
    payload: dict[str, Any],
    *,
    session_genesis_id: str,
    recorded_mac: str,
) -> None:
    recorded = validate_state_mac(recorded_mac, field="manifest_id")
    expected = state_payload_mac(payload, session_genesis_id)
    if not hmac.compare_digest(recorded, expected):
        raise ValueError("manifest MAC authentication failed")
