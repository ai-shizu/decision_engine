"""Process-wide environment and socket hardening for PKB's offline runtime."""
from __future__ import annotations

import os
import sys


NO_PROXY_ALL = "*"
NETWORK_DENIED_MESSAGE = "PKB network capability disabled"

# AF numbers are platform constants. We MUST NOT `import socket` here to derive
# them: the audit hook exists to deny socket creation, and importing socket
# (or touching socket.AF_*) can itself trigger socket-side init / lazy C API
# load on some builds — defeating the guard's "before any network family"
# posture. Keep this table in sync when adding a platform.
#
#   AF_INET  = 2          (all common platforms)
#   AF_INET6 = 10         (Linux)
#   AF_INET6 = 23         (Windows / CPython-Win)
#   AF_INET6 = 30         (Darwin / BSD — macOS developer host; was missing)
_IP_SOCKET_FAMILIES = frozenset({2, 10, 23, 30})
_DNS_AUDIT_EVENTS = frozenset(
    {
        "socket.getaddrinfo",
        "socket.gethostbyaddr",
        "socket.gethostbyname",
        "socket.gethostbyname_ex",
        "socket.getnameinfo",
    }
)
_audit_hook_installed = False
_event_loop_policy_installed = False

OFFLINE_ENV: dict[str, str] = {
    "HF_HUB_OFFLINE": "1",
    "TRANSFORMERS_OFFLINE": "1",
    "HF_DATASETS_OFFLINE": "1",
    "HF_HUB_DISABLE_TELEMETRY": "1",
    "DO_NOT_TRACK": "1",
    "LLAMA_ARG_OFFLINE": "1",
}

SAFE_CHILD_ENV = (
    "APPDATA",
    "HOME",
    "HOMEDRIVE",
    "HOMEPATH",
    "LANG",
    "LC_ALL",
    "LC_CTYPE",
    "LOCALAPPDATA",
    "PROGRAMDATA",
    "SystemRoot",
    "TEMP",
    "TMP",
    "TMPDIR",
    "TZ",
    "USERPROFILE",
    "WINDIR",
)

SENSITIVE_ENV = (
    "HTTP_PROXY",
    "HTTPS_PROXY",
    "ALL_PROXY",
    "http_proxy",
    "https_proxy",
    "all_proxy",
    "HF_TOKEN",
    "HUGGING_FACE_HUB_TOKEN",
    "LLAMA_ARG_RPC",
    "LLAMA_ARG_MODEL_URL",
    "LLAMA_ARG_DOCKER_REPO",
    "LLAMA_ARG_HF_REPO",
    "LLAMA_ARG_HF_FILE",
    "LLAMA_ARG_HF_REPO_V",
    "LLAMA_ARG_HF_FILE_V",
    "LLAMA_ARG_LOG_FILE",
)


def _deny_network_audit_event(event: str, args: tuple[object, ...]) -> None:
    if event == "socket.__new__":
        family = args[1] if len(args) > 1 else None
        if family in _IP_SOCKET_FAMILIES:
            raise PermissionError(NETWORK_DENIED_MESSAGE)
    elif event in _DNS_AUDIT_EVENTS:
        raise PermissionError(NETWORK_DENIED_MESSAGE)


def _install_networkless_windows_event_loop_policy() -> None:
    """Wake the Windows Proactor through a kernel event, never an IP socketpair."""
    global _event_loop_policy_installed

    if os.name != "nt" or _event_loop_policy_installed:
        return
    import _overlapped
    import _winapi
    import asyncio
    from asyncio import base_events, windows_events

    class NetworklessProactorEventLoop(windows_events.ProactorEventLoop):
        def __init__(self) -> None:
            base_events.BaseEventLoop.__init__(self)
            self._proactor = windows_events.IocpProactor()
            self._selector = self._proactor
            self._self_reading_future = None
            self._accept_futures = {}
            self._ssock = None
            self._csock = None
            self._wake_event = None
            self._wake_event = _overlapped.CreateEvent(None, False, False, None)
            self._proactor.set_loop(self)

        def _loop_self_reading(self, future=None) -> None:
            try:
                if future is not None:
                    future.result()
                if self._self_reading_future is not future:
                    return
                future = self._proactor.wait_for_handle(self._wake_event)
            except asyncio.CancelledError:
                return
            except (SystemExit, KeyboardInterrupt):
                raise
            except BaseException as exc:
                self.call_exception_handler(
                    {
                        "message": "Error while waiting on the event loop wake event",
                        "exception": exc,
                        "loop": self,
                    }
                )
            else:
                self._self_reading_future = future
                future.add_done_callback(self._loop_self_reading)

        def _write_to_self(self) -> None:
            wake_event = self._wake_event
            if wake_event is None:
                return
            try:
                _overlapped.SetEvent(wake_event)
            except OSError:
                if self._debug:
                    raise

        def _make_self_pipe(self) -> None:
            return None

        def _close_self_pipe(self) -> None:
            if self._self_reading_future is not None:
                self._self_reading_future.cancel()
                self._self_reading_future = None
            if self._wake_event is not None:
                _winapi.CloseHandle(self._wake_event)
                self._wake_event = None
            self._ssock = None
            self._csock = None

    class NetworklessWindowsPolicy(asyncio.WindowsProactorEventLoopPolicy):
        _loop_factory = NetworklessProactorEventLoop

    asyncio.set_event_loop_policy(NetworklessWindowsPolicy())
    _event_loop_policy_installed = True


def enforce_offline_environment() -> None:
    """Remove inherited egress configuration and deny Python IP sockets."""
    global _audit_hook_installed

    for key, value in OFFLINE_ENV.items():
        os.environ[key] = value
    for key in SENSITIVE_ENV:
        os.environ.pop(key, None)
    os.environ["NO_PROXY"] = NO_PROXY_ALL
    os.environ["no_proxy"] = NO_PROXY_ALL
    if not _audit_hook_installed:
        sys.addaudithook(_deny_network_audit_event)
        _audit_hook_installed = True
    _install_networkless_windows_event_loop_policy()


def offline_subprocess_environment() -> dict[str, str]:
    """Build a minimal environment for owned local inference children."""
    environment = {
        key: os.environ[key]
        for key in SAFE_CHILD_ENV
        if key in os.environ
    }
    environment.update(OFFLINE_ENV)
    environment["NO_PROXY"] = NO_PROXY_ALL
    environment["no_proxy"] = NO_PROXY_ALL
    return environment
