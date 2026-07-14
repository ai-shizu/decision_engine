"""Cryptographic identities for inference runtimes and session genesis."""

from __future__ import annotations

import hashlib
import json
import math
import platform
import re
import sys
from dataclasses import dataclass
from functools import lru_cache
from pathlib import Path
from typing import Any


_HEX64 = re.compile(r"^[0-9a-f]{64}$")
_HEX128 = re.compile(r"^[0-9a-f]{128}$")
_RUNTIME_COMPONENT_KEYS = frozenset(
    {
        "gguf_hash",
        "llama_server_hash",
        "embedding_model_version",
        "prompt_text",
        "json_schema",
        "generation_params",
        "numeric_runtime_version",
    }
)


def _nonempty_text(value: Any, field: str) -> str:
    if type(value) is not str or not value.strip():
        raise ValueError(f"{field} must be non-empty str")
    return value


def _sha256_digest(value: Any, field: str) -> str:
    text = _nonempty_text(value, field)
    if not _HEX64.fullmatch(text):
        raise ValueError(f"{field} must be 64-char lowercase hex")
    return text


def validate_runtime_digest(value: Any) -> str:
    if type(value) is not str or not _HEX128.fullmatch(value):
        raise ValueError(
            "canonical runtime identity must be 128-char lowercase hex"
        )
    return value


def _validate_json_value(value: Any, path: str) -> None:
    if value is None or type(value) in {str, bool, int}:
        return
    if type(value) is float:
        if not math.isfinite(value):
            raise ValueError(f"{path} must not contain NaN or Infinity")
        return
    if type(value) is list:
        for index, item in enumerate(value):
            _validate_json_value(item, f"{path}[{index}]")
        return
    if type(value) is dict:
        for key, item in value.items():
            if type(key) is not str:
                raise ValueError(f"{path} keys must be str")
            _validate_json_value(item, f"{path}.{key}")
        return
    raise ValueError(f"{path} must contain only JSON values")


def _canonical_json_bytes(payload: dict[str, Any]) -> bytes:
    _validate_json_value(payload, "identity")
    try:
        encoded = json.dumps(
            payload,
            sort_keys=True,
            ensure_ascii=False,
            separators=(",", ":"),
            allow_nan=False,
        )
    except (TypeError, ValueError) as exc:
        raise ValueError("identity must be canonical JSON") from exc
    return encoded.encode("utf-8")


@dataclass(frozen=True)
class CanonicalRuntimeIdentity:
    """Immutable canonical record binding every inference precondition."""

    canonical_json: str
    digest: str

    @classmethod
    def from_components(cls, **components: Any) -> "CanonicalRuntimeIdentity":
        keys = set(components)
        missing = sorted(_RUNTIME_COMPONENT_KEYS - keys)
        if missing:
            raise ValueError(f"{missing[0]} is required")
        extra = sorted(keys - _RUNTIME_COMPONENT_KEYS)
        if extra:
            raise ValueError(f"unexpected runtime component: {extra[0]}")

        payload = {
            "gguf_hash": _sha256_digest(components["gguf_hash"], "gguf_hash"),
            "llama_server_hash": _sha256_digest(
                components["llama_server_hash"], "llama_server_hash"
            ),
            "embedding_model_version": _nonempty_text(
                components["embedding_model_version"], "embedding_model_version"
            ),
            "prompt_text": _nonempty_text(components["prompt_text"], "prompt_text"),
            "json_schema": components["json_schema"],
            "generation_params": components["generation_params"],
            "numeric_runtime_version": _nonempty_text(
                components["numeric_runtime_version"], "numeric_runtime_version"
            ),
        }
        if type(payload["json_schema"]) is not dict or not payload["json_schema"]:
            raise ValueError("json_schema must be non-empty dict")
        if (
            type(payload["generation_params"]) is not dict
            or not payload["generation_params"]
        ):
            raise ValueError("generation_params must be non-empty dict")
        if "temperature" not in payload["generation_params"]:
            raise ValueError("generation_params.temperature is required")

        canonical = _canonical_json_bytes(payload)
        return cls(
            canonical_json=canonical.decode("utf-8"),
            digest=hashlib.blake2b(canonical).hexdigest(),
        )

    def to_components(self) -> dict[str, Any]:
        return json.loads(self.canonical_json)


