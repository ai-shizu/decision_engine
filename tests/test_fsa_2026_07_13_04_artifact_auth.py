# -*- coding: utf-8 -*-
"""FSA-2026-07-13-04: signed artifact allowlist and preflight contracts."""
from __future__ import annotations

import hashlib
import importlib
import json
from pathlib import Path
import subprocess
import sys

import pytest


ROOT = Path(__file__).resolve().parents[1]
PYTHON_SRC = ROOT / "src" / "python"
if str(PYTHON_SRC) not in sys.path:
    sys.path.insert(0, str(PYTHON_SRC))
PROD_PUBLIC_KEY = "3e8f9a2b4c6d5e1f7a8b9c0d1e2f3a4b5c6d7e8f9a0b1c2d3e4f5a6b7c8d9e0f"
PUBLIC_TEST_SEED = "9d61b19deffd5a60ba844af492ec2cc44449c5697b326919703bac031cae7f60"


def _auth_module():
    return importlib.import_module("core.artifact_auth")


def _canonical_manifest(path: Path, artifact_path: str, digest: str, size: int) -> bytes:
    payload = {
        "artifacts": [{
            "base": "data",
            "id": "gguf:consult",
            "kind": "file",
            "path": artifact_path,
            "sha256": digest,
            "size": size,
        }],
        "key_id": "pkb-test-ed25519-rfc8032",
        "schema": "pkb.artifact_allowlist.v1",
    }
    encoded = json.dumps(
        payload,
        ensure_ascii=False,
        sort_keys=True,
        separators=(",", ":"),
    ).encode("utf-8")
    path.write_bytes(encoded)
    return encoded


def test_01_production_key_is_fixed_and_test_secret_is_fixture_only() -> None:
    rust = (ROOT / "apps/desktop/src-tauri/src/artifact_auth.rs").read_text(encoding="utf-8")
    assert PROD_PUBLIC_KEY in rust
    production = "\n".join(
        path.read_text(encoding="utf-8", errors="ignore")
        for path in (ROOT / "src").rglob("*")
        if path.is_file()
    )
    assert PUBLIC_TEST_SEED not in production


