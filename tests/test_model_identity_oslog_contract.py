# -*- coding: utf-8 -*-
"""Tier 3 P0-6-1: model identity must ride the numeric OSLog path after load."""
from __future__ import annotations

from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
IOS_OSLOG = ROOT / "apps" / "desktop" / "src-tauri" / "src" / "ios_oslog.rs"
SERVICE = ROOT / "apps" / "desktop" / "src-tauri" / "src" / "llm" / "service.rs"
MODEL_PATH = ROOT / "apps" / "desktop" / "src-tauri" / "src" / "llm" / "model_path.rs"
AI_SKILLS = ROOT / "docs" / "AI_SKILLS.md"
IOS_OSLOG_C = ROOT / "apps" / "desktop" / "src-tauri" / "native" / "ios_oslog.c"

MODEL_LABELS = (
    "model.n_layer",
    "model.n_params",
    "model.size",
    "model.meta_count",
    "model.n_vocab",
    "model.origin",
    "model.force_cpu",
)


def test_model_identity_labels_are_constants() -> None:
    text = IOS_OSLOG.read_text(encoding="utf-8")
    for label in MODEL_LABELS:
        assert f'"{label}"' in text, f"missing label constant for {label!r}"


def test_log_model_u64_uses_model_category_and_emit_u64() -> None:
    text = IOS_OSLOG.read_text(encoding="utf-8")
    assert "pub fn log_model_u64" in text
    idx = text.index("pub fn log_model_u64")
    rest = text[idx:]
    end = rest.find("\npub ")
    if end < 0:
        end = rest.find("\nfn ")
    body = rest[: end if end > 0 else 400]
    assert "CAT_MODEL" in body
    assert "emit_u64" in body


def test_load_model_calls_identity_logger() -> None:
    text = SERVICE.read_text(encoding="utf-8")
    assert "log_model_identity_ios" in text
    assert "fn load_model(" in text
    load_idx = text.index("fn load_model(")
    # Bound roughly to next top-level fn after load_model
    rest = text[load_idx:]
    # find closing of load_model by looking for log call before next fn generate
    assert "log_model_identity_ios" in rest[:2500], (
        "load_model must call log_model_identity_ios on the iOS path"
    )
    assert "log_model_u64" in text
    assert "MODEL_FORCE_CPU" in text


def test_identity_logger_emits_all_numeric_fields() -> None:
    text = SERVICE.read_text(encoding="utf-8")
    assert "fn log_model_identity_ios" in text
    idx = text.index("fn log_model_identity_ios")
    body = text[idx : idx + 1200]
    for needle in (
        "MODEL_N_LAYER",
        "MODEL_N_PARAMS",
        "MODEL_SIZE",
        "MODEL_META_COUNT",
        "MODEL_N_VOCAB",
        "MODEL_ORIGIN",
        "n_layer()",
        "n_params()",
        "size()",
        "meta_count()",
        "n_vocab()",
        "model_origin_code",
    ):
        assert needle in body, f"identity logger missing {needle}"


def test_origin_code_helper_exists_without_raw_path_logging() -> None:
    text = MODEL_PATH.read_text(encoding="utf-8")
    assert "fn model_origin_code" in text or "pub fn model_origin_code" in text
    # The helper must not call log with path display for the OSLog public path.
    idx = text.index("fn model_origin_code")
    body = text[idx : idx + 600]
    assert "log::" not in body
    assert "Application Support" in body
    assert "/assets/models/" in body


def test_c_shim_public_llu_still_present() -> None:
    c = IOS_OSLOG_C.read_text(encoding="utf-8")
    assert "%{public}llu" in c


def test_ai_skills_lists_model_search_strings() -> None:
    text = AI_SKILLS.read_text(encoding="utf-8")
    for label in MODEL_LABELS:
        assert label in text, f"AI_SKILLS must document {label!r}"
