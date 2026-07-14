# -*- coding: utf-8 -*-
"""FSA-2026-07-13-01/02: networkless, owned-child LLM transport."""
from __future__ import annotations

import ast
import json
import os
import subprocess
import sys
from pathlib import Path
from types import SimpleNamespace

import pytest


ROOT = Path(__file__).resolve().parents[1]
PYTHON_ROOT = ROOT / "src" / "python"
CORE = PYTHON_ROOT / "core"

FORBIDDEN_NETWORK_MODULES = {
    "aiohttp",
    "ftplib",
    "http",
    "httpx",
    "imaplib",
    "poplib",
    "requests",
    "smtplib",
    "socket",
    "telnetlib",
    "urllib",
    "urllib3",
    "websockets",
    "xmlrpc",
}

LEGACY_TRANSPORT_TOKENS = (
    "LlamaServerBackend",
    "LLAMA_SERVER_EXE",
    "SERVER_PORT",
    "PKB_LLM_PORT",
    "llama_server_cmd",
    "SlotCacheClient",
    "127.0.0.1",
    "http://",
    "https://",
    "/v1/chat/completions",
    "/health",
)


def _import_root(name: str | None) -> str:
    return (name or "").split(".", 1)[0]


def test_python_core_has_no_tcp_http_owner_or_legacy_transport() -> None:
    violations: list[str] = []
    for path in sorted(PYTHON_ROOT.rglob("*.py")):
        source = path.read_text(encoding="utf-8")
        tree = ast.parse(source, filename=str(path))
        for node in ast.walk(tree):
            if isinstance(node, ast.Import):
                for alias in node.names:
                    root = _import_root(alias.name)
                    if root in FORBIDDEN_NETWORK_MODULES:
                        violations.append(f"{path.name}:{node.lineno}: import {alias.name}")
            elif isinstance(node, ast.ImportFrom):
                if _import_root(node.module) in FORBIDDEN_NETWORK_MODULES:
                    violations.append(f"{path.name}:{node.lineno}: from {node.module}")
            elif isinstance(node, ast.Call) and node.args:
                is_dynamic_import = (
                    isinstance(node.func, ast.Name) and node.func.id == "__import__"
                ) or (
                    isinstance(node.func, ast.Attribute)
                    and node.func.attr == "import_module"
                )
                if is_dynamic_import and isinstance(node.args[0], ast.Constant):
                    name = node.args[0].value
                    if isinstance(name, str) and _import_root(name) in FORBIDDEN_NETWORK_MODULES:
                        violations.append(f"{path.name}:{node.lineno}: dynamic import {name}")

        for token in LEGACY_TRANSPORT_TOKENS:
            if token in source:
                violations.append(f"{path.name}: legacy token {token!r}")

    assert violations == []

    package_init = (CORE / "__init__.py").read_text(encoding="utf-8")
    assert "enforce_offline_environment()" in package_init
    for entrypoint in (
        PYTHON_ROOT / "run_engine.py",
        PYTHON_ROOT / "engine_stdio.py",
        PYTHON_ROOT / "ui_tui" / "app.py",
    ):
        source = entrypoint.read_text(encoding="utf-8")
        assert "enforce_offline_environment()" in source, entrypoint

    transport_source = (CORE / "llm_transport.py").read_text(encoding="utf-8")
    assert "PIPE_REJECT_REMOTE_CLIENTS" in transport_source
    assert "FILE_FLAG_FIRST_PIPE_INSTANCE" in transport_source
    assert 'OWNER_ONLY_PIPE_SDDL = "D:P(A;;GA;;;OW)(A;;GA;;;SY)"' in transport_source
    assert "PKB_APPCONTAINER_SID" in transport_source
    assert 'namespace = "LOCAL\\\\" if appcontainer else ""' in transport_source
    assert "GetNamedPipeClientProcessId" in transport_source
    assert "bind_client" in transport_source
    assert "tempfile" not in transport_source


