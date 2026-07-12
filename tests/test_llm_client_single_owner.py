# -*- coding: utf-8 -*-
"""Finding 14 — single-owner llama-server HTTP client (networkless fakes)."""
from __future__ import annotations

import io
import json
import re
import subprocess
import sys
from pathlib import Path
from types import SimpleNamespace
from urllib.error import URLError

import pytest

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "src" / "python"))
CORE = ROOT / "src" / "python" / "core"

# Split tokens so this file is not a false positive for ownership greps.
_CLS = "class " + "LlamaServerBackend"
_HEALTH = "/" + "health"
_CHAT = "/v1/" + "chat/completions"
_URLLIB = "urllib" + ".request"
_SOCKET = "import " + "socket"


def _core_py_files() -> list[Path]:
    return sorted(CORE.rglob("*.py"))


def test_single_class_owner_is_llm_backend() -> None:
    hits: list[Path] = []
    for path in _core_py_files():
        text = path.read_text(encoding="utf-8")
        if re.search(rf"^{re.escape(_CLS)}\b", text, flags=re.M):
            hits.append(path)
    assert len(hits) == 1, f"expected 1 class def, got {hits}"
    assert hits[0].name == "llm_backend.py"


def test_cli_and_consultation_have_no_local_class() -> None:
    for name in ("cli.py", "consultation_engine.py"):
        text = (CORE / name).read_text(encoding="utf-8")
        assert not re.search(rf"^{re.escape(_CLS)}\b", text, flags=re.M)


def test_cli_and_consultation_have_no_private_http_client() -> None:
    for name in ("cli.py", "consultation_engine.py"):
        text = (CORE / name).read_text(encoding="utf-8")
        assert _URLLIB not in text
        assert _SOCKET not in text
        assert _HEALTH not in text
        assert _CHAT not in text


def test_endpoints_only_in_llm_backend() -> None:
    owners_h: list[str] = []
    owners_c: list[str] = []
    for path in _core_py_files():
        text = path.read_text(encoding="utf-8")
        if _HEALTH in text:
            owners_h.append(path.name)
        if _CHAT in text:
            owners_c.append(path.name)
    assert owners_h == ["llm_backend.py"]
    assert owners_c == ["llm_backend.py"]


def test_shared_import_identity() -> None:
    from core import cli, consultation_engine
    from core.llm_backend import LlamaServerBackend

    assert cli.LlamaServerBackend is LlamaServerBackend
    assert consultation_engine.LlamaServerBackend is LlamaServerBackend


class _FakeHttpResponse:
    def __init__(self, payload: bytes | list[bytes]):
        if isinstance(payload, list):
            self._chunks = payload
            self._json = None
        else:
            self._chunks = None
            self._json = payload

    def __enter__(self):
        return self

    def __exit__(self, *args):
        return False

    def __iter__(self):
        assert self._chunks is not None
        return iter(self._chunks)

    def read(self):
        assert self._json is not None
        return self._json


