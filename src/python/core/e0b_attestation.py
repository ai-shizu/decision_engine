# -*- coding: utf-8 -*-
"""E0b attestation v2 foundation — canonicalize_for_match + PIISnapshot.

Byte-exact contract for future Rust Dual-Run (STEP 3).
Parent: docs/SPEC_E0B_STEP1_ATTESTATION.md / SPEC_E0B_KNOWLEDGE_GATEWAY_v3.md §3.

Allowed imports only: unicodedata, hashlib, dataclasses, typing, collections.abc.
Scope E cage forbids subprocess/ctypes/network/dynamic exec.
"""
from __future__ import annotations

import hashlib
import unicodedata
from collections.abc import Iterable, Sequence
from dataclasses import dataclass

# Explicit codepoint sets — never unicodedata.category() (Python↔Rust drift).
REMOVE_SET: frozenset[int] = frozenset(
    {
        *range(0x00, 0x09),  # C0 except TAB..CR
        *range(0x0E, 0x20),  # C0 after CR
        *range(0x7F, 0xA0),  # DEL + C1
        0x00AD,  # SOFT HYPHEN
        0x061C,  # ARABIC LETTER MARK
        0x180E,  # MONGOLIAN VOWEL SEPARATOR
        0x200B,
        0x200C,
        0x200D,
        0x200E,
        0x200F,  # ZWSP ZWNJ ZWJ LRM RLM
        0x202A,
        0x202B,
        0x202C,
        0x202D,
        0x202E,  # LRE RLE PDF LRO RLO
        0x2060,
        0x2061,
        0x2062,
        0x2063,
        0x2064,  # WJ + invisible ops
        0x2066,
        0x2067,
        0x2068,
        0x2069,  # LRI RLI FSI PDI
        0xFEFF,  # BOM / ZWNBSP
    }
)

WHITESPACE_SET: frozenset[int] = frozenset(
    {
        0x09,
        0x0A,
        0x0B,
        0x0C,
        0x0D,  # TAB LF VT FF CR
        0x20,  # SPACE
        0x2028,  # LINE SEPARATOR
        0x2029,  # PARAGRAPH SEPARATOR
    }
)

_SURROGATE_LO = 0xD800
_SURROGATE_HI = 0xDFFF

DOMAIN_SEP = b"PKB-PII-DICT-V1"
MAX_PII_TERM_BYTES = 4096
MAX_PII_TERMS = 100_000


def canonicalize_for_match(s: str) -> str:
    """Deterministic match normalization for Dual-Run byte-exact parity.

    Order is fixed (do not reorder): surrogate reject → REMOVE_SET strip →
    NFKC → casefold → NFKC → whitespace fold/collapse/strip.
    """
    if type(s) is not str:
        raise TypeError("canonicalize_for_match requires str")
    for ch in s:
        cp = ord(ch)
        if _SURROGATE_LO <= cp <= _SURROGATE_HI:
            raise ValueError("lone surrogate rejected (Rust &str parity)")

    stripped = "".join(ch for ch in s if ord(ch) not in REMOVE_SET)
    normalized = unicodedata.normalize("NFKC", stripped)
    folded = normalized.casefold()
    normalized = unicodedata.normalize("NFKC", folded)

    mapped: list[str] = []
    for ch in normalized:
        if ord(ch) in WHITESPACE_SET:
            mapped.append(" ")
        else:
            mapped.append(ch)
    collapsed: list[str] = []
    prev_space = False
    for ch in mapped:
        if ch == " ":
            if prev_space:
                continue
            prev_space = True
            collapsed.append(ch)
        else:
            prev_space = False
            collapsed.append(ch)
    return "".join(collapsed).strip(" ")


def preimage(terms: Sequence[str], revision: int) -> bytes:
    """Length-prefixed injective encoding (UTF-8 byte lengths, big-endian u64)."""
    if type(revision) is not int or isinstance(revision, bool):
        raise TypeError("revision must be int")
    if revision < 0:
        raise ValueError("revision must be non-negative")
    out = bytearray(DOMAIN_SEP)
    out.extend(revision.to_bytes(8, "big"))
    for t in terms:
        raw = t.encode("utf-8")
        out.extend(len(raw).to_bytes(8, "big"))
        out.extend(raw)
    return bytes(out)


@dataclass(frozen=True, slots=True)
class PIISnapshot:
    """Immutable PII dictionary snapshot: sorted canonical terms + revision hash."""

    revision: int
    terms: tuple[str, ...]
    hash_hex: str

    @classmethod
    def build(
        cls,
        raw_terms: Iterable[str],
        revision: int,
        *,
        previous: PIISnapshot | None = None,
    ) -> PIISnapshot:
        if type(revision) is not int or isinstance(revision, bool):
            raise TypeError("revision must be int")
        if revision < 0:
            raise ValueError("revision must be non-negative")
        if previous is not None and revision <= previous.revision:
            raise ValueError("revision must be strictly greater than previous.revision")

        canonical: list[str] = []
        seen: set[str] = set()
        for raw in raw_terms:
            if type(raw) is not str:
                raise TypeError("raw_terms must contain strings")
            term = canonicalize_for_match(raw)
            if not term:
                raise ValueError("canonical term must not be empty")
            nbytes = len(term.encode("utf-8"))
            if nbytes > MAX_PII_TERM_BYTES:
                raise ValueError("canonical term exceeds MAX_PII_TERM_BYTES")
            if term not in seen:
                seen.add(term)
                canonical.append(term)

        if len(canonical) > MAX_PII_TERMS:
            raise ValueError("term count exceeds MAX_PII_TERMS")

        # UTF-8 byte order == Python str default sort for these codepoints.
        terms = tuple(sorted(canonical))
        digest = hashlib.sha256(preimage(terms, revision)).hexdigest()
        return cls(revision=revision, terms=terms, hash_hex=digest)
