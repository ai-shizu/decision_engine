# -*- coding: utf-8 -*-
"""Finding 18 — diary/LINE import.stats must use streaming + process-local cache."""
from __future__ import annotations

import inspect
import json
import os
import re
import sys
import textwrap
from pathlib import Path
from unittest.mock import MagicMock

import pytest

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "src" / "python"))

from core import facade  # noqa: E402
from core.paths import CALENDAR_JSON, DIARY_MD, FINANCE_JSON, LINE_HISTORY  # noqa: E402


@pytest.fixture(autouse=True)
def _clear_source_count_cache() -> None:
    cache = getattr(facade, "_SOURCE_COUNT_CACHE", None)
    if cache is not None:
        cache.clear()
    yield
    cache = getattr(facade, "_SOURCE_COUNT_CACHE", None)
    if cache is not None:
        cache.clear()


def _seed_diary(text: str | None = None) -> None:
    DIARY_MD.parent.mkdir(parents=True, exist_ok=True)
    DIARY_MD.write_text(
        text
        if text is not None
        else "## 2026-07-01\nA\n\n## 2026-07-02\nB\n",
        encoding="utf-8",
    )


def _seed_line(text: str | None = None) -> None:
    LINE_HISTORY.parent.mkdir(parents=True, exist_ok=True)
    LINE_HISTORY.write_text(
        text
        if text is not None
        else (
            "[LINE] peerとのトーク履歴\n2026/07/01(水)\n10:00\tpeer\thi\n"
            "[LINE] peerとのトーク履歴\n2026/07/02(木)\n10:00\tpeer\tyo\n"
        ),
        encoding="utf-8",
    )


def _spy_scanners(monkeypatch: pytest.MonkeyPatch) -> dict[str, int]:
    calls = {"diary": 0, "line": 0}
    real_diary = facade._diary_entry_count
    real_line = facade._line_export_count

    def diary_wrap(path: Path) -> int:
        calls["diary"] += 1
        return real_diary(path)

    def line_wrap(path: Path) -> int:
        calls["line"] += 1
        return real_line(path)

    monkeypatch.setattr(facade, "_diary_entry_count", diary_wrap)
    monkeypatch.setattr(facade, "_line_export_count", line_wrap)
    return calls


def test_01_unchanged_cache_hit(monkeypatch: pytest.MonkeyPatch) -> None:
    _seed_diary()
    _seed_line()
    calls = _spy_scanners(monkeypatch)
    first = facade.data_source_stats()
    assert calls == {"diary": 1, "line": 1}
    second = facade.data_source_stats()
    assert calls == {"diary": 1, "line": 1}
    assert second == first


def test_02_size_or_mtime_change_rescans(monkeypatch: pytest.MonkeyPatch) -> None:
    _seed_diary()
    _seed_line()
    calls = _spy_scanners(monkeypatch)
    facade.data_source_stats()
    facade.data_source_stats()
    assert calls == {"diary": 1, "line": 1}

    DIARY_MD.write_text("## 2026-07-01\nA\n\n## 2026-07-02\nB\n\n## 2026-07-03\nC\n", encoding="utf-8")
    LINE_HISTORY.write_text(
        "[LINE] one\n[LINE] two\n[LINE] three\n",
        encoding="utf-8",
    )
    mid = facade.data_source_stats()
    assert calls == {"diary": 2, "line": 2}
    assert mid["diary"]["count"] == 3
    assert mid["line"]["count"] == 3

    after = facade.data_source_stats()
    assert calls == {"diary": 2, "line": 2}
    assert after["diary"]["count"] == 3
    assert after["line"]["count"] == 3


def test_03_atomic_replacement_same_size_mtime(monkeypatch: pytest.MonkeyPatch) -> None:
    DIARY_MD.parent.mkdir(parents=True, exist_ok=True)
    # Binary-stable equal-size payloads (LF only) so size/mtime spoof is exact.
    original = b"## 2026-07-01\nXXXXXXXXXXXXXXX\n"  # 1 heading, 30 bytes
    replacement = b"## 2026-07-01\nY\n## 2026-07-02\n"  # 2 headings, 30 bytes
    assert len(original) == len(replacement) == 30
    DIARY_MD.write_bytes(original)

    calls = _spy_scanners(monkeypatch)
    first = facade.data_source_stats()
    assert first["diary"]["count"] == 1
    facade.data_source_stats()
    assert calls["diary"] == 1

    st = DIARY_MD.stat()
    if st.st_ino == 0:
        pytest.skip("file identity unavailable (st_ino==0); identity cache disabled")

    tmp = DIARY_MD.with_suffix(".tmp-replace")
    tmp.write_bytes(replacement)
    os.utime(tmp, ns=(st.st_atime_ns, st.st_mtime_ns))
    os.replace(tmp, DIARY_MD)

    after = facade.data_source_stats()
    assert calls["diary"] == 2
    expected = sum(
        1
        for line in DIARY_MD.open("r", encoding="utf-8")
        if re.match(r"^##\s+\d{4}-\d{2}-\d{2}", line)
    )
    assert after["diary"]["count"] == expected
    assert after["diary"]["count"] != first["diary"]["count"]

    facade.data_source_stats()
    assert calls["diary"] == 2