def test_02_python_rejects_missing_attestation_and_one_byte_tamper(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    auth = _auth_module()
    data = tmp_path / "data"
    app = tmp_path / "app"
    data.mkdir()
    app.mkdir()
    model = data / "models" / "model.gguf"
    model.parent.mkdir()
    model.write_bytes(b"trusted-model")
    manifest_path = data / "artifacts_allowlist.json"
    manifest = _canonical_manifest(
        manifest_path,
        "models/model.gguf",
        hashlib.sha256(model.read_bytes()).hexdigest(),
        model.stat().st_size,
    )

    monkeypatch.setenv("PKB_REQUIRE_ARTIFACT_AUTH", "1")
    monkeypatch.delenv("PKB_ARTIFACT_MANIFEST_SHA256", raising=False)
    with pytest.raises(auth.ArtifactIntegrityError):
        auth.verify_artifact_path("gguf:consult", model)

    monkeypatch.setenv("PKB_ARTIFACT_MANIFEST", str(manifest_path))
    monkeypatch.setenv("PKB_ARTIFACT_MANIFEST_SHA256", hashlib.sha256(manifest).hexdigest())
    monkeypatch.setenv("PKB_ARTIFACT_APP_ROOT", str(app))
    monkeypatch.setenv("PKB_PROJECT_ROOT", str(data))
    assert auth.verify_artifact_path("gguf:consult", model) == model

    model.write_bytes(b"trusted-modeL")
    with pytest.raises(auth.ArtifactIntegrityError):
        auth.verify_artifact_path("gguf:consult", model)


def test_03_python_rejects_manifest_rewrite_and_path_substitution(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    auth = _auth_module()
    data = tmp_path / "data"
    app = tmp_path / "app"
    data.mkdir()
    app.mkdir()
    model = data / "model.gguf"
    other = data / "other.gguf"
    model.write_bytes(b"model")
    other.write_bytes(b"model")
    manifest_path = data / "artifacts_allowlist.json"
    canonical = _canonical_manifest(
        manifest_path,
        "model.gguf",
        hashlib.sha256(b"model").hexdigest(),
        5,
    )
    monkeypatch.setenv("PKB_REQUIRE_ARTIFACT_AUTH", "1")
    monkeypatch.setenv("PKB_ARTIFACT_MANIFEST", str(manifest_path))
    monkeypatch.setenv("PKB_ARTIFACT_MANIFEST_SHA256", hashlib.sha256(canonical).hexdigest())
    monkeypatch.setenv("PKB_ARTIFACT_APP_ROOT", str(app))
    monkeypatch.setenv("PKB_PROJECT_ROOT", str(data))

    with pytest.raises(auth.ArtifactIntegrityError):
        auth.verify_artifact_path("gguf:consult", other)

    manifest_path.write_text(json.dumps(json.loads(canonical), indent=2), encoding="utf-8")
    with pytest.raises(auth.ArtifactIntegrityError):
        auth.verify_artifact_path("gguf:consult", model)


def test_03b_required_config_deletion_cannot_fall_back_to_defaults(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    auth = _auth_module()
    from core import llm_config

    data = tmp_path / "data"
    app = tmp_path / "app"
    config = data / "config" / "model_params.json"
    config.parent.mkdir(parents=True)
    app.mkdir()
    config.write_text('{"schema":"model_params.v1"}', encoding="utf-8")
    payload = {
        "artifacts": [{
            "base": "data",
            "id": "config:model_params",
            "kind": "file",
            "path": "config/model_params.json",
            "sha256": hashlib.sha256(config.read_bytes()).hexdigest(),
            "size": config.stat().st_size,
        }],
        "key_id": "pkb-test-ed25519-rfc8032",
        "schema": "pkb.artifact_allowlist.v1",
    }
    encoded = json.dumps(
        payload, ensure_ascii=False, sort_keys=True, separators=(",", ":")
    ).encode("utf-8")
    manifest = data / "artifacts_allowlist.json"
    manifest.write_bytes(encoded)
    monkeypatch.setenv("PKB_REQUIRE_ARTIFACT_AUTH", "1")
    monkeypatch.setenv("PKB_ARTIFACT_MANIFEST", str(manifest))
    monkeypatch.setenv("PKB_ARTIFACT_MANIFEST_SHA256", hashlib.sha256(encoded).hexdigest())
    monkeypatch.setenv("PKB_ARTIFACT_APP_ROOT", str(app))
    monkeypatch.setenv("PKB_PROJECT_ROOT", str(data))
    monkeypatch.setattr(llm_config, "MODEL_PARAMS_JSON", config)

    config.unlink()
    with pytest.raises(auth.ArtifactIntegrityError):
        llm_config.load_model_params()


def test_04_all_native_launch_paths_have_preflight_verification() -> None:
    llm = (ROOT / "src/python/core/llm_backend.py").read_text(encoding="utf-8")
    consult = (ROOT / "src/python/core/consultation_engine.py").read_text(encoding="utf-8")
    cli = (ROOT / "src/python/core/cli.py").read_text(encoding="utf-8")
    daemon = (ROOT / "src/python/core/search_daemon.py").read_text(encoding="utf-8")
    assert 'verify_artifact_path("llama_runtime"' in llm
    assert 'verify_artifact_path("search_engine"' in consult
    assert 'verify_artifact_path("search_engine"' in cli
    assert 'verify_artifact_path("search_engine"' in daemon


def test_05_model_and_embedding_loaders_require_manifest_binding() -> None:
    config = (ROOT / "src/python/core/llm_config.py").read_text(encoding="utf-8")
    pipeline = (ROOT / "src/python/core/pipeline.py").read_text(encoding="utf-8")
    assert "verified_gguf_candidates" in config
    assert "verify_artifact_path" in config
    assert "PKB_EMBEDDING_MODEL_DIR" in pipeline
    assert 'verify_artifact_path("embedding_model"' in pipeline


def test_06_release_engine_verifies_signature_before_sidecar_spawn() -> None:
    engine = (ROOT / "apps/desktop/src-tauri/src/engine.rs").read_text(encoding="utf-8")
    verify_at = engine.index("verify_production_artifacts(")
    spawn_at = engine.index("spawn_bundled_engine(")
    assert verify_at < spawn_at
    assert "PKB_ARTIFACT_MANIFEST_SHA256" in engine
    assert "PKB_ARTIFACT_MANIFEST" in engine


def test_07_build_and_release_are_pinned_signed_and_auditable() -> None:
    powershell = (ROOT / "apps/desktop/scripts/build-engine.ps1").read_text(encoding="utf-8")
    shell = (ROOT / "apps/desktop/scripts/build-sidecar.sh").read_text(encoding="utf-8")
    workflow = (ROOT / ".github/workflows/build-macos.yml").read_text(encoding="utf-8")
    windows_workflow = (ROOT / ".github/workflows/build-windows.yml").read_text(encoding="utf-8")
    cargo = (ROOT / "apps/desktop/src-tauri/Cargo.toml").read_text(encoding="utf-8")
    assert "--require-hashes" in powershell
    assert "--require-hashes" in shell
    assert "PKB_ARTIFACT_SIGNING_KEY" in workflow
    assert "pkb-artifact-manifest" in workflow
    assert "SBOM" in workflow
    assert "PKB_ARTIFACT_SIGNING_KEY" in windows_workflow
    assert "pkb-artifact-manifest" in windows_workflow
    assert "SBOM" in windows_workflow
    all_workflows = workflow + windows_workflow
    assert "@v" not in all_workflows and "@stable" not in all_workflows
    assert 'ed25519-dalek = "=3.0.0"' in cargo
    assert (ROOT / "rust-toolchain.toml").is_file()


def test_08_release_sbom_is_deterministic_and_path_free(tmp_path: Path) -> None:
    script = ROOT / "scripts" / "generate_release_sbom.py"
    first = tmp_path / "first.cdx.json"
    second = tmp_path / "second.cdx.json"
    command = [
        sys.executable,
        str(script),
        "--root",
        str(ROOT),
        "--output",
    ]
    subprocess.run(command + [str(first)], check=True)
    subprocess.run(command + [str(second)], check=True)
    assert first.read_bytes() == second.read_bytes()
    payload = json.loads(first.read_bytes())
    assert payload["bomFormat"] == "CycloneDX"
    assert payload["specVersion"] == "1.5"
    refs = [component["bom-ref"] for component in payload["components"]]
    assert refs == sorted(refs)
    assert len(refs) == len(set(refs))
    assert b"pkb-desktop" in first.read_bytes()
    assert str(ROOT).encode("utf-8") not in first.read_bytes()
