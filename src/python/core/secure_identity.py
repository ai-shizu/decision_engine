"""OS-protected root key and domain-separated persistent identities."""
from __future__ import annotations

import contextlib
import ctypes
import hashlib
import hmac
import os
import re
import secrets
from functools import lru_cache
from pathlib import Path
from typing import Iterator

from .canonicalization import canonicalize_text
from .durable_persistence import durable_atomic_write
from .paths import DATA_PROCESSED

_KEY_ENV = "PKB_IDENTITY_ROOT_KEY_HEX"
_KEY_PATH = DATA_PROCESSED / "identity_root_key.dpapi"
_LOCK_PATH = DATA_PROCESSED / ".identity_root_key.lock"
_BLOB_MAGIC = b"PKBIDKEY1\x00"
_DPAPI_ENTROPY = b"decision-engine/identity-root-key/v1"
_KEY_BYTES = 32
_CONTACT_ID_RE = re.compile(r"^C-[0-9a-f]{64}$")
_CONTACT_SHORT_RE = re.compile(r"^C~[0-9a-f]{12}$")


class IdentityKeyError(RuntimeError):
    """The persistent identity root key is absent, corrupt, or unavailable."""


class _DataBlob(ctypes.Structure):
    _fields_ = [
        ("cbData", ctypes.c_uint32),
        ("pbData", ctypes.POINTER(ctypes.c_ubyte)),
    ]


def _input_blob(data: bytes) -> tuple[_DataBlob, ctypes.Array]:
    buffer = (ctypes.c_ubyte * len(data)).from_buffer_copy(data)
    return _DataBlob(len(data), buffer), buffer


def _dpapi_transform(payload: bytes, *, protect: bool) -> bytes:
    if os.name != "nt":
        raise IdentityKeyError("DPAPI is available only on Windows")
    crypt32 = ctypes.WinDLL("crypt32", use_last_error=True)
    kernel32 = ctypes.WinDLL("kernel32", use_last_error=True)
    input_blob, input_buffer = _input_blob(payload)
    entropy_blob, entropy_buffer = _input_blob(_DPAPI_ENTROPY)
    output_blob = _DataBlob()
    keepalive = (input_buffer, entropy_buffer)

    if protect:
        function = crypt32.CryptProtectData
        function.argtypes = [
            ctypes.POINTER(_DataBlob),
            ctypes.c_wchar_p,
            ctypes.POINTER(_DataBlob),
            ctypes.c_void_p,
            ctypes.c_void_p,
            ctypes.c_uint32,
            ctypes.POINTER(_DataBlob),
        ]
        args = (
            ctypes.byref(input_blob),
            "Decision Engine persistent identity root",
            ctypes.byref(entropy_blob),
            None,
            None,
            0x1,
            ctypes.byref(output_blob),
        )
    else:
        function = crypt32.CryptUnprotectData
        function.argtypes = [
            ctypes.POINTER(_DataBlob),
            ctypes.c_void_p,
            ctypes.POINTER(_DataBlob),
            ctypes.c_void_p,
            ctypes.c_void_p,
            ctypes.c_uint32,
            ctypes.POINTER(_DataBlob),
        ]
        args = (
            ctypes.byref(input_blob),
            None,
            ctypes.byref(entropy_blob),
            None,
            None,
            0x1,
            ctypes.byref(output_blob),
        )
    function.restype = ctypes.c_int
    if not function(*args):
        error = ctypes.get_last_error()
        raise IdentityKeyError(f"DPAPI operation failed with Win32 {error}")
    try:
        del keepalive
        return ctypes.string_at(output_blob.pbData, output_blob.cbData)
    finally:
        kernel32.LocalFree.argtypes = [ctypes.c_void_p]
        kernel32.LocalFree.restype = ctypes.c_void_p
        kernel32.LocalFree(ctypes.cast(output_blob.pbData, ctypes.c_void_p))


