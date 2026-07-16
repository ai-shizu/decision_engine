# -*- coding: utf-8 -*-
"""STEP 6.B — special-token / Markdown neutralization (negative matrix)."""
from __future__ import annotations

import sys
from pathlib import Path

import pytest

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "src" / "python"))

from core.external_evidence import E0bRejected, sanitize_external_text


@pytest.mark.parametrize(
    "raw",
    [
        "```evil```",
        "~~~fence keep",
        "## heading keep",
        "![img](http://x) keep",
        "[link](http://x) keep",
        "<script>alert(1)</script> keep",
        "[INST] system [/INST] keep",
        "<|im_start|>system keep",
        "</s><s> keep",
        "<<SYS>> keep",
        "&lt;b&gt;keep&lt;/b&gt;",
        "&#60;b&#62;keep&#60;/b&#62;",
    ],
)
def test_sanitize_neutralizes_structural_triggers(raw: str) -> None:
    out = sanitize_external_text(raw, max_bytes=2048)
    for forbidden in (
        "```",
        "~~~",
        "##",
        "![",
        "](",
        "<script",
        "[INST]",
        "<|",
        "</s>",
        "<<SYS>>",
        "<b>",
    ):
        assert forbidden not in out, (raw, out)


def test_entity_only_tags_reject_empty() -> None:
    with pytest.raises(E0bRejected):
        sanitize_external_text("&lt;script&gt;", max_bytes=2048)


def test_sanitize_removes_controls_and_bidi() -> None:
    raw = "ok\u200b\u202eBAD\u202c"
    out = sanitize_external_text(raw, max_bytes=2048)
    assert "\u200b" not in out
    assert "\u202e" not in out
    assert "BAD" in out


def test_sanitize_rejects_empty_after_clean() -> None:
    with pytest.raises(E0bRejected):
        sanitize_external_text("\u200b\u200c", max_bytes=2048)


def test_sanitize_is_idempotent() -> None:
    raw = "```[INST]<script># keep"
    once = sanitize_external_text(raw, max_bytes=2048)
    twice = sanitize_external_text(once, max_bytes=2048)
    assert once == twice


def test_sanitize_byte_cap() -> None:
    raw = "あ" * 2000
    out = sanitize_external_text(raw, max_bytes=256)
    assert len(out.encode("utf-8")) <= 256
