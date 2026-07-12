# -*- coding: utf-8 -*-
"""Finding 15 — dead readFileAsText export must be removed from engine.ts."""
from __future__ import annotations

import re
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
DESKTOP_SRC = ROOT / "apps" / "desktop" / "src"
ENGINE_TS = DESKTOP_SRC / "lib" / "engine.ts"
TEXT_DECODE_TS = DESKTOP_SRC / "lib" / "textDecode.ts"

# Split so this test file is not a false positive for tree greps of the legacy name.
LEGACY_READER = "readFile" + "AsText"
CANONICAL_READER = "readText" + "Lenient"
DIRECT_TEXT_CALL = re.compile(r"\.\s*text\s*\(")


def _iter_desktop_production() -> list[Path]:
    files: list[Path] = []
    for pattern in ("**/*.ts", "**/*.tsx"):
        files.extend(DESKTOP_SRC.glob(pattern))
    return sorted({p.resolve() for p in files if p.is_file()})


def test_legacy_reader_absent_from_desktop_production() -> None:
    hits: list[str] = []
    for path in _iter_desktop_production():
        text = path.read_text(encoding="utf-8")
        if LEGACY_READER in text:
            hits.append(str(path.relative_to(ROOT)).replace("\\", "/"))
    assert hits == [], f"legacy reader still present in production: {hits}"


def test_engine_ts_has_no_direct_file_text_call() -> None:
    text = ENGINE_TS.read_text(encoding="utf-8")
    matches = list(DIRECT_TEXT_CALL.finditer(text))
    assert matches == [], (
        f"engine.ts must not call .text(); found {len(matches)} match(es)"
    )


def test_canonical_lenient_reader_path_preserved() -> None:
    engine = ENGINE_TS.read_text(encoding="utf-8")
    decode = TEXT_DECODE_TS.read_text(encoding="utf-8")

    assert re.search(
        rf"import\s*\{{[^}}]*\b{re.escape(CANONICAL_READER)}\b[^}}]*\}}\s*from\s*[\"'].*textDecode[\"']",
        engine,
        flags=re.S,
    ), f"{CANONICAL_READER} import missing from engine.ts"

    assert re.search(
        rf"\b{re.escape(CANONICAL_READER)}\s*\(",
        engine,
    ), f"{CANONICAL_READER}(...) call missing from engine.ts"

    assert re.search(
        rf"export\s+async\s+function\s+{re.escape(CANONICAL_READER)}\s*\(",
        decode,
    ), f"{CANONICAL_READER} export missing from textDecode.ts"
