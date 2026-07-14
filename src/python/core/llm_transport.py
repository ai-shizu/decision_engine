"""Private prompt channels for the local llama.cpp child process."""
from __future__ import annotations

import os
import re
import secrets
import subprocess
import threading
from pathlib import Path
from typing import Protocol


PROMPT_CHANNEL_FAILURE = "Local LLM prompt channel failed."


class PromptChannel(Protocol):
    source: str
    stdin: object
    input_bytes: bytes | None

    def bind_client(self, process_id: int | None) -> None: ...

    def start(self) -> None: ...

    def finish(self, timeout_s: float) -> None: ...

    def close(self) -> None: ...


class StdioPromptChannel:
    """Use /dev/stdin as llama.cpp's prompt file on POSIX systems."""

    source = "/dev/stdin"
    stdin = subprocess.PIPE

    def __init__(self, prompt: bytes) -> None:
        self.input_bytes = prompt

    def bind_client(self, process_id: int | None) -> None:  # noqa: ARG002
        return None

    def start(self) -> None:
        return None

    def finish(self, timeout_s: float) -> None:  # noqa: ARG002
        return None

    def close(self) -> None:
        return None


if os.name == "nt":
    import ctypes
    from ctypes import wintypes

    PIPE_ACCESS_OUTBOUND = 0x00000002
    FILE_FLAG_FIRST_PIPE_INSTANCE = 0x00080000
    PIPE_TYPE_BYTE = 0x00000000
    PIPE_READMODE_BYTE = 0x00000000
    PIPE_WAIT = 0x00000000
    PIPE_REJECT_REMOTE_CLIENTS = 0x00000008
    ERROR_PIPE_CONNECTED = 535
    SDDL_REVISION_1 = 1
    OWNER_ONLY_PIPE_SDDL = "D:P(A;;GA;;;OW)(A;;GA;;;SY)"
    _APPCONTAINER_SID = re.compile(r"S-1-15-2(?:-[0-9]{1,10}){7}\Z")

    def _pipe_security_descriptor() -> tuple[str, bool]:
        sid = os.environ.get("PKB_APPCONTAINER_SID")
        if sid is None:
            return OWNER_ONLY_PIPE_SDDL, False
        if _APPCONTAINER_SID.fullmatch(sid) is None:
            raise RuntimeError(PROMPT_CHANNEL_FAILURE)
        return f"{OWNER_ONLY_PIPE_SDDL}(A;;GA;;;{sid})", True

    class _SecurityAttributes(ctypes.Structure):
        _fields_ = [
            ("nLength", wintypes.DWORD),
            ("lpSecurityDescriptor", wintypes.LPVOID),
            ("bInheritHandle", wintypes.BOOL),
        ]

    _kernel32 = ctypes.WinDLL("kernel32", use_last_error=True)
    _advapi32 = ctypes.WinDLL("advapi32", use_last_error=True)

    _advapi32.ConvertStringSecurityDescriptorToSecurityDescriptorW.argtypes = [
        wintypes.LPCWSTR,
        wintypes.DWORD,
        ctypes.POINTER(wintypes.LPVOID),
        ctypes.POINTER(wintypes.ULONG),
    ]
    _advapi32.ConvertStringSecurityDescriptorToSecurityDescriptorW.restype = wintypes.BOOL
    _kernel32.CreateNamedPipeW.argtypes = [
        wintypes.LPCWSTR,
        wintypes.DWORD,
        wintypes.DWORD,
        wintypes.DWORD,
        wintypes.DWORD,
        wintypes.DWORD,
        wintypes.DWORD,
        ctypes.POINTER(_SecurityAttributes),
    ]
    _kernel32.CreateNamedPipeW.restype = wintypes.HANDLE
    _kernel32.ConnectNamedPipe.argtypes = [wintypes.HANDLE, wintypes.LPVOID]
    _kernel32.ConnectNamedPipe.restype = wintypes.BOOL
    _kernel32.GetNamedPipeClientProcessId.argtypes = [
        wintypes.HANDLE,
        ctypes.POINTER(wintypes.ULONG),
    ]
    _kernel32.GetNamedPipeClientProcessId.restype = wintypes.BOOL
    _kernel32.WriteFile.argtypes = [
        wintypes.HANDLE,
        wintypes.LPCVOID,
        wintypes.DWORD,
        ctypes.POINTER(wintypes.DWORD),
        wintypes.LPVOID,
    ]
    _kernel32.WriteFile.restype = wintypes.BOOL
    _kernel32.FlushFileBuffers.argtypes = [wintypes.HANDLE]
    _kernel32.FlushFileBuffers.restype = wintypes.BOOL
    _kernel32.DisconnectNamedPipe.argtypes = [wintypes.HANDLE]
    _kernel32.DisconnectNamedPipe.restype = wintypes.BOOL
    _kernel32.CancelIoEx.argtypes = [wintypes.HANDLE, wintypes.LPVOID]
    _kernel32.CancelIoEx.restype = wintypes.BOOL
    _kernel32.CloseHandle.argtypes = [wintypes.HANDLE]
    _kernel32.CloseHandle.restype = wintypes.BOOL
    _kernel32.LocalFree.argtypes = [wintypes.HLOCAL]
    _kernel32.LocalFree.restype = wintypes.HLOCAL

    _INVALID_HANDLE_VALUE = ctypes.c_void_p(-1).value


    class WindowsNamedPipePromptChannel:
        """One-use local byte pipe with a protected owner-only DACL."""

        stdin = subprocess.DEVNULL
        input_bytes = None

        def __init__(self, prompt: bytes) -> None:
            descriptor, appcontainer = _pipe_security_descriptor()
            namespace = "LOCAL\\" if appcontainer else ""
            self.source = rf"\\.\pipe\{namespace}pkb-llm-{os.getpid()}-{secrets.token_hex(16)}"
            self._security_descriptor = descriptor
            self._prompt = prompt
            self._handle: int | None = None
            self._thread: threading.Thread | None = None
            self._error: BaseException | None = None
            self._expected_client_pid: int | None = None
            self._handle_lock = threading.Lock()
            self._create_pipe()

        def bind_client(self, process_id: int | None) -> None:
            if (
                not isinstance(process_id, int)
                or process_id <= 0
                or self._expected_client_pid is not None
            ):
                raise RuntimeError(PROMPT_CHANNEL_FAILURE)
            self._expected_client_pid = process_id

        def _create_pipe(self) -> None:
            descriptor = wintypes.LPVOID()
            if not _advapi32.ConvertStringSecurityDescriptorToSecurityDescriptorW(
                self._security_descriptor,
                SDDL_REVISION_1,
                ctypes.byref(descriptor),
                None,
            ):
                raise OSError(ctypes.get_last_error(), PROMPT_CHANNEL_FAILURE)
            attributes = _SecurityAttributes(
                ctypes.sizeof(_SecurityAttributes),
                descriptor,
                False,
            )
            try:
                handle = _kernel32.CreateNamedPipeW(
                    self.source,
                    PIPE_ACCESS_OUTBOUND | FILE_FLAG_FIRST_PIPE_INSTANCE,
                    PIPE_TYPE_BYTE
                    | PIPE_READMODE_BYTE
                    | PIPE_WAIT
                    | PIPE_REJECT_REMOTE_CLIENTS,
                    1,
                    64 * 1024,
                    64 * 1024,
                    0,
                    ctypes.byref(attributes),
                )
            finally:
                _kernel32.LocalFree(descriptor)
            if handle == _INVALID_HANDLE_VALUE:
                raise OSError(ctypes.get_last_error(), PROMPT_CHANNEL_FAILURE)
            self._handle = handle

        def _take_handle(self) -> int | None:
            with self._handle_lock:
                handle = self._handle
                self._handle = None
                return handle

        def _write_prompt(self) -> None:
            handle = self._handle
            if handle is None:
                return
            try:
                connected = _kernel32.ConnectNamedPipe(handle, None)
                if not connected and ctypes.get_last_error() != ERROR_PIPE_CONNECTED:
                    raise OSError(ctypes.get_last_error(), PROMPT_CHANNEL_FAILURE)
                client_pid = wintypes.ULONG()
                if not _kernel32.GetNamedPipeClientProcessId(
                    handle,
                    ctypes.byref(client_pid),
                ):
                    raise OSError(ctypes.get_last_error(), PROMPT_CHANNEL_FAILURE)
                if client_pid.value != self._expected_client_pid:
                    raise PermissionError(PROMPT_CHANNEL_FAILURE)
                offset = 0
                while offset < len(self._prompt):
                    chunk = self._prompt[offset : offset + 64 * 1024]
                    written = wintypes.DWORD()
                    buffer = ctypes.create_string_buffer(chunk)
                    if not _kernel32.WriteFile(
                        handle,
                        buffer,
                        len(chunk),
                        ctypes.byref(written),
                        None,
                    ):
                        raise OSError(ctypes.get_last_error(), PROMPT_CHANNEL_FAILURE)
                    if written.value == 0:
                        raise OSError(PROMPT_CHANNEL_FAILURE)
                    offset += written.value
                _kernel32.FlushFileBuffers(handle)
            except BaseException as exc:  # delivered to the invoking thread
                self._error = exc
            finally:
                self._prompt = b""
                owned = self._take_handle()
                if owned is not None:
                    _kernel32.DisconnectNamedPipe(owned)
                    _kernel32.CloseHandle(owned)

        def start(self) -> None:
            if self._expected_client_pid is None:
                raise RuntimeError(PROMPT_CHANNEL_FAILURE)
            self._thread = threading.Thread(
                target=self._write_prompt,
                name="pkb-llm-prompt-pipe",
                daemon=True,
            )
            self._thread.start()

        def finish(self, timeout_s: float) -> None:
            if self._thread is None:
                return
            self._thread.join(timeout=timeout_s)
            if self._thread.is_alive():
                self.close()
                self._thread.join(timeout=5)
                raise RuntimeError(PROMPT_CHANNEL_FAILURE)
            if self._error is not None:
                raise RuntimeError(PROMPT_CHANNEL_FAILURE) from self._error

        def close(self) -> None:
            self._prompt = b""
            handle = self._take_handle()
            if handle is not None:
                _kernel32.CancelIoEx(handle, None)
                _kernel32.DisconnectNamedPipe(handle)
                _kernel32.CloseHandle(handle)


def secure_prompt_channel(prompt: bytes) -> PromptChannel:
    if os.name == "nt":
        return WindowsNamedPipePromptChannel(prompt)
    if not Path("/dev/stdin").exists():
        raise RuntimeError(PROMPT_CHANNEL_FAILURE)
    return StdioPromptChannel(prompt)