def test_04_delete_recreate_does_not_reuse_stale(monkeypatch: pytest.MonkeyPatch) -> None:
    _seed_diary("## 2026-07-01\nA\n\n## 2026-07-02\nB\n")
    calls = _spy_scanners(monkeypatch)
    assert facade.data_source_stats()["diary"]["count"] == 2
    facade.data_source_stats()
    assert calls["diary"] == 1

    DIARY_MD.unlink()
    missing = facade.data_source_stats()
    assert missing["diary"] == {"exists": False, "count": 0, "mtime": None}

    _seed_diary("## 2026-08-01\nonly\n")
    recreated = facade.data_source_stats()
    assert calls["diary"] == 2
    assert recreated["diary"]["count"] == 1
    assert recreated["diary"]["exists"] is True


def test_05_mutation_during_scan_not_cached(monkeypatch: pytest.MonkeyPatch) -> None:
    _seed_diary("## 2026-07-01\nA\n")
    real = facade._diary_entry_count
    calls = {"n": 0}
    mutated = {"done": False}

    def mutating(path: Path) -> int:
        calls["n"] += 1
        if not mutated["done"]:
            mutated["done"] = True
            # Change size so pre/post fingerprint diverge.
            path.write_text("## 2026-07-01\nA\n\n## 2026-07-02\nB\n", encoding="utf-8")
        return real(path)

    monkeypatch.setattr(facade, "_diary_entry_count", mutating)
    first = facade.data_source_stats()
    assert calls["n"] == 1
    # Unstable scan must not poison cache: second call rescans.
    second = facade.data_source_stats()
    assert calls["n"] == 2
    assert second["diary"]["count"] == 2
    # Stable second result may be cached.
    third = facade.data_source_stats()
    assert calls["n"] == 2
    assert third["diary"]["count"] == first["diary"]["count"] or third["diary"]["count"] == 2


def test_06_scanner_exception_not_cached(monkeypatch: pytest.MonkeyPatch) -> None:
    _seed_line()
    real = facade._line_export_count
    calls = {"n": 0}

    def flaky(path: Path) -> int:
        calls["n"] += 1
        if calls["n"] == 1:
            raise RuntimeError("boom-scan")
        return real(path)

    monkeypatch.setattr(facade, "_line_export_count", flaky)
    with pytest.raises(RuntimeError, match="boom-scan"):
        facade.data_source_stats()
    assert calls["n"] == 1

    ok = facade.data_source_stats()
    assert calls["n"] == 2
    assert ok["line"]["count"] == 2

    facade.data_source_stats()
    assert calls["n"] == 2


def test_07_line_known_write_invalidates(monkeypatch: pytest.MonkeyPatch) -> None:
    _seed_line()
    calls = _spy_scanners(monkeypatch)
    assert facade.data_source_stats()["line"]["count"] == 2
    facade.data_source_stats()
    assert calls["line"] == 1

    facade._append_line_text(
        "[LINE] extraとのトーク履歴\n2026/07/03(金)\n10:00\textra\tmsg\n",
        "extra.txt",
    )
    after = facade.data_source_stats()
    assert calls["line"] == 2
    assert after["line"]["count"] == 3
    facade.data_source_stats()
    assert calls["line"] == 2


def test_08_diary_known_write_invalidates(monkeypatch: pytest.MonkeyPatch) -> None:
    _seed_diary("## 2026-07-01\nA\n")
    calls = _spy_scanners(monkeypatch)
    assert facade.data_source_stats()["diary"]["count"] == 1
    facade.data_source_stats()
    assert calls["diary"] == 1

    fake_engine = MagicMock()
    fake_engine.sync_diary_index.return_value = False
    monkeypatch.setattr(facade, "get_engine", lambda: fake_engine)

    facade.save_record(
        "2026-07-09",
        events=[],
        transactions=[],
        diary="new diary body for heading",
    )
    after = facade.data_source_stats()
    assert calls["diary"] == 2
    assert after["diary"]["count"] >= 1
    # save_diary_for_date writes a ## date heading; count must reflect post-write file.
    expected = sum(
        1
        for line in DIARY_MD.read_text(encoding="utf-8").splitlines()
        if re.match(r"^##\s+\d{4}-\d{2}-\d{2}", line)
    )
    assert after["diary"]["count"] == expected
    facade.data_source_stats()
    assert calls["diary"] == 2


