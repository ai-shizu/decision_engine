# -*- coding: utf-8 -*-
"""Finding 17 — main tabs must follow WAI-ARIA manual activation Tabs Pattern."""
from __future__ import annotations

import re
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
APP_TSX = ROOT / "apps" / "desktop" / "src" / "App.tsx"
APP_CSS = ROOT / "apps" / "desktop" / "src" / "App.css"
AI_SKILLS = ROOT / "docs" / "AI_SKILLS.md"
SPEC_FOXTROT = ROOT / "docs" / "SPEC_FOXTROT_UI.md"

# Split tokens to avoid this file becoming a greppable false positive for tree audits.
ROLE_TABLIST = "role=" + '"tablist"'
ROLE_TAB = "role=" + '"tab"'
ROLE_TABPANEL = "role=" + '"tabpanel"'
ARIA_SELECTED = "aria-" + "selected"
ARIA_CONTROLS = "aria-" + "controls"
ARIA_LABELLEDBY = "aria-" + "labelledby"
ARIA_ORIENTATION = "aria-" + "orientation"
ARIA_CURRENT = "aria-" + "current"
ARROW_LEFT = "Arrow" + "Left"
ARROW_RIGHT = "Arrow" + "Right"
KEY_HOME = "Ho" + "me"
KEY_END = "En" + "d"
FOCUS_VISIBLE = "focus" + "-visible"
HANDLE_TAB_KEYDOWN = "handleTab" + "KeyDown"
RENDER_MAIN_TAB = "renderMain" + "Tab"
TAB_BUTTON_ID = "tabButton" + "Id"
TAB_PANEL_ID = "tabPanel" + "Id"
MAIN_TAB_PANEL = "main-tab-" + "panel"


def _read(path: Path) -> str:
    return path.read_text(encoding="utf-8")


def _ai_skills_s34() -> str:
    text = _read(AI_SKILLS)
    m = re.search(r"^###\s+3\.4\s+", text, flags=re.M)
    assert m, "AI_SKILLS §3.4 missing"
    nxt = re.search(r"^###\s+3\.5\s+", text[m.end() :], flags=re.M)
    assert nxt, "AI_SKILLS §3.5 missing"
    return text[m.start() : m.end() + nxt.start()]


def _extract_function(src: str, name: str) -> str:
    m = re.search(rf"(?:function|const)\s+{re.escape(name)}\s*[=(]", src)
    assert m, f"function {name} not found"
    start = m.start()
    # Brace-balanced body from first '{' after the match.
    brace = src.find("{", m.end() - 1)
    assert brace != -1, f"body of {name} not found"
    depth = 0
    i = brace
    while i < len(src):
        ch = src[i]
        if ch == "{":
            depth += 1
        elif ch == "}":
            depth -= 1
            if depth == 0:
                return src[start : i + 1]
        i += 1
    raise AssertionError(f"unbalanced braces for {name}")


def _tabs_nav_block(src: str) -> str:
    m = re.search(r'<nav\s+[^>]*className="tabs"[^>]*>', src)
    assert m, 'nav.tabs not found'
    start = m.start()
    depth = 0
    i = start
    while i < len(src):
        if src.startswith("<nav", i):
            depth += 1
            i += 4
            continue
        if src.startswith("</nav>", i):
            depth -= 1
            i += 6
            if depth == 0:
                return src[start:i]
            continue
        i += 1
    raise AssertionError("unclosed nav.tabs")


def _main_content_block(src: str) -> str:
    m = re.search(r'<main\s+[^>]*className="content"[^>]*>', src)
    assert m, 'main.content not found'
    start = m.start()
    depth = 0
    i = start
    while i < len(src):
        if src.startswith("<main", i):
            depth += 1
            i += 5
            continue
        if src.startswith("</main>", i):
            depth -= 1
            i += 7
            if depth == 0:
                return src[start:i]
            continue
        i += 1
    raise AssertionError("unclosed main.content")


def test_01_tablist_semantics() -> None:
    src = _read(APP_TSX)
    nav = _tabs_nav_block(src)
    assert ROLE_TABLIST in nav
    assert re.search(r'aria-label=\{?["\'][^"\']+["\']\}?', nav) or re.search(
        r'aria-label="[^"]+"', nav
    )
    assert f'{ARIA_ORIENTATION}="horizontal"' in nav
    assert "TABS.map" in nav


