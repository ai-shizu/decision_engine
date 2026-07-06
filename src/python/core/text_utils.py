#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""テキスト正規化 (JSON / プロファイル出力用)。"""

from __future__ import annotations

from typing import Any


def sanitize_text(text: str) -> str:
    """孤立サロゲート等を除去し UTF-8 安全な文字列にする。"""
    if not text:
        return ""
    return text.encode("utf-8", "surrogatepass").decode("utf-8", "replace")


def sanitize_obj(value: Any) -> Any:
    if isinstance(value, str):
        return sanitize_text(value)
    if isinstance(value, dict):
        return {k: sanitize_obj(v) for k, v in value.items()}
    if isinstance(value, list):
        return [sanitize_obj(v) for v in value]
    return value
