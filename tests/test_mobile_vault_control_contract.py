# -*- coding: utf-8 -*-
"""Mobile SETTINGS must expose the native secure-vault biometric gate."""
from __future__ import annotations

from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
DESKTOP = ROOT / "apps" / "desktop" / "src"


def _read(relative: str) -> str:
    return (DESKTOP / relative).read_text(encoding="utf-8")


def test_mobile_settings_mounts_compact_vault_control() -> None:
    chrome = _read("components/MobileChrome.tsx")
    nav = _read("lib/mobileNav.ts")
    assert 'import { VaultPanel } from "./VaultPanel"' in chrome
    assert 'case "settings"' in chrome
    assert '<VaultPanel variant="compact" />' in chrome
    assert '{ id: "settings", label: "SETTINGS", caption: "設定" }' in nav


def test_compact_control_reuses_vault_panel_state_machine() -> None:
    panel = _read("components/VaultPanel.tsx")
    assert 'variant?: "full" | "compact"' in panel
    assert "useReducer(vaultReducer, INITIAL_VAULT_STATE)" in panel
    assert "await vaultUnlock()" in panel
    assert 'data-vault-tone={tone}' in panel
    assert '!compact && state.status === "unlocked"' in panel
    assert "vaultSystemErrorLine(state.unlock.error)" in panel
    assert "Subscribe first so there is no status-probe" in panel
    assert "shouldApplyVaultSnapshot" in panel


def test_vault_hud_colors_and_rag_stream_color_are_semantic_tokens() -> None:
    css = _read("App.css")
    active = css.split(
        '.vault-panel[data-vault-tone="active"] .vault-status', 1
    )[1].split("}", 1)[0]
    locked = css.split(
        '.vault-panel[data-vault-tone="locked"] .vault-status', 1
    )[1].split("}", 1)[0]
    unlocked = css.split(
        '.vault-panel[data-vault-tone="unlocked"] .vault-status', 1
    )[1].split("}", 1)[0]
    rag = css.split(".rag-bubble-assistant .rag-bubble-body", 1)[1].split(
        "}", 1
    )[0]
    assert "color: var(--sys-cyan);" in active
    assert "color: var(--err);" in locked
    assert "color: var(--ok);" in unlocked
    assert "color: var(--sys-cyan);" in rag
    assert ".mobile-chrome .vault-panel--compact" in css
    assert "border: 1px solid var(--border);" in css
    assert "margin: 0 0 -1px;" in css
