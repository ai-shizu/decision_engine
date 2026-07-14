# -*- coding: utf-8 -*-
"""FSA-2026-07-13-01: process-wide offline boundary contracts."""
from __future__ import annotations

import importlib
import os
import sys
import types
from pathlib import Path

import numpy as np
import pytest


ROOT = Path(__file__).resolve().parents[1]
PYTHON_ROOT = ROOT / "src" / "python"
PIPELINE_PATH = PYTHON_ROOT / "core" / "pipeline.py"
CE_PATH = PYTHON_ROOT / "core" / "consultation_engine.py"
CLI_PATH = PYTHON_ROOT / "core" / "cli.py"
RUST_ENGINE_PATH = ROOT / "apps" / "desktop" / "src-tauri" / "src" / "engine.rs"

OFFLINE_ENV = {
    "HF_HUB_OFFLINE": "1",
    "TRANSFORMERS_OFFLINE": "1",
    "HF_DATASETS_OFFLINE": "1",
    "HF_HUB_DISABLE_TELEMETRY": "1",
    "DO_NOT_TRACK": "1",
    "LLAMA_ARG_OFFLINE": "1",
}

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


def _rust_fn_body(source: str, name: str) -> str:
    marker = f"fn {name}("
    start = source.find(marker)
    assert start >= 0, f"missing Rust function: {name}"
    brace = source.find("{", start)
    assert brace >= 0, f"missing Rust function body: {name}"
    depth = 0
    for idx in range(brace, len(source)):
        char = source[idx]
        if char == "{":
            depth += 1
        elif char == "}":
            depth -= 1
            if depth == 0:
                return source[brace + 1 : idx]
    raise AssertionError(f"unterminated Rust function: {name}")


def test_offline_environment_overwrites_poisoned_parent(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    monkeypatch.syspath_prepend(str(PYTHON_ROOT))
    for key in OFFLINE_ENV:
        monkeypatch.setenv(key, "0")
    for key in SENSITIVE_ENV:
        monkeypatch.setenv(key, "https://attacker.invalid/secret")
    monkeypatch.setenv("NO_PROXY", "attacker.invalid")
    monkeypatch.setenv("no_proxy", "attacker.invalid")

    offline_runtime = importlib.import_module("core.offline_runtime")
    offline_runtime.enforce_offline_environment()

    for key, expected in OFFLINE_ENV.items():
        assert os.environ.get(key) == expected, key
    for key in SENSITIVE_ENV:
        assert key not in os.environ, key
    assert os.environ.get("NO_PROXY") == "*"
    assert os.environ.get("no_proxy") == "*"


def test_sentence_transformer_loader_is_local_only(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    monkeypatch.syspath_prepend(str(PYTHON_ROOT))
    for key in OFFLINE_ENV:
        monkeypatch.setenv(key, "0")
    for key in SENSITIVE_ENV:
        monkeypatch.setenv(key, "https://attacker.invalid/secret")

    observed: dict[str, object] = {}

    class FakeSentenceTransformer:
        def __init__(self, model_name: str, **kwargs: object) -> None:
            observed["model_name"] = model_name
            observed["kwargs"] = kwargs
            observed["environment"] = {
                key: os.environ.get(key) for key in (*OFFLINE_ENV, *SENSITIVE_ENV)
            }

        def encode(self, texts: list[str]) -> np.ndarray:
            return np.zeros((len(texts), 384), dtype=np.float32)

    fake_module = types.ModuleType("sentence_transformers")
    fake_module.SentenceTransformer = FakeSentenceTransformer
    monkeypatch.setitem(sys.modules, "sentence_transformers", fake_module)

    pipeline = importlib.import_module("core.pipeline")
    pipeline.build_embedder()

    kwargs = observed["kwargs"]
    assert isinstance(kwargs, dict)
    assert kwargs.get("local_files_only") is True
    assert kwargs.get("trust_remote_code") is False
    environment = observed["environment"]
    assert isinstance(environment, dict)
    for key, expected in OFFLINE_ENV.items():
        assert environment[key] == expected, key
    for key in SENSITIVE_ENV:
        assert environment[key] is None, key


def test_python_entrypoints_force_offline_environment() -> None:
    for path in (CE_PATH, CLI_PATH):
        source = path.read_text(encoding="utf-8")
        assert 'setdefault("HF_HUB_OFFLINE"' not in source, path
        assert 'setdefault("TRANSFORMERS_OFFLINE"' not in source, path
        assert "enforce_offline_environment" in source, path

    pipeline_source = PIPELINE_PATH.read_text(encoding="utf-8")
    assert "enforce_offline_environment" in pipeline_source
    assert "local_files_only=True" in pipeline_source
    assert "trust_remote_code=False" in pipeline_source


def test_rust_engine_children_use_allowlisted_offline_environment() -> None:
    source = RUST_ENGINE_PATH.read_text(encoding="utf-8")
    helper = _rust_fn_body(source, "configure_offline_child_environment")
    python_spawn = _rust_fn_body(source, "spawn_python_engine")
    bundled_spawn = _rust_fn_body(source, "spawn_bundled_engine")

    assert "cmd.env_clear();" in helper
    for key, expected in OFFLINE_ENV.items():
        assert f'.env("{key}", "{expected}")' in helper, key
    for key in SENSITIVE_ENV:
        assert f'.env("{key}"' not in helper, key
    assert '.env("NO_PROXY", "*")' in helper
    assert '.env("no_proxy", "*")' in helper
    assert '.env("PATH"' not in helper

    call = "configure_offline_child_environment(&mut cmd, root);"
    assert python_spawn.count(call) == 1
    assert bundled_spawn.count(call) == 1


def test_native_tcp_bypass_is_owned_by_production_kernel_probe() -> None:
    """Unsandboxed pytest is dev-only; native denial belongs to the Rust launcher."""
    runtime = (
        ROOT
        / "apps"
        / "desktop"
        / "src-tauri"
        / "tests"
        / "os_sandbox_runtime.rs"
    ).read_text(encoding="utf-8")
    probe = (
        ROOT
        / "apps"
        / "desktop"
        / "src-tauri"
        / "src"
        / "bin"
        / "pkb-sandbox-probe.rs"
    ).read_text(encoding="utf-8")
    assert "spawn_kernel_sandboxed" in runtime
    assert "native_tcp_socket_probe" in runtime
    assert "socket(" in probe
    assert "connect(" in probe
