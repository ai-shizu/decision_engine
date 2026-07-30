# -*- coding: utf-8 -*-
"""Tier 3 P0-6-2: CORAXIS_FORCE_CPU=1 must apply the full 4-point CPU set.

Missing with_devices(&[]) while zeroing layers alone still initializes Metal.
"""
from __future__ import annotations

import re
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
SERVICE = ROOT / "apps" / "desktop" / "src-tauri" / "src" / "llm" / "service.rs"
PARAMS = ROOT / "apps" / "desktop" / "src-tauri" / "src" / "llm" / "params.rs"
CONSULT = ROOT / "apps" / "desktop" / "src-tauri" / "src" / "llm" / "commands_consult.rs"
AI_SKILLS = ROOT / "docs" / "AI_SKILLS.md"

ENV_NAME = "CORAXIS_FORCE_CPU"


def _service() -> str:
    return SERVICE.read_text(encoding="utf-8")


def _params() -> str:
    return PARAMS.read_text(encoding="utf-8")


def _slice_between(text: str, start: str, end: str) -> str:
    i = text.index(start)
    j = text.index(end, i + len(start))
    return text[i:j]


def test_force_cpu_env_name_is_literal() -> None:
    text = _params()
    assert f'"{ENV_NAME}"' in text, f"env constant must be exactly {ENV_NAME!r}"
    assert "CORAXIS_FORCE_CPU_ENV" in text


def test_force_cpu_requires_exact_one() -> None:
    """D-40 target: enabling without env==\"1\" must fail this scan."""
    body = _slice_between(_params(), "fn force_cpu_oracle_enabled", "\n#[derive")
    assert 'Some("1")' in body, (
        'force_cpu_oracle_enabled must gate on Some("1") only'
    )


def test_four_point_cpu_oracle_set_complete() -> None:
    """D-38 target: dropping with_devices(&[]) must fail this scan."""
    text = _service()
    body = _slice_between(
        text, "fn apply_force_cpu_model_params", "fn apply_context_device_policy"
    )
    assert "with_devices(&[])" in body, "point 1: with_devices(&[]) required"
    assert "with_n_gpu_layers(0)" in body, "point 2: with_n_gpu_layers(0) required"

    ctx_body = _slice_between(text, "fn apply_context_device_policy", "fn load_model")
    assert "force_cpu_oracle_enabled()" in ctx_body
    assert "with_offload_kqv(false)" in ctx_body, "point 3: with_offload_kqv(false)"
    assert "with_op_offload(false)" in ctx_body, "point 4: with_op_offload(false)"


def test_with_devices_failure_does_not_metal_fallback() -> None:
    body = _slice_between(
        _service(), "fn apply_force_cpu_model_params", "fn apply_context_device_policy"
    )
    assert "refusing Metal fallback" in body
    assert "map_err" in body
    assert "unwrap_or" not in body


def test_on_device_default_layers_remain_999_in_source() -> None:
    body = _slice_between(
        _service(), "fn effective_n_gpu_layers", "fn apply_force_cpu_model_params"
    )
    assert re.search(r"else\s*\{\s*requested\s*\}", body), (
        "on-device arm must return requested layers (default 999 path)"
    )
    assert "n_gpu_layers: 999" in CONSULT.read_text(encoding="utf-8")


def test_ai_skills_documents_force_cpu() -> None:
    text = AI_SKILLS.read_text(encoding="utf-8")
    assert ENV_NAME in text
    assert "with_devices(&[])" in text
    assert "model.force_cpu" in text