@contextlib.contextmanager
def _creation_lock() -> Iterator[None]:
    _LOCK_PATH.parent.mkdir(parents=True, exist_ok=True)
    flags = os.O_RDWR | os.O_CREAT
    flags |= getattr(os, "O_CLOEXEC", 0)
    flags |= getattr(os, "O_NOFOLLOW", 0)
    try:
        fd = os.open(_LOCK_PATH, flags, 0o600)
    except OSError as exc:
        raise IdentityKeyError("identity key lock is unavailable") from exc
    with os.fdopen(fd, "a+b", buffering=0) as handle:
        if handle.seek(0, os.SEEK_END) == 0:
            handle.write(b"\0")
            os.fsync(handle.fileno())
        handle.seek(0)
        if os.name == "nt":
            import msvcrt

            msvcrt.locking(handle.fileno(), msvcrt.LK_LOCK, 1)
            try:
                yield
            finally:
                handle.seek(0)
                msvcrt.locking(handle.fileno(), msvcrt.LK_UNLCK, 1)
        else:
            import fcntl

            fcntl.flock(handle.fileno(), fcntl.LOCK_EX)
            try:
                yield
            finally:
                fcntl.flock(handle.fileno(), fcntl.LOCK_UN)


def _read_dpapi_key() -> bytes:
    if _KEY_PATH.is_symlink():
        raise IdentityKeyError("identity key symlinks are forbidden")
    try:
        blob = _KEY_PATH.read_bytes()
    except FileNotFoundError:
        raise
    except OSError as exc:
        raise IdentityKeyError("identity key cannot be read") from exc
    if not blob.startswith(_BLOB_MAGIC):
        raise IdentityKeyError("identity key blob format is invalid")
    key = _dpapi_transform(blob[len(_BLOB_MAGIC):], protect=False)
    if len(key) != _KEY_BYTES:
        raise IdentityKeyError("identity root key length is invalid")
    return key


def _environment_key() -> bytes | None:
    encoded = os.environ.get(_KEY_ENV)
    if encoded is None:
        return None
    if len(encoded) != _KEY_BYTES * 2:
        raise IdentityKeyError(f"{_KEY_ENV} must be 64 lowercase hex chars")
    try:
        key = bytes.fromhex(encoded)
    except ValueError as exc:
        raise IdentityKeyError(f"{_KEY_ENV} must be 64 lowercase hex chars") from exc
    if encoded != encoded.lower() or len(key) != _KEY_BYTES:
        raise IdentityKeyError(f"{_KEY_ENV} must be 64 lowercase hex chars")
    return key


@lru_cache(maxsize=1)
def identity_root_key() -> bytes:
    """Load the managed root key without ever accepting plaintext key files."""
    environment_key = _environment_key()
    if environment_key is not None:
        return environment_key
    if os.name != "nt":
        raise IdentityKeyError(
            f"non-Windows runtimes require {_KEY_ENV} from a secure key provider"
        )
    with _creation_lock():
        try:
            return _read_dpapi_key()
        except FileNotFoundError:
            key = secrets.token_bytes(_KEY_BYTES)
            protected = _dpapi_transform(key, protect=True)
            durable_atomic_write(_KEY_PATH, _BLOB_MAGIC + protected)
            persisted = _read_dpapi_key()
            if not hmac.compare_digest(key, persisted):
                raise IdentityKeyError("persisted identity key verification failed")
            return persisted


def keyed_text_identity(text: str, *, domain: bytes) -> bytes:
    """Return a 256-bit HMAC identity over canonical text and a fixed domain."""
    if type(domain) is not bytes or not domain or b"\0" in domain:
        raise ValueError("identity domain must be non-empty bytes without NUL")
    canonical = canonicalize_text(text)
    if not canonical:
        raise ValueError("identity text must be non-empty")
    return hmac.new(
        identity_root_key(),
        domain + b"\0" + canonical.encode("utf-8"),
        hashlib.sha256,
    ).digest()


def validate_contact_identity(identity: str) -> str:
    if type(identity) is not str or not _CONTACT_ID_RE.fullmatch(identity):
        raise ValueError("contact identity must be C- plus 64 lowercase hex chars")
    return identity


def contact_short_id(identity: str) -> str:
    """Return a display-only label that must never be used for matching."""
    persistent = validate_contact_identity(identity)
    short = "C~" + persistent[2:14]
    assert _CONTACT_SHORT_RE.fullmatch(short)
    return short