def test_shared_generate_payload_uses_config(monkeypatch: pytest.MonkeyPatch, tmp_path: Path) -> None:
    from core import llm_backend, llm_config
    from core.llm_backend import LlamaServerBackend

    fake_gen = lambda: {"temperature": 0.42, "max_tokens": 321}
    monkeypatch.setattr(llm_config, "generation_params", fake_gen)
    monkeypatch.setattr(llm_backend, "generation_params", fake_gen)
    monkeypatch.setattr(llm_config, "model_startup_timeout", lambda _m: 1)
    monkeypatch.setattr(llm_backend, "model_startup_timeout", lambda _m: 1)

    captured: dict = {}

    def fake_urlopen(req, timeout=0):  # noqa: ARG001
        url = req.full_url if hasattr(req, "full_url") else req
        if isinstance(url, str) and url.endswith(_HEALTH):
            return _FakeHttpResponse(json.dumps({"status": "ok"}).encode("utf-8"))
        body = json.loads(req.data.decode("utf-8"))
        captured["url"] = req.full_url
        captured["headers"] = dict(req.headers)
        captured["body"] = body
        payload = {
            "choices": [{"message": {"content": "  answer-ok  "}}],
        }
        return _FakeHttpResponse(json.dumps(payload).encode("utf-8"))

    import urllib.request as ur

    monkeypatch.setattr(ur, "urlopen", fake_urlopen)

    model = tmp_path / "model.gguf"
    model.write_bytes(b"x")
    be = LlamaServerBackend(tmp_path / "exe", model, 18081)
    be.proc = None
    monkeypatch.setattr(be, "_port_open", lambda: True)

    secret_sys = "SYS_SECRET_xyz"
    secret_user = "USER_SECRET_xyz"
    out = be.generate(secret_sys, secret_user)
    assert out == "answer-ok"
    assert captured["url"] == f"http://127.0.0.1:18081{_CHAT}"
    assert "application/json" in captured["headers"].get(
        "Content-type", captured["headers"].get("Content-Type", "")
    )
    assert captured["body"]["messages"] == [
        {"role": "system", "content": secret_sys},
        {"role": "user", "content": secret_user},
    ]
    assert captured["body"]["temperature"] == 0.42
    assert captured["body"]["max_tokens"] == 321
    assert captured["body"]["stream"] is False
    # Secrets must not leak into URL.
    assert "SECRET" not in captured["url"]

    out2 = be.generate(secret_sys, secret_user, max_tokens=11)
    assert captured["body"]["max_tokens"] == 11
    assert out2 == "answer-ok"


def test_streaming_sse_order_and_malformed_ignore(
    monkeypatch: pytest.MonkeyPatch, tmp_path: Path,
) -> None:
    from core import llm_config
    from core.llm_backend import LlamaServerBackend
    import urllib.request as ur

    monkeypatch.setattr(llm_config, "model_startup_timeout", lambda _m: 1)
    chunks = [
        b'data: {"choices":[{"delta":{"content":"A"}}]}\n',
        b"not-data\n",
        b"data: {bad-json\n",
        b'data: {"choices":[{"delta":{"content":"B"}}]}\n',
        b"data: [DONE]\n",
        b'data: {"choices":[{"delta":{"content":"C"}}]}\n',
    ]

    def fake_urlopen(req, timeout=0):  # noqa: ARG001
        url = req.full_url if hasattr(req, "full_url") else req
        if isinstance(url, str) and url.endswith(_HEALTH):
            return _FakeHttpResponse(json.dumps({"status": "ok"}).encode("utf-8"))
        return _FakeHttpResponse(chunks)

    monkeypatch.setattr(ur, "urlopen", fake_urlopen)
    model = tmp_path / "model.gguf"
    model.write_bytes(b"x")
    be = LlamaServerBackend(tmp_path / "exe", model, 18081)
    monkeypatch.setattr(be, "_port_open", lambda: True)
    seen: list[str] = []
    answer = be.generate("s", "u", on_token=seen.append)
    assert seen == ["A", "B"]
    assert answer == "AB"


def test_process_ownership_and_lifecycle(monkeypatch: pytest.MonkeyPatch) -> None:
    from core import llm_backend
    from core.llm_backend import LlamaServerBackend

    spawned: list = []
    cmds: list = []

    class FakeProc:
        def __init__(self):
            self._alive = True
            self.terminated = False
            self.killed = False

        def poll(self):
            return None if self._alive else 0

        def terminate(self):
            self.terminated = True
            self._alive = False

        def kill(self):
            self.killed = True
            self._alive = False

        def wait(self, timeout=None):  # noqa: ARG002
            return 0

    def fake_popen(cmd, stdout=None, stderr=None):  # noqa: ARG001
        cmds.append(list(cmd))
        proc = FakeProc()
        spawned.append(proc)
        return proc

    monkeypatch.setattr(llm_backend.subprocess, "Popen", fake_popen)
    monkeypatch.setattr(
        llm_backend,
        "llama_server_cmd",
        lambda exe, model, port: ["fake-server", str(exe), str(model), str(port)],
    )

    be = LlamaServerBackend(Path("exe"), Path("m.gguf"), 18081)

    # Live self-owned process → no spawn.
    live = FakeProc()
    be.proc = live
    be.start(timeout_s=1)
    assert cmds == []

    # Existing port → no spawn, proc stays None.
    be.proc = None
    monkeypatch.setattr(be, "_port_open", lambda: True)
    be.start(timeout_s=1)
    assert cmds == []
    assert be.proc is None
    # stop must not invent a kill of external server.
    be.stop()
    assert be.proc is None

    # Need spawn: port closed, health never ok → timeout stops owned proc.
    monkeypatch.setattr(be, "_port_open", lambda: False)

    def boom_urlopen(*args, **kwargs):  # noqa: ARG001
        raise OSError("down")

    import urllib.request as ur
    monkeypatch.setattr(ur, "urlopen", boom_urlopen)
    monkeypatch.setattr(llm_backend.time, "time", lambda: 1_000_000.0)
    monkeypatch.setattr(llm_backend.time, "sleep", lambda _s: None)

    with pytest.raises(RuntimeError, match="タイムアウト"):
        be.start(timeout_s=0)
    assert len(spawned) == 1
    assert spawned[0].terminated is True
    assert be.proc is None
    assert cmds and cmds[0][0] == "fake-server"


