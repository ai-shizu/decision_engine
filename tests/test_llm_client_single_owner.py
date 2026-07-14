# -*- coding: utf-8 -*-
"""Single-owner llama.cpp stdio transport contracts (networkless fakes)."""
from __future__ import annotations

import io
import json
import subprocess
import sys
from pathlib import Path

import pytest


ROOT = Path(__file__).resolve().parents[1]
PYTHON_ROOT = ROOT / "src" / "python"
CORE = PYTHON_ROOT / "core"
sys.path.insert(0, str(PYTHON_ROOT))

from core.llm_transport import StdioPromptChannel  # noqa: E402


class _CommunicateProcess:
    def __init__(self, stdout: bytes = b"answer\n", returncode: int = 0) -> None:
        self.stdout_bytes = stdout
        self.returncode = returncode
        self.terminated = False
        self.killed = False

    def communicate(self, input: bytes, timeout: int):  # noqa: A002, ARG002
        self.input = input
        return self.stdout_bytes, b""

    def poll(self):
        return self.returncode

    def terminate(self) -> None:
        self.terminated = True

    def kill(self) -> None:
        self.killed = True

    def wait(self, timeout: int | None = None):  # noqa: ARG002
        return self.returncode


def test_single_class_owner_is_llm_backend() -> None:
    owners: list[str] = []
    for path in sorted(CORE.glob("*.py")):
        source = path.read_text(encoding="utf-8")
        if "class LlamaStdioBackend" in source:
            owners.append(path.name)
    assert owners == ["llm_backend.py"]

    for path in sorted(CORE.glob("*.py")):
        source = path.read_text(encoding="utf-8")
        assert "LlamaServerBackend" not in source, path
        assert "LlamaCliBackend" not in source, path


def test_generate_uses_shared_generation_config_and_fresh_child(
    monkeypatch: pytest.MonkeyPatch,
    tmp_path: Path,
) -> None:
    from core import llm_backend
    from core.llm_backend import LlamaStdioBackend

    monkeypatch.setattr(
        llm_backend,
        "generation_params",
        lambda: {"temperature": 0.31, "max_tokens": 222},
    )
    commands: list[list[str]] = []
    processes: list[_CommunicateProcess] = []

    def factory(command: list[str], **kwargs: object):  # noqa: ARG001
        commands.append(list(command))
        proc = _CommunicateProcess(stdout=b"result\n[end of text]\n")
        processes.append(proc)
        return proc

    backend = LlamaStdioBackend(
        tmp_path / "llama.exe",
        tmp_path / "model.gguf",
        process_factory=factory,
        prompt_channel_factory=StdioPromptChannel,
    )
    assert backend.generate("system", "user") == "result"
    assert backend.generate("system", "user", max_tokens=55) == "result"

    assert len(processes) == 2
    assert processes[0] is not processes[1]
    first, second = commands
    assert first[first.index("-n") + 1] == "222"
    assert second[second.index("-n") + 1] == "55"
    assert first[first.index("--temp") + 1] == "0.31"


def test_streaming_preserves_pipe_order(tmp_path: Path) -> None:
    from core.llm_backend import LlamaStdioBackend

    class ChunkReader:
        def __init__(self) -> None:
            self._chunks = [
                "こ".encode(),
                "んに".encode(),
                "ちは".encode(),
                b"\n[end of text]",
                b"",
            ]

        def read(self, size: int):  # noqa: ARG002
            return self._chunks.pop(0)

    class StreamProcess:
        returncode = 0

        def __init__(self) -> None:
            self.stdin = io.BytesIO()
            self.stdout = ChunkReader()

        def poll(self):
            return self.returncode

        def wait(self, timeout: int | None = None):  # noqa: ARG002
            return self.returncode

    proc = StreamProcess()
    backend = LlamaStdioBackend(
        tmp_path / "llama.exe",
        tmp_path / "model.gguf",
        process_factory=lambda command, **kwargs: proc,  # noqa: ARG005
        prompt_channel_factory=StdioPromptChannel,
    )
    seen: list[str] = []
    answer = backend.generate("system", "user", on_token=seen.append)
    assert answer == "こんにちは"
    assert "".join(seen) == "こんにちは"


