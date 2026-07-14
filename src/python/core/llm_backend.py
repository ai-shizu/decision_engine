# -*- coding: utf-8 -*-
"""Single owner for local LLM inference over parent-owned anonymous stdio."""
from __future__ import annotations

import atexit
import codecs
import re
import subprocess
import threading
from pathlib import Path
from typing import Callable

from .artifact_auth import verify_artifact_path, verify_artifact_path_by_prefix
from .llm_config import generation_params, llama_stdio_cmd
from .llm_transport import PromptChannel, secure_prompt_channel
from .offline_runtime import enforce_offline_environment, offline_subprocess_environment


LLM_FAILURE_MESSAGE = "Local LLM subprocess failed."
LLM_TIMEOUT_MESSAGE = "Local LLM subprocess timed out."
DEFAULT_TIMEOUT_S = 1200


def _prompt_bytes(system: str, user: str) -> bytes:
    prompt = (
        "<|im_start|>system\n"
        f"{system}\n"
        "<|im_end|>\n"
        "<|im_start|>user\n"
        f"{user}\n"
        "<|im_end|>\n"
        "<|im_start|>assistant\n"
    )
    return prompt.encode("utf-8")


def _clean_output(raw: bytes | str) -> str:
    text = raw if isinstance(raw, str) else raw.decode("utf-8", errors="replace")
    text = re.sub(r"\x1b\[[0-?]*[ -/]*[@-~]", "", text)
    text = text.replace("Exiting...", "")
    text = re.sub(r"(?:\r?\n)?\[end of text\]\s*$", "", text)
    return text.strip()