@pytest.mark.skipif(os.name != "nt", reason="Windows AppContainer pipe contract")
def test_windows_appcontainer_prompt_pipe_binds_package_sid(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    sys.path.insert(0, str(PYTHON_ROOT))
    try:
        from core import llm_transport
    finally:
        sys.path.pop(0)

    sid = (
        "S-1-15-2-1881917175-2158713548-1896254251-2206531621-"
        "1219496785-1513993009-2474060162"
    )
    monkeypatch.setenv("PKB_APPCONTAINER_SID", sid)
    descriptor, appcontainer = llm_transport._pipe_security_descriptor()
    assert appcontainer is True
    assert descriptor == f"{llm_transport.OWNER_ONLY_PIPE_SDDL}(A;;GA;;;{sid})"
    channel = llm_transport.secure_prompt_channel(b"PUBLIC_TEST_PROMPT")
    try:
        assert channel.source.startswith(r"\\.\pipe\LOCAL\pkb-llm-")
    finally:
        channel.close()


def test_stdio_command_is_local_only_and_has_no_secret_arguments(tmp_path: Path) -> None:
    sys.path.insert(0, str(PYTHON_ROOT))
    try:
        from core.llm_config import llama_stdio_cmd
        from core.llm_transport import secure_prompt_channel
    finally:
        sys.path.pop(0)

    exe = tmp_path / "llama.exe"
    model = tmp_path / "model.gguf"
    secret_system = "SYSTEM_SECRET_58d85c"
    secret_user = "USER_SECRET_a1107f"
    cmd = llama_stdio_cmd(
        exe,
        model,
        prompt_file="/dev/stdin",
        max_tokens=77,
        temperature=0.0,
    )
    joined = "\0".join(cmd)

    assert cmd[:2] == [str(exe), "completion"]
    assert "--offline" in cmd
    assert "--simple-io" in cmd
    assert "--single-turn" in cmd
    assert "--no-conversation" in cmd
    assert "--no-display-prompt" in cmd
    assert "--log-file" not in cmd
    assert "-m" in cmd and cmd[cmd.index("-m") + 1] == str(model)
    assert "-n" in cmd and cmd[cmd.index("-n") + 1] == "77"
    assert "--temp" in cmd and cmd[cmd.index("--temp") + 1] == "0.0"
    assert "-f" in cmd and cmd[cmd.index("-f") + 1] == "/dev/stdin"
    for forbidden in ("--host", "--port", "--model-url", "--hf-repo", "--rpc"):
        assert forbidden not in cmd
    assert secret_system not in joined
    assert secret_user not in joined

    if os.name == "nt":
        channel = secure_prompt_channel(b"PUBLIC_TEST_PROMPT")
        try:
            assert channel.source.startswith(r"\\.\pipe\pkb-llm-")
            assert channel.stdin is subprocess.DEVNULL
            assert channel.input_bytes is None
            reader = subprocess.Popen(
                [
                    sys.executable,
                    "-I",
                    "-c",
                    "import sys; print(open(sys.argv[1], 'rb', buffering=0).read(1024).decode())",
                    channel.source,
                ],
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,
                text=True,
                encoding="utf-8",
            )
            channel.bind_client(reader.pid)
            channel.start()
            stdout, stderr = reader.communicate(timeout=10)
            channel.finish(10)
            assert reader.returncode == 0, stderr
            assert stdout.strip() == "PUBLIC_TEST_PROMPT"
        finally:
            channel.close()

        rejected = secure_prompt_channel(b"MUST_NOT_REACH_WRONG_PROCESS")
        wrong_reader = subprocess.Popen(
            [
                sys.executable,
                "-I",
                "-c",
                "import sys; open(sys.argv[1], 'rb', buffering=0).read(1024)",
                rejected.source,
            ]
        )
        try:
            rejected.bind_client(os.getpid())
            rejected.start()
            try:
                rejected.finish(10)
            except RuntimeError as exc:
                assert str(exc) == "Local LLM prompt channel failed."
            else:
                raise AssertionError("mismatched Named Pipe client PID was accepted")
            wrong_reader.wait(timeout=10)
        finally:
            rejected.close()
            if wrong_reader.poll() is None:
                wrong_reader.kill()
                wrong_reader.wait(timeout=10)


def test_backend_uses_owned_anonymous_stdio_and_prompt_only_on_stdin(
    monkeypatch,
    tmp_path: Path,
) -> None:
    sys.path.insert(0, str(PYTHON_ROOT))
    try:
        from core.llm_backend import LlamaStdioBackend
        from core.llm_transport import StdioPromptChannel
        from core.offline_runtime import offline_subprocess_environment
    finally:
        sys.path.pop(0)

    captured: dict[str, object] = {}
    bound_pids: list[int] = []
    secret_system = "SYSTEM_SECRET_a42d09"
    secret_user = "USER_SECRET_8d8bca"
    monkeypatch.setenv("PKB_TEST_PARENT_SECRET", "MUST_NOT_REACH_CHILD")

    class BoundStdioPromptChannel(StdioPromptChannel):
        def bind_client(self, process_id: int) -> None:
            bound_pids.append(process_id)

    class FakeProcess:
        returncode = 0
        pid = 4242

        def communicate(self, input: bytes, timeout: int):  # noqa: A002
            captured["input"] = input
            captured["timeout"] = timeout
            return b"answer from child\n", b""

        def poll(self):
            return self.returncode

    def process_factory(cmd: list[str], **kwargs: object):
        captured["cmd"] = list(cmd)
        captured["kwargs"] = dict(kwargs)
        return FakeProcess()

    backend = LlamaStdioBackend(
        tmp_path / "llama.exe",
        tmp_path / "model.gguf",
        process_factory=process_factory,
        prompt_channel_factory=BoundStdioPromptChannel,
    )
    answer = backend.generate(secret_system, secret_user, max_tokens=33)

    assert answer == "answer from child"
    cmd = captured["cmd"]
    kwargs = captured["kwargs"]
    assert isinstance(cmd, list) and isinstance(kwargs, dict)
    assert secret_system not in "\0".join(cmd)
    assert secret_user not in "\0".join(cmd)
    prompt = captured["input"]
    assert isinstance(prompt, bytes)
    assert secret_system.encode() in prompt
    assert secret_user.encode() in prompt
    assert kwargs["stdin"] is subprocess.PIPE
    assert kwargs["stdout"] is subprocess.PIPE
    assert kwargs["stderr"] is subprocess.DEVNULL
    assert kwargs["shell"] is False
    assert kwargs["close_fds"] is True
    assert kwargs["env"] == offline_subprocess_environment()
    assert "PKB_TEST_PARENT_SECRET" not in kwargs["env"]
    assert bound_pids == [4242]
    assert not list(tmp_path.glob("*prompt*"))


def test_structured_generation_uses_same_pipe_and_fixed_schema_mode(tmp_path: Path) -> None:
    sys.path.insert(0, str(PYTHON_ROOT))
    try:
        from core.llm_backend import LlamaStdioBackend
        from core.llm_transport import StdioPromptChannel
    finally:
        sys.path.pop(0)

    calls: list[tuple[list[str], bytes]] = []

    class FakeProcess:
        returncode = 0

        def __init__(self, cmd: list[str]):
            self.cmd = cmd

        def communicate(self, input: bytes, timeout: int):  # noqa: A002, ARG002
            calls.append((self.cmd, input))
            return b'{"ok":true}\n', b""

        def poll(self):
            return self.returncode

    def process_factory(cmd: list[str], **kwargs: object):  # noqa: ARG001
        return FakeProcess(list(cmd))

    backend = LlamaStdioBackend(
        tmp_path / "llama.exe",
        tmp_path / "model.gguf",
        process_factory=process_factory,
        prompt_channel_factory=StdioPromptChannel,
    )
    schema = {"type": "object", "properties": {"ok": {"type": "boolean"}}}
    result = backend.generate_structured("system", "user", schema, max_tokens=41)

    assert result == '{"ok":true}'
    assert len(calls) == 1
    cmd, prompt = calls[0]
    assert "--json-schema" in cmd
    encoded_schema = cmd[cmd.index("--json-schema") + 1]
    assert json.loads(encoded_schema) == schema
    assert "--temp" in cmd and cmd[cmd.index("--temp") + 1] == "0.0"
    assert b"system" in prompt and b"user" in prompt


def test_runtime_guard_rejects_ipv4_ipv6_and_loopback_before_connect() -> None:
    script = f"""
import sys
sys.path.insert(0, {str(PYTHON_ROOT)!r})
from core.offline_runtime import enforce_offline_environment
enforce_offline_environment()
import socket
for family in (socket.AF_INET, socket.AF_INET6):
    try:
        socket.socket(family, socket.SOCK_STREAM)
    except PermissionError as exc:
        assert str(exc) == "PKB network capability disabled"
    else:
        raise AssertionError(f"network family {{family}} was not blocked")
import asyncio
import threading
async def verify_cross_thread_wakeup():
    loop = asyncio.get_running_loop()
    loop.set_debug(True)
    woke = asyncio.Event()
    worker = threading.Thread(target=lambda: loop.call_soon_threadsafe(woke.set))
    worker.start()
    await asyncio.wait_for(woke.wait(), timeout=2)
    worker.join(timeout=2)
    assert not worker.is_alive()
asyncio.run(verify_cross_thread_wakeup())
print("NETWORK_DENIED")
"""
    proc = subprocess.run(
        [sys.executable, "-I", "-c", script],
        cwd=str(ROOT),
        capture_output=True,
        text=True,
        encoding="utf-8",
        errors="replace",
        timeout=30,
    )
    assert proc.returncode == 0, proc.stderr
    assert proc.stdout.strip() == "NETWORK_DENIED"


def test_all_llm_callers_use_the_single_stdio_owner() -> None:
    owner = (CORE / "llm_backend.py").read_text(encoding="utf-8")
    assert owner.count("class LlamaStdioBackend") == 1

    expected_callers = (
        CORE / "cli.py",
        CORE / "consultation_engine.py",
        CORE / "gap_analysis.py",
        CORE / "profiler.py",
    )
    for path in expected_callers:
        source = path.read_text(encoding="utf-8")
        assert "LlamaStdioBackend" in source, path
        for token in ("LlamaServerBackend", "LlamaCliBackend", "LLAMA_SERVER_EXE", "SERVER_PORT"):
            assert token not in source, f"{path}: {token}"

    config = (CORE / "llm_config.py").read_text(encoding="utf-8")
    paths = (CORE / "paths.py").read_text(encoding="utf-8")
    assert "llama_stdio_cmd" in config
    assert "llama_server_cmd" not in config
    assert "LLAMA_SERVER_EXE" not in paths
    assert not (CORE / "kv_cache.py").exists()