def test_timeout_kills_only_owned_child_and_returns_fixed_error(tmp_path: Path) -> None:
    from core.llm_backend import LLM_TIMEOUT_MESSAGE, LlamaStdioBackend

    class TimeoutProcess(_CommunicateProcess):
        def communicate(self, input: bytes, timeout: int):  # noqa: A002
            raise subprocess.TimeoutExpired("llama", timeout)

        def poll(self):
            return None if not self.terminated else 0

    proc = TimeoutProcess()
    backend = LlamaStdioBackend(
        tmp_path / "llama.exe",
        tmp_path / "model.gguf",
        process_factory=lambda command, **kwargs: proc,  # noqa: ARG005
        prompt_channel_factory=StdioPromptChannel,
    )
    with pytest.raises(RuntimeError, match=r"^Local LLM subprocess timed out\.$"):
        backend.generate("PRIVATE_SYSTEM", "PRIVATE_USER")
    assert str(LLM_TIMEOUT_MESSAGE) == "Local LLM subprocess timed out."
    assert proc.terminated is True
    assert proc.killed is False


def test_structured_success_and_failure_falls_back_to_new_child(
    tmp_path: Path,
) -> None:
    from core.llm_backend import LlamaStdioBackend

    commands: list[list[str]] = []
    queued = [
        _CommunicateProcess(stdout=b"failure", returncode=1),
        _CommunicateProcess(stdout=b"fallback\n", returncode=0),
    ]

    def factory(command: list[str], **kwargs: object):  # noqa: ARG001
        commands.append(list(command))
        return queued.pop(0)

    backend = LlamaStdioBackend(
        tmp_path / "llama.exe",
        tmp_path / "model.gguf",
        process_factory=factory,
        prompt_channel_factory=StdioPromptChannel,
    )
    schema = {"type": "object", "properties": {"ok": {"type": "boolean"}}}
    assert backend.generate_structured("system", "user", schema) == "fallback"
    assert len(commands) == 2
    assert "--json-schema" in commands[0]
    assert json.loads(commands[0][commands[0].index("--json-schema") + 1]) == schema
    assert commands[0][commands[0].index("--temp") + 1] == "0.0"
    assert "--json-schema" not in commands[1]


def test_cli_selects_shared_stdio_backend_for_consult_role(
    monkeypatch: pytest.MonkeyPatch,
    tmp_path: Path,
) -> None:
    from core import cli, paths
    from core.llm_backend import LlamaStdioBackend

    roles: list[str | None] = []
    model = tmp_path / "consult.gguf"
    model.write_bytes(b"x")
    exe = tmp_path / "llama.exe"
    exe.write_bytes(b"x")

    def fake_find_gguf(role: str | None = None):
        roles.append(role)
        return model

    monkeypatch.setattr(cli, "find_gguf", fake_find_gguf)
    monkeypatch.setattr(paths, "LLAMA_CLI_EXE", exe)
    backend = cli.select_backend({}, [])
    assert isinstance(backend, LlamaStdioBackend)
    assert roles == ["consult"]

    monkeypatch.setattr(paths, "LLAMA_CLI_EXE", tmp_path / "missing")
    assert isinstance(cli.select_backend({}, []), cli.RuleBasedBackend)


def test_runtime_command_config_has_no_port_or_remote_option(tmp_path: Path) -> None:
    from core import llm_config

    command = llm_config.llama_stdio_cmd(
        tmp_path / "llama.exe",
        tmp_path / "model.gguf",
        prompt_file="/dev/stdin",
        max_tokens=99,
        temperature=0.25,
    )
    assert command[command.index("--ctx-size") + 1] == str(llm_config.LLAMA_CTX)
    assert command[command.index("--threads") + 1] == str(llm_config.LLAMA_THREADS)
    assert command[command.index("--batch-size") + 1] == str(llm_config.LLAMA_BATCH)
    for forbidden in ("--host", "--port", "--model-url", "--hf-repo", "--rpc"):
        assert forbidden not in command


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
