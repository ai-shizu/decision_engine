# -*- coding: utf-8 -*-
"""Finding 12 — sterile fixed diagnostic on engine stderr; no traceback leakage."""
from __future__ import annotations

import json
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
ENGINE = ROOT / "src" / "python" / "engine_stdio.py"
DIAG = "[PKB_DIAG_V1] REQUEST_FAILED"
SECRET = "SECRET_DIARY_LINE_BODY_xyzzy_42"


def _run(lines: list[str], timeout: float = 15.0) -> subprocess.CompletedProcess[str]:
    payload = "".join(line if line.endswith("\n") else line + "\n" for line in lines)
    return subprocess.run(
        [sys.executable, "-X", "utf8", "-u", str(ENGINE)],
        input=payload,
        capture_output=True,
        text=True,
        encoding="utf-8",
        errors="replace",
        timeout=timeout,
        cwd=str(ROOT / "src" / "python"),
        env={
            **dict(**{k: v for k, v in __import__("os").environ.items()}),
            "PYTHONUTF8": "1",
            "PYTHONIOENCODING": "utf-8",
            "PKB_ENGINE": "1",
            "PKB_PROJECT_ROOT": str(ROOT),
        },
    )


def _stdout_json_lines(stdout: str) -> list[dict]:
    out: list[dict] = []
    for line in stdout.splitlines():
        line = line.strip()
        if not line:
            continue
        out.append(json.loads(line))
    return out


def test_malformed_json_with_secret_does_not_leak_to_stderr() -> None:
    proc = _run([f'{{"id":1,"cmd":"health","params":{{"q":"{SECRET}"}} BROKEN'])
    assert SECRET not in proc.stderr
    assert "Traceback" not in proc.stderr
    assert DIAG in proc.stderr
    assert proc.stderr.count(DIAG) == 1
    rows = _stdout_json_lines(proc.stdout)
    assert rows[0].get("event") == "ready"
    fail = rows[-1]
    assert fail.get("ok") is False
    assert "error" in fail


def test_unknown_command_with_secret_does_not_leak_to_stderr() -> None:
    req = json.dumps(
        {"id": 2, "cmd": "not.a.real.command", "params": {"note": SECRET}},
        ensure_ascii=False,
    )
    proc = _run([req])
    assert SECRET not in proc.stderr
    assert "Traceback" not in proc.stderr
    assert ".py" not in proc.stderr
    assert req not in proc.stderr
    assert DIAG in proc.stderr
    assert proc.stderr.count(DIAG) == 1
    rows = _stdout_json_lines(proc.stdout)
    assert rows[0].get("event") == "ready"
    fail = rows[-1]
    assert fail.get("ok") is False
    assert fail.get("id") == 2
    assert isinstance(fail.get("error"), str) and fail["error"]


def test_health_success_emits_no_failure_diag() -> None:
    req = json.dumps({"id": 3, "cmd": "health", "params": {}})
    proc = _run([req])
    assert DIAG not in proc.stderr
    assert "Traceback" not in proc.stderr
    rows = _stdout_json_lines(proc.stdout)
    assert rows[0].get("event") == "ready"
    assert rows[-1].get("ok") is True
    assert rows[-1].get("id") == 3


def test_engine_stdio_source_has_no_traceback_print_exc() -> None:
    src = ENGINE.read_text(encoding="utf-8")
    assert "traceback.print_exc" not in src
    assert "import traceback" not in src
    assert "sys.stdout = sys.stderr" in src
    assert 'REQUEST_FAILED' in src or "PKB_DIAG_V1" in src


def test_failure_ipc_error_shape_preserved() -> None:
    req = json.dumps({"id": 9, "cmd": "unknown.cmd", "params": {}})
    proc = _run([req])
    rows = _stdout_json_lines(proc.stdout)
    fail = rows[-1]
    assert set(fail.keys()) >= {"id", "ok", "error"}
    assert fail["ok"] is False
    # Existing shape embeds exception type/message (Finding 13 out of scope).
    assert ": " in fail["error"]