@dataclass(frozen=True)
class SessionGenesisIdentity:
    """Full BLAKE2b identity of an immutable session genesis record."""

    canonical_json: str
    digest: str

    @classmethod
    def from_components(
        cls,
        *,
        mode: str,
        config: dict[str, Any],
        canonical_runtime_identity: str,
        session_started_at: str,
        session_nonce: str,
        parent_state_id: str | None,
        initial_transcript_head: str,
    ) -> "SessionGenesisIdentity":
        mode = _nonempty_text(mode, "mode")
        if type(config) is not dict:
            raise ValueError("config must be dict")
        runtime_digest = validate_runtime_digest(canonical_runtime_identity)
        started_at = _nonempty_text(session_started_at, "session_started_at")
        if type(session_nonce) is not str or not _HEX64.fullmatch(session_nonce):
            raise ValueError("session_nonce must be 64-char lowercase hex")
        if parent_state_id is not None:
            if type(parent_state_id) is not str or not _HEX128.fullmatch(parent_state_id):
                raise ValueError("parent_state_id must be null or 128-char lowercase hex")
        if (
            type(initial_transcript_head) is not str
            or not _HEX128.fullmatch(initial_transcript_head)
        ):
            raise ValueError(
                "initial_transcript_head must be 128-char lowercase hex"
            )

        payload = {
            "canonical_runtime_identity": runtime_digest,
            "config": config,
            "initial_transcript_head": initial_transcript_head,
            "mode": mode,
            "parent_state_id": parent_state_id,
            "session_nonce": session_nonce,
            "session_started_at": started_at,
        }
        canonical = _canonical_json_bytes(payload)
        return cls(
            canonical_json=canonical.decode("utf-8"),
            digest=hashlib.blake2b(canonical).hexdigest(),
        )


def transcript_head(transcript: Any) -> str:
    if type(transcript) is not list:
        raise ValueError("transcript must be list")
    canonical_turns: list[list[str]] = []
    for index, turn in enumerate(transcript):
        if type(turn) not in {tuple, list} or len(turn) != 2:
            raise ValueError(f"transcript[{index}] must be a role/text pair")
        role, text = turn
        if type(role) is not str or type(text) is not str:
            raise ValueError(f"transcript[{index}] role/text must be str")
        canonical_turns.append([role, text])
    encoded = _canonical_json_bytes({"transcript": canonical_turns})
    return hashlib.sha512(encoded).hexdigest()


@lru_cache(maxsize=16)
def _hash_file_cached(path_text: str, size: int, mtime_ns: int) -> str:
    del size, mtime_ns
    hasher = hashlib.sha256()
    with Path(path_text).open("rb") as handle:
        while chunk := handle.read(1024 * 1024):
            hasher.update(chunk)
    return hasher.hexdigest()


def hash_file_sha256(path: Path | str) -> str:
    candidate = Path(path)
    try:
        stat_result = candidate.stat()
    except OSError as exc:
        raise ValueError("runtime artifact is unavailable") from exc
    if not candidate.is_file():
        raise ValueError("runtime artifact must be a file")
    try:
        return _hash_file_cached(
            str(candidate.resolve(strict=True)),
            stat_result.st_size,
            stat_result.st_mtime_ns,
        )
    except OSError as exc:
        raise ValueError("runtime artifact hashing failed") from exc


def explicit_absence_hash(component: str, implementation: bytes) -> str:
    name = _nonempty_text(component, "component").encode("utf-8")
    payload = b"PKB_EXPLICIT_ABSENCE_V1\0" + name + b"\0" + implementation
    return hashlib.sha256(payload).hexdigest()


def numeric_runtime_version() -> str:
    try:
        import numpy as np

        numpy_version = np.__version__
    except ImportError:
        numpy_version = "absent"
    return "|".join(
        (
            f"python-{platform.python_version()}",
            f"implementation-{platform.python_implementation()}",
            f"numpy-{numpy_version}",
            f"platform-{platform.system()}-{platform.release()}",
            f"machine-{platform.machine()}",
            f"byteorder-{sys.byteorder}",
        )
    )
