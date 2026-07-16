# -*- coding: utf-8 -*-
"""E0b STEP 0 — Constitutional Guard: lock the networkless sterile state.

Test-only ratchet. Production code is never modified here.
Parent: docs/SPEC_E0B_STEP0_GUARD.md / docs/SPEC_E0B_KNOWLEDGE_GATEWAY_v3.md §12 STEP 0.

Scope T  = tree-wide network egress ban (src/python/**/*.py).
Scope E  = E0b-file-scoped strict ban (knowledge_gateway*.py / *e0b*.py only).
subprocess/ctypes are NOT in Scope T (legitimate local substrate).
bare asyncio is allowed; only asyncio.subprocess is denied.
"""
from __future__ import annotations

import ast
import fnmatch
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
PYTHON_SRC = ROOT / "src" / "python"
KF_PATH = PYTHON_SRC / "core" / "knowledge_fetcher.py"

# ---------------------------------------------------------------------------
# Scope T — tree-wide network egress deny-set (§1.4)
# ---------------------------------------------------------------------------
DENY_NET_TOP = frozenset({
    "urllib",
    "socket",
    "requests",
    "httpx",
    "aiohttp",
    "ftplib",
    "websockets",
    "smtplib",
    "telnetlib",
    "poplib",
    "imaplib",
    "ssl",
    "xmlrpc",
    "webbrowser",
})
DENY_NET_SUBMODULE = frozenset({"http.client", "asyncio.subprocess"})

# ---------------------------------------------------------------------------
# Scope E — E0b-module strict cage (§1.4); reserved until E0b modules appear
# ---------------------------------------------------------------------------
DENY_E0B_EXTRA = frozenset({"subprocess", "ctypes", "multiprocessing"})
DENY_E0B_TOP = DENY_NET_TOP | DENY_E0B_EXTRA
DENY_E0B_SUBMODULE = DENY_NET_SUBMODULE

DENY_CALL_NAMES = frozenset({
    "__import__",
    "urlopen",
    "system",
    "popen",
    "Popen",
    "run",
    "call",
    "check_call",
    "check_output",
    "eval",
    "exec",
})

E0A_MSG = "Egress blocked by E0a strict lockdown."

E0B_BASENAME_GLOBS = ("knowledge_gateway*.py", "*e0b*.py")


# ---------------------------------------------------------------------------
# AST helpers
# ---------------------------------------------------------------------------
def _call_name(node: ast.Call) -> str | None:
    func = node.func
    if isinstance(func, ast.Name):
        return func.id
    if isinstance(func, ast.Attribute):
        return func.attr
    return None


def _denied_imports(
    tree: ast.AST,
    *,
    deny_top: frozenset[str],
    deny_submodule: frozenset[str],
) -> list[str]:
    """Collect denied import forms (top-level, nested, from-variants, aliases)."""
    hits: list[str] = []
    for node in ast.walk(tree):
        if isinstance(node, ast.Import):
            for alias in node.names:
                name = alias.name
                top = name.split(".", 1)[0]
                if top in deny_top or name in deny_top:
                    hits.append(f"import {name}")
                for sub in deny_submodule:
                    if name == sub or name.startswith(sub + "."):
                        hits.append(f"import {name}")
        elif isinstance(node, ast.ImportFrom):
            mod = node.module or ""
            names = [a.name for a in node.names]
            top = mod.split(".", 1)[0] if mod else ""
            if top in deny_top or mod in deny_top:
                hits.append(f"from {mod} import {', '.join(names)}")
            for sub in deny_submodule:
                if mod == sub or mod.startswith(sub + "."):
                    hits.append(f"from {mod} import {', '.join(names)}")
            # from http import client  /  from http import client as X
            if mod == "http" and "client" in names and "http.client" in deny_submodule:
                hits.append("from http import client")
            # from asyncio import subprocess
            if (
                mod == "asyncio"
                and "subprocess" in names
                and "asyncio.subprocess" in deny_submodule
            ):
                hits.append("from asyncio import subprocess")
    return hits


def denied_imports_scope_t(tree: ast.AST) -> list[str]:
    return _denied_imports(tree, deny_top=DENY_NET_TOP, deny_submodule=DENY_NET_SUBMODULE)


def denied_imports_scope_e(tree: ast.AST) -> list[str]:
    return _denied_imports(tree, deny_top=DENY_E0B_TOP, deny_submodule=DENY_E0B_SUBMODULE)


