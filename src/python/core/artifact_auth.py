#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""Rust-attested artifact allowlist enforcement for production Python paths."""
from __future__ import annotations

import hashlib
import json
import os
from pathlib import Path
import stat
import sys
from typing import Any


MANIFEST_SCHEMA = "pkb.artifact_allowlist.v1"
MAX_MANIFEST_BYTES = 1024 * 1024
TREE_DOMAIN = b"PKB_TREE_V1\0"
_ENTRY_KEYS = {"base", "id", "kind", "path", "sha256", "size"}
_MANIFEST_KEYS = {"artifacts", "key_id", "schema"}


class ArtifactIntegrityError(RuntimeError):
    """Fixed-shape hard failure at an authenticated artifact boundary."""


def _fail() -> None:
    raise ArtifactIntegrityError("artifact integrity verification failed")


def artifact_auth_required() -> bool:
    return bool(getattr(sys, "frozen", False)) or os.environ.get(
        "PKB_REQUIRE_ARTIFACT_AUTH"
    ) == "1"


def _reject_link(path: Path) -> None:
    absolute = path.absolute()
    current = Path(absolute.anchor)
    for part in absolute.parts[1:]:
        current = current / part
        try:
            metadata = current.lstat()
        except OSError:
            _fail()
        if stat.S_ISLNK(metadata.st_mode):
            _fail()
        is_junction = getattr(current, "is_junction", None)
        if callable(is_junction) and is_junction():
            _fail()


def _read_manifest(path: Path) -> bytes:
    _reject_link(path)
    try:
        metadata = path.stat()
        if not path.is_file() or metadata.st_size > MAX_MANIFEST_BYTES:
            _fail()
        return path.read_bytes()
    except (OSError, ArtifactIntegrityError):
        _fail()


def _validate_json_types(value: Any) -> None:
    if value is None or isinstance(value, bool) or isinstance(value, float):
        _fail()
    if isinstance(value, int):
        if value < 0:
            _fail()
        return
    if isinstance(value, str):
        return
    if isinstance(value, list):
        for item in value:
            _validate_json_types(item)
        return
    if isinstance(value, dict):
        if not all(isinstance(key, str) for key in value):
            _fail()
        for item in value.values():
            _validate_json_types(item)
        return
    _fail()


def _canonical_bytes(value: Any) -> bytes:
    _validate_json_types(value)
    try:
        return json.dumps(
            value,
            ensure_ascii=False,
            sort_keys=True,
            separators=(",", ":"),
        ).encode("utf-8")
    except (TypeError, ValueError):
        _fail()


def _valid_hex(raw: Any, length: int) -> bool:
    return (
        isinstance(raw, str)
        and len(raw) == length
        and raw == raw.lower()
        and all(char in "0123456789abcdef" for char in raw)
    )


def _validate_relative_path(raw: Any) -> str:
    if (
        not isinstance(raw, str)
        or not raw
        or "\\" in raw
        or ":" in raw
        or raw.startswith("/")
        or raw.endswith("/")
        or any(ord(char) < 0x20 or ord(char) == 0x7F for char in raw)
    ):
        _fail()
    parts = raw.split("/")
    if any(not part or part in {".", ".."} for part in parts):
        _fail()
    return raw


def _load_attested_manifest() -> tuple[dict[str, Any], Path, Path]:
    manifest_raw = os.environ.get("PKB_ARTIFACT_MANIFEST")
    digest = os.environ.get("PKB_ARTIFACT_MANIFEST_SHA256")
    app_raw = os.environ.get("PKB_ARTIFACT_APP_ROOT")
    data_raw = os.environ.get("PKB_PROJECT_ROOT")
    if not manifest_raw or not app_raw or not data_raw or not _valid_hex(digest, 64):
        _fail()
    manifest_path = Path(manifest_raw)
    encoded = _read_manifest(manifest_path)
    if hashlib.sha256(encoded).hexdigest() != digest:
        _fail()
    try:
        payload = json.loads(encoded)
    except (UnicodeDecodeError, json.JSONDecodeError):
        _fail()
    if _canonical_bytes(payload) != encoded or not isinstance(payload, dict):
        _fail()
    if set(payload) != _MANIFEST_KEYS:
        _fail()
    if payload.get("schema") != MANIFEST_SCHEMA:
        _fail()
    key_id = payload.get("key_id")
    artifacts = payload.get("artifacts")
    if not isinstance(key_id, str) or not key_id or not isinstance(artifacts, list):
        _fail()
    if not artifacts or len(artifacts) > 4096:
        _fail()

    previous = ""
    seen: set[str] = set()
    for entry in artifacts:
        if not isinstance(entry, dict) or set(entry) != _ENTRY_KEYS:
            _fail()
        artifact_id = entry.get("id")
        if (
            not isinstance(artifact_id, str)
            or not artifact_id
            or len(artifact_id) > 128
            or not all(char.isascii() and (char.isalnum() or char in "._:-") for char in artifact_id)
            or artifact_id in seen
            or artifact_id <= previous
        ):
            _fail()
        previous = artifact_id
        seen.add(artifact_id)
        if entry.get("base") not in {"app", "data"} or entry.get("kind") not in {
            "file",
            "tree",
        }:
            _fail()
        _validate_relative_path(entry.get("path"))
        if not _valid_hex(entry.get("sha256"), 64):
            _fail()
        size = entry.get("size")
        if isinstance(size, bool) or not isinstance(size, int) or size < 0:
            _fail()
    return payload, Path(app_raw), Path(data_raw)