def test_structured_success_and_fallback(
    monkeypatch: pytest.MonkeyPatch, tmp_path: Path,
) -> None:
    from core import llm_config
    from core.llm_backend import LlamaServerBackend
    import urllib.request as ur

    monkeypatch.setattr(llm_config, "model_startup_timeout", lambda _m: 1)
    calls: list[str] = []

    def fake_urlopen(req, timeout=0):  # noqa: ARG001
        url = req.full_url if hasattr(req, "full_url") else req
        if isinstance(url, str) and url.endswith(_HEALTH):
            return _FakeHttpResponse(json.dumps({"status": "ok"}).encode("utf-8"))
        body = json.loads(req.data.decode("utf-8"))
        calls.append("structured" if "response_format" in body else "plain")
        if "response_format" in body:
            assert body["temperature"] == 0
            assert body["stream"] is False
            assert body["response_format"]["type"] == "json_schema"
            assert body["response_format"]["json_schema"]["strict"] is True
            payload = {"choices": [{"message": {"content": '{"ok":true}'}}]}
            return _FakeHttpResponse(json.dumps(payload).encode("utf-8"))
        payload = {"choices": [{"message": {"content": "fallback"}}]}
        return _FakeHttpResponse(json.dumps(payload).encode("utf-8"))

    monkeypatch.setattr(ur, "urlopen", fake_urlopen)
    model = tmp_path / "m.gguf"
    model.write_bytes(b"x")
    be = LlamaServerBackend(tmp_path / "exe", model, 18081)
    monkeypatch.setattr(be, "_port_open", lambda: True)

    out = be.generate_structured("s", "u", {"type": "object"})
    assert out == '{"ok":true}'
    assert calls == ["structured"]

    def fail_then_ok(req, timeout=0):  # noqa: ARG001
        url = req.full_url if hasattr(req, "full_url") else req
        if isinstance(url, str) and url.endswith(_HEALTH):
            return _FakeHttpResponse(json.dumps({"status": "ok"}).encode("utf-8"))
        body = json.loads(req.data.decode("utf-8"))
        if "response_format" in body:
            calls.append("structured-fail")
            raise URLError("nope")
        calls.append("plain")
        payload = {"choices": [{"message": {"content": "fallback"}}]}
        return _FakeHttpResponse(json.dumps(payload).encode("utf-8"))

    calls.clear()
    monkeypatch.setattr(ur, "urlopen", fail_then_ok)
    out2 = be.generate_structured("s", "u", {"type": "object"})
    assert out2 == "fallback"
    assert calls == ["structured-fail", "plain"]


