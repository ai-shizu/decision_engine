"""FSA-2026-07-13-06 RED contracts for runtime and session identity binding."""

from __future__ import annotations

import hashlib
import importlib
import json
import re
import sys
from pathlib import Path

import pytest


ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "src" / "python"))

from core.consultation_engine import ConsultationEngine  # noqa: E402
from core.retrieval_manifest import (  # noqa: E402
    build_bounded_context_with_manifest,
    manifest_from_dict,
    manifest_to_dict,
)


_RUNTIME_COMPONENTS = {
    "gguf_hash": "11" * 32,
    "llama_server_hash": "22" * 32,
    "embedding_model_version": "multilingual-e5-small@1.0.0",
    "prompt_text": "system prompt\n\nComplete prompt body, byte for byte.",
    "json_schema": {
        "type": "object",
        "properties": {"answer": {"type": "string"}},
        "required": ["answer"],
        "additionalProperties": False,
    },
    "generation_params": {
        "temperature": 0.0,
        "top_p": 1.0,
        "seed": 7,
        "max_tokens": 512,
    },
    "numeric_runtime_version": "python-3.12.10|numpy-2.2.6|openblas-0.3.29",
}


def _runtime_components(**overrides: object) -> dict[str, object]:
    components = dict(_RUNTIME_COMPONENTS)
    components.update(overrides)
    return components


def _oracle_runtime_digest(components: dict[str, object]) -> str:
    canonical = json.dumps(
        components,
        sort_keys=True,
        ensure_ascii=False,
        separators=(",", ":"),
    ).encode("utf-8")
    return hashlib.blake2b(canonical).hexdigest()


def _canonical_runtime_identity(**overrides: object):
    module = importlib.import_module("core.runtime_identity")
    identity_type = module.CanonicalRuntimeIdentity
    return identity_type.from_components(**_runtime_components(**overrides))


@pytest.mark.parametrize(
    "override",
    [
        {"gguf_hash": "33" * 32},
        {"prompt_text": "system prompt\n\nA different complete prompt body."},
    ],
    ids=["different-gguf", "different-prompt"],
)
def test_same_mode_and_config_do_not_collide_across_runtime_identity(
    override: dict[str, object],
) -> None:
    engine = ConsultationEngine()
    base_digest = _oracle_runtime_digest(_runtime_components())
    changed_digest = _oracle_runtime_digest(_runtime_components(**override))
    common_genesis = {
        "config": {"genre": "system_design", "difficulty": "hard"},
        "session_started_at": "2026-07-14T00:00:00.000000Z",
        "session_nonce": "44" * 32,
        "parent_state_id": None,
        "initial_transcript_head": hashlib.blake2b(b"[]").hexdigest(),
    }
    state_a = {**common_genesis, "canonical_runtime_identity": base_digest}
    state_b = {**common_genesis, "canonical_runtime_identity": changed_digest}

    id_a = engine._session_id_for_state(state_a, "interview_sim")
    id_b = engine._session_id_for_state(state_b, "interview_sim")

    assert base_digest != changed_digest
    assert id_a != id_b
    assert re.fullmatch(r"[0-9a-f]{128}", id_a)
    assert re.fullmatch(r"[0-9a-f]{128}", id_b)


def test_canonical_runtime_identity_binds_every_required_component() -> None:
    baseline = _canonical_runtime_identity()
    assert baseline.digest == _oracle_runtime_digest(_runtime_components())
    assert re.fullmatch(r"[0-9a-f]{128}", baseline.digest)

    variants = {
        "gguf_hash": "33" * 32,
        "llama_server_hash": "44" * 32,
        "embedding_model_version": "multilingual-e5-small@2.0.0",
        "prompt_text": "different full prompt",
        "json_schema": {"type": "object", "additionalProperties": False},
        "generation_params": {"temperature": 0.2, "max_tokens": 256},
        "numeric_runtime_version": "python-3.12.11|numpy-2.3.0|openblas-0.3.30",
    }
    for field, changed_value in variants.items():
        changed = _canonical_runtime_identity(**{field: changed_value})
        assert changed.digest != baseline.digest, f"identity omitted {field}"


def test_canonical_runtime_identity_hard_fails_when_any_component_is_missing() -> None:
    module = importlib.import_module("core.runtime_identity")
    identity_type = module.CanonicalRuntimeIdentity
    for missing in _RUNTIME_COMPONENTS:
        incomplete = _runtime_components()
        incomplete.pop(missing)
        with pytest.raises(ValueError, match=missing):
            identity_type.from_components(**incomplete)


def test_session_genesis_identity_binds_nonce_parent_and_transcript_head() -> None:
    engine = ConsultationEngine()
    common = {
        "config": {"genre": "system_design", "difficulty": "hard"},
        "canonical_runtime_identity": _oracle_runtime_digest(_runtime_components()),
        "session_started_at": "2026-07-14T00:00:00.000000Z",
        "session_nonce": "55" * 32,
        "parent_state_id": "66" * 64,
        "initial_transcript_head": "77" * 64,
    }
    baseline_id = engine._session_id_for_state(dict(common), "interview_sim")
    variants = {
        "session_nonce": "88" * 32,
        "parent_state_id": "99" * 64,
        "initial_transcript_head": "aa" * 64,
    }
    for field, changed_value in variants.items():
        changed = {**common, field: changed_value}
        changed_id = engine._session_id_for_state(changed, "interview_sim")
        assert changed_id != baseline_id, f"session genesis omitted {field}"


def _legacy_manifest_with_empty_model_hash():
    _, _, manifest = build_bounded_context_with_manifest(
        session_id="fsa06-red-session",
        transcript=[("user", "initial transcript")],
        current_query="initial transcript",
        model_hash="",
        prompt_version="pv1",
    )
    return manifest


def _valid_runtime_bound_manifest():
    _, _, manifest = build_bounded_context_with_manifest(
        session_id="fsa06-green-session",
        transcript=[("user", "initial transcript")],
        current_query="initial transcript",
        runtime_identity=_oracle_runtime_digest(_runtime_components()),
        prompt_version="pv1",
    )
    return manifest


def test_manifest_generation_hard_fails_without_canonical_runtime_identity() -> None:
    with pytest.raises(ValueError, match="canonical runtime identity"):
        _legacy_manifest_with_empty_model_hash()


def test_manifest_load_hard_fails_without_canonical_runtime_identity() -> None:
    data = manifest_to_dict(_valid_runtime_bound_manifest())
    data["model_hash"] = ""
    with pytest.raises(ValueError, match="canonical runtime identity"):
        manifest_from_dict(data)
