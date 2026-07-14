# -*- coding: utf-8 -*-
"""FSA-2026-07-13-08 RED contracts for non-destructive persistence."""
from __future__ import annotations

import json
import os
import sys
from datetime import datetime
from pathlib import Path

import pytest

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "src" / "python"))

from core import (  # noqa: E402
    calendar_manager,
    consultation_log,
    durable_persistence,
    finance_manager,
    interview_report,
    knowledge_fetcher,
    llm_config,
    profile_store,
    profiler,
)


def test_corrupt_profile_json_hard_fails_without_touching_original_bytes(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    profile_path = tmp_path / "user_profile.json"
    corrupt_bytes = b'{"schema":"user_profile.v2","fixed_attributes":'
    profile_path.write_bytes(corrupt_bytes)
    monkeypatch.setattr(profile_store, "USER_PROFILE", profile_path)

    try:
        loaded = profile_store.load_user_profile()
    except (json.JSONDecodeError, OSError, ValueError):
        pass
    else:
        pytest.fail(
            "silent repair accepted corrupt profile bytes as "
            f"{loaded.get('schema', '<missing>')}"
        )

    assert profile_path.read_bytes() == corrupt_bytes


def test_same_second_same_genre_reports_never_overwrite_each_other(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    records_dir = tmp_path / "interviews"
    fixed_now = datetime(2026, 7, 14, 12, 34, 56)

    class FrozenDateTime:
        @classmethod
        def now(cls) -> datetime:
            return fixed_now

    monkeypatch.setattr(interview_report, "INTERVIEW_RECORDS_DIR", records_dir)
    monkeypatch.setattr(interview_report, "datetime", FrozenDateTime)

    first = {
        "schema": interview_report.SCHEMA_VERSION,
        "date": "2026-07-14T12:34:56",
        "config": {"genre": "system_design"},
        "metrics": [],
        "summary": "first physically distinct report",
        "latency": {},
        "simulated": True,
    }
    second = {
        **first,
        "summary": "second physically distinct report",
    }

    first_path = Path(interview_report.persist_report(first, "system_design"))
    second_path = Path(interview_report.persist_report(second, "system_design"))

    if first_path == second_path:
        stored = json.loads(first_path.read_text(encoding="utf-8"))
        pytest.fail(
            "same-second report collision overwrote the first payload with "
            f"{stored['summary']!r}"
        )

    assert json.loads(first_path.read_text(encoding="utf-8")) == first
    assert json.loads(second_path.read_text(encoding="utf-8")) == second
    assert len(list(records_dir.glob("*.json"))) == 2


@pytest.mark.parametrize(
    ("module", "path_attr", "loader"),
    [
        (finance_manager, "FINANCE_JSON", finance_manager.load_finance),
        (calendar_manager, "CALENDAR_JSON", calendar_manager.load_calendar),
        (
            consultation_log,
            "AI_CONSULTATIONS_JSON",
            consultation_log.load_consultations,
        ),
    ],
)
def test_other_json_stores_hard_fail_without_touching_corrupt_bytes(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
    module,
    path_attr: str,
    loader,
) -> None:
    path = tmp_path / f"{path_attr.lower()}.json"
    corrupt_bytes = b'{"2026-07-14":['
    path.write_bytes(corrupt_bytes)
    monkeypatch.setattr(module, path_attr, path)

    with pytest.raises(durable_persistence.PersistenceReadError):
        loader()

    assert path.read_bytes() == corrupt_bytes


def test_profiler_never_repairs_corrupt_user_profile(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    profile_path = tmp_path / "user_profile.json"
    corrupt_bytes = b'{"schema":"user_profile.v2"'
    profile_path.write_bytes(corrupt_bytes)
    monkeypatch.setattr(profiler, "USER_PROFILE_JSON", profile_path)

    generated_profile = {
        "value_hierarchy": [],
        "cognitive_biases": [],
        "decision_rules": [],
    }
    with pytest.raises(durable_persistence.PersistenceReadError):
        profiler.update_user_profile(generated_profile)

    assert profile_path.read_bytes() == corrupt_bytes


@pytest.mark.parametrize(
    ("module", "path_attr", "loader"),
    [
        (knowledge_fetcher, "QUEUE_JSON", knowledge_fetcher.load_queue),
        (llm_config, "MODEL_PARAMS_JSON", llm_config.load_model_params),
    ],
)
def test_auxiliary_stores_never_replace_corrupt_json_with_defaults(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
    module,
    path_attr: str,
    loader,
) -> None:
    path = tmp_path / f"{path_attr.lower()}.json"
    corrupt_bytes = b'{"truncated":'
    path.write_bytes(corrupt_bytes)
    monkeypatch.setattr(module, path_attr, path)
    if module is llm_config:
        monkeypatch.setattr(llm_config, "artifact_auth_required", lambda: False)

    with pytest.raises(durable_persistence.PersistenceReadError):
        loader()

    assert path.read_bytes() == corrupt_bytes


def test_replace_failure_preserves_original_and_cleans_temporary_file(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    target = tmp_path / "state.json"
    original = b'{"version":1}'
    target.write_bytes(original)

    def fail_replace(src, dst) -> None:
        raise OSError("simulated replace failure")

    monkeypatch.setattr(durable_persistence.os, "replace", fail_replace)
    with pytest.raises(durable_persistence.PersistenceWriteError):
        durable_persistence.durable_atomic_write(target, b'{"version":2}')

    assert target.read_bytes() == original
    assert list(tmp_path.glob(f".{target.name}.*.tmp")) == []


def test_file_fsync_precedes_atomic_replace(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    target = tmp_path / "state.json"
    events: list[str] = []
    real_replace = os.replace

    def record_fsync(fd: int) -> None:
        events.append("fsync")

    def record_replace(src, dst) -> None:
        events.append("replace")
        real_replace(src, dst)

    monkeypatch.setattr(durable_persistence.os, "fsync", record_fsync)
    monkeypatch.setattr(durable_persistence.os, "replace", record_replace)
    durable_persistence.durable_atomic_write(target, b"durable")

    assert target.read_bytes() == b"durable"
    assert events.index("fsync") < events.index("replace")


def test_corrupt_interview_history_hard_fails_without_deleting_bytes(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    records_dir = tmp_path / "interviews"
    records_dir.mkdir()
    corrupt_path = records_dir / (
        f"interview_20260714T123456_{'a' * 64}_system_design.json"
    )
    corrupt_bytes = b'{"schema":"interview_report.v1"'
    corrupt_path.write_bytes(corrupt_bytes)
    monkeypatch.setattr(interview_report, "INTERVIEW_RECORDS_DIR", records_dir)

    with pytest.raises(durable_persistence.PersistenceReadError):
        interview_report.load_recent_reports("system_design")

    assert corrupt_path.read_bytes() == corrupt_bytes