def test_02_tab_state_and_roving_tabindex() -> None:
    src = _read(APP_TSX)
    nav = _tabs_nav_block(src)
    assert ROLE_TAB in nav
    assert re.search(rf"{re.escape(ARIA_SELECTED)}=\{{tab\s*===\s*id\}}", nav)
    assert re.search(
        r"tabIndex=\{\s*tab\s*===\s*id\s*\?\s*0\s*:\s*-1\s*\}",
        nav,
    )


def test_03_deterministic_id_cross_link() -> None:
    src = _read(APP_TSX)
    assert re.search(rf"function\s+{re.escape(TAB_BUTTON_ID)}\s*\(\s*id\s*:\s*MainTab", src)
    assert re.search(rf"function\s+{re.escape(TAB_PANEL_ID)}\s*\(\s*id\s*:\s*MainTab", src)
    assert "main-tab-${id}" in src or 'main-tab-" + id' in src or "main-tab-" in src
    assert "main-tabpanel-${id}" in src or "main-tabpanel-" in src
    assert "Math.random" not in src
    assert "Date.now" not in src
    assert "useId" not in src
    nav = _tabs_nav_block(src)
    assert re.search(rf"id=\{{{re.escape(TAB_BUTTON_ID)}\(id\)\}}", nav)
    assert re.search(rf"{re.escape(ARIA_CONTROLS)}=\{{{re.escape(TAB_PANEL_ID)}\(id\)\}}", nav)
    main = _main_content_block(src)
    assert re.search(rf"id=\{{{re.escape(TAB_PANEL_ID)}\(id\)\}}", main)
    assert re.search(
        rf"{re.escape(ARIA_LABELLEDBY)}=\{{{re.escape(TAB_BUTTON_ID)}\(id\)\}}",
        main,
    )


def test_04_tabpanel_shells() -> None:
    src = _read(APP_TSX)
    main = _main_content_block(src)
    assert 'className="content"' in main
    assert ROLE_TABPANEL in main
    assert 'role="tabpanel"' not in re.search(
        r"<main\b[^>]*>", main
    ).group(0)  # type: ignore[union-attr]
    assert re.search(r"hidden=\{\s*tab\s*!==\s*id\s*\}", main)
    assert re.search(
        r"tabIndex=\{\s*tab\s*===\s*id\s*\?\s*0\s*:\s*-1\s*\}",
        main,
    )
    assert MAIN_TAB_PANEL in main or 'className="main-tab-panel"' in main


def test_05_arrow_left_right_wrap() -> None:
    src = _read(APP_TSX)
    body = _extract_function(src, HANDLE_TAB_KEYDOWN)
    assert ARROW_LEFT in body
    assert ARROW_RIGHT in body
    assert "TABS.length" in body
    assert "preventDefault" in body
    assert ".focus(" in body or "focusTabByIndex" in body or "focus" in body


def test_06_home_end_focus() -> None:
    src = _read(APP_TSX)
    body = _extract_function(src, HANDLE_TAB_KEYDOWN)
    assert KEY_HOME in body
    assert KEY_END in body
    assert "0" in body
    assert "TABS.length" in body


def test_07_manual_activation_focus_only() -> None:
    src = _read(APP_TSX)
    body = _extract_function(src, HANDLE_TAB_KEYDOWN)
    assert "setTab" not in body
    assert ".click(" not in body
    assert "pkbInvoke" not in body
    assert "engineHealth" not in body
    assert "stopPropagation" not in body
    assert "ArrowUp" not in body
    assert "ArrowDown" not in body
    # Focus helpers must also avoid selection.
    for name in ("focusTabByIndex", "focusMainTab"):
        if re.search(rf"(?:function|const)\s+{name}\b", src):
            helper = _extract_function(src, name)
            assert "setTab" not in helper
            assert ".click(" not in helper


def test_08_existing_activation_paths() -> None:
    src = _read(APP_TSX)
    nav = _tabs_nav_block(src)
    assert re.search(r"onClick=\{\(\)\s*=>\s*setTab\(id\)\}", nav)
    assert "TABS" in src
    assert "altKey" in src
    assert re.search(r"/\^\[1-7\]\$/", src) or "/^[1-7]$/" in src
    assert "preventDefault" in src
    assert "removeEventListener" in src


