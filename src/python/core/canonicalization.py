"""Canonical text and JSON encoding for all identity construction."""
from __future__ import annotations

import json
import math
import numbers
import re
import unicodedata
from typing import Any

_HORIZONTAL_WHITESPACE = re.compile(r"[^\S\n]+")
_AROUND_NEWLINE = re.compile(r" *\n *")
_REPEATED_NEWLINES = re.compile(r"\n+")


class CanonicalizationError(ValueError):
    """Input cannot be represented by the canonical identity encoding."""


def canonicalize_text(text: str) -> str:
    """Normalize Unicode, line endings, and insignificant whitespace."""
    if type(text) is not str:
        raise CanonicalizationError("canonical text input must be str")
    normalized = unicodedata.normalize("NFC", text)
    normalized = (
        normalized.replace("\r\n", "\n")
        .replace("\r", "\n")
        .replace("\u0085", "\n")
        .replace("\u2028", "\n")
        .replace("\u2029", "\n")
    )
    normalized = _HORIZONTAL_WHITESPACE.sub(" ", normalized)
    normalized = _AROUND_NEWLINE.sub("\n", normalized)
    normalized = _REPEATED_NEWLINES.sub("\n", normalized)
    return normalized.strip()


def canonicalize_json_value(value: Any, *, path: str = "$") -> Any:
    """Return a JSON-only tree with every string canonicalized."""
    if value is None or type(value) is bool:
        return value
    if isinstance(value, numbers.Integral):
        return int(value)
    if isinstance(value, numbers.Real):
        numeric = float(value)
        if not math.isfinite(numeric):
            raise CanonicalizationError(f"{path} must not contain NaN or Infinity")
        return numeric
    if type(value) is str:
        return canonicalize_text(value)
    if type(value) is list:
        return [
            canonicalize_json_value(item, path=f"{path}[{index}]")
            for index, item in enumerate(value)
        ]
    if type(value) is dict:
        canonical: dict[str, Any] = {}
        for key, item in value.items():
            if type(key) is not str:
                raise CanonicalizationError(f"{path} keys must be str")
            canonical_key = canonicalize_text(key)
            if canonical_key in canonical:
                raise CanonicalizationError(
                    f"{path} contains colliding keys after canonicalization"
                )
            canonical[canonical_key] = canonicalize_json_value(
                item,
                path=f"{path}.{canonical_key}",
            )
        return canonical
    raise CanonicalizationError(f"{path} must contain only JSON values")


def canonicalize_json(value: Any) -> str:
    """Serialize a canonical JSON tree with one exact UTF-8 representation."""
    canonical = canonicalize_json_value(value)
    try:
        return json.dumps(
            canonical,
            sort_keys=True,
            ensure_ascii=False,
            separators=(",", ":"),
            allow_nan=False,
        )
    except (TypeError, ValueError) as exc:
        raise CanonicalizationError("value is not canonical JSON") from exc


def canonical_json_bytes(value: Any) -> bytes:
    return canonicalize_json(value).encode("utf-8")