class LlamaStdioBackend:
    """Spawn one owned llama-cli child per inference and communicate by pipes."""

    name = "llama.cpp (owned anonymous stdio)"

    def __init__(
        self,
        exe: Path,
        model: Path,
        *,
        process_factory: Callable[..., object] | None = None,
        prompt_channel_factory: Callable[[bytes], PromptChannel] | None = None,
        timeout_s: int = DEFAULT_TIMEOUT_S,
    ) -> None:
        self.exe = exe
        self.model = model
        self.timeout_s = timeout_s
        self._process_factory = process_factory or subprocess.Popen
        self._prompt_channel_factory = prompt_channel_factory or secure_prompt_channel
        self._invoke_lock = threading.Lock()
        self._state_lock = threading.Lock()
        self._proc: object | None = None
        enforce_offline_environment()
        atexit.register(self.stop)

    def _spawn(self, command: list[str], stdin: object) -> object:
        enforce_offline_environment()
        proc = self._process_factory(
            command,
            stdin=stdin,
            stdout=subprocess.PIPE,
            stderr=subprocess.DEVNULL,
            shell=False,
            close_fds=True,
            env=offline_subprocess_environment(),
        )
        with self._state_lock:
            self._proc = proc
        return proc

    def _clear_process(self, proc: object) -> None:
        with self._state_lock:
            if self._proc is proc:
                self._proc = None

    @staticmethod
    def _returncode(proc: object) -> int | None:
        return getattr(proc, "returncode", None)

    @staticmethod
    def _terminate_process(proc: object) -> None:
        poll = getattr(proc, "poll", None)
        if callable(poll) and poll() is not None:
            return
        terminate = getattr(proc, "terminate", None)
        if callable(terminate):
            terminate()
        wait = getattr(proc, "wait", None)
        if callable(wait):
            try:
                wait(timeout=5)
                return
            except subprocess.TimeoutExpired:
                pass
        kill = getattr(proc, "kill", None)
        if callable(kill):
            kill()
        if callable(wait):
            try:
                wait(timeout=5)
            except subprocess.TimeoutExpired:
                pass

    def _communicate(
        self,
        proc: object,
        input_bytes: bytes | None,
    ) -> bytes | str:
        communicate = getattr(proc, "communicate")
        try:
            stdout, _ = communicate(input=input_bytes, timeout=self.timeout_s)
        except subprocess.TimeoutExpired as exc:
            self._terminate_process(proc)
            raise RuntimeError(LLM_TIMEOUT_MESSAGE) from exc
        if self._returncode(proc) not in (0, None):
            raise RuntimeError(LLM_FAILURE_MESSAGE)
        return stdout

    def _stream(
        self,
        proc: object,
        input_bytes: bytes | None,
        on_token: Callable[[str], None],
    ) -> str:
        stdin = getattr(proc, "stdin", None)
        stdout = getattr(proc, "stdout", None)
        wait = getattr(proc, "wait", None)
        if (
            stdout is None
            or not callable(wait)
            or (input_bytes is not None and stdin is None)
        ):
            raw = self._communicate(proc, input_bytes)
            answer = _clean_output(raw)
            if answer:
                on_token(answer)
            return answer

        timed_out = threading.Event()

        def expire() -> None:
            timed_out.set()
            self._terminate_process(proc)

        timer = threading.Timer(self.timeout_s, expire)
        timer.daemon = True
        timer.start()
        decoder = codecs.getincrementaldecoder("utf-8")(errors="replace")
        parts: list[str] = []
        callback_tail = ""

        def emit_safe(piece: str) -> None:
            nonlocal callback_tail
            callback_tail += piece
            if len(callback_tail) > 64:
                ready, callback_tail = callback_tail[:-64], callback_tail[-64:]
                if ready:
                    on_token(ready)

        try:
            if input_bytes is not None:
                stdin.write(input_bytes)
                stdin.close()
            while True:
                chunk = stdout.read(4096)
                if not chunk:
                    break
                piece = decoder.decode(chunk)
                if piece:
                    parts.append(piece)
                    emit_safe(piece)
            tail = decoder.decode(b"", final=True)
            if tail:
                parts.append(tail)
                emit_safe(tail)
            wait()
        except OSError as exc:
            if timed_out.is_set():
                raise RuntimeError(LLM_TIMEOUT_MESSAGE) from exc
            self._terminate_process(proc)
            raise RuntimeError(LLM_FAILURE_MESSAGE) from exc
        finally:
            timer.cancel()
        if timed_out.is_set():
            raise RuntimeError(LLM_TIMEOUT_MESSAGE)
        if self._returncode(proc) not in (0, None):
            raise RuntimeError(LLM_FAILURE_MESSAGE)
        clean_tail = _clean_output(callback_tail)
        if clean_tail:
            on_token(clean_tail)
        return _clean_output("".join(parts))

    def _execute(
        self,
        system: str,
        user: str,
        *,
        max_tokens: int,
        temperature: float,
        json_schema: dict | None,
        on_token: Callable[[str], None] | None,
    ) -> str:
        verify_artifact_path("llama_runtime", self.exe.parent)
        verify_artifact_path_by_prefix("gguf", self.model)
        prompt = _prompt_bytes(system, user)
        channel = self._prompt_channel_factory(prompt)
        command = llama_stdio_cmd(
            self.exe,
            self.model,
            prompt_file=channel.source,
            max_tokens=max_tokens,
            temperature=temperature,
            json_schema=json_schema,
        )
        with self._invoke_lock:
            proc: object | None = None
            try:
                proc = self._spawn(command, channel.stdin)
                channel.bind_client(getattr(proc, "pid", None))
                channel.start()
                if on_token is None:
                    answer = _clean_output(
                        self._communicate(proc, channel.input_bytes)
                    )
                else:
                    answer = self._stream(proc, channel.input_bytes, on_token)
                channel.finish(self.timeout_s)
                return answer
            except OSError as exc:
                if proc is not None:
                    self._terminate_process(proc)
                raise RuntimeError(LLM_FAILURE_MESSAGE) from exc
            finally:
                channel.close()
                if proc is not None:
                    self._clear_process(proc)

    def generate(
        self,
        system: str,
        user: str,
        max_tokens: int | None = None,
        on_token: Callable[[str], None] | None = None,
    ) -> str:
        gen = generation_params()
        return self._execute(
            system,
            user,
            max_tokens=max_tokens if max_tokens is not None else gen["max_tokens"],
            temperature=gen["temperature"],
            json_schema=None,
            on_token=on_token,
        )

    def generate_structured(
        self,
        system: str,
        user: str,
        json_schema: dict,
        max_tokens: int | None = None,
    ) -> str:
        gen = generation_params()
        try:
            return self._execute(
                system,
                user,
                max_tokens=(
                    max_tokens if max_tokens is not None else gen["max_tokens"]
                ),
                temperature=0.0,
                json_schema=json_schema,
                on_token=None,
            )
        except RuntimeError:
            return self.generate(system, user, max_tokens=max_tokens, on_token=None)

    def stop(self) -> None:
        with self._state_lock:
            proc = self._proc
            self._proc = None
        if proc is not None:
            self._terminate_process(proc)
