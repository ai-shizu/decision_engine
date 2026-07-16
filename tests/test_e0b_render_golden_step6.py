# -*- coding: utf-8 -*-
"""STEP 6.E — cross-language sanitize golden (byte parity with Rust)."""
from __future__ import annotations

import json
import sys
from pathlib import Path

import pytest

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "src" / "python"))

from core.external_evidence import E0bRejected, sanitize_external_text

GOLDEN = ROOT / "tests" / "golden" / "e0b_render_golden.json"


def _rows() -> list[dict]:
    return json.loads(GOLDEN.read_text(encoding="utf-8"))


@pytest.mark.parametrize("row", _rows(), ids=lambda r: r["name"])
def test_python_sanitize_matches_golden(row: dict) -> None:
    raw = bytes.fromhex(row["input_hex"]).decode("utf-8")
    if row["ok"]:
        got = sanitize_external_text(raw, max_bytes=int(row["max_bytes"]))
        assert got.encode("utf-8").hex() == row["expected_hex"], row["name"]
    else:
        with pytest.raises(E0bRejected):
            sanitize_external_text(raw, max_bytes=int(row["max_bytes"]))
