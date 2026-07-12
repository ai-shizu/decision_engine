# -*- coding: utf-8 -*-
"""Finding 10 — ContextObservatoryContainer single-flight admission control."""
from __future__ import annotations

import re
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
CONTAINER = (
    ROOT / "apps" / "desktop" / "src" / "components" / "ContextObservatoryContainer.tsx"
)
REDUCER = ROOT / "apps" / "desktop" / "src" / "lib" / "manifestFetchState.ts"


def _read(path: Path) -> str:
    return path.read_text(encoding="utf-8")


def _extract_brace_block(src: str, header_pat: str) -> str:
    m = re.search(header_pat, src)
    assert m, f"header not found: {header_pat}"
    start = src.find("{", m.end() - 1)
    assert start != -1, "opening brace missing"
    depth = 0
    for i in range(start, len(src)):
        ch = src[i]
        if ch == "{":
            depth += 1
        elif ch == "}":
            depth -= 1
            if depth == 0:
                return src[start : i + 1]
    raise AssertionError("unbalanced braces")


def _load_manifest_fn() -> str:
    return _extract_brace_block(
        _read(CONTAINER),
        r"async\s+function\s+loadManifest\s*\(",
    )


def _component_body() -> str:
    return _extract_brace_block(
        _read(CONTAINER),
        r"export\s+function\s+ContextObservatoryContainer\s*\(",
    )


def test_boolean_inflight_useref_exists() -> None:
    src = _read(CONTAINER)
    assert re.search(
        r"useRef\s*(?:<\s*boolean\s*>)?\s*\(\s*false\s*\)",
        src,
    ), "boolean in-flight useRef(false) required"


def test_load_manifest_returns_immediately_if_inflight() -> None:
    body = _load_manifest_fn()
    assert re.search(r"if\s*\([^)]*\.current[^)]*\)\s*return", body)
    # Early return must appear before IPC.
    assert body.find("return") < body.find("latestContextManifest")


def test_ref_set_true_before_dispatch_seq_and_ipc() -> None:
    body = _load_manifest_fn()
    true_idx = re.search(r"\.current\s*=\s*true", body)
    assert true_idx
    for needle in ("REQUEST_START", "latestContextManifest"):
        assert true_idx.start() < body.find(needle), needle
    # seq bump / capture must also follow the guard raise
    seq_pos = min(
        i
        for i in (
            body.find("seqRef.current"),
            body.find("++seqRef"),
            body.find("seqRef.current++"),
        )
        if i != -1
    )
    assert true_idx.start() < seq_pos


def test_latest_context_manifest_called_once() -> None:
    body = _load_manifest_fn()
    assert body.count("latestContextManifest(") == 1
    assert _component_body().count("latestContextManifest(") == 1


def test_finally_clears_inflight_ref() -> None:
    body = _load_manifest_fn()
    assert "finally" in body
    finally_block = body[body.index("finally") :]
    assert re.search(r"\.current\s*=\s*false", finally_block)


def test_success_and_failure_both_use_finally() -> None:
    body = _load_manifest_fn()
    assert "REQUEST_SUCCESS" in body
    assert "REQUEST_FAILURE" in body
    assert "try" in body and "catch" in body and "finally" in body
    # finally after both success and failure paths in source order
    assert body.index("REQUEST_SUCCESS") < body.index("finally")
    assert body.index("REQUEST_FAILURE") < body.index("finally")


def test_button_disabled_from_loading_phase() -> None:
    src = _read(CONTAINER)
    assert re.search(r"phase\s*===\s*[\"']loading[\"']", src)
    disabled = re.search(r"disabled=\{([^}]+)\}", src)
    assert disabled
    expr = disabled.group(1)
    assert "loading" in expr or "isLoading" in expr or "phase" in expr


def test_loading_button_label_is_reading() -> None:
    src = _read(CONTAINER)
    # Accept either inline phase===loading or isLoading alias from STEP 3 shape.
    assert re.search(
        r"""(?:phase\s*===\s*["']loading["']|isLoading)\s*\?[\s\S]{0,40}["']読み込み中…["']"""
        r"""|["']読み込み中…["'][\s\S]{0,80}(?:phase\s*===\s*["']loading["']|isLoading)""",
        src,
    )
    assert re.search(r"""isLoading\s*=\s*state\.phase\s*===\s*["']loading["']""", src)


def test_aria_busy_present() -> None:
    src = _read(CONTAINER)
    assert re.search(r"aria-busy=\{[^}]+\}", src)


def test_retry_and_reload_labels_preserved() -> None:
    src = _read(CONTAINER)
    assert re.search(r'["\']読み込み["\']', src)
    assert re.search(r'["\']再読み込み["\']', src)
    assert re.search(r'phase\s*===\s*["\']empty["\']', src)
    assert re.search(r'phase\s*===\s*["\']error["\']', src)


def test_no_polling_auto_retry_or_useeffect_fetch() -> None:
    src = _read(CONTAINER)
    assert "setInterval" not in src
    assert "setTimeout" not in src
    assert "useEffect" not in src
    assert "polling" not in src.lower()
    assert "retry" not in src.lower()
    assert "debounce" not in src.lower()
    assert "throttle" not in src.lower()
    # No request queueing of a second flight
    assert "queue" not in src.lower()


def test_reducer_seq_and_fixed_error_unchanged_by_container() -> None:
    """Container must keep using reducer seq events; fixed error stays in reducer."""
    container = _read(CONTAINER)
    reducer = _read(REDUCER)
    assert "REQUEST_START" in container
    assert "REQUEST_SUCCESS" in container
    assert "REQUEST_FAILURE" in container
    assert "MANIFEST_FETCH_ERROR_MESSAGE" in reducer
    assert "String(err)" not in container
    assert "error.message" not in container
    assert "catch (err" not in container and "catch(err" not in container
