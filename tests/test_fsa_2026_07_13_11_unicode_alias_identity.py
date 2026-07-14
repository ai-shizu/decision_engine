# -*- coding: utf-8 -*-
"""FSA-2026-07-13-11 RED contracts for canonical text and alias identity."""
from __future__ import annotations

import hashlib
import inspect
import re
import sys
import unicodedata
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "src" / "python"))

from core import line_telemetry, lsm_index  # noqa: E402


def test_visual_equivalent_unicode_and_whitespace_share_one_content_hash() -> None:
    nfc_chunk = {
        "title": "が",
        "text": "alpha beta\ngamma",
    }
    variant_chunk = {
        "title": "か\u3099",
        "text": "  alpha   beta\r\ngamma  ",
    }
    assert unicodedata.normalize("NFC", variant_chunk["title"]) == nfc_chunk["title"]

    nfc_hash = lsm_index.content_hash(nfc_chunk)
    variant_hash = lsm_index.content_hash(variant_chunk)

    assert nfc_hash == variant_hash, (
        "hash-before-construction lacks shared NFC/line-ending/whitespace "
        f"canonicalization: nfc={nfc_hash}, variant={variant_hash}"
    )


def test_contact_alias_is_protected_and_at_least_128_bit() -> None:
    contact = "山田太郎"
    public_salt = bytes(range(16))
    parameters = list(inspect.signature(line_telemetry.contact_alias).parameters)
    findings: list[str] = []

    if parameters != ["contact"]:
        findings.append(f"caller controls alias material via parameters={parameters}")
        alias = line_telemetry.contact_alias(contact, public_salt)
    else:
        alias = line_telemetry.contact_alias(contact)

    legacy = "C-" + hashlib.blake2b(
        contact.encode("utf-8"),
        digest_size=4,
        salt=public_salt,
    ).hexdigest()
    if alias == legacy:
        findings.append("alias is recomputable from public contact+salt without a key")

    match = re.fullmatch(r"C-([0-9a-f]+)", alias)
    identity_bits = len(match.group(1)) * 4 if match else 0
    if identity_bits < 128:
        findings.append(
            f"persistent alias has only {identity_bits} bits (minimum is 128)"
        )

    assert not findings, "; ".join(findings)
