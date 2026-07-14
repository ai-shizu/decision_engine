#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""SETTINGS タブ専用 API — ConsultationEngine を読み込まない。"""

from __future__ import annotations

from . import apple_calendar_sync
from .profile_store import (
    FIXED_ATTRIBUTE_FIELDS,
    format_user_profile_summary,
    load_user_profile,
    save_fixed_attributes,
)
from .text_utils import sanitize_obj


def get_settings() -> dict:
    profile = load_user_profile()
    return sanitize_obj({
        "fixed_fields": [{"key": k, "label": label} for k, label in FIXED_ATTRIBUTE_FIELDS],
        "fixed_attributes": profile.get("fixed_attributes", {}),
        "profile_summary": format_user_profile_summary(),
        "apple_calendar_available": apple_calendar_sync.direct_calendar_access_available(),
    })


def save_settings_fixed(attributes: dict) -> None:
    save_fixed_attributes(attributes)