def test_cli_select_backend_consult_role_and_priority(
    monkeypatch: pytest.MonkeyPatch, tmp_path: Path,
) -> None:
    from core import cli
    from core.llm_backend import LlamaServerBackend

    roles: list[str | None] = []

    def fake_find_gguf(role: str | None = None):
        roles.append(role)
        return tmp_path / "consult.gguf"

    monkeypatch.setattr(cli, "find_gguf", fake_find_gguf)

    server = tmp_path / "llama-server.exe"
    server.write_bytes(b"x")
    cli_exe = tmp_path / "llama.exe"
    cli_exe.write_bytes(b"x")

    monkeypatch.setattr(
        "core.paths.LLAMA_SERVER_EXE",
        server,
        raising=False,
    )
    # select_backend imports from .paths inside the function
    import core.paths as paths

    monkeypatch.setattr(paths, "LLAMA_SERVER_EXE", server)
    monkeypatch.setattr(paths, "LLAMA_CLI_EXE", cli_exe)

    be = cli.select_backend({}, [])
    assert isinstance(be, LlamaServerBackend)
    assert be is not None
    assert roles[-1] == "consult"

    monkeypatch.setattr(paths, "LLAMA_SERVER_EXE", tmp_path / "missing-server")
    be2 = cli.select_backend({"value_hierarchy": []}, [])
    assert isinstance(be2, cli.LlamaCliBackend)

    monkeypatch.setattr(cli, "find_gguf", lambda role=None: None)
    be3 = cli.select_backend(
        {
            "value_hierarchy": [{"value": "a", "root_need": "n"}, {"value": "b", "root_need": "n"}],
            "cognitive_biases": [],
            "decision_rules": [{"recommended_action": "x"}],
            "emotional_patterns": {
                "overall_avg_sentiment": 0.0,
                "overall_volatility_stdev": 0.0,
            },
        },
        [],
    )
    assert isinstance(be3, cli.RuleBasedBackend)


def test_cli_fallback_uses_generation_config(
    monkeypatch: pytest.MonkeyPatch, tmp_path: Path,
) -> None:
    from core import cli

    monkeypatch.setattr(cli, "generation_params", lambda: {
        "temperature": 0.31,
        "max_tokens": 222,
    })
    monkeypatch.setattr(cli, "LLAMA_CTX", 4096)
    monkeypatch.setattr(cli, "ROOT", tmp_path)
    (tmp_path / "build").mkdir()

    recorded: dict = {}

    def fake_run(cmd, capture_output=True, timeout=None):  # noqa: ARG001
        recorded["cmd"] = list(cmd)
        return SimpleNamespace(stdout=b"hello Exiting...")

    monkeypatch.setattr(cli.subprocess, "run", fake_run)
    be = cli.LlamaCliBackend(tmp_path / "llama.exe", tmp_path / "m.gguf")
    out = be.generate("sys", "user-body")
    assert out == "hello"
    cmd = recorded["cmd"]
    assert "-n" in cmd and cmd[cmd.index("-n") + 1] == "222"
    assert "--temp" in cmd and cmd[cmd.index("--temp") + 1] == "0.31"
    assert "-c" in cmd and cmd[cmd.index("-c") + 1] == "4096"

    be.generate("sys", "user-body", max_tokens=55)
    cmd = recorded["cmd"]
    assert cmd[cmd.index("-n") + 1] == "55"

    src = (CORE / "cli.py").read_text(encoding="utf-8")
    # Generation literals must not remain (knowledge budget uses 450*2).
    assert re.search(r"(?<![\d.])0\.6(?![\d])", src) is None
    assert re.search(r"(?<!\d)900(?!\d)", src) is None
    assert re.search(r"(?<!\d)8192(?!\d)", src) is None


def test_app_help_entrypoint_no_data_touch() -> None:
    before = subprocess.run(
        ["git", "status", "--short", "--", "data"],
        cwd=str(ROOT),
        capture_output=True,
        text=True,
        encoding="utf-8",
        errors="replace",
    )
    proc = subprocess.run(
        [sys.executable, "-X", "utf8", str(ROOT / "src" / "python" / "app.py"), "--help"],
        cwd=str(ROOT),
        capture_output=True,
        text=True,
        encoding="utf-8",
        errors="replace",
        timeout=30,
    )
    assert proc.returncode == 0
    assert "top-k" in proc.stdout or "top-k" in proc.stderr or "-h" in proc.stdout
    after = subprocess.run(
        ["git", "status", "--short", "--", "data"],
        cwd=str(ROOT),
        capture_output=True,
        text=True,
        encoding="utf-8",
        errors="replace",
    )
    assert before.stdout == after.stdout