def has_dynamic_bypass(tree: ast.AST) -> list[str]:
    """Detect dynamic egress bypass constructs (Scope E cage / canaries)."""
    hits: list[str] = []
    for node in ast.walk(tree):
        if isinstance(node, ast.Call):
            name = _call_name(node)
            if name in DENY_CALL_NAMES:
                hits.append(name or "?")
            if isinstance(node.func, ast.Attribute):
                if node.func.attr == "import_module":
                    hits.append("import_module")
                if node.func.attr in {"system", "popen"}:
                    hits.append(f"os.{node.func.attr}")
    return hits


def _is_e0b_scoped_basename(name: str) -> bool:
    return any(fnmatch.fnmatch(name, pat) for pat in E0B_BASENAME_GLOBS)


def iter_e0b_scoped_modules() -> list[Path]:
    """Enumerate Scope E target files under src/python/ (0 expected at STEP 0)."""
    found: list[Path] = []
    if not PYTHON_SRC.is_dir():
        return found
    for path in PYTHON_SRC.rglob("*.py"):
        if "__pycache__" in path.parts:
            continue
        if _is_e0b_scoped_basename(path.name):
            found.append(path)
    return found


def _func_defs(tree: ast.Module) -> dict[str, ast.FunctionDef]:
    return {
        n.name: n
        for n in tree.body
        if isinstance(n, ast.FunctionDef)
    }


def _is_exact_e0a_raise(body: list[ast.stmt]) -> bool:
    """Body must be exactly: raise NotImplementedError(<E0A_MSG>)."""
    if len(body) != 1 or not isinstance(body[0], ast.Raise):
        return False
    raise_node = body[0]
    if raise_node.cause is not None:
        return False
    exc = raise_node.exc
    if not isinstance(exc, ast.Call):
        return False
    if not isinstance(exc.func, ast.Name) or exc.func.id != "NotImplementedError":
        return False
    if len(exc.args) != 1 or exc.keywords:
        return False
    arg = exc.args[0]
    if not isinstance(arg, ast.Constant) or arg.value != E0A_MSG:
        return False
    return True


def _returns_false_only(body: list[ast.stmt]) -> bool:
    if len(body) != 1 or not isinstance(body[0], ast.Return):
        return False
    val = body[0].value
    return isinstance(val, ast.Constant) and val.value is False


# ---------------------------------------------------------------------------
# STEP 0.A — Detector canaries (prove detection power before production scan)
# ---------------------------------------------------------------------------
def test_detector_flags_synthetic_violation() -> None:
    """Canary: every synthetic egress form must be hit by Scope T / bypass detector."""
    cases: list[tuple[str, str]] = [
        ("import socket\n", "socket"),
        ("from urllib import request\n", "urllib"),
        ("import http.client\n", "http.client"),
        ("from asyncio import subprocess\n", "asyncio.subprocess"),
        ("__import__('os').system('x')\n", "__import__"),
        # Edge cases required by blueprint §2 STEP 0.A
        ("import a.b.c\nimport socket.AF_INET\n", "socket"),  # top-level extract
        ("from urllib.request import urlopen as u\n", "urllib"),  # alias
        ("from http import client\n", "http.client"),
        ("from http import client as hc\n", "http.client"),
        (
            "def f():\n    import requests\n",
            "requests",
        ),  # nested (function-body) import
        (
            "if True:\n    import aiohttp\n",
            "aiohttp",
        ),  # conditional import
        (
            "import importlib\nimportlib.import_module('socket')\n",
            "import_module",
        ),  # dynamic
    ]

    for src, label in cases:
        tree = ast.parse(src)
        import_hits = denied_imports_scope_t(tree)
        bypass_hits = has_dynamic_bypass(tree)
        combined = import_hits + bypass_hits
        assert combined, (
            f"detector missed synthetic violation ({label!r}):\n{src!r}"
        )


def test_detector_allows_legitimate_local_substrate() -> None:
    """Negative canary: Scope T must NOT flag bare asyncio / subprocess / ctypes."""
    for src in (
        "import asyncio\n",
        "import subprocess\n",
        "import ctypes\n",
        "from asyncio import get_event_loop\n",
        "import subprocess as sp\n",
        "from ctypes import windll\n",
    ):
        tree = ast.parse(src)
        hits = denied_imports_scope_t(tree)
        assert not hits, (
            f"Scope T false-positive on legitimate local substrate:\n"
            f"  src={src!r}\n  hits={hits}"
        )
        # Dynamic bypass must also stay silent on bare imports (no calls).
        assert not has_dynamic_bypass(tree), (
            f"bypass detector false-positive on bare import:\n  src={src!r}"
        )
