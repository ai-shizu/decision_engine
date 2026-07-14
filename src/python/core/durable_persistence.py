#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""Fail-closed reads and durable atomic writes for local persistence stores."""
from __future__ import annotations

import errno
import json
import os
import secrets
from pathlib import Path
from typing import Any


class PersistenceReadError(ValueError):
    """Existing persisted bytes could not be read or decoded safely."""


class PersistenceWriteError(OSError):
    """A durable atomic write could not be completed."""


def read_json_file(path: Path) -> Any:
    """Read JSON without conflating absence with corruption or I/O failure."""
    target = Path(path)
    try:
        raw = target.read_bytes()
    except FileNotFoundError:
        raise
    except OSError as exc:
        raise PersistenceReadError("persisted data read failed") from exc
    try:
        text = raw.decode("utf-8")
        return json.loads(text)
    except (UnicodeDecodeError, json.JSONDecodeError) as exc:
        raise PersistenceReadError("persisted JSON is corrupt") from exc


def _open_exclusive_temp(target: Path) -> tuple[int, Path]:
    flags = os.O_WRONLY | os.O_CREAT | os.O_EXCL
    flags |= getattr(os, "O_CLOEXEC", 0)
    flags |= getattr(os, "O_NOFOLLOW", 0)
    for _ in range(16):
        candidate = target.parent / (
            f".{target.name}.{secrets.token_hex(16)}.tmp"
        )
        try:
            return os.open(candidate, flags, 0o600), candidate
        except FileExistsError:
            continue
    raise PersistenceWriteError("could not allocate exclusive temporary file")


def _fsync_parent_directory(parent: Path) -> None:
    if os.name == "nt":
        return
    flags = os.O_RDONLY | getattr(os, "O_DIRECTORY", 0)
    flags |= getattr(os, "O_CLOEXEC", 0)
    try:
        directory_fd = os.open(parent, flags)
    except OSError as exc:
        if exc.errno in {errno.EINVAL, errno.ENOTSUP}:
            return
        raise
    try:
        os.fsync(directory_fd)
    finally:
        os.close(directory_fd)


def durable_atomic_write(path: Path, payload: bytes) -> None:
    """Atomically replace ``path`` only after file data reaches the kernel."""
    if type(payload) is not bytes:
        raise TypeError("payload must be bytes")
    target = Path(path)
    temp_path: Path | None = None
    try:
        target.parent.mkdir(parents=True, exist_ok=True)
        fd, temp_path = _open_exclusive_temp(target)
        with os.fdopen(fd, "wb") as handle:
            handle.write(payload)
            handle.flush()
            os.fsync(handle.fileno())
        os.replace(temp_path, target)
        temp_path = None
        _fsync_parent_directory(target.parent)
    except PersistenceWriteError:
        raise
    except OSError as exc:
        raise PersistenceWriteError("durable atomic write failed") from exc
    finally:
        if temp_path is not None:
            try:
                temp_path.unlink(missing_ok=True)
            except OSError:
                pass


def durable_atomic_write_text(path: Path, text: str) -> None:
    if type(text) is not str:
        raise TypeError("text must be str")
    durable_atomic_write(Path(path), text.encode("utf-8"))