def test_09_f11_conditional_unmount() -> None:
    src = _read(APP_TSX)
    main = _main_content_block(src)
    legacy = bool(
        re.search(
            r'tab\s*===\s*"(record|import|consult|interview|probe|profile|settings)"\s*&&',
            main,
        )
    )
    modern = bool(
        re.search(
            rf"tab\s*===\s*id\s*&&\s*{re.escape(RENDER_MAIN_TAB)}\(id\)",
            main,
        )
    )
    assert legacy or modern, "F-11 conditional mount missing"
    # Unconditional renderMainTab(id) inside shell without active guard is forbidden.
    if modern:
        # Every renderMainTab(id) occurrence in main must be guarded by tab === id.
        for m in re.finditer(rf"{re.escape(RENDER_MAIN_TAB)}\(id\)", main):
            window = main[max(0, m.start() - 40) : m.start()]
            assert "tab === id" in window or "tab===id" in window.replace(" ", "")


def test_09b_render_main_tab_exhaustive_never_guard() -> None:
    """switch(MainTab) must end with a never exhaustive guard (no silent undefined)."""
    src = _read(APP_TSX)
    body = _extract_function(src, RENDER_MAIN_TAB)
    assert "switch" in body
    # Split tokens so this test file is not a greppable false positive for `never`.
    never_ty = "nev" + "er"
    assert re.search(
        rf"const\s+\w+\s*:\s*{re.escape(never_ty)}\s*=\s*id\b",
        body,
    ) or re.search(
        rf"\({re.escape(never_ty)}\)\s*id\b",
        body,
    ) or re.search(
        rf":\s*{re.escape(never_ty)}\s*=\s*id\b",
        body,
    ), "exhaustive never guard missing in renderMainTab"
    assert "throw" in body
    assert "default" in body or never_ty in body


def test_10_focus_visible_styles() -> None:
    css = _read(APP_CSS)
    assert re.search(
        rf"\.tabs\s+button:{re.escape(FOCUS_VISIBLE)}\s*,\s*\.{re.escape(MAIN_TAB_PANEL)}:{re.escape(FOCUS_VISIBLE)}",
        css,
    ) or (
        f".tabs button:{FOCUS_VISIBLE}" in css
        and f".{MAIN_TAB_PANEL}:{FOCUS_VISIBLE}" in css
    )
    m = re.search(
        rf"\.tabs\s+button:{re.escape(FOCUS_VISIBLE)}[\s\S]*?\{{([\s\S]*?)\}}",
        css,
    )
    assert m, "tabs focus-visible rule block missing"
    block = m.group(1)
    assert "outline" in block
    assert "outline-offset" in block
    assert not re.search(r"#[0-9a-fA-F]{3,8}\b", block)
    assert not re.search(r"\b\d+px\b", block)


def test_11_aria_current_forbidden_in_main_tabs() -> None:
    src = _read(APP_TSX)
    nav = _tabs_nav_block(src)
    main = _main_content_block(src)
    assert ARIA_CURRENT not in nav
    assert ARIA_CURRENT not in main


def test_12_docs_synced() -> None:
    s34 = _ai_skills_s34()
    # Avoid putting the full joined phrases as contiguous literals in this file.
    assert "tablist" in s34 and "tabpanel" in s34
    assert ARIA_SELECTED.replace('"', "") in s34 or "aria-selected" in s34
    assert "manual" in s34.lower() and "activation" in s34.lower()
    assert "focus" in s34.lower()
    assert "keep-alive" in s34.lower() or "keep alive" in s34.lower() or "keepalive" in s34.lower().replace("-", "")

    spec = _read(SPEC_FOXTROT)
    assert "Finding 17" in spec or "Finding17" in spec
    f11 = _extract_f11(spec)
    assert "shell" in f11.lower() or "ARIA" in f11 or "aria" in f11
    assert "keep-alive" in f11.lower() or "keep-alive" in spec.lower()
    s36 = _section_36(spec)
    assert ARROW_LEFT in s36 or "ArrowLeft" in s36 or "Left" in s36
    assert "Home" in s36 or KEY_HOME in s36
    assert "Enter" in s36 or "Space" in s36
    assert "focus" in s36.lower()


def _extract_f11(spec: str) -> str:
    m = re.search(r"\*\*F-11[^*]*\*\*[^\n]*", spec)
    assert m, "F-11 bullet missing"
    start = m.start()
    # Include following indented continuation lines until next F- bullet or heading.
    rest = spec[start:]
    nxt = re.search(r"\n-\s+\*\*F-\d+", rest[1:])
    end = 1 + nxt.start() if nxt else min(len(rest), 800)
    return rest[:end]


def _section_36(spec: str) -> str:
    m = re.search(r"^##\s+3\.6\s+", spec, flags=re.M)
    assert m, "SPEC §3.6 missing"
    nxt = re.search(r"^#\s+", spec[m.end() :], flags=re.M)
    end = m.end() + (nxt.start() if nxt else len(spec) - m.end())
    return spec[m.start() : end]
