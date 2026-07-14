#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""Configuration and command construction for the local llama.cpp runtime."""
from __future__ import annotations

import json
import os
from pathlib import Path

from .artifact_auth import (
    ArtifactIntegrityError,
    artifact_auth_required,
    verified_artifact_paths,
    verify_artifact_path,
)
from .durable_persistence import PersistenceReadError, read_json_file
from .paths import MODELS_DIR, PROJECT_ROOT as ROOT


MODEL_PARAMS_JSON = ROOT / "config" / "model_params.json"
MIN_7B_BYTES = 3_500_000_000

_DEFAULT_PARAMS: dict = {
    "schema": "model_params.v1",
    "roles": {
        "default": {
            "preferred": [
                "Qwen2.5-7B-Instruct-IQ4_XS.gguf",
                "qwen2.5-7b-instruct-iq4_xs.gguf",
                "Qwen2.5-7B-Instruct-Q4_K_M.gguf",
                "qwen2.5-7b-instruct-q4_k_m.gguf",
            ],
        },
        "consult": {
            "preferred": [
                "DeepSeek-R1-Distill-Qwen-7B*.gguf",
                "deepseek-r1-distill-qwen-7b*.gguf",
            ],
        },
    },
    "policy": {
        "min_model_bytes": MIN_7B_BYTES,
        "allow_small_models": False,
    },
    "runtime": {"threads": 8, "ctx": 8192, "batch": 512},
    "generation": {"temperature": 0.6, "max_tokens": 900},
}


def load_model_params() -> dict:
    """Load model configuration, overlaying known sections on fixed defaults."""
    params = json.loads(json.dumps(_DEFAULT_PARAMS))
    if artifact_auth_required():
        verify_artifact_path("config:model_params", MODEL_PARAMS_JSON)
    try:
        user = read_json_file(MODEL_PARAMS_JSON)
    except FileNotFoundError:
        return params
    if type(user) is not dict:
        raise PersistenceReadError("model parameters root must be an object")
    for key in ("roles", "policy", "runtime", "generation"):
        section = user.get(key)
        if section is None:
            continue
        if type(section) is not dict:
            raise PersistenceReadError(f"model parameters {key} must be an object")
        if key == "roles":
            params["roles"].update(section)
        else:
            params[key].update(section)
    return params


def _runtime_int(env_name: str, config_key: str) -> int:
    env = os.environ.get(env_name)
    if env is not None:
        return int(env)
    return int(load_model_params()["runtime"][config_key])


LLAMA_THREADS = _runtime_int("PKB_LLAMA_THREADS", "threads")
LLAMA_CTX = _runtime_int("PKB_LLAMA_CTX", "ctx")
LLAMA_BATCH = _runtime_int("PKB_LLAMA_BATCH", "batch")

PREFERRED_7B = MODELS_DIR / "Qwen2.5-7B-Instruct-Q4_K_M.gguf"
PREFERRED_7B_ALT = MODELS_DIR / "qwen2.5-7b-instruct-q4_k_m.gguf"


def _allow_small_models(params: dict) -> bool:
    if os.environ.get("PKB_ALLOW_SMALL_LLM") == "1":
        return True
    return bool(params["policy"].get("allow_small_models"))


def _min_model_bytes(params: dict) -> int:
    return int(params["policy"].get("min_model_bytes", MIN_7B_BYTES))


def _is_valid_gguf(path: Path, params: dict) -> bool:
    if not path.exists():
        return False
    size = path.stat().st_size
    if size < 1_000_000:
        return False
    if "7b" in path.name.lower() and size < _min_model_bytes(params):
        return False
    return True


def _expand_pattern(pattern: str) -> list[Path]:
    if any(ch in pattern for ch in "*?["):
        if not MODELS_DIR.exists():
            return []
        return sorted(MODELS_DIR.glob(pattern))
    return [MODELS_DIR / pattern]


def find_gguf(role: str = "default") -> Path | None:
    """Select the configured local GGUF for a role without remote fallback."""
    params = load_model_params()
    if artifact_auth_required():
        candidates = verified_gguf_candidates(role)
        for artifact_id, candidate in candidates:
            verify_artifact_path(artifact_id, candidate)
            if _is_valid_gguf(candidate, params):
                return candidate
        raise ArtifactIntegrityError("artifact integrity verification failed")

    roles = params["roles"]

    patterns = list(roles.get(role, {}).get("preferred", []))
    if role != "default":
        for pattern in roles.get("default", {}).get("preferred", []):
            if pattern not in patterns:
                patterns.append(pattern)

    for pattern in patterns:
        for candidate in _expand_pattern(pattern):
            if _is_valid_gguf(candidate, params):
                return candidate

    if not MODELS_DIR.exists():
        return None
    candidates = [
        candidate
        for candidate in MODELS_DIR.glob("*.gguf")
        if _is_valid_gguf(candidate, params)
    ]
    if not _allow_small_models(params):
        candidates = [
            candidate
            for candidate in candidates
            if candidate.stat().st_size >= _min_model_bytes(params)
        ]
    candidates.sort(key=lambda path: path.stat().st_size, reverse=True)
    for candidate in candidates:
        if "7b" in candidate.name.lower():
            return candidate
    return candidates[0] if candidates else None


def verified_gguf_candidates(role: str) -> list[tuple[str, Path]]:
    """Return only GGUF paths bound by the Rust-attested production manifest."""
    artifacts = verified_artifact_paths("gguf")
    role_prefix = f"gguf:{role}"
    selected = [
        item
        for item in artifacts
        if item[0] == role_prefix or item[0].startswith(role_prefix + ":")
    ]
    if role != "default":
        selected.extend(
            item
            for item in artifacts
            if item[0] == "gguf:default" or item[0].startswith("gguf:default:")
        )
    if not selected:
        raise ArtifactIntegrityError("artifact integrity verification failed")
    return selected


def generation_params() -> dict:
    gen = load_model_params()["generation"]
    return {
        "temperature": float(gen.get("temperature", 0.6)),
        "max_tokens": int(gen.get("max_tokens", 900)),
    }


def llama_stdio_cmd(
    exe: Path,
    model: Path,
    *,
    prompt_file: str,
    max_tokens: int,
    temperature: float,
    json_schema: dict | None = None,
) -> list[str]:
    """Build the fixed local-only command for a one-shot anonymous pipe."""
    cmd = [
        str(exe),
        "completion",
        "-m",
        str(model),
        "-f",
        prompt_file,
        "--offline",
        "--simple-io",
        "--single-turn",
        "--no-conversation",
        "--no-display-prompt",
        "--no-perf",
        "--color",
        "off",
        "--no-warmup",
        "-n",
        str(max_tokens),
        "--temp",
        str(float(temperature)),
        "--ctx-size",
        str(LLAMA_CTX),
        "--threads",
        str(LLAMA_THREADS),
        "--batch-size",
        str(LLAMA_BATCH),
    ]
    if json_schema is not None:
        encoded = json.dumps(
            json_schema,
            ensure_ascii=False,
            sort_keys=True,
            separators=(",", ":"),
        )
        cmd += ["--json-schema", encoded]
    return cmd