def test_09_streaming_scanners_no_read_text() -> None:
    _seed_diary("## 2026-07-01\nA\n\n## 2026-07-02\nB\n")
    _seed_line("[LINE] a\n[LINE] b\n[LINE] c\n")
    assert facade._diary_entry_count(DIARY_MD) == 2
    assert facade._line_export_count(LINE_HISTORY) == 3

    diary_src = textwrap.dedent(inspect.getsource(facade._diary_entry_count))
    line_src = textwrap.dedent(inspect.getsource(facade._line_export_count))
    assert ".read_text(" not in diary_src
    assert ".read_text(" not in line_src
    assert 'encoding="utf-8"' in diary_src
    assert 'encoding="utf-8"' in line_src
    assert "errors=" not in diary_src
    assert "errors=" not in line_src
    assert "open(" in diary_src or ".open(" in diary_src
    assert "open(" in line_src or ".open(" in line_src
    assert r"^##\s+\d{4}-\d{2}-\d{2}" in facade._DIARY_ENTRY_RE.pattern


def test_10_bounded_process_local_cache() -> None:
    cache = getattr(facade, "_SOURCE_COUNT_CACHE", None)
    assert isinstance(cache, dict)
    owned = getattr(facade, "_CACHEABLE_SOURCE_PATHS", None)
    assert owned == frozenset((DIARY_MD, LINE_HISTORY))

    _seed_diary()
    _seed_line()
    facade.data_source_stats()
    assert set(cache.keys()).issubset({DIARY_MD, LINE_HISTORY})
    assert len(cache) <= 2

    helper_names = (
        "_cached_source_count",
        "_invalidate_source_count",
        "_source_fingerprint",
    )
    for name in helper_names:
        assert hasattr(facade, name)
        src = inspect.getsource(getattr(facade, name))
        assert "write_text" not in src
        assert "open(" not in src or name == "_cached_source_count"
        # cached count may open only via count_fn, not its own file writes.
        if name != "_cached_source_count":
            assert "open(" not in src
        assert "pickle" not in src.lower()
        assert "sqlite" not in src.lower()
        assert "Timer" not in src
        assert "Thread" not in src
        assert "ttl" not in src.lower()

    # No sidecar cache files under sandbox data roots.
    for p in DIARY_MD.parent.rglob("*cache*"):
        assert p.suffix not in {".json", ".pkl", ".sqlite"}, p


def test_11_output_shape_preserved() -> None:
    _seed_diary()
    _seed_line()
    CALENDAR_JSON.write_text(json.dumps({"2026-07-01": [{"t": 1}]}), encoding="utf-8")
    FINANCE_JSON.write_text(json.dumps({"2026-07-01": [{"a": 1}]}), encoding="utf-8")
    stats = facade.data_source_stats()
    for key in ("diary", "line", "calendar", "finance", "es", "knowledge"):
        assert set(stats[key].keys()) == {"exists", "count", "mtime"}
    assert stats["diary"]["count"] == 2
    assert stats["line"]["count"] == 2
    assert stats["calendar"]["count"] == 1
    assert stats["finance"]["count"] == 1
    assert isinstance(stats["diary"]["mtime"], str)


def test_12_calendar_finance_not_cached(monkeypatch: pytest.MonkeyPatch) -> None:
    CALENDAR_JSON.write_text(json.dumps({"2026-07-01": [{"t": 1}]}), encoding="utf-8")
    FINANCE_JSON.write_text(json.dumps({"2026-07-01": [{"a": 1}]}), encoding="utf-8")
    first = facade.data_source_stats()
    assert first["calendar"]["count"] == 1
    assert first["finance"]["count"] == 1

    CALENDAR_JSON.write_text(
        json.dumps({"2026-07-01": [{"t": 1}, {"t": 2}], "2026-07-02": [{"t": 3}]}),
        encoding="utf-8",
    )
    FINANCE_JSON.write_text(
        json.dumps({"2026-07-01": [{"a": 1}, {"a": 2}]}),
        encoding="utf-8",
    )
    second = facade.data_source_stats()
    assert second["calendar"]["count"] == 3
    assert second["finance"]["count"] == 2

    cache = getattr(facade, "_SOURCE_COUNT_CACHE", {})
    assert CALENDAR_JSON not in cache
    assert FINANCE_JSON not in cache