def _entry_path(entry: dict[str, Any], app_root: Path, data_root: Path) -> Path:
    base = app_root if entry["base"] == "app" else data_root
    path = base
    for part in _validate_relative_path(entry["path"]).split("/"):
        path = path / part
    return path


def _hash_file(path: Path) -> tuple[str, int]:
    _reject_link(path)
    try:
        metadata = path.stat()
        if not path.is_file():
            _fail()
        hasher = hashlib.sha256()
        with path.open("rb") as handle:
            while chunk := handle.read(1024 * 1024):
                hasher.update(chunk)
        return hasher.hexdigest(), metadata.st_size
    except (OSError, ArtifactIntegrityError):
        _fail()


def _tree_files(root: Path) -> list[tuple[str, Path, int]]:
    _reject_link(root)
    try:
        if not root.is_dir():
            _fail()
        output: list[tuple[str, Path, int]] = []
        for path in sorted(root.rglob("*"), key=lambda item: item.relative_to(root).as_posix()):
            _reject_link(path)
            if path.is_dir():
                continue
            if not path.is_file():
                _fail()
            relative = path.relative_to(root).as_posix()
            _validate_relative_path(relative)
            output.append((relative, path, path.stat().st_size))
        if not output:
            _fail()
        return output
    except (OSError, ValueError, ArtifactIntegrityError):
        _fail()


def _hash_tree(root: Path) -> tuple[str, int]:
    hasher = hashlib.sha256()
    hasher.update(TREE_DOMAIN)
    total_size = 0
    for relative, path, size in _tree_files(root):
        encoded = relative.encode("utf-8")
        hasher.update(len(encoded).to_bytes(8, "big"))
        hasher.update(encoded)
        hasher.update(size.to_bytes(8, "big"))
        digest, actual_size = _hash_file(path)
        if actual_size != size:
            _fail()
        hasher.update(bytes.fromhex(digest))
        total_size += size
    return hasher.hexdigest(), total_size


def _verify_entry(
    entry: dict[str, Any],
    app_root: Path,
    data_root: Path,
) -> Path:
    path = _entry_path(entry, app_root, data_root)
    digest, size = _hash_file(path) if entry["kind"] == "file" else _hash_tree(path)
    if digest != entry["sha256"] or size != entry["size"]:
        _fail()
    return path


def _same_path(left: Path, right: Path) -> bool:
    _reject_link(left)
    _reject_link(right)
    try:
        return left.samefile(right)
    except OSError:
        _fail()


def verify_artifact_path(artifact_id: str, path: Path | str) -> Path:
    candidate = Path(path)
    if not artifact_auth_required():
        return candidate
    payload, app_root, data_root = _load_attested_manifest()
    for entry in payload["artifacts"]:
        if entry["id"] == artifact_id:
            expected = _verify_entry(entry, app_root, data_root)
            if not _same_path(expected, candidate):
                _fail()
            return candidate
    _fail()


def verify_artifact_path_by_prefix(prefix: str, path: Path | str) -> Path:
    candidate = Path(path)
    if not artifact_auth_required():
        return candidate
    payload, app_root, data_root = _load_attested_manifest()
    for entry in payload["artifacts"]:
        if entry["id"] == prefix or entry["id"].startswith(prefix + ":"):
            expected = _entry_path(entry, app_root, data_root)
            if _same_path(expected, candidate):
                _verify_entry(entry, app_root, data_root)
                return candidate
    _fail()


def verified_artifact_paths(prefix: str) -> list[tuple[str, Path]]:
    if not artifact_auth_required():
        return []
    payload, app_root, data_root = _load_attested_manifest()
    output: list[tuple[str, Path]] = []
    for entry in payload["artifacts"]:
        if entry["id"] == prefix or entry["id"].startswith(prefix + ":"):
            output.append((entry["id"], _verify_entry(entry, app_root, data_root)))
    if not output:
        _fail()
    return output
